// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-06 «Save as copy» (O-019): one [`VerifiedCreatePlan`] creates one new
//! private global dashboard from the Local of a workcopy in the same
//! connection; the source dashboard is never written. A dashboard draft
//! copies every page of Local; a page draft creates a dashboard of exactly
//! its page (owner decision 28.10.2026, item 2), with the dashboard
//! properties of Local. Everything else — plan invariants, journal,
//! reconciliation — is the same for both kinds.
//!
//! Verification steps of [`plan_save_as_copy`], in order (any failure means
//! no plan and no write):
//!
//! 1. the local session that admitted the request is still current, and no
//!    publication journal entry blocks the target (an unfinished create
//!    with the same name or from the same workcopy, P2-04D);
//! 2. the draft is the one previewed: `local_id`, `revision`, Base and
//!    Local fingerprints; the workcopy belongs to the requested connection;
//! 3. the target name is checked against visible dashboards — a hint only:
//!    another user's private dashboard with the name is invisible
//!    (P1-IN-04), so the server may still answer `already exists`;
//! 4. publication blockers: exact server version, live authority, every
//!    `dashboard.create` contract (`create_private_dashboard`,
//!    `create_copy_read_widgets`, `create_reconcile_unknown_outcome`, and
//!    `create_copy_unknown_widgets` for widgets the registry does not know),
//!    for a page draft no widget link to another page of Local
//!    (`scope_link_crossing`, side `provider_outside`), valid and not
//!    visibly taken name;
//! 5. the adapter builds the create bound to the live publication authority
//!    (endpoint, server identity, version, credential, write epoch).
//!
//! Every create contract is `Unsupported` until the P2-06A laboratory
//! contract passes, so step 4 blocks every real plan today.
//!
//! [`publish_save_as_copy_plan`] is the only send path: it repeats the
//! session, draft and authority checks right before send, opens the
//! publication journal entry (the target must still be free; the record
//! holds the [`CreateExpectation`], the durable reconciliation key), lets
//! the adapter re-check contracts and the name, commits `submitting` with
//! fsync, sends exactly one `dashboard.create`, reads back and appends the
//! outcome (state mapping in `create_dashboard/journaling.rs`).
//! `already exists` is the final `dashboard_name_taken`; an unknown outcome
//! is reconciled by reads only ([`reconcile_create`] after a restart);
//! nothing is ever retried or renamed.
//!
//! A `read_only` connection cannot plan at all:
//!
//! ```compile_fail
//! async fn read_only(
//!     service: &zdashstitch_application::workcopy::WorkcopyService,
//!     journal: &zdashstitch_application::PublishJournal,
//!     session: &zdashstitch_application::LocalSession,
//!     reader: &zdashstitch_zabbix::ReadOnlyGateway,
//!     request: &zdashstitch_application::SaveAsCopyRequest,
//! ) {
//!     let _ =
//!         zdashstitch_application::plan_save_as_copy(service, journal, session, reader, request)
//!             .await;
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
//!     request: &zdashstitch_application::SaveAsCopyRequest,
//! ) {
//!     let _ =
//!         zdashstitch_application::plan_save_as_copy(service, journal, session, writer, request)
//!             .await;
//! }
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;
use zdashstitch_zabbix::{
    CapabilityState, CreateExpectation, CreateReconciliation, CreateRejection, CreateRejectionCode,
    CreateScope, DashboardCreateOutcome, Failure, PublicationContext, ReadOnlyGateway,
    ReadWriteGateway, VerifiedDashboardCreate, VersionedDashboardSnapshot, create_requirements_for,
    is_valid_dashboard_name, verify_dashboard_create_for,
};

use crate::publish_journal::{JournalError, PublishJournal, TargetKey, TargetRef};
use crate::publisher::{
    CapabilityCheck, CompatibilityFinding, CompatibilityReport, FindingCode, FindingGroup,
    FindingSeverity, FindingSubject, JournalState, PlanInvalidReason, plan_nonce,
};
use crate::workcopy::formats::CreatedDashboardRecord;
use crate::workcopy::{DraftKind, WorkcopyError, WorkcopyService, now_ms};
use crate::{LocalSession, SessionGeneration, WriteStage};

