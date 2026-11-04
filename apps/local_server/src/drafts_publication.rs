// SPDX-License-Identifier: MIT OR Apache-2.0

//! Publication of a ReplaceHost draft (P2-04E/F, P1-08D–G, P1-01G §4, §7)
//! behind the `/api` boundary: session cookie, Origin/Host, CSRF, content
//! type and body limits are enforced by `security::protect_api` before any
//! handler runs, and every handler here receives the session generation its
//! request was admitted under.
//!
//! The only write path is `POST /drafts/{local_id}/publication/confirm`:
//!
//! 1. `…/publication/plan` (read-write mode of the draft's own active
//!    connection only) asks the application for a [`VerifiedPlan`] or a
//!    [`VerifiedCreatePlan`]: the application re-reads the server and every
//!    precondition. The plan never leaves this process: it is kept in the
//!    [`PlanRegistry`] under an opaque random handle bound to the draft,
//!    the session generation and the connection context, and only its
//!    preview (codes, counts, fingerprints, the `plan_id`) is returned. No
//!    plan type is deserializable, so an HTTP body can never carry one.
//! 2. `…/publication/confirm` takes the plan out of the registry (one-shot:
//!    a second confirmation, or one after «Отмена», finds nothing) and
//!    confirms it only with the explicit confirmation of exactly what the
//!    dialog showed: the handle, the displayed `plan_id`, `confirmed: true`
//!    and, for an update, the digest of the acknowledged «останутся на
//!    старом узле» list or, for a copy, the typed name. Then, holding the
//!    connection and the write stage (so «Завершить сеанс» waits), the
//!    application publisher sends exactly one request and reads back; an
//!    unknown outcome is never retried.
//!
//! Plans have no timer (P1-08F): they end when consumed or discarded, when
//! a newer plan of the same draft replaces them, when the local session
//! ends (the registry is erased with the session secrets) or with the
//! process; anything that changed meanwhile is caught by the mandatory
//! re-checks right before send.
//!
//! The journal routes read and record only: the recovery listing,
//! reconciliation by reads (read-only mode is enough), «Закрыть без сверки»
//! and «Убрать в карантин» after the user's confirmation. Responses carry
//! codes, safe parameters and data; dashboard and host names are server
//! data. Times are RFC 3339 UTC strings (O-035).

use std::{collections::BTreeMap, future::Future, sync::Arc};

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use zdashstitch_application::{
    CloseManuallyRequest, CompatibilityReport, CreatePlanError, CreatePlanPreview,
    CreatePublishReport, DamagedJournal, JournalEntry, JournalError, JournalState, JournalTarget,
    LocalSession, OperationKind, PlanError, PlanPreview, PublishJournal, PublishReport,
    QuarantineRequest, ReplaceHostPlanRequest, SaveAsCopyRequest, SessionGeneration, TargetKey,
    VerifiedCreatePlan, VerifiedPlan,
    connections::ConnectionService,
    plan_replace_host,
    publication::{self, RemoteDiff},
    publish_replace_host_plan, publish_save_as_copy_plan, recheck_replace_host, reconcile_create,
    reconcile_update,
    workcopy::{PublishedUpdate, WorkcopyError, WorkcopyService},
};

use super::replace_host::{PreviewBody, ReportBody, ensure_draft_connection, session_of};
use super::{DraftsHandle, Failure, PathParams, not_connected, rfc3339_utc_ms};
use crate::connections::ConnectionsHandle;
use crate::security::LocalSecurity;
use crate::session_end::{EraseFuture, SessionMemory};

/// Upper bound of plans kept at once; the oldest is dropped beyond it.
const MAX_PLANS: usize = 8;

pub(super) fn routes() -> Router<LocalSecurity> {
    Router::new()
        .route("/publication/journal", get(journal_list))
        .route("/publication/journal/{operation_id}", get(journal_entry))
        .route(
            "/publication/journal/{operation_id}/reconcile",
            post(journal_reconcile),
        )
        .route(
            "/publication/journal/{operation_id}/close",
            post(journal_close),
        )
        .route(
            "/publication/journal/{operation_id}/quarantine",
            post(journal_quarantine),
        )
        .route("/drafts/{local_id}/publication/check", post(check))
        .route("/drafts/{local_id}/publication/plan", post(plan))
        .route("/drafts/{local_id}/publication/confirm", post(confirm))
        .route("/drafts/{local_id}/publication/discard", post(discard))
        .route("/drafts/{local_id}/publication/refetch", post(refetch))
}

// ---------------------------------------------------------------- plans

enum HeldPlan {
    /// «Обновить исходную панель»: one `dashboard.update`.
    Update {
        plan: Box<VerifiedPlan>,
        /// Digest of the kept list the plan was verified with.
        kept_digest: Option<String>,
    },
    /// «Сохранить копию»: one `dashboard.create`.
    Create {
        plan: Box<VerifiedCreatePlan>,
        /// User data: the new dashboard name.
        name: String,
    },
}

struct StoredPlan {
    handle: String,
    local_id: String,
    generation: SessionGeneration,
    connection_id: String,
    /// Context generation of the connection the plan was built on.
    context_generation: u64,
    plan: HeldPlan,
}

/// Verified plans of this process, in memory only, by opaque handle. Not
/// `Clone`, never serialized; erased when the local session ends.
#[derive(Default)]
pub(crate) struct PlanRegistry {
    plans: std::sync::Mutex<Vec<StoredPlan>>,
    /// The start-up pass of the journal (retention, then recovery) ran in
    /// this process; later listings only recover.
    journal_started: std::sync::atomic::AtomicBool,
}

impl PlanRegistry {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<StoredPlan>> {
        self.plans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Keeps `plan`; a previous plan of the same draft is dropped.
    fn insert(&self, plan: StoredPlan) {
        let mut plans = self.lock();
        plans.retain(|stored| stored.local_id != plan.local_id);
        if plans.len() >= MAX_PLANS {
            plans.remove(0);
        }
        plans.push(plan);
    }

