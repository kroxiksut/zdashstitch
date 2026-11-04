// SPDX-License-Identifier: MIT OR Apache-2.0

//! Planning, confirmation and the closed dashboard publication boundary
//! (P0-07, P2-04B). Findings and reports are data; the UI renders codes.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;
use zdashstitch_zabbix::{
    CapabilityState, DashboardPublishOutcome, DashboardWriteContracts, Failure, FailureParam,
    PublicationContext, ReadWriteGateway, VerifiedDashboardUpdate, VersionedDashboardSnapshot,
    WidgetFieldReplacement, WriteRejection, WriteRequirements, verify_widget_field_replacements,
    write_requirements,
};

use crate::publish_journal::{
    JournalError, JournalSource, JournalTarget, OperationKind, OperationRecord, PublishJournal,
    TargetKey,
};
use crate::resolver::ReasonCode;
use crate::workcopy::WorkcopyError;
use crate::{SessionGeneration, WriteStage};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityCheck {
    pub name: &'static str,
    pub state: CapabilityState,
    pub blocking: bool,
}

/// Whether a finding stops publication.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FindingSeverity {
    /// No write plan is built while the finding remains (P1-08C «блокирует»).
    BlocksPublication,
    /// Shown and reviewed, never blocks (P1-08C «предупреждение»).
    Informational,
}

impl FindingSeverity {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BlocksPublication => "blocks_publication",
            Self::Informational => "informational",
        }
    }
}

/// Where the user fixes a finding (P1-08C «исправляется: …»).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FindingGroup {
    /// Connection, access mode, version, write contracts.
    Connection,
    /// Freshness of the calculation against the server.
    Source,
    /// Matches and decisions.
    Bindings,
    /// What stays unchanged and why.
    Widgets,
    /// The plan as a whole.
    Plan,
}

impl FindingGroup {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Connection => "connection",
            Self::Source => "source",
            Self::Bindings => "bindings",
            Self::Widgets => "widgets",
            Self::Plan => "plan",
        }
    }
}

/// Stable finding codes; the UI maps them to message IDs.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FindingCode {
    /// The plan changes nothing.
    NoChanges,
    /// A required capability check is not `supported` (param `capability`).
    CapabilityNotVerified,
    /// No connection: nothing about the server can be checked.
    ConnectionRequired,
    /// The session is `read_only`.
    WriteModeRequired,
    /// The connected server version differs from the Base snapshot.
    ServerVersionMismatch,
    /// A field the operation changes has no proven laboratory write
    /// contract (param `contract`).
    WriteContractUnverified,
    /// The target host is missing or hidden.
    TargetHostNotVisible,
    /// A changing reference has no applicable target and no decision
    /// (`ambiguous`, `missing`, `unresolved_lld`, `denied`, blocking reason).
    BindingUnresolved,
    /// A reference in scope whose owner cannot be read (hidden object,
    /// prototype): it may point at the source host.
    SourceUnverified,
    /// A stored decision no longer fits its row.
    DecisionInvalid,
    /// A fresh check found another result than the stored calculation.
    BindingStale,
    /// A fresh check found the stored target or choice no longer visible.
    BindingInvalid,
    /// The resolved target differs from the source (units, value type,
    /// origin, pattern coverage); a warning (P1-05D `compatible`).
    MatchDifferences,
    /// The reference stays on the source host (O-003).
    KeptOnSource,
    /// The list of references kept on the source host is not confirmed.
    KeptOnSourceUnconfirmed,
    /// The confirmed list differs from the current one.
    KeptOnSourceConfirmationStale,
    /// References of the source host outside the scope (param `count`).
    ReferencesOutsideScope,
    /// A widget link crosses the scope boundary (param `side`).
    ScopeLinkCrossing,
    /// The account has no write permission on the target dashboard
    /// (`dashboard.get` with `editable`).
    DashboardNotEditable,
    /// A dashboard with the target name is visible (P2-06C). An invisible
    /// one is only found by the server's `already exists`.
    DashboardNameTaken,
    /// The target name is empty, too long, padded or has control
    /// characters.
    DashboardNameInvalid,
    /// The draft kind cannot be saved as a new dashboard (param `kind`).
    DraftKindUnsupported,
}