mod journaling;

pub use journaling::{CreateRecovery, reconcile_create};

/// Intent code of this plan (ADR-006).
pub const SAVE_AS_COPY_OPERATION: &str = "save_as_copy";

/// What the user saw in the Save as copy preview. The plan is bound to
/// exactly this; any difference invalidates it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveAsCopyRequest {
    pub local_id: String,
    /// Connection profile of the active session.
    pub connection_id: String,
    /// Workcopy revision shown.
    pub revision: u64,
    /// Base fingerprint shown.
    pub base_fingerprint: String,
    /// Local fingerprint shown.
    pub local_fingerprint: String,
    /// New dashboard name typed by the user (user data).
    pub target_name: String,
    /// Generation the request was admitted under.
    pub session: SessionGeneration,
}

/// The name check before send. A hint, never a proof that the name is free.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NameCheck {
    /// No visible dashboard has the name; an invisible one may.
    NotVisible,
    /// A visible dashboard has the name: the plan is blocked.
    TakenVisible,
    /// The name is invalid, so it was not looked up.
    NotChecked,
}

impl NameCheck {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotVisible => "not_visible",
            Self::TakenVisible => "taken_visible",
            Self::NotChecked => "not_checked",
        }
    }
}

/// Why no create plan was built or a plan was dropped.
#[derive(Debug, Error)]
pub enum CreatePlanError {
    #[error("compatibility checks block publication")]
    Incompatible(CompatibilityReport),
    #[error("adapter validation rejected the create: {0}")]
    Adapter(CreateRejection),
    #[error("confirmation does not match the prepared plan")]
    ConfirmationMismatch,
    #[error("the plan inputs are no longer current: {}", .0.as_str())]
    Invalidated(PlanInvalidReason),
    #[error("{0}")]
    Workcopy(WorkcopyError),
    #[error("reading the server failed: {}", .0.code.as_str())]
    Read(Failure),
    /// The publication journal blocks the target or cannot be read
    /// (`target_blocked`, `journal_damaged`, storage codes).
    #[error("{0}")]
    Journal(JournalError),
}

/// What the plan was verified against; checked again right before send.
#[derive(Clone, Debug, Eq, PartialEq)]
struct CreateBinding {
    local_id: String,
    connection_id: String,
    revision: u64,
    base_fingerprint: String,
    local_fingerprint: String,
    source_dashboardid: String,
}

/// Trusted planner output: one `dashboard.create` of Local under a new
/// name, bound to the publication authority, the exact server version,
/// the create contracts, the previewed draft (revision, Base, Local) and
/// the local session generation.
///
/// The constructor is private to this module, the type is not `Clone` and
/// not deserializable, and confirmation consumes it (one-shot).
///
/// ```compile_fail
/// fn forge() -> zdashstitch_application::VerifiedCreatePlan {
///     zdashstitch_application::VerifiedCreatePlan { plan_id: String::new() }
/// }
/// ```
///
/// ```compile_fail
/// fn from_http(body: &str) -> zdashstitch_application::VerifiedCreatePlan {
///     serde_json::from_str(body).unwrap_or_else(|_| panic!())
/// }
/// ```
///
/// ```compile_fail
/// fn replay(
///     plan: &zdashstitch_application::VerifiedCreatePlan,
/// ) -> zdashstitch_application::VerifiedCreatePlan {
///     plan.clone()
/// }
/// ```
///
/// Control: reading the preview of a plan compiles.
///
/// ```no_run
/// fn shown(plan: &zdashstitch_application::VerifiedCreatePlan) -> String {
///     plan.preview().plan_id
/// }
/// ```
pub struct VerifiedCreatePlan {
    plan_id: String,
    created_at: SystemTime,
    create: VerifiedDashboardCreate,
    compatibility: CompatibilityReport,
    name_check: NameCheck,
    binding: CreateBinding,
    session: SessionGeneration,
}