    /// One-shot: the plan leaves the registry whatever happens next.
    fn take(&self, handle: &str) -> Option<StoredPlan> {
        let mut plans = self.lock();
        let position = plans.iter().position(|stored| stored.handle == handle)?;
        Some(plans.remove(position))
    }

    fn discard(&self, local_id: &str, handle: &str) -> bool {
        let mut plans = self.lock();
        let before = plans.len();
        plans.retain(|stored| !(stored.local_id == local_id && stored.handle == handle));
        before != plans.len()
    }

    #[cfg(test)]
    pub(crate) fn count(&self) -> usize {
        self.lock().len()
    }
}

impl SessionMemory for PlanRegistry {
    /// «Завершить сеанс»: every plan of the session is dropped.
    fn erase(&self) -> EraseFuture<'_> {
        Box::pin(async move {
            self.lock().clear();
        })
    }
}

/// `plan-` + 128 random bits: the only reference to a plan the UI holds.
fn new_handle() -> Result<String, Refusal> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| {
        code(
            StatusCode::INTERNAL_SERVER_ERROR,
            "random_source_unavailable",
        )
    })?;
    let mut handle = String::with_capacity(37);
    handle.push_str("plan-");
    for byte in bytes {
        handle.push_str(&format!("{byte:02x}"));
    }
    Ok(handle)
}

// ---------------------------------------------------------------- errors

/// A refused request, boxed: an HTTP response is too large to travel in
/// every `Err` of the use cases.
struct Refusal(Box<Response>);

impl From<Response> for Refusal {
    fn from(response: Response) -> Self {
        Self(Box::new(response))
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        *self.0
    }
}

#[derive(Serialize)]
struct CodeBody {
    code: &'static str,
    params: BTreeMap<&'static str, String>,
}

impl CodeBody {
    /// A repeated parameter keeps every value, comma-separated.
    fn new<'a>(
        code: &'static str,
        params: impl IntoIterator<Item = (&'static str, &'a str)>,
    ) -> Self {
        let mut map: BTreeMap<&'static str, String> = BTreeMap::new();
        for (name, value) in params {
            map.entry(name)
                .and_modify(|existing| {
                    existing.push(',');
                    existing.push_str(value);
                })
                .or_insert_with(|| value.to_owned());
        }
        Self { code, params: map }
    }

    fn journal(error: &JournalError) -> Self {
        Self::new(
            error.code(),
            error
                .params()
                .iter()
                .map(|param| (param.name, param.value.as_str())),
        )
    }

    fn workcopy(error: &WorkcopyError) -> Self {
        Self::new(
            error.code(),
            error
                .params()
                .iter()
                .map(|param| (param.name, param.value.as_str())),
        )
    }
}

/// A refusal with a code and no parameters.
fn code(status: StatusCode, code: &'static str) -> Refusal {
    Refusal::from((status, Json(CodeBody::new(code, []))).into_response())
}

fn with_params(status: StatusCode, body: CodeBody) -> Refusal {
    Refusal::from((status, Json(body)).into_response())
}

fn plan_invalidated(reason: &'static str) -> Refusal {
    with_params(
        StatusCode::CONFLICT,
        CodeBody::new("plan_invalidated", [("reason", reason)]),
    )
}

fn failure(failure: Failure) -> Refusal {
    Refusal::from(failure.into_response())
}

fn workcopy_failure(error: WorkcopyError) -> Refusal {
    Refusal::from(Failure::Workcopy(error).into_response())
}

/// HTTP status of a journal code. Connection taxonomy codes of a
/// reconciliation read are upstream errors: 502, never 401/403.
fn journal_status(code: &str) -> StatusCode {
    match code {
        "operation_not_found" => StatusCode::NOT_FOUND,
        "operation_id_invalid" => StatusCode::UNPROCESSABLE_ENTITY,
        "close_not_confirmed" | "quarantine_not_confirmed" => StatusCode::BAD_REQUEST,
        "target_blocked"
        | "journal_damaged"
        | "operation_running"
        | "reconcile_not_needed"
        | "reconcile_kind_mismatch"
        | "reconcile_server_changed"
        | "journal_not_damaged"
        | "quarantine_exists" => StatusCode::CONFLICT,
        "journal_corrupt"
        | "journal_write_failed"
        | "journal_transition_invalid"
        | "storage_unavailable"
        | "storage_linked_path"
        | "storage_file_too_large"
        | "storage_write_failed"
        | "storage_limit_reached" => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::BAD_GATEWAY,
    }
}

fn journal_failure(error: &JournalError) -> Refusal {
    with_params(journal_status(error.code()), CodeBody::journal(error))
}

#[derive(Serialize)]
struct BlockedBody {
    code: &'static str,
    params: BTreeMap<&'static str, String>,
    report: ReportBody,
}

fn blocked(report: &CompatibilityReport) -> Refusal {
    Refusal::from(
        (
            StatusCode::CONFLICT,
            Json(BlockedBody {
                code: "publication_blocked",
                params: BTreeMap::new(),
                report: ReportBody::from(report),
            }),
        )
            .into_response(),
    )
}

fn plan_failure(error: PlanError) -> Refusal {
    match error {
        PlanError::Incompatible(report) => blocked(&report),
        PlanError::Adapter(rejection) => with_params(
            StatusCode::CONFLICT,
            CodeBody::new(
                rejection.code().as_str(),
                rejection
                    .params()
                    .iter()
                    .map(|param| (param.name, param.value.as_str())),
            ),
        ),
        PlanError::ConfirmationMismatch => code(StatusCode::CONFLICT, "plan_mismatch"),
        PlanError::Invalidated(reason) => plan_invalidated(reason.as_str()),
        PlanError::Workcopy(error) => workcopy_failure(error),
        PlanError::Read(failure) => with_params(
            StatusCode::BAD_GATEWAY,
            CodeBody::new(
                failure.code.as_str(),
                failure
                    .params
                    .iter()
                    .map(|param| (param.name, param.value.as_str())),
            ),
        ),
        PlanError::Journal(error) => journal_failure(&error),
        PlanError::LocalMismatch => code(StatusCode::CONFLICT, "local_mismatch"),
    }
}