impl FindingCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoChanges => "no_changes",
            Self::CapabilityNotVerified => "capability_not_verified",
            Self::ConnectionRequired => "connection_required",
            Self::WriteModeRequired => "write_mode_required",
            Self::ServerVersionMismatch => "server_version_mismatch",
            Self::WriteContractUnverified => "write_contract_unverified",
            Self::TargetHostNotVisible => "target_host_not_visible",
            Self::BindingUnresolved => "binding_unresolved",
            Self::SourceUnverified => "source_unverified",
            Self::DecisionInvalid => "decision_invalid",
            Self::BindingStale => "binding_stale",
            Self::BindingInvalid => "binding_invalid",
            Self::MatchDifferences => "match_differences",
            Self::KeptOnSource => "kept_on_source",
            Self::KeptOnSourceUnconfirmed => "kept_on_source_unconfirmed",
            Self::KeptOnSourceConfirmationStale => "kept_on_source_confirmation_stale",
            Self::ReferencesOutsideScope => "references_outside_scope",
            Self::ScopeLinkCrossing => "scope_link_crossing",
            Self::DashboardNotEditable => "dashboard_not_editable",
            Self::DashboardNameTaken => "dashboard_name_taken",
            Self::DashboardNameInvalid => "dashboard_name_invalid",
            Self::DraftKindUnsupported => "draft_kind_unsupported",
        }
    }
}

/// The widget field a finding is about (Base positions plus server IDs).
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FindingSubject {
    pub page_index: usize,
    pub widget_index: usize,
    pub widgetid: Option<String>,
    /// Zabbix field name; `None` for the widget as a whole.
    pub field: Option<String>,
}

/// One machine-readable result of the publication checks. No prose: the
/// UI renders `code`, `reason` and `params`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompatibilityFinding {
    pub code: FindingCode,
    pub severity: FindingSeverity,
    pub group: FindingGroup,
    pub subject: Option<FindingSubject>,
    /// Resolver reason behind the finding (P1-IN-03), when there is one.
    pub reason: Option<ReasonCode>,
    /// Safe parameters: codes, counts, IDs; never secrets.
    pub params: Vec<FailureParam>,
}

impl CompatibilityFinding {
    #[must_use]
    pub const fn new(code: FindingCode, severity: FindingSeverity, group: FindingGroup) -> Self {
        Self {
            code,
            severity,
            group,
            subject: None,
            reason: None,
            params: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_subject(mut self, subject: FindingSubject) -> Self {
        self.subject = Some(subject);
        self
    }

    #[must_use]
    pub const fn with_reason(mut self, reason: Option<ReasonCode>) -> Self {
        self.reason = reason;
        self
    }

    #[must_use]
    pub fn with_param(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.params.push(FailureParam {
            name,
            value: value.into(),
        });
        self
    }

    #[must_use]
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|param| param.name == name)
            .map(|param| param.value.as_str())
    }

    #[must_use]
    pub fn blocks_publication(&self) -> bool {
        self.severity == FindingSeverity::BlocksPublication
    }
}

/// Capability checks plus findings. Publication is allowed only when every
/// blocking check is `supported` and no finding blocks; informational
/// findings never block. Local diff and preview never depend on it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompatibilityReport {
    pub checks: Vec<CapabilityCheck>,
    pub findings: Vec<CompatibilityFinding>,
}

impl CompatibilityReport {
    pub fn allows_publication(&self) -> bool {
        self.findings
            .iter()
            .all(|finding| !finding.blocks_publication())
            && self
                .checks
                .iter()
                .filter(|check| check.blocking)
                .all(|check| check.state == CapabilityState::Supported)
    }

    /// Findings that stop publication.
    pub fn blockers(&self) -> impl Iterator<Item = &CompatibilityFinding> {
        self.findings
            .iter()
            .filter(|finding| finding.blocks_publication())
    }

    /// Findings with `code`.
    pub fn with_code(&self, code: FindingCode) -> impl Iterator<Item = &CompatibilityFinding> {
        self.findings
            .iter()
            .filter(move |finding| finding.code == code)
    }
}

/// Why a plan stopped being valid (P1-01G §6). The UI maps each code to
/// `publication.plan.reason.<code>`; `session_ended` is shown by the
/// session-ended screen instead.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PlanInvalidReason {
    /// The draft changed after preview (revision, Local or Base).
    DraftChanged,
    /// The target dashboard or the action changed.
    TargetChanged,
    /// Connection, credentials, server identity, version or write mode
    /// changed.
    ConnectionChanged,
    /// Decisions or the list kept on the source host changed.
    BindingsChanged,
    /// The dashboard on the server differs from Base or is gone.
    SourceChanged,
    /// The application restarted; plans live only in memory.
    AppRestarted,
    /// The local session the plan was admitted under ended.
    SessionEnded,
}

impl PlanInvalidReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DraftChanged => "draft_changed",
            Self::TargetChanged => "target_changed",
            Self::ConnectionChanged => "connection_changed",
            Self::BindingsChanged => "bindings_changed",
            Self::SourceChanged => "source_changed",
            Self::AppRestarted => "app_restarted",
            Self::SessionEnded => "session_ended",
        }
    }
}

