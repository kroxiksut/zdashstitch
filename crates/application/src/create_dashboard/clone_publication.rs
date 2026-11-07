// SPDX-License-Identifier: MIT OR Apache-2.0

//! Publication preflight for a pinned Clone workcopy. The active connection
//! supplies read access, mode and the scoped own-role permission check.

use zdashstitch_zabbix::{AccessMode, CreateScope, verify_dashboard_create_for};

use super::{
    ClonePlanBinding, ConfirmedCreatePlan, CreateBinding, CreatePlanError, CreatePublishReport,
    NameCheck, VerifiedCreatePlan, blocker, create_report, create_target_key, prepare_create_send,
    publish_prepared_create, supported,
};
use crate::clone_dashboard::SavedCloneWorkcopy;
use crate::clone_preflight::CloneLocalPreview;
use crate::connections::ConnectionService;
use crate::publisher::{
    CapabilityCheck, CompatibilityReport, FindingCode, FindingGroup, PlanInvalidReason,
};
use crate::workcopy::{DraftKind, WorkcopyService};
use crate::{JournalState, LocalSession, PublishJournal, SessionGeneration, WriteStage};

const CREATE_PERMISSION: &str = "dashboard_create_permission_verified";

/// The exact pinned draft and acknowledgement the user previewed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClonePublicationRequest {
    pub local_id: String,
    pub connection_id: String,
    pub mapping_revision: u64,
    pub base_fingerprint: String,
    pub local_fingerprint: String,
    pub target_name: String,
    pub kept_digest: Option<String>,
    pub session: SessionGeneration,
}

/// Data for publication review. This type carries no write authority.
#[must_use]
pub struct ClonePublicationPreview {
    pub draft: CloneLocalPreview,
    pub compatibility: CompatibilityReport,
}

/// Revalidates a pinned Clone using the currently active connection and
/// common P2-06 capability checks, without sending any write or opening an
/// operation in the journal.
pub async fn preflight_clone_publication(
    service: &WorkcopyService,
    journal: &PublishJournal,
    session: &LocalSession,
    connections: &ConnectionService,
    request: &ClonePublicationRequest,
) -> Result<ClonePublicationPreview, CreatePlanError> {
    inspect_clone(service, journal, session, connections, request)
        .await
        .map(|(preview, _)| preview)
}

async fn inspect_clone(
    service: &WorkcopyService,
    journal: &PublishJournal,
    session: &LocalSession,
    connections: &ConnectionService,
    request: &ClonePublicationRequest,
) -> Result<(ClonePublicationPreview, SavedCloneWorkcopy), CreatePlanError> {
    let invalid = CreatePlanError::Invalidated;
    if !session.is_current(request.session) {
        return Err(invalid(PlanInvalidReason::SessionEnded));
    }
    let connection_changed = || invalid(PlanInvalidReason::ConnectionChanged);
    let (active_connection_id, _, mode) = connections.active().ok_or_else(connection_changed)?;
    let reader = connections.reader().ok_or_else(connection_changed)?;
    if active_connection_id != request.connection_id {
        return Err(connection_changed());
    }
    let branch = service
        .open_clone_workcopy(&request.local_id)
        .map_err(CreatePlanError::Workcopy)?;
    let intent = branch.draft().intent();
    let source = intent.source();
    if source.connection_id != request.connection_id
        || reader.identity().fingerprint() != intent.server_identity_fingerprint()
        || reader.capabilities().version != *branch.local().server_version()
    {
        return Err(connection_changed());
    }
    if intent.mapping_revision() != request.mapping_revision
        || source.base_fingerprint != request.base_fingerprint
        || branch.local().fingerprint() != request.local_fingerprint
    {
        return Err(invalid(PlanInvalidReason::DraftChanged));
    }
    if intent.target_name() != request.target_name {
        return Err(invalid(PlanInvalidReason::TargetChanged));
    }

    let local = service
        .revalidate_saved_clone(reader, &branch)
        .await
        .map_err(CreatePlanError::Workcopy)?;
    let permission = mode == AccessMode::ReadWrite
        && matches!(reader.dashboard_create_permission().await, Ok(true));
    if !session.is_current(request.session) {
        return Err(invalid(PlanInvalidReason::SessionEnded));
    }
    if !service
        .begin_clone_workcopy(intent.clone())
        .is_ok_and(|current| current.mapping() == branch.draft().mapping())
    {
        return Err(invalid(PlanInvalidReason::DraftChanged));
    }
    journal
        .check_target(&create_target_key(
            &request.connection_id,
            reader.identity().endpoint_fingerprint(),
            intent.target_name(),
            branch.local_id(),
        ))
        .map_err(CreatePlanError::Journal)?;

    let mut compatibility = create_report(
        local.local(),
        DraftKind::Dashboard,
        Some(CreateScope::Dashboard),
        intent.target_name(),
        NameCheck::NotVisible,
        reader.capabilities(),
        true,
    )?;
    if mode == AccessMode::ReadOnly {
        compatibility.findings.push(blocker(
            FindingCode::WriteModeRequired,
            FindingGroup::Connection,
        ));
    }
    if local.kept_digest() != request.kept_digest.as_deref() {
        let code = if request.kept_digest.is_none() {
            FindingCode::KeptOnSourceUnconfirmed
        } else {
            FindingCode::KeptOnSourceConfirmationStale
        };
        compatibility
            .findings
            .push(blocker(code, FindingGroup::Bindings));
    }
    compatibility.checks.push(CapabilityCheck {
        name: CREATE_PERMISSION,
        state: supported(permission),
        blocking: true,
    });
    if !permission {
        compatibility.findings.push(
            blocker(FindingCode::CapabilityNotVerified, FindingGroup::Connection)
                .with_param("capability", CREATE_PERMISSION),
        );
    }
    Ok((
        ClonePublicationPreview {
            draft: local,
            compatibility,
        },
        branch,
    ))
}

