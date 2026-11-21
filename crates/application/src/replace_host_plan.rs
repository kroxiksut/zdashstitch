// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-04B: one [`VerifiedPlan`] for all ReplaceHost field changes of a
//! workcopy — N field changes on M pages of one dashboard, one
//! `dashboard.update`.
//!
//! Verification steps of [`plan_replace_host`], in order (any failure means
//! no plan and no write):
//!
//! 1. the local session that admitted the request is still current, and
//!    no publication journal entry blocks the dashboard (an unfinished
//!    write to it needs reconciliation first, P2-04D);
//! 2. fresh, read-only revalidation of the stored ReplaceHost calculation
//!    (`revalidate_replace_host`: stale or invalid rows become blockers);
//! 3. the draft is exactly what the user previewed: `local_id`, `revision`,
//!    Base and Local fingerprints, and the digest of the kept-on-source
//!    list (decisions carry no expected revision, so revision and Local
//!    fingerprint are the binding);
//! 4. the workcopy belongs to the active connection profile;
//! 5. Remote is re-read through the read gateway and must equal Base
//!    (`Remote != Base` → `source_changed`, zero write);
//! 6. the account may write the dashboard (`dashboard.get` + `editable`);
//! 7. every blocker of the publication report is resolved: connection,
//!    `read_write` mode, exact server version, contracts, bindings, kept
//!    list confirmation;
//! 8. per change kind, the adapter derives the contracts from the re-read
//!    Remote (`item_widget_itemid`, `svggraph_dataset_itemids`,
//!    `svggraph_dataset_hosts`, `classic_graph_graphid`, plus
//!    `update_several_pages` for more than one page and a proven resend for
//!    every widget of a changed page); each must be `Supported`;
//! 9. the adapter builds the update bound to the live publication
//!    authority (endpoint, server identity, version, credential, write
//!    epoch), and its logical result must equal Local. The adapter also
//!    binds the expected server read-back to the plan and journal.
//!
//! [`publish_replace_host_plan`] repeats the draft check (revision, Local,
//! Base, kept digest), the session generation and the authority right
//! before send, opens the journal entry, and the adapter then re-reads
//! Remote, commits `submitting` to the journal, sends exactly one
//! `dashboard.update` and reads back.
//!
//! A `read_only` connection cannot plan at all:
//!
//! ```compile_fail
//! async fn read_only(
//!     service: &zdashstitch_application::workcopy::WorkcopyService,
//!     journal: &zdashstitch_application::PublishJournal,
//!     session: &zdashstitch_application::LocalSession,
//!     reader: &zdashstitch_zabbix::ReadOnlyGateway,
//!     request: &zdashstitch_application::ReplaceHostPlanRequest,
//! ) {
//!     let _ = zdashstitch_application::plan_replace_host(service, journal, session, reader, request)
//!         .await;
//! }
//! ```
//!
//! Control: the same call compiles with the read-write gateway.
//!
//! ```no_run
//! async fn read_write(
//!     service: &zdashstitch_application::workcopy::WorkcopyService,
//!     journal: &zdashstitch_application::PublishJournal,
//!     session: &zdashstitch_application::LocalSession,
//!     writer: &zdashstitch_zabbix::ReadWriteGateway,
//!     request: &zdashstitch_application::ReplaceHostPlanRequest,
//! ) {
//!     let _ = zdashstitch_application::plan_replace_host(service, journal, session, writer, request)
//!         .await;
//! }
//! ```

use std::collections::BTreeSet;

use zdashstitch_zabbix::{
    AdapterProfile, PublicationContext, ReadWriteGateway, VersionedDashboardSnapshot,
    WidgetFieldReplacement, verify_widget_field_replacements, verify_workcopy_style_replacements,
    write_requirements,
};