/// What a workcopy-based plan was verified against; checked again right
/// before send.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkcopyBinding {
    pub local_id: String,
    pub connection_id: String,
    pub revision: u64,
    pub base_fingerprint: String,
    pub local_fingerprint: String,
    /// Digest of the confirmed kept list; `None` when the list is empty.
    pub kept_digest: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PlanOrigin {
    /// P0-07: explicit replacements on a read snapshot.
    Direct,
    /// P2-04B: the ReplaceHost changes of a workcopy.
    ReplaceHost(WorkcopyBinding),
}

impl PlanOrigin {
    const fn operation(&self) -> &'static str {
        match self {
            Self::Direct => "replace_item_fields",
            Self::ReplaceHost(_) => "replace_host",
        }
    }
}

/// Trusted planner output: one `dashboard.update` covering N field changes
/// on M pages, bound to the publication authority (endpoint, server
/// identity, version, credential, write epoch), the re-read source snapshot,
/// the exact contracts it needs, the local session generation and, for a
/// workcopy plan, the previewed revision, Local, Base and kept list.
///
/// The constructor is private to this crate, the type is not `Clone` and
/// not deserializable, and confirmation consumes it (one-shot).
///
/// ```compile_fail
/// fn forge() -> zdashstitch_application::VerifiedPlan {
///     zdashstitch_application::VerifiedPlan { plan_id: String::new() }
/// }
/// ```
///
/// ```compile_fail
/// fn from_http(body: &str) -> zdashstitch_application::VerifiedPlan {
///     serde_json::from_str(body).unwrap_or_else(|_| panic!())
/// }
/// ```
///
/// ```compile_fail
/// fn replay(plan: &zdashstitch_application::VerifiedPlan) -> zdashstitch_application::VerifiedPlan {
///     plan.clone()
/// }
/// ```
///
/// Control: reading the preview of a plan compiles.
///
/// ```no_run
/// fn shown(plan: &zdashstitch_application::VerifiedPlan) -> String {
///     plan.preview().plan_id
/// }
/// ```
pub struct VerifiedPlan {
    plan_id: String,
    created_at: SystemTime,
    update: VerifiedDashboardUpdate,
    compatibility: CompatibilityReport,
    origin: PlanOrigin,
    session: SessionGeneration,
}

pub struct ConfirmedPlan {
    plan_id: String,
    created_at: SystemTime,
    confirmed_at: SystemTime,
    update: VerifiedDashboardUpdate,
    compatibility: CompatibilityReport,
    origin: PlanOrigin,
    session: SessionGeneration,
}

/// What the confirmation dialog shows (P1-08F); no payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanPreview {
    pub plan_id: String,
    /// `replace_item_fields` (P0-07) or `replace_host`.
    pub operation: &'static str,
    /// Always `dashboard.update`.
    pub api_method: &'static str,
    /// Always 1: one plan, one write request.
    pub write_requests: u8,
    pub created_at_ms: u64,
    pub dashboard_id: String,
    pub replacement_count: usize,
    /// Page positions sent with full widgets.
    pub changed_pages: Vec<usize>,
    /// Laboratory contracts the update relies on.
    pub contracts: Vec<&'static str>,
    /// Replacement count per field-kind contract.
    pub replacement_kinds: Vec<(&'static str, usize)>,
    /// Remote fingerprint at plan time (equals Base for a workcopy plan).
    pub source_fingerprint: String,
    /// Fingerprint the read-back must show (equals Local).
    pub expected_fingerprint: String,
    pub local_id: Option<String>,
    pub revision: Option<u64>,
    /// Digest of the confirmed «останутся на старом узле» list.
    pub kept_digest: Option<String>,
    pub compatibility: CompatibilityReport,
}

/// States of a publication (ADR-012 D-07, P1-08G). `Planned` and
/// `Previewed` exist only in memory; the durable journal starts at
/// `Confirmed` (see [`crate::PublishJournal`]).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum JournalState {
    Planned,
    Previewed,
    Confirmed,
    Submitting,
    Verified,
    Conflict,
    FailedBeforeSend,
    FailedAfterResponse,
    UnknownOutcome,
    ReconciledVerified,
    ReconciledFailed,
    /// Reconciliation found neither the expected nor the source state
    /// («Нужна проверка вручную»); the target stays blocked.
    ManualCheckRequired,
    /// «Закрыть без сверки» (P1-01G §7): closed by the user without a
    /// checked result; unblocks the target, never claims success.
    ClosedManually,
}