/// A plan the user confirmed; consumed by the send.
pub struct ConfirmedCreatePlan {
    plan_id: String,
    created_at: SystemTime,
    confirmed_at: SystemTime,
    create: VerifiedDashboardCreate,
    compatibility: CompatibilityReport,
    binding: CreateBinding,
    session: SessionGeneration,
}

/// What the confirmation dialog shows (P1-08F); no payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatePlanPreview {
    pub plan_id: String,
    /// Always `save_as_copy`.
    pub operation: &'static str,
    /// Always `dashboard.create`.
    pub api_method: &'static str,
    /// Always 1: one plan, one write request.
    pub write_requests: u8,
    pub created_at_ms: u64,
    pub local_id: String,
    pub revision: u64,
    /// Source dashboard (server ID); never written.
    pub source_dashboardid: String,
    /// New dashboard name (user data).
    pub target_name: String,
    /// Always `true` until access writes are proven (P5-08, O-031).
    pub private: bool,
    /// Pages of the new dashboard: 1 for a page draft.
    pub page_count: usize,
    pub widget_count: usize,
    pub reference_definitions: usize,
    pub reference_consumers: usize,
    /// Widgets the registry does not know, copied as read.
    pub unknown_widgets: usize,
    /// Laboratory contracts the create relies on.
    pub contracts: Vec<&'static str>,
    pub base_fingerprint: String,
    pub local_fingerprint: String,
    /// Fingerprint the read-back and the reconciliation compare.
    pub content_fingerprint: String,
    pub name_check: NameCheck,
    pub compatibility: CompatibilityReport,
}

/// Result of a create send for the publication journal and the screens.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatePublishReport {
    pub plan_id: String,
    /// `None`: refused by the application before the adapter (see
    /// `invalidated` and `journal_error`); nothing was sent.
    pub outcome: Option<DashboardCreateOutcome>,
    pub invalidated: Option<PlanInvalidReason>,
    /// Recording the confirmed dashboard in the workcopy (O-022) failed;
    /// the outcome itself stands.
    pub record_error: Option<WorkcopyError>,
    /// The last state committed to the publication journal; `None` when no
    /// entry was opened.
    pub journal_state: Option<JournalState>,
    /// The journal refused the operation (`target_blocked`,
    /// `journal_damaged`, `journal_write_failed`, …; nothing was sent) or
    /// could not record the outcome (the entry then stays `submitting` and
    /// needs reconciliation).
    pub journal_error: Option<JournalError>,
}

impl CreatePublishReport {
    /// Whether `dashboard.create` was dispatched (0 or 1 times).
    #[must_use]
    pub fn sent(&self) -> bool {
        self.outcome
            .as_ref()
            .is_some_and(DashboardCreateOutcome::sent)
    }
}

/// A confirmed plan dropped by the checks right before send; nothing was
/// sent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateRefusal {
    pub plan_id: String,
    pub reason: PlanInvalidReason,
}

impl From<CreateRefusal> for CreatePublishReport {
    fn from(refusal: CreateRefusal) -> Self {
        Self {
            plan_id: refusal.plan_id,
            outcome: None,
            invalidated: Some(refusal.reason),
            record_error: None,
            journal_state: None,
            journal_error: None,
        }
    }
}

static PLAN_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |elapsed| {
        u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
    })
}

fn prefix(fingerprint: &str) -> &str {
    fingerprint.get(..12).unwrap_or(fingerprint)
}

impl VerifiedCreatePlan {
    fn new(
        create: VerifiedDashboardCreate,
        compatibility: CompatibilityReport,
        name_check: NameCheck,
        binding: CreateBinding,
        session: SessionGeneration,
    ) -> Self {
        let sequence = PLAN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let created_at = SystemTime::now();
        // The journal file name: unique across restarts (64 random bits,
        // as for update plans); a collision is refused by the journal.
        let plan_id = format!(
            "zds-create-{}-{}-{sequence:08x}-{:016x}",
            prefix(create.local_fingerprint()),
            prefix(create.content_fingerprint()),
            plan_nonce(created_at),
        );
        Self {
            plan_id,
            created_at,
            create,
            compatibility,
            name_check,
            binding,
            session,
        }
    }