fn create_plan_failure(error: CreatePlanError) -> Refusal {
    match error {
        CreatePlanError::Incompatible(report) => blocked(&report),
        CreatePlanError::Adapter(rejection) => with_params(
            StatusCode::CONFLICT,
            CodeBody::new(
                rejection.code().as_str(),
                rejection
                    .params()
                    .iter()
                    .map(|param| (param.name, param.value.as_str())),
            ),
        ),
        CreatePlanError::ConfirmationMismatch => code(StatusCode::CONFLICT, "plan_mismatch"),
        CreatePlanError::Invalidated(reason) => plan_invalidated(reason.as_str()),
        CreatePlanError::Workcopy(error) => workcopy_failure(error),
        CreatePlanError::Read(failure) => with_params(
            StatusCode::BAD_GATEWAY,
            CodeBody::new(
                failure.code.as_str(),
                failure
                    .params
                    .iter()
                    .map(|param| (param.name, param.value.as_str())),
            ),
        ),
        CreatePlanError::Journal(error) => journal_failure(&error),
    }
}

/// Parses a JSON body without echoing parser details. An empty body is `{}`.
fn parse<T: serde::de::DeserializeOwned>(body: &Bytes) -> Result<T, Refusal> {
    super::parse(body).map_err(failure)
}

// ---------------------------------------------------------------- execution

type Connections = Option<Arc<Mutex<ConnectionService>>>;