/// Builds the common opaque one-shot create plan only for a fresh,
/// compatible Clone with proven permission on the active connection.
pub async fn plan_clone_dashboard(
    service: &WorkcopyService,
    journal: &PublishJournal,
    session: &LocalSession,
    connections: &ConnectionService,
    request: &ClonePublicationRequest,
) -> Result<VerifiedCreatePlan, CreatePlanError> {
    let (preview, branch) = inspect_clone(service, journal, session, connections, request).await?;
    if !preview.compatibility.allows_publication() {
        return Err(CreatePlanError::Incompatible(preview.compatibility));
    }
    let client = connections.writer().ok_or(CreatePlanError::Invalidated(
        PlanInvalidReason::ConnectionChanged,
    ))?;
    let create = verify_dashboard_create_for(
        preview.draft.local(),
        CreateScope::Dashboard,
        &client.publication_context(),
        &request.target_name,
    )
    .map_err(CreatePlanError::Adapter)?;
    let binding = CreateBinding {
        local_id: request.local_id.clone(),
        connection_id: request.connection_id.clone(),
        revision: request.mapping_revision,
        base_fingerprint: request.base_fingerprint.clone(),
        local_fingerprint: request.local_fingerprint.clone(),
        source_dashboardid: branch.draft().intent().source().dashboardid.clone(),
        clone: Some(ClonePlanBinding {
            intent: branch.draft().intent().clone(),
            kept_digest: request.kept_digest.clone(),
        }),
    };
    Ok(VerifiedCreatePlan::new(
        create,
        preview.compatibility,
        NameCheck::NotVisible,
        binding,
        request.session,
    ))
}

/// Revalidates the confirmed Clone, then uses the P2-06 journal and sender.
/// The adapter checks the original and the current role again immediately
/// before journal admission. A durable verified outcome is projected to O-022.
pub async fn publish_clone_dashboard_plan(
    service: &WorkcopyService,
    connections: &ConnectionService,
    journal: &PublishJournal,
    plan: ConfirmedCreatePlan,
    stage: &WriteStage,
) -> CreatePublishReport {
    let Some(clone) = &plan.binding.clone else {
        return plan.refuse(PlanInvalidReason::TargetChanged);
    };
    let Some(client) = connections.writer().filter(|_| {
        connections
            .active()
            .is_some_and(|(id, _, _)| id == plan.binding.connection_id)
    }) else {
        return plan.refuse(PlanInvalidReason::ConnectionChanged);
    };
    let local_id = plan.binding.local_id.clone();
    let kept_digest = clone.kept_digest.clone();
    let Ok(branch) = service.open_clone_workcopy(&local_id) else {
        return plan.refuse(PlanInvalidReason::DraftChanged);
    };
    let prepared = match prepare_create_send(
        service,
        client,
        plan,
        stage,
        Some(branch.draft().base().clone()),
    ) {
        Ok(prepared) => prepared,
        Err(refusal) => return refusal.into(),
    };
    match service
        .revalidate_saved_clone(client.reader(), &branch)
        .await
    {
        Ok(preview) if preview.check_confirmation(kept_digest.as_deref()).is_ok() => {}
        Err(error) if error.code() == "clone_remote_changed" => {
            return prepared.plan.refuse(PlanInvalidReason::SourceChanged);
        }
        Err(error) if error.code() == "dashboard_name_taken" => {
            return prepared.plan.refuse(PlanInvalidReason::TargetChanged);
        }
        _ => return prepared.plan.refuse(PlanInvalidReason::BindingsChanged),
    }
    let mut report = publish_prepared_create(service, client, journal, prepared, stage).await;
    if report.journal_error.is_none()
        && matches!(
            report.journal_state,
            Some(JournalState::Verified | JournalState::ReconciledVerified)
        )
    {
        report.record_error = service
            .record_clone_read_back(journal, &report.plan_id)
            .err();
    }
    if report.sent() {
        report.clone_source = Some(service.check_clone_source(client.reader(), &local_id).await);
    }
    report
}