use crate::publish_journal::{PublishJournal, TargetKey};
use crate::publisher::{
    CompatibilityFinding, FindingCode, FindingGroup, FindingSeverity, PlanError, PlanInvalidReason,
    WorkcopyBinding, compatibility_report, requirement_findings, send_confirmed,
};
use crate::replace_host::{FieldChange, FieldValue, ReplaceHostPreview, SessionContext};
use crate::workcopy::WorkcopyService;
use crate::{
    ConfirmedPlan, LocalSession, PublishReport, SessionGeneration, VerifiedPlan, WriteStage,
};

/// What the user saw in the ReplaceHost preview. The plan is bound to
/// exactly this; any difference invalidates it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaceHostPlanRequest {
    pub local_id: String,
    /// Connection profile of the active session.
    pub connection_id: String,
    /// `ReplaceHostPreview::revision` shown.
    pub revision: u64,
    /// `ReplaceHostPreview::base_fingerprint` shown.
    pub base_fingerprint: String,
    /// `ReplaceHostPreview::local_fingerprint` shown.
    pub local_fingerprint: String,
    /// Digest of the kept-on-source list shown; required when the list is
    /// not empty.
    pub kept_digest: Option<String>,
    /// Generation the request was admitted under.
    pub session: SessionGeneration,
}

/// Builds a plan for the style-only log the user reviewed. The adapter
/// independently checks the exact snapshot shape and visual write contracts.
pub async fn plan_workcopy_style(
    service: &WorkcopyService,
    journal: &PublishJournal,
    session: &LocalSession,
    gateway: &ReadWriteGateway,
    request: &ReplaceHostPlanRequest,
) -> Result<VerifiedPlan, PlanError> {
    if !session.is_current(request.session) {
        return Err(PlanError::Invalidated(PlanInvalidReason::SessionEnded));
    }
    let (file, base, log) = service
        .load_log(&request.local_id)
        .map_err(PlanError::Workcopy)?;
    let binding = WorkcopyBinding {
        local_id: request.local_id.clone(),
        connection_id: request.connection_id.clone(),
        revision: request.revision,
        base_fingerprint: request.base_fingerprint.clone(),
        local_fingerprint: request.local_fingerprint.clone(),
        kept_digest: None,
    };
    if request.kept_digest.is_some() || !style_binding_matches(&binding, &file, &base, &log) {
        return Err(PlanError::Invalidated(PlanInvalidReason::DraftChanged));
    }
    if !file.source.matches_publication_identity(gateway.reader()) {
        return Err(PlanError::Invalidated(PlanInvalidReason::ConnectionChanged));
    }
    journal
        .check_target(&TargetKey::dashboard(
            base.connection_id(),
            Some(
                gateway
                    .reader()
                    .identity()
                    .endpoint_fingerprint()
                    .to_owned(),
            ),
            base.dashboard_id(),
        ))
        .map_err(PlanError::Journal)?;
    let read = |error: zdashstitch_zabbix::ZabbixError| PlanError::Read(error.failure());
    let remote = gateway
        .reader()
        .dashboard_snapshot(base.connection_id(), base.dashboard_id())
        .await
        .map_err(read)?
        .filter(|remote| remote.fingerprint() == base.fingerprint())
        .ok_or(PlanError::Invalidated(PlanInvalidReason::SourceChanged))?;
    let replacements =
        crate::workcopy::reusable::style_replacements(&base, &log).map_err(PlanError::Workcopy)?;
    let context = gateway.publication_context();
    let requirements = write_requirements(&remote, &replacements).map_err(PlanError::Adapter)?;
    let mut report = compatibility_report(&remote, &context, &requirements);
    if !gateway
        .dashboard_is_editable(base.dashboard_id())
        .await
        .map_err(read)?
    {
        report.findings.push(CompatibilityFinding::new(
            FindingCode::DashboardNotEditable,
            FindingSeverity::BlocksPublication,
            FindingGroup::Connection,
        ));
    }
    if !report.allows_publication() {
        return Err(PlanError::Incompatible(report));
    }
    let update = verify_workcopy_style_replacements(&remote, &context, &replacements)
        .map_err(PlanError::Adapter)?;
    if update.local_fingerprint() != binding.local_fingerprint {
        return Err(PlanError::LocalMismatch);
    }
    Ok(VerifiedPlan::for_workcopy_style(
        update,
        report,
        binding,
        request.session,
    ))
}