/// Runs a use case on the blocking pool with the workcopy service, the
/// journal of the same storage root and the connection service handle. A
/// client that disconnects cancels only the wait for the answer, never a
/// running write: the use case runs to its end on its own thread.
async fn run<T, F, Fut>(
    connections: ConnectionsHandle,
    drafts: DraftsHandle,
    use_case: F,
) -> Result<T, Refusal>
where
    T: Send + 'static,
    F: FnOnce(WorkcopyService, PublishJournal, Connections) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, Refusal>>,
{
    let service = connections.service();
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let (workcopies, journal) = drafts.stores().map_err(failure)?;
        runtime.block_on(use_case(workcopies, journal, service))
    })
    .await
    .unwrap_or_else(|_| Err(code(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")))
}

fn respond<T: Serialize>(result: Result<T, Refusal>) -> Response {
    match result {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

// ---------------------------------------------------------------- wire types

#[derive(Serialize)]
struct TransitionBody {
    state: &'static str,
    at: Option<String>,
}

#[derive(Serialize)]
struct TargetBody {
    /// `existing_dashboard` or `new_dashboard`.
    kind: &'static str,
    /// Zabbix ID: API bodies only.
    dashboardid: Option<String>,
    /// Server or user data.
    name: Option<String>,
}

impl From<&JournalTarget> for TargetBody {
    fn from(target: &JournalTarget) -> Self {
        match target {
            JournalTarget::ExistingDashboard { dashboardid, name } => Self {
                kind: "existing_dashboard",
                dashboardid: Some(dashboardid.clone()),
                name: name.clone(),
            },
            JournalTarget::NewDashboard { name } => Self {
                kind: "new_dashboard",
                dashboardid: None,
                name: Some(name.clone()),
            },
        }
    }
}

#[derive(Serialize)]
struct EntrySourceBody {
    local_id: String,
    revision: u64,
}

/// One journal entry (P1-08H audit fields). No payload, secrets or raw
/// server answers exist in the journal.
#[derive(Serialize)]
struct EntryBody {
    operation_id: String,
    kind: &'static str,
    intent: String,
    connection_local_id: String,
    server_version: String,
    target: TargetBody,
    source: Option<EntrySourceBody>,
    state: &'static str,
    progress: &'static str,
    sends: u8,
    transitions: Vec<TransitionBody>,
    plan_created_at: Option<String>,
    confirmed_at: Option<String>,
    submitted_at: Option<String>,
    evidence: Option<String>,
    created_dashboardid: Option<String>,
    reported_dashboardid: Option<String>,
    /// Local operating-system account of «Закрыть без сверки».
    closed_by: Option<String>,
    /// Technical details.
    source_fingerprint: String,
    expected_fingerprint: String,
    observed_fingerprint: Option<String>,
}

impl From<&JournalEntry> for EntryBody {
    fn from(entry: &JournalEntry) -> Self {
        Self {
            operation_id: entry.operation_id.clone(),
            kind: entry.kind.as_str(),
            intent: entry.intent.clone(),
            connection_local_id: entry.connection_id.clone(),
            server_version: entry.server_version.clone(),
            target: TargetBody::from(&entry.target),
            source: entry.source.as_ref().map(|source| EntrySourceBody {
                local_id: source.local_id.clone(),
                revision: source.revision,
            }),
            state: entry.state.as_str(),
            progress: entry.progress.as_str(),
            sends: entry.sends,
            transitions: entry
                .transitions
                .iter()
                .map(|transition| TransitionBody {
                    state: transition.state.as_str(),
                    at: rfc3339_utc_ms(transition.at_ms),
                })
                .collect(),
            plan_created_at: rfc3339_utc_ms(entry.plan_created_at_ms),
            confirmed_at: rfc3339_utc_ms(entry.confirmed_at_ms),
            submitted_at: entry.submitted_at_ms().and_then(rfc3339_utc_ms),
            evidence: entry.evidence.clone(),
            created_dashboardid: entry.created_dashboardid.clone(),
            reported_dashboardid: entry.reported_dashboardid.clone(),
            closed_by: entry.closed_by.clone(),
            source_fingerprint: entry.source_fingerprint.clone(),
            expected_fingerprint: entry.expected_fingerprint.clone(),
            observed_fingerprint: entry.observed_fingerprint.clone(),
        }
    }
}

#[derive(Serialize)]
struct DamagedBody {
    operation_id: String,
    code: &'static str,
    connection_local_id: Option<String>,
    target: Option<TargetBody>,
}

impl From<&DamagedJournal> for DamagedBody {
    fn from(damaged: &DamagedJournal) -> Self {
        Self {
            operation_id: damaged.operation_id.clone(),
            code: damaged.error.code(),
            connection_local_id: damaged.connection_id.clone(),
            target: damaged.target.as_ref().map(TargetBody::from),
        }
    }
}

#[derive(Serialize)]
struct JournalListBody {
    /// Entries that block their target (`startup.recovery.found`).
    unfinished: Vec<EntryBody>,
    damaged: Vec<DamagedBody>,
}

#[derive(Serialize)]
struct SourceCheckBody {
    /// `current`, `conflict`, `source_missing` or `stale`.
    state: &'static str,
    checked_at: Option<String>,
    base_fingerprint: String,
    remote_fingerprint: Option<String>,
    invalidated: Option<&'static str>,
}

#[derive(Serialize)]
struct RemoteChangeBody {
    kind: &'static str,
    page_name: Option<String>,
    page_number: Option<u32>,
    widget_name: Option<String>,
    widget_type: Option<String>,
    widgetid: Option<String>,
    overlaps: bool,
}

#[derive(Serialize)]
struct RemoteDiffBody {
    missing: bool,
    changes: Vec<RemoteChangeBody>,
    truncated: usize,
    overlapping_widgets: usize,
}

impl From<RemoteDiff> for RemoteDiffBody {
    fn from(diff: RemoteDiff) -> Self {
        Self {
            missing: diff.missing,
            changes: diff
                .changes
                .into_iter()
                .map(|change| RemoteChangeBody {
                    kind: change.kind.as_str(),
                    page_name: change.page_name,
                    page_number: change.page_number,
                    widget_name: change.widget_name,
                    widget_type: change.widget_type,
                    widgetid: change.widgetid,
                    overlaps: change.overlaps,
                })
                .collect(),
            truncated: diff.truncated,
            overlapping_widgets: diff.overlapping_widgets,
        }
    }
}

#[derive(Serialize)]
struct CheckResultBody {
    source: SourceCheckBody,
    preview: PreviewBody,
    /// «На сервере: база → Zabbix сейчас»; only when Remote differs.
    remote: Option<RemoteDiffBody>,
    /// `read_only` or `read_write`.
    mode: &'static str,
    /// An unfinished operation or a damaged journal file blocks the
    /// dashboard (`target_blocked` / `journal_damaged`).
    journal_block: Option<CodeBody>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyBody {}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum PlanRequestBody {
    /// «Обновить исходную панель».
    UpdateSource {
        revision: u64,
        base_fingerprint: String,
        local_fingerprint: String,
        #[serde(default)]
        kept_digest: Option<String>,
    },
    /// «Сохранить копию».
    SaveAsCopy {
        revision: u64,
        base_fingerprint: String,
        local_fingerprint: String,
        name: String,
    },
}

#[derive(Serialize)]
struct FieldKindCount {
    contract: &'static str,
    count: usize,
}

/// What the confirmation dialog shows (P1-08F); no payload.
#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum PlanPreviewBody {
    UpdateSource {
        plan_id: String,
        api_method: &'static str,
        write_requests: u8,
        created_at: Option<String>,
        dashboardid: String,
        replacement_count: usize,
        changed_pages: Vec<usize>,
        contracts: Vec<&'static str>,
        replacement_kinds: Vec<FieldKindCount>,
        revision: Option<u64>,
        kept_digest: Option<String>,
        source_fingerprint: String,
        expected_fingerprint: String,
        report: ReportBody,
    },
    SaveAsCopy {
        plan_id: String,
        api_method: &'static str,
        write_requests: u8,
        created_at: Option<String>,
        /// User data.
        target_name: String,
        private: bool,
        page_count: usize,
        widget_count: usize,
        unknown_widgets: usize,
        contracts: Vec<&'static str>,
        name_check: &'static str,
        revision: u64,
        local_fingerprint: String,
        content_fingerprint: String,
        report: ReportBody,
    },
}

impl From<&PlanPreview> for PlanPreviewBody {
    fn from(preview: &PlanPreview) -> Self {
        Self::UpdateSource {
            plan_id: preview.plan_id.clone(),
            api_method: preview.api_method,
            write_requests: preview.write_requests,
            created_at: rfc3339_utc_ms(preview.created_at_ms),
            dashboardid: preview.dashboard_id.clone(),
            replacement_count: preview.replacement_count,
            changed_pages: preview.changed_pages.clone(),
            contracts: preview.contracts.clone(),
            replacement_kinds: preview
                .replacement_kinds
                .iter()
                .map(|(contract, count)| FieldKindCount {
                    contract,
                    count: *count,
                })
                .collect(),
            revision: preview.revision,
            kept_digest: preview.kept_digest.clone(),
            source_fingerprint: preview.source_fingerprint.clone(),
            expected_fingerprint: preview.expected_fingerprint.clone(),
            report: ReportBody::from(&preview.compatibility),
        }
    }
}

impl From<&CreatePlanPreview> for PlanPreviewBody {
    fn from(preview: &CreatePlanPreview) -> Self {
        Self::SaveAsCopy {
            plan_id: preview.plan_id.clone(),
            api_method: preview.api_method,
            write_requests: preview.write_requests,
            created_at: rfc3339_utc_ms(preview.created_at_ms),
            target_name: preview.target_name.clone(),
            private: preview.private,
            page_count: preview.page_count,
            widget_count: preview.widget_count,
            unknown_widgets: preview.unknown_widgets,
            contracts: preview.contracts.clone(),
            name_check: preview.name_check.as_str(),
            revision: preview.revision,
            local_fingerprint: preview.local_fingerprint.clone(),
            content_fingerprint: preview.content_fingerprint.clone(),
            report: ReportBody::from(&preview.compatibility),
        }
    }
}

#[derive(Serialize)]
struct PlanBody {
    /// The only reference to the plan; pass it back to confirm or discard.
    handle: String,
    plan: PlanPreviewBody,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmBody {
    handle: String,
    /// The `plan_id` the dialog showed.
    plan_id: String,
    /// «Записать в Zabbix» was pressed in the dialog.
    confirmed: bool,
    /// Update: the digest of the kept list the dialog showed (`null` when
    /// the list is empty).
    #[serde(default)]
    kept_digest: Option<String>,
    /// Copy: the new dashboard name the dialog showed.
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HandleBody {
    handle: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmedBody {
    confirmed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RefetchBody {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Serialize)]
struct AdvancedBody {
    revision: u64,
    base_fingerprint: String,
    read_at: Option<String>,
}

/// Result of a write or of a reconciliation (P1-08G).
#[derive(Serialize)]
struct OutcomeBody {
    operation_id: String,
    /// `update` or `create`.
    kind: &'static str,
    /// The last committed journal state, or the in-memory state when the
    /// journal was never opened (`failed_before_send`).
    state: &'static str,
    /// The write request was dispatched (0 or 1 times).
    sent: bool,
    /// `publication.plan.reason.*` code of a refusal right before send.
    invalidated: Option<&'static str>,
    journal_error: Option<CodeBody>,
    /// What decided the state (a code), e.g. `dashboard_name_taken`,
    /// `reconcile_not_found`.
    evidence: Option<String>,
    /// Create: the confirmed new dashboard.
    created_dashboardid: Option<String>,
    /// Create: user data, the new dashboard name.
    target_name: Option<String>,
    /// Update: the snapshot after writing became the new base.
    base_advanced: Option<AdvancedBody>,
    base_advance_error: Option<CodeBody>,
    /// Create: recording the new dashboard in the draft failed (O-022).
    record_error: Option<CodeBody>,
    entry: Option<EntryBody>,
}

impl OutcomeBody {
    fn from_entry(entry: &JournalEntry) -> Self {
        Self {
            operation_id: entry.operation_id.clone(),
            kind: entry.kind.as_str(),
            state: entry.state.as_str(),
            sent: entry.sends > 0,
            invalidated: None,
            journal_error: None,
            evidence: entry.evidence.clone(),
            created_dashboardid: entry.created_dashboardid.clone(),
            target_name: match &entry.target {
                JournalTarget::NewDashboard { name } => Some(name.clone()),
                JournalTarget::ExistingDashboard { .. } => None,
            },
            base_advanced: None,
            base_advance_error: None,
            record_error: None,
            entry: Some(EntryBody::from(entry)),
        }
    }

    fn update(report: &PublishReport, entry: Option<&JournalEntry>) -> Self {
        let mut body = entry.map_or_else(
            || Self {
                operation_id: report.plan_id.clone(),
                kind: OperationKind::Update.as_str(),
                state: report
                    .states
                    .last()
                    .map_or(JournalState::FailedBeforeSend.as_str(), |state| {
                        state.as_str()
                    }),
                sent: report.states.contains(&JournalState::Submitting),
                invalidated: None,
                journal_error: None,
                evidence: None,
                created_dashboardid: None,
                target_name: None,
                base_advanced: None,
                base_advance_error: None,
                record_error: None,
                entry: None,
            },
            Self::from_entry,
        );
        body.invalidated = report.invalidated.map(|reason| reason.as_str());
        body.journal_error = report.journal_error.as_ref().map(CodeBody::journal);
        body
    }

    fn create(report: &CreatePublishReport, name: String, entry: Option<&JournalEntry>) -> Self {
        let mut body = entry.map_or_else(
            || Self {
                operation_id: report.plan_id.clone(),
                kind: OperationKind::Create.as_str(),
                state: report.journal_state.map_or(
                    JournalState::FailedBeforeSend.as_str(),
                    JournalState::as_str,
                ),
                sent: report.sent(),
                invalidated: None,
                journal_error: None,
                evidence: report
                    .outcome
                    .as_ref()
                    .map(|outcome| outcome.as_str().to_owned()),
                created_dashboardid: None,
                target_name: None,
                base_advanced: None,
                base_advance_error: None,
                record_error: None,
                entry: None,
            },
            Self::from_entry,
        );
        body.target_name = Some(name);
        body.invalidated = report.invalidated.map(|reason| reason.as_str());
        body.journal_error = report.journal_error.as_ref().map(CodeBody::journal);
        body.record_error = report.record_error.as_ref().map(CodeBody::workcopy);
        body
    }
}

#[derive(Serialize)]
struct RefetchResultBody {
    local_id: String,
    kind: &'static str,
    /// User data.
    name: String,
    replayed: bool,
    replay_code: Option<&'static str>,
}

// ---------------------------------------------------------------- helpers

/// After a verified update: the snapshot after writing becomes the new base
/// of the draft (P1-08G). Its failure never changes the outcome.
async fn advance(
    workcopies: &WorkcopyService,
    connection: &ConnectionService,
    entry: &JournalEntry,
    body: &mut OutcomeBody,
) {
    let verified = matches!(
        entry.state,
        JournalState::Verified | JournalState::ReconciledVerified
    );
    let (true, OperationKind::Update, Some(source)) = (verified, entry.kind, &entry.source) else {
        return;
    };
    let published = PublishedUpdate {
        local_id: source.local_id.clone(),
        connection_id: entry.connection_id.clone(),
        revision: source.revision,
        source_fingerprint: entry.source_fingerprint.clone(),
        expected_fingerprint: entry.expected_fingerprint.clone(),
    };
    let Some(reader) = connection.reader() else {
        return;
    };
    match workcopies.advance_base(reader, &published).await {
        Ok(advanced) => {
            body.base_advanced = Some(AdvancedBody {
                revision: advanced.revision,
                base_fingerprint: advanced.base_fingerprint,
                read_at: rfc3339_utc_ms(advanced.read_at_ms),
            });
        }
        Err(error) => body.base_advance_error = Some(CodeBody::workcopy(&error)),
    }
}

fn is_operation_path(operation_id: &str) -> Result<(), Refusal> {
    if zdashstitch_application::publish_journal::is_operation_id(operation_id) {
        Ok(())
    } else {
        Err(code(
            StatusCode::UNPROCESSABLE_ENTITY,
            "operation_id_invalid",
        ))
    }
}

// ---------------------------------------------------------------- journal

/// `startup.recovery.found` (P1-01G §4): entries that block their target
/// and damaged files. Local files only; nothing is sent. The UI asks for it
/// right after start, so the first listing of the process runs the journal
/// start-up pass (`PublishJournal::start_up`: finished entries older than
/// 90 days are pruned, unfinished ones are never touched, then recovery);
/// later listings only recover.
async fn journal_list(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    Extension(plans): Extension<Arc<PlanRegistry>>,
) -> Response {
    respond(
        run(connections, drafts, move |_, journal, _| async move {
            let first = !plans
                .journal_started
                .swap(true, std::sync::atomic::Ordering::SeqCst);
            let report = if first {
                journal.start_up().map(|startup| startup.recovery)
            } else {
                journal.recover()
            };
            let report = report.map_err(|error| {
                if first {
                    plans
                        .journal_started
                        .store(false, std::sync::atomic::Ordering::SeqCst);
                }
                journal_failure(&error)
            })?;
            Ok(JournalListBody {
                unfinished: report.unfinished.iter().map(EntryBody::from).collect(),
                damaged: report.damaged.iter().map(DamagedBody::from).collect(),
            })
        })
        .await,
    )
}

/// «Журнал операции»: one entry.
async fn journal_entry(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(operation_id): PathParams<String>,
) -> Response {
    if let Err(response) = is_operation_path(&operation_id) {
        return response.into_response();
    }
    respond(
        run(connections, drafts, move |_, journal, _| async move {
            let entry = journal
                .entry(&operation_id)
                .map_err(|error| journal_failure(&error))?;
            Ok(EntryBody::from(&entry))
        })
        .await,
    )
}

/// «Проверить результат» (P1-08G, P1-01G §4): operation-specific reads
/// only, through the read-only gateway of the active connection (read-only
/// mode is enough); never a write. A verified update advances the base.
async fn journal_reconcile(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(operation_id): PathParams<String>,
    body: Bytes,
) -> Response {
    if let Err(response) = parse::<EmptyBody>(&body).and(is_operation_path(&operation_id)) {
        return response.into_response();
    }
    respond(
        run(
            connections,
            drafts,
            move |workcopies, journal, service| async move {
                let entry = journal
                    .entry(&operation_id)
                    .map_err(|error| journal_failure(&error))?;
                let service = service.ok_or_else(|| failure(not_connected()))?;
                let guard = service.lock().await;
                let reader = guard.reader().ok_or_else(|| failure(not_connected()))?;
                match entry.kind {
                    OperationKind::Update => {
                        let entry = reconcile_update(&journal, reader, &operation_id)
                            .await
                            .map_err(|error| journal_failure(&error))?;
                        let mut body = OutcomeBody::from_entry(&entry);
                        advance(&workcopies, &guard, &entry, &mut body).await;
                        Ok(body)
                    }
                    OperationKind::Create => {
                        let recovery =
                            reconcile_create(&journal, &workcopies, reader, &operation_id)
                                .await
                                .map_err(|error| journal_failure(&error))?;
                        let mut body = OutcomeBody::from_entry(&recovery.entry);
                        body.record_error = recovery.record_error.as_ref().map(CodeBody::workcopy);
                        Ok(body)
                    }
                }
            },
        )
        .await,
    )
}

/// «Закрыть без сверки» (P1-01G §7): only with the confirmation; the entry
/// stays in the journal, the target is unblocked, nothing claims a result.
async fn journal_close(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(operation_id): PathParams<String>,
    body: Bytes,
) -> Response {
    let request: ConfirmedBody = match parse(&body) {
        Ok(request) => request,
        Err(response) => return response.into_response(),
    };
    if let Err(response) = is_operation_path(&operation_id) {
        return response.into_response();
    }
    respond(
        run(connections, drafts, move |_, journal, _| async move {
            let entry = journal
                .close_manually(&CloseManuallyRequest {
                    operation_id,
                    confirmed: request.confirmed,
                })
                .map_err(|error| journal_failure(&error))?;
            Ok(EntryBody::from(&entry))
        })
        .await,
    )
}

#[derive(Serialize)]
struct QuarantinedBody {
    operation_id: String,
    quarantined: bool,
}

/// «Убрать в карантин» (OPEN_DECISIONS 28.10.2026 evening, item 4): a
/// damaged journal file moves to `journal/quarantine/` after the
/// confirmation, which lifts its block.
async fn journal_quarantine(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(operation_id): PathParams<String>,
    body: Bytes,
) -> Response {
    let request: ConfirmedBody = match parse(&body) {
        Ok(request) => request,
        Err(response) => return response.into_response(),
    };
    if let Err(response) = is_operation_path(&operation_id) {
        return response.into_response();
    }
    respond(
        run(connections, drafts, move |_, journal, _| async move {
            journal
                .quarantine(&QuarantineRequest {
                    operation_id: operation_id.clone(),
                    confirmed: request.confirmed,
                })
                .map_err(|error| journal_failure(&error))?;
            Ok(QuarantinedBody {
                operation_id,
                quarantined: true,
            })
        })
        .await,
    )
}

// ---------------------------------------------------------------- draft

/// `publication.recheck` (P1-08C/D, P2-04C): re-reads Remote and
/// revalidates the stored ReplaceHost; on a conflict also the comparison
/// «На сервере: база → Zabbix сейчас». Read-only; any mode.
async fn check(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(local_id): PathParams<String>,
    body: Bytes,
) -> Response {
    if let Err(response) = parse::<EmptyBody>(&body) {
        return response.into_response();
    }
    respond(
        run(
            connections,
            drafts,
            move |workcopies, journal, service| async move {
                let draft = workcopies
                    .draft_connection_id(&local_id)
                    .map_err(workcopy_failure)?;
                let service = service.ok_or_else(|| failure(not_connected()))?;
                let guard = service.lock().await;
                ensure_draft_connection(&guard, &draft).map_err(failure)?;
                let reader = guard.reader().ok_or_else(|| failure(not_connected()))?;
                let mode = if guard.writer().is_some() {
                    "read_write"
                } else {
                    "read_only"
                };
                let session = session_of(Some(&guard), &draft);
                let recheck = recheck_replace_host(&workcopies, reader, &draft, &local_id, session)
                    .await
                    .map_err(workcopy_failure)?;
                let remote = if recheck.source.is_current() {
                    None
                } else {
                    let widgets = publication::changed_widgetids(&recheck.preview);
                    Some(
                        publication::read_remote_changes(&workcopies, reader, &local_id, &widgets)
                            .await
                            .map_err(workcopy_failure)?,
                    )
                };
                let base = workcopies
                    .base_snapshot(&local_id)
                    .map_err(workcopy_failure)?;
                let journal_block = journal
                    .check_target(&TargetKey::dashboard(
                        draft.clone(),
                        Some(reader.identity().endpoint_fingerprint().to_owned()),
                        base.dashboard_id(),
                    ))
                    .err()
                    .map(|error| CodeBody::journal(&error));
                let source = &recheck.source;
                Ok(CheckResultBody {
                    source: SourceCheckBody {
                        state: source.freshness.state.as_str(),
                        checked_at: source.freshness.checked_at_ms.and_then(rfc3339_utc_ms),
                        base_fingerprint: source.base_fingerprint.clone(),
                        remote_fingerprint: source.remote_fingerprint.clone(),
                        invalidated: source.invalidated.map(|reason| reason.as_str()),
                    },
                    preview: PreviewBody::from(&recheck.preview),
                    remote: remote.map(RemoteDiffBody::from),
                    mode,
                    journal_block,
                })
            },
        )
        .await,
    )
}

/// `publication.preview` → plan (P1-08F): the application re-reads the
/// server and verifies everything; the plan stays in this process.
async fn plan(
    State(security): State<LocalSecurity>,
    Extension(admitted): Extension<SessionGeneration>,
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    Extension(plans): Extension<Arc<PlanRegistry>>,
    PathParams(local_id): PathParams<String>,
    body: Bytes,
) -> Response {
    let request: PlanRequestBody = match parse(&body) {
        Ok(request) => request,
        Err(response) => return response.into_response(),
    };
    let session: LocalSession = security.local_session().clone();
    respond(
        run(
            connections,
            drafts,
            move |workcopies, journal, service| async move {
                let draft = workcopies
                    .draft_connection_id(&local_id)
                    .map_err(workcopy_failure)?;
                let service = service.ok_or_else(|| failure(not_connected()))?;
                let guard = service.lock().await;
                // The draft's own connection, active and in read-write mode.
                ensure_draft_connection(&guard, &draft).map_err(failure)?;
                let context_generation = guard.active().map_or(0, |(_, generation, _)| generation);
                let writer = guard
                    .writer()
                    .ok_or_else(|| code(StatusCode::CONFLICT, "write_mode_required"))?;
                let handle = new_handle()?;
                let (held, preview) = match request {
                    PlanRequestBody::UpdateSource {
                        revision,
                        base_fingerprint,
                        local_fingerprint,
                        kept_digest,
                    } => {
                        let request = ReplaceHostPlanRequest {
                            local_id: local_id.clone(),
                            connection_id: draft.clone(),
                            revision,
                            base_fingerprint,
                            local_fingerprint,
                            kept_digest,
                            session: admitted,
                        };
                        let plan =
                            plan_replace_host(&workcopies, &journal, &session, writer, &request)
                                .await
                                .map_err(plan_failure)?;
                        let preview = plan.preview();
                        let body = PlanPreviewBody::from(&preview);
                        (
                            HeldPlan::Update {
                                plan: Box::new(plan),
                                kept_digest: preview.kept_digest,
                            },
                            body,
                        )
                    }
                    PlanRequestBody::SaveAsCopy {
                        revision,
                        base_fingerprint,
                        local_fingerprint,
                        name,
                    } => {
                        let request = SaveAsCopyRequest {
                            local_id: local_id.clone(),
                            connection_id: draft.clone(),
                            revision,
                            base_fingerprint,
                            local_fingerprint,
                            target_name: name,
                            session: admitted,
                        };
                        let plan = publication::plan_replace_host_copy(
                            &workcopies,
                            &journal,
                            &session,
                            writer,
                            &request,
                        )
                        .await
                        .map_err(create_plan_failure)?;
                        let preview = plan.preview();
                        let body = PlanPreviewBody::from(&preview);
                        (
                            HeldPlan::Create {
                                plan: Box::new(plan),
                                name: preview.target_name,
                            },
                            body,
                        )
                    }
                };
                plans.insert(StoredPlan {
                    handle: handle.clone(),
                    local_id,
                    generation: admitted,
                    connection_id: draft,
                    context_generation,
                    plan: held,
                });
                Ok(PlanBody {
                    handle,
                    plan: preview,
                })
            },
        )
        .await,
    )
}

/// «Записать в Zabbix» (P1-08F/G): the one write path. The plan is taken
/// out of the registry first (one-shot), confirmed with exactly what the
/// dialog showed, and sent by the application publisher while the write
/// stage is held.
async fn confirm(
    State(security): State<LocalSecurity>,
    Extension(admitted): Extension<SessionGeneration>,
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    Extension(plans): Extension<Arc<PlanRegistry>>,
    PathParams(local_id): PathParams<String>,
    body: Bytes,
) -> Response {
    let request: ConfirmBody = match parse(&body) {
        Ok(request) => request,
        Err(response) => return response.into_response(),
    };
    if !request.confirmed {
        return code(StatusCode::BAD_REQUEST, "confirmation_required").into_response();
    }
    let Some(stored) = plans.take(&request.handle) else {
        return code(StatusCode::NOT_FOUND, "plan_not_found").into_response();
    };
    if stored.local_id != local_id {
        return code(StatusCode::CONFLICT, "plan_mismatch").into_response();
    }
    let session: LocalSession = security.local_session().clone();
    if stored.generation != admitted || !session.is_current(admitted) {
        return plan_invalidated("session_ended").into_response();
    }
    let StoredPlan {
        connection_id,
        context_generation,
        plan,
        ..
    } = stored;
    respond(
        run(
            connections,
            drafts,
            move |workcopies, journal, service| async move {
                let service = service.ok_or_else(|| plan_invalidated("connection_changed"))?;
                let guard = service.lock().await;
                let current = guard
                    .active()
                    .map(|(active, generation, _)| (active, generation));
                if current != Some((connection_id.as_str(), context_generation)) {
                    return Err(plan_invalidated("connection_changed"));
                }
                let writer = guard
                    .writer()
                    .ok_or_else(|| plan_invalidated("connection_changed"))?;
                match plan {
                    HeldPlan::Update { plan, kept_digest } => {
                        if request.name.is_some() || request.kept_digest != kept_digest {
                            return Err(code(StatusCode::CONFLICT, "plan_mismatch"));
                        }
                        let confirmed = plan.confirm(&request.plan_id).map_err(plan_failure)?;
                        let stage = session
                            .begin_write(admitted)
                            .map_err(|_| plan_invalidated("session_ended"))?;
                        let report = publish_replace_host_plan(
                            &workcopies,
                            writer,
                            &journal,
                            confirmed,
                            &stage,
                        )
                        .await;
                        drop(stage);
                        let entry = journal.entry(&report.plan_id).ok();
                        let mut body = OutcomeBody::update(&report, entry.as_ref());
                        if let Some(entry) = &entry {
                            advance(&workcopies, &guard, entry, &mut body).await;
                        }
                        Ok(body)
                    }
                    HeldPlan::Create { plan, name } => {
                        if request.kept_digest.is_some() || request.name.as_deref() != Some(&name) {
                            return Err(code(StatusCode::CONFLICT, "plan_mismatch"));
                        }
                        let confirmed = plan
                            .confirm(&request.plan_id)
                            .map_err(create_plan_failure)?;
                        let stage = session
                            .begin_write(admitted)
                            .map_err(|_| plan_invalidated("session_ended"))?;
                        let report = publish_save_as_copy_plan(
                            &workcopies,
                            writer,
                            &journal,
                            confirmed,
                            &stage,
                        )
                        .await;
                        drop(stage);
                        let entry = journal.entry(&report.plan_id).ok();
                        Ok(OutcomeBody::create(&report, name, entry.as_ref()))
                    }
                }
            },
        )
        .await,
    )
}

#[derive(Serialize)]
struct DiscardedBody {
    discarded: bool,
}

/// «Отмена» in the confirmation dialog: the plan is dropped.
async fn discard(
    Extension(plans): Extension<Arc<PlanRegistry>>,
    PathParams(local_id): PathParams<String>,
    body: Bytes,
) -> Response {
    let request: HandleBody = match parse(&body) {
        Ok(request) => request,
        Err(response) => return response.into_response(),
    };
    respond(Ok::<_, Refusal>(DiscardedBody {
        discarded: plans.discard(&local_id, &request.handle),
    }))
}

/// «Скопировать заново» (P2-04E): a new copy of the same source read now,
/// the replacement calculated again on it; this draft is not changed.
async fn refetch(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(local_id): PathParams<String>,
    body: Bytes,
) -> Response {
    let request: RefetchBody = match parse(&body) {
        Ok(request) => request,
        Err(response) => return response.into_response(),
    };
    let result = run(
        connections,
        drafts,
        move |workcopies, _journal, service| async move {
            let draft = workcopies
                .draft_connection_id(&local_id)
                .map_err(workcopy_failure)?;
            let service = service.ok_or_else(|| failure(not_connected()))?;
            let guard = service.lock().await;
            ensure_draft_connection(&guard, &draft).map_err(failure)?;
            let reader = guard.reader().ok_or_else(|| failure(not_connected()))?;
            let session = session_of(Some(&guard), &draft);
            let outcome = workcopies
                .refetch_replace_host(reader, &draft, &local_id, request.name, session)
                .await
                .map_err(workcopy_failure)?;
            Ok(RefetchResultBody {
                local_id: outcome.import.local_id,
                kind: outcome.import.kind.as_str(),
                name: outcome.import.name,
                replayed: outcome.replayed,
                replay_code: outcome.replay_code,
            })
        },
    )
    .await;
    match result {
        Ok(body) => (StatusCode::CREATED, Json(body)).into_response(),
        Err(refusal) => refusal.into_response(),
    }
}