    #[must_use]
    pub fn preview(&self) -> CreatePlanPreview {
        let requirements = self.create.requirements();
        CreatePlanPreview {
            plan_id: self.plan_id.clone(),
            operation: SAVE_AS_COPY_OPERATION,
            api_method: "dashboard.create",
            write_requests: 1,
            created_at_ms: millis(self.created_at),
            local_id: self.binding.local_id.clone(),
            revision: self.binding.revision,
            source_dashboardid: self.binding.source_dashboardid.clone(),
            target_name: self.create.name().to_owned(),
            private: true,
            page_count: requirements.page_count,
            widget_count: requirements.widget_count,
            reference_definitions: requirements.reference_definitions,
            reference_consumers: requirements.reference_consumers,
            unknown_widgets: requirements.unknown_widgets.len(),
            contracts: self
                .create
                .required_contracts()
                .iter()
                .map(|contract| contract.as_str())
                .collect(),
            base_fingerprint: self.binding.base_fingerprint.clone(),
            local_fingerprint: self.binding.local_fingerprint.clone(),
            content_fingerprint: self.create.content_fingerprint().to_owned(),
            name_check: self.name_check,
            compatibility: self.compatibility.clone(),
        }
    }

    /// Session generation the plan was admitted under.
    #[must_use]
    pub const fn session(&self) -> SessionGeneration {
        self.session
    }

    /// Consumes the plan when the user confirmed exactly the displayed
    /// `plan_id`; a mismatch drops the plan.
    pub fn confirm(self, displayed_plan_id: &str) -> Result<ConfirmedCreatePlan, CreatePlanError> {
        if displayed_plan_id != self.plan_id {
            return Err(CreatePlanError::ConfirmationMismatch);
        }
        Ok(ConfirmedCreatePlan {
            plan_id: self.plan_id,
            created_at: self.created_at,
            confirmed_at: SystemTime::now(),
            create: self.create,
            compatibility: self.compatibility,
            binding: self.binding,
            session: self.session,
        })
    }
}

impl ConfirmedCreatePlan {
    #[must_use]
    pub fn plan_id(&self) -> &str {
        &self.plan_id
    }

    /// Session generation to open the write stage under.
    #[must_use]
    pub const fn session(&self) -> SessionGeneration {
        self.session
    }

    fn refusal(self, reason: PlanInvalidReason) -> CreateRefusal {
        CreateRefusal {
            plan_id: self.plan_id,
            reason,
        }
    }

    fn refuse(self, reason: PlanInvalidReason) -> CreatePublishReport {
        self.refusal(reason).into()
    }
}

/// Facts gathered by [`plan_save_as_copy`]; verified by a pure function.
pub(crate) struct CreatePlanInputs<'a> {
    pub request: &'a SaveAsCopyRequest,
    pub kind: DraftKind,
    /// `dashboard_pageid` selected by a page draft.
    pub page: Option<&'a str>,
    /// Current workcopy revision.
    pub revision: u64,
    /// Current Base fingerprint.
    pub base_fingerprint: &'a str,
    /// Workcopy connection.
    pub connection_id: &'a str,
    /// Materialized Local.
    pub local: &'a VersionedDashboardSnapshot,
    pub name_check: NameCheck,
    pub context: &'a PublicationContext,
}

/// The journal target of a create of `name` from workcopy `local_id`.
pub(crate) fn create_target_key(
    connection_id: &str,
    endpoint_fingerprint: &str,
    name: &str,
    local_id: &str,
) -> TargetKey {
    TargetKey {
        connection_id: connection_id.to_owned(),
        endpoint_fingerprint: Some(endpoint_fingerprint.to_owned()),
        target: TargetRef::NewDashboard {
            name: name.to_owned(),
            source_local_id: Some(local_id.to_owned()),
        },
    }
}