pub async fn plan_workcopy_blueprint(
    service: &WorkcopyService,
    journal: &PublishJournal,
    session: &LocalSession,
    gateway: &ReadWriteGateway,
    request: &ReplaceHostPlanRequest,
) -> Result<VerifiedPlan, PlanError> {
    if !session.is_current(request.session) {
        return Err(PlanError::Invalidated(PlanInvalidReason::SessionEnded));
    }
    let (file, base, log) = service
        .load_log(&request.local_id)
        .map_err(PlanError::Workcopy)?;
    let binding = WorkcopyBinding {
        local_id: request.local_id.clone(),
        connection_id: request.connection_id.clone(),
        revision: request.revision,
        base_fingerprint: request.base_fingerprint.clone(),
        local_fingerprint: request.local_fingerprint.clone(),
        kept_digest: None,
    };
    let entry = blueprint_binding_entry(&binding, &file, &base, &log)
        .filter(|_| request.kept_digest.is_none())
        .ok_or(PlanError::Invalidated(PlanInvalidReason::DraftChanged))?;
    let reader = gateway.reader();
    if file.source.server_identity_fingerprint.as_deref() != Some(reader.identity().fingerprint())
        || &reader.capabilities().version != base.server_version()
    {
        return Err(PlanError::Invalidated(PlanInvalidReason::ConnectionChanged));
    }
    journal
        .check_target(&TargetKey::dashboard(
            base.connection_id(),
            Some(reader.identity().endpoint_fingerprint().to_owned()),
            base.dashboard_id(),
        ))
        .map_err(PlanError::Journal)?;
    let read = |error: zdashstitch_zabbix::ZabbixError| PlanError::Read(error.failure());
    let remote = reader
        .dashboard_snapshot(base.connection_id(), base.dashboard_id())
        .await
        .map_err(read)?
        .filter(|remote| remote.fingerprint() == base.fingerprint())
        .ok_or(PlanError::Invalidated(PlanInvalidReason::SourceChanged))?;
    let unsupported = || {
        PlanError::Workcopy(crate::workcopy::WorkcopyError::new(
            "reusable_blueprint_shape_unsupported",
        ))
    };
    let (widget, bindings) = entry.publication_input().ok_or_else(unsupported)?;
    let update = gateway
        .verify_blueprint_append(
            &remote,
            &entry.page_id,
            widget,
            bindings,
            binding.local_fingerprint.clone(),
        )
        .await
        .map_err(read)?
        .map_err(PlanError::Adapter)?;
    let report = compatibility_report(
        &remote,
        &gateway.publication_context(),
        &zdashstitch_zabbix::WriteRequirements {
            contracts: update.required_contracts().clone(),
            changed_pages: update.changed_pages().iter().copied().collect(),
            kinds: update.replacement_kinds().clone(),
            unproven_widgets: Vec::new(),
        },
    );
    if !report.allows_publication() {
        return Err(PlanError::Incompatible(report));
    }
    Ok(VerifiedPlan::for_workcopy_blueprint(
        update,
        report,
        binding,
        request.session,
    ))
}

fn blueprint_binding_entry<'a>(
    binding: &WorkcopyBinding,
    file: &crate::workcopy::formats::WorkcopyFile,
    base: &VersionedDashboardSnapshot,
    log: &'a [crate::workcopy::operations::Operation],
) -> Option<&'a crate::workcopy::blueprint::AddWidgetEntry> {
    let entry = crate::workcopy::blueprint::only_blueprint(log)?;
    (file.revision == binding.revision
        && base.connection_id() == binding.connection_id
        && base.fingerprint() == binding.base_fingerprint
        && entry
            .fingerprint()
            .is_ok_and(|digest| digest == binding.local_fingerprint))
    .then_some(entry)
}