impl JournalState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Previewed => "previewed",
            Self::Confirmed => "confirmed",
            Self::Submitting => "submitting",
            Self::Verified => "verified",
            Self::Conflict => "conflict",
            Self::FailedBeforeSend => "failed_before_send",
            Self::FailedAfterResponse => "failed_after_response",
            Self::UnknownOutcome => "unknown_outcome",
            Self::ReconciledVerified => "reconciled_verified",
            Self::ReconciledFailed => "reconciled_failed",
            Self::ManualCheckRequired => "manual_check_required",
            Self::ClosedManually => "closed_manually",
        }
    }

    /// States of the durable journal; the in-memory `planned` and
    /// `previewed` are never read back.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Some(match code {
            "confirmed" => Self::Confirmed,
            "submitting" => Self::Submitting,
            "verified" => Self::Verified,
            "conflict" => Self::Conflict,
            "failed_before_send" => Self::FailedBeforeSend,
            "failed_after_response" => Self::FailedAfterResponse,
            "unknown_outcome" => Self::UnknownOutcome,
            "reconciled_verified" => Self::ReconciledVerified,
            "reconciled_failed" => Self::ReconciledFailed,
            "manual_check_required" => Self::ManualCheckRequired,
            "closed_manually" => Self::ClosedManually,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishReport {
    pub plan_id: String,
    pub states: Vec<JournalState>,
    pub deployment_state_may_advance: bool,
    /// Why the plan was refused before send, or found stale (`Conflict`).
    pub invalidated: Option<PlanInvalidReason>,
    /// The journal refused the send (`target_blocked`, `journal_damaged`,
    /// `journal_write_failed`, …) or could not record the outcome (the
    /// entry then stays `submitting` and needs reconciliation).
    pub journal_error: Option<JournalError>,
}

#[derive(Debug, Error)]
pub enum PlanError {
    #[error("compatibility checks block publication")]
    Incompatible(CompatibilityReport),
    #[error("adapter validation rejected the plan: {0}")]
    Adapter(WriteRejection),
    #[error("confirmation does not match the prepared plan")]
    ConfirmationMismatch,
    #[error("the plan inputs are no longer current: {}", .0.as_str())]
    Invalidated(PlanInvalidReason),
    #[error("{0}")]
    Workcopy(WorkcopyError),
    #[error("re-reading the server failed: {}", .0.code.as_str())]
    Read(Failure),
    /// The publication journal blocks the target or cannot be read
    /// (`target_blocked`, `journal_damaged`, storage codes).
    #[error("{0}")]
    Journal(JournalError),
    /// The update the adapter built does not reproduce Local.
    #[error("the verified update differs from the local draft")]
    LocalMismatch,
}

static PLAN_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// 64 random bits, so a `plan_id` (the journal file name) is never reused
/// after a restart. Uniqueness is all it needs: without a random source the
/// clock is used, and a collision is refused by the journal (`create_new`).
pub(crate) fn plan_nonce(created_at: SystemTime) -> u64 {
    let mut bytes = [0u8; 8];
    if getrandom::fill(&mut bytes).is_ok() {
        return u64::from_be_bytes(bytes);
    }
    created_at.duration_since(UNIX_EPOCH).map_or(0, |elapsed| {
        u64::try_from(elapsed.as_nanos() & u128::from(u64::MAX)).unwrap_or(0)
    })
}

fn millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |elapsed| {
        u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
    })
}

impl VerifiedPlan {
    /// The only constructor; used by the trusted planners of this crate.
    fn new(
        update: VerifiedDashboardUpdate,
        compatibility: CompatibilityReport,
        origin: PlanOrigin,
        session: SessionGeneration,
    ) -> Self {
        let sequence = PLAN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let created_at = SystemTime::now();
        let plan_id = format!(
            "zds-{}-{}-{sequence:08x}-{:016x}",
            &update.source_fingerprint()[..12],
            &update.expected_fingerprint()[..12],
            plan_nonce(created_at),
        );
        Self {
            plan_id,
            created_at,
            update,
            compatibility,
            origin,
            session,
        }
    }

    pub(crate) fn for_workcopy(
        update: VerifiedDashboardUpdate,
        compatibility: CompatibilityReport,
        binding: WorkcopyBinding,
        session: SessionGeneration,
    ) -> Self {
        Self::new(
            update,
            compatibility,
            PlanOrigin::ReplaceHost(binding),
            session,
        )
    }