/// Builds the Save as copy plan of a workcopy (steps in the module docs).
/// Reads only; never writes. No plan while the journal blocks the target
/// (`CreatePlanError::Journal`).
pub async fn plan_save_as_copy(
    service: &WorkcopyService,
    journal: &PublishJournal,
    session: &LocalSession,
    gateway: &ReadWriteGateway,
    request: &SaveAsCopyRequest,
) -> Result<VerifiedCreatePlan, CreatePlanError> {
    if !session.is_current(request.session) {
        return Err(CreatePlanError::Invalidated(
            PlanInvalidReason::SessionEnded,
        ));
    }
    journal
        .check_target(&create_target_key(
            &request.connection_id,
            gateway.reader().identity().endpoint_fingerprint(),
            &request.target_name,
            &request.local_id,
        ))
        .map_err(CreatePlanError::Journal)?;
    let file = service
        .load(&request.local_id)
        .map_err(CreatePlanError::Workcopy)?;
    let local = service
        .local_snapshot(&request.local_id)
        .map_err(CreatePlanError::Workcopy)?;
    let name_check = if is_valid_dashboard_name(&request.target_name) {
        let named = gateway
            .reader()
            .dashboards_named(local.connection_id(), &request.target_name)
            .await
            .map_err(|error| CreatePlanError::Read(error.failure()))?;
        if named.dashboards.is_empty() && !named.truncated {
            NameCheck::NotVisible
        } else {
            NameCheck::TakenVisible
        }
    } else {
        NameCheck::NotChecked
    };
    verify_save_as_copy_plan(&CreatePlanInputs {
        request,
        kind: file.kind,
        page: file
            .source
            .page
            .as_ref()
            .map(|page| page.dashboard_pageid.as_str()),
        revision: file.revision,
        base_fingerprint: &file.base.fingerprint,
        connection_id: &file.source.connection_id,
        local: &local,
        name_check,
        context: &gateway.publication_context(),
    })
}

fn check_draft(inputs: &CreatePlanInputs<'_>) -> Result<(), CreatePlanError> {
    let request = inputs.request;
    let invalid = |reason| Err(CreatePlanError::Invalidated(reason));
    if inputs.connection_id != request.connection_id
        || inputs.local.connection_id() != request.connection_id
    {
        return invalid(PlanInvalidReason::ConnectionChanged);
    }
    if inputs.revision != request.revision
        || inputs.base_fingerprint != request.base_fingerprint
        || inputs.local.fingerprint() != request.local_fingerprint
    {
        return invalid(PlanInvalidReason::DraftChanged);
    }
    Ok(())
}

fn blocker(code: FindingCode, group: FindingGroup) -> CompatibilityFinding {
    CompatibilityFinding::new(code, FindingSeverity::BlocksPublication, group)
}

fn supported(condition: bool) -> CapabilityState {
    if condition {
        CapabilityState::Supported
    } else {
        CapabilityState::Unsupported
    }
}

/// The pages of Local the draft copies; `None` for an inconsistent kind.
fn create_scope<'a>(inputs: &CreatePlanInputs<'a>) -> Option<CreateScope<'a>> {
    match (inputs.kind, inputs.page) {
        (DraftKind::Dashboard, None) => Some(CreateScope::Dashboard),
        (DraftKind::Page, Some(dashboard_pageid)) => Some(CreateScope::Page { dashboard_pageid }),
        _ => None,
    }
}