fn style_binding_matches(
    binding: &WorkcopyBinding,
    file: &crate::workcopy::formats::WorkcopyFile,
    base: &VersionedDashboardSnapshot,
    operations: &[crate::workcopy::operations::Operation],
) -> bool {
    file.revision == binding.revision
        && base.connection_id() == binding.connection_id
        && base.fingerprint() == binding.base_fingerprint
        && !operations.is_empty()
        && operations
            .iter()
            .all(|op| matches!(op, crate::workcopy::operations::Operation::Style(_)))
        && crate::workcopy::reusable::overlay_style(base.clone(), operations)
            .is_ok_and(|local| local.fingerprint() == binding.local_fingerprint)
}

/// Facts gathered by [`plan_replace_host`]; verified by a pure function.
pub(crate) struct PlanInputs<'a> {
    pub request: &'a ReplaceHostPlanRequest,
    /// Freshly revalidated preview of the draft.
    pub preview: &'a ReplaceHostPreview,
    /// Materialized Local.
    pub local: &'a VersionedDashboardSnapshot,
    /// Remote re-read now; `None` when missing or hidden.
    pub remote: Option<&'a VersionedDashboardSnapshot>,
    /// `dashboard.get` with `editable` returned the dashboard.
    pub editable: bool,
    pub context: &'a PublicationContext,
}

/// Builds the ReplaceHost plan of a workcopy (steps in the module docs).
/// Reads only; never writes.
pub async fn plan_replace_host(
    service: &WorkcopyService,
    journal: &PublishJournal,
    session: &LocalSession,
    gateway: &ReadWriteGateway,
    request: &ReplaceHostPlanRequest,
) -> Result<VerifiedPlan, PlanError> {
    if !session.is_current(request.session) {
        return Err(PlanError::Invalidated(PlanInvalidReason::SessionEnded));
    }
    let (file, base) = service
        .load_with_base(&request.local_id)
        .map_err(PlanError::Workcopy)?;
    if !file.source.matches_publication_identity(gateway.reader()) {
        return Err(PlanError::Invalidated(PlanInvalidReason::ConnectionChanged));
    }
    journal
        .check_target(&TargetKey::dashboard(
            base.connection_id(),
            Some(
                gateway
                    .reader()
                    .identity()
                    .endpoint_fingerprint()
                    .to_owned(),
            ),
            base.dashboard_id(),
        ))
        .map_err(PlanError::Journal)?;
    let preview = service
        .revalidate_replace_host(
            gateway.reader(),
            &request.local_id,
            SessionContext::Connected {
                access_mode: gateway.mode(),
                capabilities: gateway.capabilities(),
            },
        )
        .await
        .map_err(PlanError::Workcopy)?;
    check_draft(request, &preview)?;
    let local = service
        .local_snapshot(&request.local_id)
        .map_err(PlanError::Workcopy)?;
    if local.connection_id() != request.connection_id {
        return Err(PlanError::Invalidated(PlanInvalidReason::ConnectionChanged));
    }
    let read = |error: zdashstitch_zabbix::ZabbixError| PlanError::Read(error.failure());
    let remote = gateway
        .reader()
        .dashboard_snapshot(local.connection_id(), local.dashboard_id())
        .await
        .map_err(read)?;
    let context = if let Some(remote) = remote.as_ref().filter(|remote| {
        remote.adapter_id() == AdapterProfile::V8_0.adapter_id()
            && remote.fingerprint() == preview.base_fingerprint
    }) {
        gateway
            .verified_replace_host_context(remote, &replacements(&preview.changes)?)
            .await
            .map_err(read)?
            .unwrap_or_else(|| gateway.publication_context())
    } else {
        gateway.publication_context()
    };
    let editable = match remote {
        Some(_) => gateway
            .dashboard_is_editable(local.dashboard_id())
            .await
            .map_err(read)?,
        None => false,
    };
    verify_replace_host_plan(&PlanInputs {
        request,
        preview: &preview,
        local: &local,
        remote: remote.as_ref(),
        editable,
        context: &context,
    })
}