    pub fn preview(&self) -> PlanPreview {
        let binding = match &self.origin {
            PlanOrigin::ReplaceHost(binding) => Some(binding),
            PlanOrigin::Direct => None,
        };
        PlanPreview {
            plan_id: self.plan_id.clone(),
            operation: self.origin.operation(),
            api_method: "dashboard.update",
            write_requests: 1,
            created_at_ms: millis(self.created_at),
            dashboard_id: self.update.dashboard_id().to_owned(),
            replacement_count: self.update.replacement_count(),
            changed_pages: self.update.changed_pages().to_vec(),
            contracts: self
                .update
                .required_contracts()
                .iter()
                .map(|contract| contract.as_str())
                .collect(),
            replacement_kinds: self
                .update
                .replacement_kinds()
                .iter()
                .map(|(contract, count)| (contract.as_str(), *count))
                .collect(),
            source_fingerprint: self.update.source_fingerprint().to_owned(),
            expected_fingerprint: self.update.expected_fingerprint().to_owned(),
            local_id: binding.map(|binding| binding.local_id.clone()),
            revision: binding.map(|binding| binding.revision),
            kept_digest: binding.and_then(|binding| binding.kept_digest.clone()),
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
    pub fn confirm(self, displayed_plan_id: &str) -> Result<ConfirmedPlan, PlanError> {
        if displayed_plan_id != self.plan_id {
            return Err(PlanError::ConfirmationMismatch);
        }
        Ok(ConfirmedPlan {
            plan_id: self.plan_id,
            created_at: self.created_at,
            confirmed_at: SystemTime::now(),
            update: self.update,
            compatibility: self.compatibility,
            origin: self.origin,
            session: self.session,
        })
    }
}

impl ConfirmedPlan {
    pub fn plan_id(&self) -> &str {
        &self.plan_id
    }

    /// Session generation to open the write stage under.
    #[must_use]
    pub const fn session(&self) -> SessionGeneration {
        self.session
    }

    pub(crate) fn workcopy_binding(&self) -> Option<&WorkcopyBinding> {
        match &self.origin {
            PlanOrigin::ReplaceHost(binding) => Some(binding),
            PlanOrigin::Direct => None,
        }
    }

    pub(crate) fn refuse(self, reason: Option<PlanInvalidReason>) -> PublishReport {
        refused(self.plan_id, reason)
    }
}

const fn pre_send_states() -> [JournalState; 3] {
    [
        JournalState::Planned,
        JournalState::Previewed,
        JournalState::Confirmed,
    ]
}

fn refused(plan_id: String, reason: Option<PlanInvalidReason>) -> PublishReport {
    let mut states = pre_send_states().to_vec();
    states.push(JournalState::FailedBeforeSend);
    PublishReport {
        plan_id,
        states,
        deployment_state_may_advance: false,
        invalidated: reason,
        journal_error: None,
    }
}

/// P0-07 plan: explicit replacements on a freshly read snapshot. Workcopy
/// operations use [`crate::plan_replace_host`]. No plan while the journal
/// blocks the dashboard (`PlanError::Journal`).
pub fn build_verified_plan(
    journal: &PublishJournal,
    snapshot: &VersionedDashboardSnapshot,
    context: &PublicationContext,
    replacements: &[WidgetFieldReplacement],
    session: SessionGeneration,
) -> Result<VerifiedPlan, PlanError> {
    journal
        .check_target(&TargetKey::dashboard(
            snapshot.connection_id(),
            None,
            snapshot.dashboard_id(),
        ))
        .map_err(PlanError::Journal)?;
    let requirements = write_requirements(snapshot, replacements).map_err(PlanError::Adapter)?;
    let compatibility = compatibility_report(snapshot, context, &requirements);
    if !compatibility.allows_publication() {
        return Err(PlanError::Incompatible(compatibility));
    }
    let update = verify_widget_field_replacements(snapshot, context, replacements)
        .map_err(PlanError::Adapter)?;
    Ok(VerifiedPlan::new(
        update,
        compatibility,
        PlanOrigin::Direct,
        session,
    ))
}

/// Sends a confirmed P0-07 plan. The caller holds `stage`, opened under the
/// plan's session generation, from before the write request until this
/// returns with an outcome (including an unknown one), so «End session» is
/// refused for the whole write stage (P2-SEC-01E). A workcopy plan is
/// refused here: it must go through [`crate::publish_replace_host_plan`],
/// which re-checks the draft.
pub async fn publish_confirmed_plan(
    client: &ReadWriteGateway,
    journal: &PublishJournal,
    plan: ConfirmedPlan,
    stage: &WriteStage,
) -> PublishReport {
    if plan.workcopy_binding().is_some() {
        return plan.refuse(None);
    }
    send_confirmed(client, journal, plan, stage).await
}

/// Checks shared by every plan right before send, then the journal and the
/// closed adapter path:
///
/// 1. session generation, publication authority, server version, report;
/// 2. the journal opens the operation (`confirmed` on disk) only when no
///    entry blocks the target;
/// 3. the adapter re-reads Remote (`Remote != source` → `Conflict`, zero
///    write) and, right before the one write request, the journal commits
///    `submitting` with fsync — when that fails nothing is sent;
/// 4. the outcome is appended. If that append fails, the entry stays
///    `submitting` and needs reconciliation: never a silent success.
pub(crate) async fn send_confirmed(
    client: &ReadWriteGateway,
    journal: &PublishJournal,
    plan: ConfirmedPlan,
    stage: &WriteStage,
) -> PublishReport {
    if stage.generation() != plan.session {
        return plan.refuse(Some(PlanInvalidReason::SessionEnded));
    }
    if client.publication_context().binding_fingerprint() != plan.update.authority_fingerprint()
        || client.capabilities().version != *plan.update.server_version()
    {
        return plan.refuse(Some(PlanInvalidReason::ConnectionChanged));
    }
    if plan.confirmed_at < plan.created_at || !plan.compatibility.allows_publication() {
        return plan.refuse(None);
    }
    let record = operation_record(client, &plan);
    let mut handle = match journal.begin(&plan.plan_id, &record) {
        Ok(handle) => handle,
        Err(error) => {
            let mut report = plan.refuse(None);
            report.journal_error = Some(error);
            return report;
        }
    };
    let ConfirmedPlan {
        plan_id, update, ..
    } = plan;
    let outcome = client
        .publish_verified_dashboard_update(&update, || handle.admit_send())
        .await;
    // A refused submit record leaves the file at `confirmed` or with an
    // unconfirmed `submitting`; nothing more is appended to it.
    let journal_error = match handle.take_submit_error() {
        Some(error) => Some(error),
        None => handle.finish(journal_state(outcome), None).err(),
    };
    let mut report = finish_report(plan_id, pre_send_states().to_vec(), outcome);
    report.journal_error = journal_error;
    report
}

/// The first journal line of an update plan.
fn operation_record(client: &ReadWriteGateway, plan: &ConfirmedPlan) -> OperationRecord {
    let identity = client.reader().identity();
    OperationRecord {
        kind: OperationKind::Update,
        intent: plan.origin.operation(),
        connection_id: plan.update.connection_id().to_owned(),
        server_version: plan.update.server_version().to_string(),
        endpoint_fingerprint: identity.endpoint_fingerprint().to_owned(),
        server_identity: identity.fingerprint().to_owned(),
        target: JournalTarget::ExistingDashboard {
            dashboardid: plan.update.dashboard_id().to_owned(),
            name: None,
        },
        source: plan.workcopy_binding().map(|binding| JournalSource {
            local_id: binding.local_id.clone(),
            revision: binding.revision,
        }),
        source_fingerprint: plan.update.source_fingerprint().to_owned(),
        expected_fingerprint: plan.update.expected_fingerprint().to_owned(),
        owner_userid: None,
        plan_created_at_ms: millis(plan.created_at),
        confirmed_at_ms: millis(plan.confirmed_at),
    }
}

/// The journal state of an adapter outcome.
const fn journal_state(outcome: DashboardPublishOutcome) -> JournalState {
    match outcome {
        DashboardPublishOutcome::Conflict => JournalState::Conflict,
        DashboardPublishOutcome::Verified => JournalState::Verified,
        DashboardPublishOutcome::FailedBeforeSend => JournalState::FailedBeforeSend,
        DashboardPublishOutcome::FailedAfterResponse => JournalState::FailedAfterResponse,
        DashboardPublishOutcome::UnknownOutcome => JournalState::UnknownOutcome,
        DashboardPublishOutcome::ReconciledVerified => JournalState::ReconciledVerified,
        DashboardPublishOutcome::ReconciledFailed => JournalState::ReconciledFailed,
    }
}

fn finish_report(
    plan_id: String,
    mut states: Vec<JournalState>,
    outcome: DashboardPublishOutcome,
) -> PublishReport {
    let (state, sent, deployment_state_may_advance) = match outcome {
        DashboardPublishOutcome::Conflict => (JournalState::Conflict, false, false),
        DashboardPublishOutcome::Verified => (JournalState::Verified, true, true),
        DashboardPublishOutcome::FailedBeforeSend => (JournalState::FailedBeforeSend, false, false),
        DashboardPublishOutcome::FailedAfterResponse => {
            (JournalState::FailedAfterResponse, true, false)
        }
        DashboardPublishOutcome::UnknownOutcome => (JournalState::UnknownOutcome, true, false),
        DashboardPublishOutcome::ReconciledVerified => {
            (JournalState::ReconciledVerified, true, true)
        }
        DashboardPublishOutcome::ReconciledFailed => (JournalState::ReconciledFailed, true, false),
    };
    if sent {
        states.push(JournalState::Submitting);
    }
    states.push(state);
    PublishReport {
        plan_id,
        states,
        deployment_state_may_advance,
        invalidated: (outcome == DashboardPublishOutcome::Conflict)
            .then_some(PlanInvalidReason::SourceChanged),
        journal_error: None,
    }
}

/// `write_contract_unverified` findings for the gaps of `requirements`,
/// skipping contracts `report` already lists.
pub(crate) fn requirement_findings(
    requirements: &WriteRequirements,
    contracts: &DashboardWriteContracts,
    report: &CompatibilityReport,
) -> Vec<CompatibilityFinding> {
    let listed = |name: &str| {
        report
            .with_code(FindingCode::WriteContractUnverified)
            .any(|finding| finding.param("contract") == Some(name))
    };
    let mut findings = Vec::new();
    for contract in requirements.missing_contracts(contracts) {
        if !listed(contract.as_str()) {
            findings.push(
                CompatibilityFinding::new(
                    FindingCode::WriteContractUnverified,
                    FindingSeverity::BlocksPublication,
                    FindingGroup::Connection,
                )
                .with_param("contract", contract.as_str()),
            );
        }
    }
    for widget in &requirements.unproven_widgets {
        findings.push(
            CompatibilityFinding::new(
                FindingCode::WriteContractUnverified,
                FindingSeverity::BlocksPublication,
                FindingGroup::Widgets,
            )
            .with_subject(FindingSubject {
                page_index: widget.page_index,
                widget_index: widget.widget_index,
                widgetid: None,
                field: None,
            })
            .with_param("contract", CONTRACT_RESEND_WIDGET_TYPE)
            .with_param("widget_type", widget.widget_type.clone()),
        );
    }
    findings
}

/// Contract code of a widget on a changed page whose full resend is not
/// proven (param `widget_type`).
pub const CONTRACT_RESEND_WIDGET_TYPE: &str = "resend_widget_type";

fn compatibility_report(
    snapshot: &VersionedDashboardSnapshot,
    context: &PublicationContext,
    requirements: &WriteRequirements,
) -> CompatibilityReport {
    let capabilities = context.capabilities();
    let contracts = &capabilities.dashboard_write_contracts;
    let mut checks = vec![
        CapabilityCheck {
            name: "exact_server_version",
            state: if capabilities.version == *snapshot.server_version() {
                CapabilityState::Supported
            } else {
                CapabilityState::Unsupported
            },
            blocking: true,
        },
        CapabilityCheck {
            name: "authenticated_connection_and_principal_binding",
            state: if context.is_live_authenticated() {
                CapabilityState::Supported
            } else {
                CapabilityState::Unsupported
            },
            blocking: true,
        },
    ];
    checks.extend(
        requirements
            .contracts
            .iter()
            .map(|contract| CapabilityCheck {
                name: contract.as_str(),
                state: contracts.state(*contract),
                blocking: true,
            }),
    );
    let mut report = CompatibilityReport {
        checks,
        findings: Vec::new(),
    };
    for check in &report.checks {
        if check.blocking && check.state != CapabilityState::Supported {
            report.findings.push(
                CompatibilityFinding::new(
                    FindingCode::CapabilityNotVerified,
                    FindingSeverity::BlocksPublication,
                    FindingGroup::Connection,
                )
                .with_param("capability", check.name),
            );
        }
    }
    let extra = requirement_findings(requirements, contracts, &report)
        .into_iter()
        .filter(|finding| finding.param("contract") == Some(CONTRACT_RESEND_WIDGET_TYPE))
        .collect::<Vec<_>>();
    report.findings.extend(extra);
    report
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use zdashstitch_zabbix::{AdapterRegistry, ZabbixVersion};

    use super::*;
    use crate::LocalSession;

    fn snapshot(version: &str, widget_type: &str) -> VersionedDashboardSnapshot {
        let version: ZabbixVersion = version
            .parse()
            .unwrap_or_else(|error| panic!("valid version: {error}"));
        let raw: Value = json!({
            "dashboardid": "7", "name": "contract",
            "pages": [{
                "dashboard_pageid": "11", "name": "page", "display_period": "0",
                "widgets": [{
                    "widgetid": "13", "type": widget_type, "name": "source",
                    "x": "0", "y": "0", "width": "12", "height": "3", "view_mode": "0",
                    "fields": [{"type": "4", "name": "itemid.0", "value": "101"}]
                }]
            }], "users": [], "userGroups": []
        });
        VersionedDashboardSnapshot::from_adapter_raw("test".to_owned(), version, raw)
            .unwrap_or_else(|error| panic!("valid snapshot: {error}"))
    }

    fn journal() -> (tempfile::TempDir, PublishJournal) {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"));
        let journal = PublishJournal::new(dir.path().to_path_buf());
        (dir, journal)
    }

    fn replacement() -> WidgetFieldReplacement {
        WidgetFieldReplacement {
            page_index: 0,
            widgetid: "13".to_owned(),
            field_name: "itemid.0".to_owned(),
            expected_value: "101".to_owned(),
            replacement_value: "202".to_owned(),
        }
    }

    #[test]
    fn verified_plan_is_previewed_and_consumed_by_matching_confirmation() {
        let source = snapshot("7.4.16", "item");
        let capabilities = AdapterRegistry::resolve(source.server_version().clone())
            .unwrap_or_else(|error| panic!("supported adapter: {error}"));
        let context = PublicationContext::detached_for_testing(capabilities);
        let session = LocalSession::new();
        let (_dir, journal) = journal();
        let plan = build_verified_plan(
            &journal,
            &source,
            &context,
            &[replacement()],
            session.current(),
        )
        .unwrap_or_else(|error| panic!("plan must verify: {error}"));
        let preview = plan.preview();
        assert!(crate::publish_journal::is_operation_id(&preview.plan_id));
        assert_eq!(preview.operation, "replace_item_fields");
        assert_eq!(preview.api_method, "dashboard.update");
        assert_eq!(preview.write_requests, 1);
        assert_eq!(preview.replacement_count, 1);
        assert_eq!(preview.changed_pages, vec![0]);
        assert!(preview.contracts.contains(&"item_widget_itemid"));
        assert!(preview.local_id.is_none());
        assert!(
            plan.confirm("zds-other")
                .is_err_and(|error| matches!(error, PlanError::ConfirmationMismatch))
        );
        let plan = build_verified_plan(
            &journal,
            &source,
            &context,
            &[replacement()],
            session.current(),
        )
        .unwrap_or_else(|error| panic!("plan must verify: {error}"));
        let shown = plan.preview();
        assert_ne!(shown.plan_id, preview.plan_id, "plan IDs are never reused");
        let confirmed = plan
            .confirm(&shown.plan_id)
            .unwrap_or_else(|error| panic!("matching confirmation: {error}"));
        assert_eq!(confirmed.plan_id(), shown.plan_id);
    }

    #[test]
    fn unproven_patch_and_unknown_widget_cannot_create_a_plan() {
        let session = LocalSession::new();
        let (_dir, journal) = journal();
        let unproven = snapshot("7.4.17", "item");
        let capabilities = AdapterRegistry::resolve(unproven.server_version().clone())
            .unwrap_or_else(|error| panic!("read adapter: {error}"));
        assert!(matches!(
            build_verified_plan(
                &journal,
                &unproven,
                &PublicationContext::detached_for_testing(capabilities),
                &[replacement()],
                session.current(),
            ),
            Err(PlanError::Incompatible(_))
        ));

        let unknown = snapshot("7.4.16", "vendor.private");
        let capabilities = AdapterRegistry::resolve(unknown.server_version().clone())
            .unwrap_or_else(|error| panic!("write contracts: {error}"));
        assert!(matches!(
            build_verified_plan(
                &journal,
                &unknown,
                &PublicationContext::detached_for_testing(capabilities),
                &[replacement()],
                session.current(),
            ),
            Err(PlanError::Adapter(_))
        ));
    }

    #[test]
    fn journal_distinguishes_conflict_unknown_and_reconciled_success() {
        let initial = pre_send_states().to_vec();
        let conflict = finish_report(
            "conflict".to_owned(),
            initial.clone(),
            DashboardPublishOutcome::Conflict,
        );
        assert_eq!(conflict.states.last(), Some(&JournalState::Conflict));
        assert!(!conflict.states.contains(&JournalState::Submitting));
        assert!(!conflict.deployment_state_may_advance);
        assert_eq!(conflict.invalidated, Some(PlanInvalidReason::SourceChanged));

        let unknown = finish_report(
            "unknown".to_owned(),
            initial.clone(),
            DashboardPublishOutcome::UnknownOutcome,
        );
        assert_eq!(unknown.states.last(), Some(&JournalState::UnknownOutcome));
        assert!(unknown.states.contains(&JournalState::Submitting));
        assert!(!unknown.deployment_state_may_advance);

        let reconciled = finish_report(
            "reconciled".to_owned(),
            initial,
            DashboardPublishOutcome::ReconciledVerified,
        );
        assert_eq!(
            reconciled.states.last(),
            Some(&JournalState::ReconciledVerified)
        );
        assert!(reconciled.deployment_state_may_advance);
    }
}