/// Capability checks and findings of a create (no prose; the UI renders
/// codes).
fn create_report(inputs: &CreatePlanInputs<'_>) -> Result<CompatibilityReport, CreatePlanError> {
    let capabilities = inputs.context.capabilities();
    let mut report = CompatibilityReport {
        checks: vec![
            CapabilityCheck {
                name: "exact_server_version",
                state: supported(capabilities.version == *inputs.local.server_version()),
                blocking: true,
            },
            CapabilityCheck {
                name: "authenticated_connection_and_principal_binding",
                state: supported(inputs.context.is_live_authenticated()),
                blocking: true,
            },
        ],
        findings: Vec::new(),
    };
    let scope = create_scope(inputs);
    if scope.is_none() {
        report.findings.push(
            blocker(FindingCode::DraftKindUnsupported, FindingGroup::Plan)
                .with_param("kind", inputs.kind.as_str()),
        );
    }
    match inputs.name_check {
        NameCheck::NotChecked => report.findings.push(blocker(
            FindingCode::DashboardNameInvalid,
            FindingGroup::Plan,
        )),
        NameCheck::TakenVisible => report
            .findings
            .push(blocker(FindingCode::DashboardNameTaken, FindingGroup::Plan)),
        NameCheck::NotVisible => {}
    }
    if let (Some(scope), true) = (scope, inputs.name_check != NameCheck::NotChecked) {
        let requirements =
            create_requirements_for(inputs.local, scope, &inputs.request.target_name)
                .map_err(CreatePlanError::Adapter)?;
        for link in &requirements.links_outside_page {
            report.findings.push(
                blocker(FindingCode::ScopeLinkCrossing, FindingGroup::Widgets)
                    .with_subject(FindingSubject {
                        page_index: link.page_index,
                        widget_index: link.widget_index,
                        widgetid: None,
                        field: Some(link.field.clone()),
                    })
                    .with_param("side", "provider_outside"),
            );
        }
        let contracts = &capabilities.dashboard_create_contracts;
        report.checks.extend(
            requirements
                .contracts
                .iter()
                .map(|contract| CapabilityCheck {
                    name: contract.as_str(),
                    state: contracts.state(*contract),
                    blocking: true,
                }),
        );
        if !contracts.is_verified(zdashstitch_zabbix::CreateContract::CopyUnknownWidgets) {
            for widget in &requirements.unknown_widgets {
                report.findings.push(
                    blocker(FindingCode::WriteContractUnverified, FindingGroup::Widgets)
                        .with_subject(FindingSubject {
                            page_index: widget.page_index,
                            widget_index: widget.widget_index,
                            widgetid: None,
                            field: None,
                        })
                        .with_param(
                            "contract",
                            zdashstitch_zabbix::CreateContract::CopyUnknownWidgets.as_str(),
                        )
                        .with_param("widget_type", widget.widget_type.clone())
                        .with_param("reason", widget.reason.as_str()),
                );
            }
        }
    }
    let unverified = report
        .checks
        .iter()
        .filter(|check| check.blocking && check.state != CapabilityState::Supported)
        .map(|check| {
            blocker(FindingCode::CapabilityNotVerified, FindingGroup::Connection)
                .with_param("capability", check.name)
        })
        .collect::<Vec<_>>();
    report.findings.extend(unverified);
    Ok(report)
}

/// Steps 2–5 of the module docs on gathered facts. Pure.
pub(crate) fn verify_save_as_copy_plan(
    inputs: &CreatePlanInputs<'_>,
) -> Result<VerifiedCreatePlan, CreatePlanError> {
    check_draft(inputs)?;
    let report = create_report(inputs)?;
    if !report.allows_publication() {
        return Err(CreatePlanError::Incompatible(report));
    }
    let scope = create_scope(inputs).ok_or(CreatePlanError::Invalidated(
        PlanInvalidReason::DraftChanged,
    ))?;
    let create = verify_dashboard_create_for(
        inputs.local,
        scope,
        inputs.context,
        &inputs.request.target_name,
    )
    .map_err(CreatePlanError::Adapter)?;
    if create.local_fingerprint() != inputs.request.local_fingerprint {
        return Err(CreatePlanError::Invalidated(
            PlanInvalidReason::DraftChanged,
        ));
    }
    let request = inputs.request;
    let binding = CreateBinding {
        local_id: request.local_id.clone(),
        connection_id: request.connection_id.clone(),
        revision: request.revision,
        base_fingerprint: request.base_fingerprint.clone(),
        local_fingerprint: request.local_fingerprint.clone(),
        source_dashboardid: inputs.local.dashboard_id().to_owned(),
    };
    Ok(VerifiedCreatePlan::new(
        create,
        report,
        inputs.name_check,
        binding,
        request.session,
    ))
}