/// The draft is the one previewed: same draft, revision, Base, Local and
/// kept list.
fn check_draft(
    request: &ReplaceHostPlanRequest,
    preview: &ReplaceHostPreview,
) -> Result<(), PlanError> {
    let invalid = |reason| Err(PlanError::Invalidated(reason));
    if preview.local_id != request.local_id {
        return invalid(PlanInvalidReason::TargetChanged);
    }
    if preview.revision != request.revision
        || preview.base_fingerprint != request.base_fingerprint
        || preview.local_fingerprint != request.local_fingerprint
    {
        return invalid(PlanInvalidReason::DraftChanged);
    }
    let kept = &preview.kept_on_source;
    let shown = request.kept_digest.as_deref();
    let current = (!kept.entries.is_empty()).then_some(kept.digest.as_str());
    if shown != current {
        return invalid(PlanInvalidReason::BindingsChanged);
    }
    Ok(())
}

/// Field changes of Local as adapter replacements, addressed by page
/// position and `widgetid`: Remote may return the widgets of a page in
/// another order than Base (LAB-02), so the Base widget position is not part
/// of the address. A change without a `widgetid` or with mixed value kinds
/// means the draft is inconsistent.
fn replacements(changes: &[FieldChange]) -> Result<Vec<WidgetFieldReplacement>, PlanError> {
    changes
        .iter()
        .map(|change| {
            let consistent = matches!(
                (&change.source, &change.target),
                (FieldValue::Item { .. }, FieldValue::Item { .. })
                    | (
                        FieldValue::HostPattern { .. },
                        FieldValue::HostPattern { .. }
                    )
                    | (FieldValue::Graph { .. }, FieldValue::Graph { .. })
            );
            let widgetid = change.widgetid.clone().filter(|_| consistent);
            let widgetid = widgetid.ok_or(PlanError::LocalMismatch)?;
            Ok(WidgetFieldReplacement {
                page_index: change.key.page_index,
                widgetid,
                field_name: change.key.field.clone(),
                expected_value: change.source.raw().to_owned(),
                replacement_value: change.target.raw().to_owned(),
            })
        })
        .collect()
}

/// Steps 3–9 of the module docs on gathered facts. Pure.
pub(crate) fn verify_replace_host_plan(inputs: &PlanInputs<'_>) -> Result<VerifiedPlan, PlanError> {
    let PlanInputs {
        request,
        preview,
        local,
        remote,
        editable,
        context,
    } = inputs;
    check_draft(request, preview)?;
    if local.fingerprint() != preview.local_fingerprint {
        return Err(PlanError::Invalidated(PlanInvalidReason::DraftChanged));
    }
    if local.connection_id() != request.connection_id {
        return Err(PlanError::Invalidated(PlanInvalidReason::ConnectionChanged));
    }
    let Some(remote) = remote else {
        return Err(PlanError::Invalidated(PlanInvalidReason::SourceChanged));
    };
    if remote.fingerprint() != preview.base_fingerprint
        || remote.dashboard_id() != local.dashboard_id()
    {
        return Err(PlanError::Invalidated(PlanInvalidReason::SourceChanged));
    }

    let mut report = preview.report.clone();
    if let Some(count) = context.outside_pattern_matches() {
        report.findings.push(
            CompatibilityFinding::new(
                FindingCode::PatternMatchesOutsideSelection,
                FindingSeverity::Informational,
                FindingGroup::Bindings,
            )
            .with_param("count", count.to_string()),
        );
    }
    if !editable {
        report.findings.push(CompatibilityFinding::new(
            FindingCode::DashboardNotEditable,
            FindingSeverity::BlocksPublication,
            FindingGroup::Connection,
        ));
    }
    let replacements = replacements(&preview.changes)?;
    // Kept-on-source fields are never part of the update (both lists are
    // keyed by Base positions).
    let kept_keys: BTreeSet<(usize, usize, &str)> = preview
        .kept_on_source
        .entries
        .iter()
        .map(|entry| (entry.page_index, entry.widget_index, entry.field.as_str()))
        .collect();
    if preview.changes.iter().any(|change| {
        kept_keys.contains(&(
            change.key.page_index,
            change.key.widget_index,
            change.key.field.as_str(),
        ))
    }) {
        return Err(PlanError::LocalMismatch);
    }
    if !replacements.is_empty() {
        let requirements = write_requirements(remote, &replacements).map_err(PlanError::Adapter)?;
        let extra = requirement_findings(
            &requirements,
            &context.capabilities().dashboard_write_contracts,
            &report,
        );
        report.findings.extend(extra);
    }
    if !report.allows_publication() {
        return Err(PlanError::Incompatible(report));
    }
    let update = verify_widget_field_replacements(remote, context, &replacements)
        .map_err(PlanError::Adapter)?;
    if update.local_fingerprint() != local.fingerprint() {
        return Err(PlanError::LocalMismatch);
    }
    let binding = WorkcopyBinding {
        local_id: request.local_id.clone(),
        connection_id: request.connection_id.clone(),
        revision: preview.revision,
        base_fingerprint: preview.base_fingerprint.clone(),
        local_fingerprint: preview.local_fingerprint.clone(),
        kept_digest: request.kept_digest.clone(),
    };
    Ok(VerifiedPlan::for_workcopy(
        update,
        report,
        binding,
        request.session,
    ))
}

/// Sends a confirmed ReplaceHost plan after checking, right before send,
/// that the draft is unchanged (revision, Base, Local, kept list), that
/// `stage` belongs to the plan's session generation and that the
/// publication authority is the one the plan was built for. Then the
/// adapter re-reads Remote (`Remote != Base` → `Conflict`, zero write),
/// sends exactly one `dashboard.update` and reads back. The caller holds
/// `stage` until this returns.
pub async fn publish_replace_host_plan(
    service: &WorkcopyService,
    client: &ReadWriteGateway,
    journal: &PublishJournal,
    plan: ConfirmedPlan,
    stage: &WriteStage,
) -> PublishReport {
    let Some(binding) = plan.workcopy_binding().cloned() else {
        return plan.refuse(None);
    };
    let style = plan.is_style();
    let blueprint = plan.is_blueprint();
    let current = || {
        if blueprint {
            return service
                .load_log(&binding.local_id)
                .is_ok_and(|(file, base, log)| {
                    blueprint_binding_entry(&binding, &file, &base, &log).is_some()
                });
        }
        if style {
            return service
                .load_log(&binding.local_id)
                .is_ok_and(|(file, base, log)| {
                    style_binding_matches(&binding, &file, &base, &log)
                });
        }
        service
            .replace_host_preview(&binding.local_id, SessionContext::Offline)
            .is_ok_and(|preview| {
                let kept = &preview.kept_on_source;
                preview.revision == binding.revision
                    && preview.base_fingerprint == binding.base_fingerprint
                    && preview.local_fingerprint == binding.local_fingerprint
                    && (!kept.entries.is_empty()).then_some(&kept.digest)
                        == binding.kept_digest.as_ref()
            })
    };
    if !current() {
        return plan.refuse(Some(PlanInvalidReason::DraftChanged));
    }
    send_confirmed(client, journal, plan, stage, current).await
}

#[cfg(test)]
mod tests;