/// A confirmed plan that passed every check right before send. Holds the
/// expectation the publication journal records before the send.
pub(crate) struct PreparedCreateSend {
    plan: ConfirmedCreatePlan,
    expectation: CreateExpectation,
}

impl PreparedCreateSend {
    #[must_use]
    pub(crate) fn plan_id(&self) -> &str {
        &self.plan.plan_id
    }

    /// What the created dashboard must look like; the durable key of a
    /// later read-only reconciliation.
    #[must_use]
    pub(crate) const fn expectation(&self) -> &CreateExpectation {
        &self.expectation
    }
}

/// The draft is still exactly the planned one.
fn draft_unchanged(service: &WorkcopyService, binding: &CreateBinding) -> bool {
    let Ok(file) = service.load(&binding.local_id) else {
        return false;
    };
    let Ok(local) = service.local_snapshot(&binding.local_id) else {
        return false;
    };
    file.revision == binding.revision
        && file.base.fingerprint == binding.base_fingerprint
        && file.source.connection_id == binding.connection_id
        && local.fingerprint() == binding.local_fingerprint
}

/// Checks right before send: session generation of `stage`, unchanged
/// draft (revision, Base, Local, connection), the same publication
/// authority and version. The journal records
/// [`PreparedCreateSend::expectation`] and then [`send_prepared_create`]
/// runs while the caller holds `stage`.
pub(crate) fn prepare_save_as_copy_send(
    service: &WorkcopyService,
    client: &ReadWriteGateway,
    plan: ConfirmedCreatePlan,
    stage: &WriteStage,
) -> Result<PreparedCreateSend, CreateRefusal> {
    if stage.generation() != plan.session {
        return Err(plan.refusal(PlanInvalidReason::SessionEnded));
    }
    if !draft_unchanged(service, &plan.binding) {
        return Err(plan.refusal(PlanInvalidReason::DraftChanged));
    }
    if client.publication_context().binding_fingerprint() != plan.create.authority_fingerprint()
        || client.capabilities().version != *plan.create.server_version()
    {
        return Err(plan.refusal(PlanInvalidReason::ConnectionChanged));
    }
    if plan.confirmed_at < plan.created_at || !plan.compatibility.allows_publication() {
        return Err(plan.refusal(PlanInvalidReason::TargetChanged));
    }
    let expectation = client.create_expectation(&plan.create);
    Ok(PreparedCreateSend { plan, expectation })
}

/// Sends a prepared create through the closed adapter path: contracts and
/// name re-checked, `before_send` (the journal's durable `submitting`
/// record; `false` sends nothing), exactly one `dashboard.create`, read-back
/// or read-only reconciliation. A confirmed dashboard is added to the
/// workcopy's list of created dashboards (O-022); that record never changes
/// the outcome. Crate-private: every send goes through the journal
/// ([`publish_save_as_copy_plan`]).
pub(crate) async fn send_prepared_create<G>(
    service: &WorkcopyService,
    client: &ReadWriteGateway,
    prepared: PreparedCreateSend,
    stage: &WriteStage,
    before_send: G,
) -> CreatePublishReport
where
    G: FnOnce() -> bool + Send,
{
    let PreparedCreateSend { plan, expectation } = prepared;
    if stage.generation() != plan.session {
        return plan.refuse(PlanInvalidReason::SessionEnded);
    }
    if !draft_unchanged(service, &plan.binding) {
        return plan.refuse(PlanInvalidReason::DraftChanged);
    }
    let outcome = client
        .create_verified_dashboard(&plan.create, before_send)
        .await;
    let record_error = outcome
        .confirmed_dashboard()
        .and_then(|(dashboardid, fingerprint)| {
            service
                .record_created_dashboard(
                    &plan.binding.local_id,
                    &expectation,
                    dashboardid,
                    fingerprint,
                )
                .err()
        });
    CreatePublishReport {
        plan_id: plan.plan_id,
        outcome: Some(outcome),
        invalidated: None,
        record_error,
        journal_state: None,
        journal_error: None,
    }
}

/// Sends a confirmed Save as copy plan (the only create send path). The
/// caller holds `stage`, opened under the plan's session generation, until
/// this returns with an outcome (including an unknown one).
///
/// 1. checks right before send (session, draft, authority, version);
/// 2. the journal opens the operation (`confirmed` on disk with the
///    reconciliation key) only when no entry blocks the target;
/// 3. the adapter re-checks contracts and the name and, right before the
///    one write request, the journal commits `submitting` with fsync —
///    when that fails nothing is sent;
/// 4. the outcome is appended (mapping in `journaling.rs`). If that append
///    fails, the entry stays `submitting` and needs reconciliation.
pub async fn publish_save_as_copy_plan(
    service: &WorkcopyService,
    client: &ReadWriteGateway,
    journal: &PublishJournal,
    plan: ConfirmedCreatePlan,
    stage: &WriteStage,
) -> CreatePublishReport {
    let prepared = match prepare_save_as_copy_send(service, client, plan, stage) {
        Ok(prepared) => prepared,
        Err(refusal) => return refusal.into(),
    };
    let identity = client.reader().identity();
    let authority = journaling::JournalAuthority {
        endpoint_fingerprint: identity.endpoint_fingerprint(),
        server_identity: identity.fingerprint(),
    };
    journaling::journaled_create(journal, prepared, &authority, async |prepared, handle| {
        send_prepared_create(service, client, prepared, stage, || handle.admit_send()).await
    })
    .await
}

/// Read-only reconciliation of a create with an unknown outcome or an
/// unverified read-back (P2-06D), also after a restart: by the reported
/// `dashboardid` when known, otherwise by exact name and content
/// fingerprint. Only `Confirmed` is a result; anything else stays
/// undetermined. Never sends a create.
pub async fn reconcile_save_as_copy(
    reader: &ReadOnlyGateway,
    expectation: &CreateExpectation,
    known_dashboardid: Option<&str>,
) -> CreateReconciliation {
    reader
        .reconcile_dashboard_create(expectation, known_dashboardid)
        .await
}

impl WorkcopyService {
    /// Adds a confirmed created dashboard to the workcopy (O-022). Idempotent
    /// per connection and `dashboardid`; a later confirmation of the same
    /// dashboard does not add a second row.
    pub fn record_created_dashboard(
        &self,
        local_id: &str,
        expectation: &CreateExpectation,
        dashboardid: &str,
        fingerprint: &str,
    ) -> Result<(), WorkcopyError> {
        if !zdashstitch_zabbix::is_zabbix_id(dashboardid)
            || !crate::workcopy::is_fingerprint(fingerprint)
        {
            return Err(WorkcopyError::new("created_dashboard_invalid"));
        }
        let mut file = self.load(local_id)?;
        if file.source.connection_id != expectation.connection_id {
            return Err(WorkcopyError::new("created_dashboard_invalid"));
        }
        if file.created_dashboards.iter().any(|record| {
            record.connection_id == expectation.connection_id && record.dashboardid == dashboardid
        }) {
            return Ok(());
        }
        let now = now_ms();
        file.created_dashboards.push(CreatedDashboardRecord {
            connection_id: expectation.connection_id.clone(),
            dashboardid: dashboardid.to_owned(),
            fingerprint: fingerprint.to_owned(),
            created_at_ms: now,
            name: expectation.name.clone(),
        });
        file.revision = file.revision.saturating_add(1);
        file.updated_at_ms = now.max(file.updated_at_ms);
        self.store.replace_workcopy(&file)
    }
}

impl CreatePlanError {
    /// Stable code of an adapter rejection, for screens.
    #[must_use]
    pub fn adapter_code(&self) -> Option<CreateRejectionCode> {
        match self {
            Self::Adapter(rejection) => Some(rejection.code()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
