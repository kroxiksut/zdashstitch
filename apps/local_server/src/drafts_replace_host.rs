// SPDX-License-Identifier: MIT OR Apache-2.0

//! ReplaceHost of a draft (P2-03D/E, P1-05H, P1-08B/C) behind the `/api`
//! boundary: inventory and target host search (read-only server reads),
//! calculation and revalidation (read-only), decisions, confirmation of the
//! kept list, undo and history (local only), and the preview «база →
//! черновик» with its publication findings (works offline).
//!
//! No route here builds a write plan or reaches a write gateway: server
//! reads use the read-only gateway of the active connection, and only when
//! it is the connection the draft was copied through. The session context
//! given to the preview only shapes the findings; it grants nothing.
//! Responses carry codes, safe parameters and data; host, item, page and
//! widget names are server data. Times are RFC 3339 UTC strings (O-035).

use std::{collections::BTreeMap, collections::BTreeSet, future::Future, sync::Arc};

use axum::{
    Extension, Router,
    body::Bytes,
    extract::Query,
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use zdashstitch_application::{
    CompatibilityReport,
    connections::ConnectionService,
    replace_host::{
        FieldKey, FieldValue, ItemFacts, KeptEntry, PatternCount, ReplaceHostInventory,
        ReplaceHostPreview, ReplaceScope, RowDecision, RowView, SessionContext, StateCounts,
        TargetHostSearch, capability_state_str,
    },
    resolver::{ReplaceHostRequest, ResolveScope},
    workcopy::WorkcopyService,
};

use super::{DraftsHandle, Failure, PathParams, blocking, not_connected, parse, rfc3339_utc_ms};
use crate::connections::ConnectionsHandle;
use crate::security::LocalSecurity;

/// The ReplaceHost routes of one draft; merged into the drafts router, which
/// adds the handles as extensions.
pub(super) fn routes() -> Router<LocalSecurity> {
    Router::new()
        .route("/drafts/{local_id}/replace-host", get(preview))
        .route("/drafts/{local_id}/replace-host/inventory", get(inventory))
        .route("/drafts/{local_id}/replace-host/hosts", get(search_hosts))
        .route("/drafts/{local_id}/replace-host/calculate", post(calculate))
        .route(
            "/drafts/{local_id}/replace-host/revalidate",
            post(revalidate),
        )
        .route("/drafts/{local_id}/replace-host/decide", post(decide))
        .route(
            "/drafts/{local_id}/replace-host/confirm-kept",
            post(confirm_kept),
        )
        .route(
            "/drafts/{local_id}/replace-host/operations/{seq}/remove",
            post(remove_operation),
        )
        .route("/drafts/{local_id}/replace-host/history", get(history))
}

/// HTTP status of a ReplaceHost code; `None` leaves it to the draft codes.
pub(super) fn replace_host_status(code: &str) -> Option<StatusCode> {
    Some(match code {
        "replace_host_not_calculated"
        | "binding_not_found"
        | "operation_not_found"
        | "source_host_not_found_or_no_access" => StatusCode::NOT_FOUND,
        "replace_host_pending"
        | "kept_on_source_empty"
        | "kept_on_source_list_changed"
        | "workcopy_changed_concurrently" => StatusCode::CONFLICT,
        "resolver_hostid_invalid"
        | "replace_host_same_host"
        | "replace_host_scope_invalid"
        | "replace_host_scope_outside_draft"
        | "decision_not_applicable"
        | "candidate_not_visible"
        | "candidate_value_type_incompatible" => StatusCode::UNPROCESSABLE_ENTITY,
        "resolver_host_objects_limit" => StatusCode::UNPROCESSABLE_ENTITY,
        "local_materialization_failed" | "resolver_facts_mismatch" => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
        _ => return None,
    })
}

fn other_connection() -> Failure {
    Failure::Code(StatusCode::CONFLICT, "draft_other_connection")
}

// ---------------------------------------------------------------- execution

/// Runs a use case on the blocking pool with the workcopy service and, when
/// the connection service exists, its lock handle. The lock is taken inside
/// the use case, after the draft is known.
async fn run<T, F, Fut>(
    connections: ConnectionsHandle,
    drafts: DraftsHandle,
    use_case: F,
) -> Result<T, Failure>
where
    T: Send + 'static,
    F: FnOnce(WorkcopyService, Option<Arc<Mutex<ConnectionService>>>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, Failure>>,
{
    let service = connections.service();
    let runtime = tokio::runtime::Handle::current();
    blocking(move || {
        let workcopies = drafts.service()?;
        runtime.block_on(use_case(workcopies, service))
    })
    .await
}

/// Server reads for a draft go only through its own connection, active now.
pub(super) fn ensure_draft_connection(
    service: &ConnectionService,
    draft: &str,
) -> Result<(), Failure> {
    match service.active() {
        Some((active, _, _)) if active == draft => Ok(()),
        Some(_) => Err(other_connection()),
        None => Err(not_connected()),
    }
}

/// What the preview may say about the session: the access mode and
/// capabilities of the active connection when it is the draft's one,
/// otherwise offline. It shapes findings only.
pub(super) fn session_of<'a>(
    service: Option<&'a ConnectionService>,
    draft: &str,
) -> SessionContext<'a> {
    let Some(service) = service else {
        return SessionContext::Offline;
    };
    match (service.active(), service.reader()) {
        (Some((active, _, access_mode)), Some(reader)) if active == draft => {
            SessionContext::Connected {
                access_mode,
                capabilities: reader.capabilities(),
            }
        }
        _ => SessionContext::Offline,
    }
}

/// A local ReplaceHost use case with the session context of the draft. The
/// connection lock is held while the context is in use, so the context
/// cannot outlive a disconnect.
async fn local<T, F>(
    connections: ConnectionsHandle,
    drafts: DraftsHandle,
    local_id: String,
    use_case: F,
) -> Response
where
    T: Serialize + Send + 'static,
    F: FnOnce(&WorkcopyService, &str, SessionContext<'_>) -> Result<T, Failure> + Send + 'static,
{
    let result = run(connections, drafts, move |workcopies, service| async move {
        let draft = workcopies.draft_connection_id(&local_id)?;
        let guard = match &service {
            Some(service) => Some(service.lock().await),
            None => None,
        };
        let session = session_of(guard.as_deref(), &draft);
        use_case(&workcopies, &local_id, session)
    })
    .await;
    respond(StatusCode::OK, result)
}

fn respond<T: Serialize>(status: StatusCode, result: Result<T, Failure>) -> Response {
    super::respond(status, result)
}

// ---------------------------------------------------------------- wire types

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ScopeBody {
    Dashboard,
    Pages { dashboard_pageids: Vec<String> },
    Widgets { widgetids: Vec<String> },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CalculateBody {
    source_hostid: String,
    target_hostid: String,
    scope: ScopeBody,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecideBody {
    key: FieldKey,
    decision: RowDecision,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmBody {
    digest: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyBody {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HostQuery {
    source_hostid: String,
    #[serde(default)]
    search: Option<String>,
}

#[derive(Serialize)]
struct HostBody {
    hostid: String,
    name: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ScopeOut {
    Dashboard,
    Pages { dashboard_pageids: Vec<String> },
    Widgets { widgetids: Vec<String> },
}

impl From<&ReplaceScope> for ScopeOut {
    fn from(scope: &ReplaceScope) -> Self {
        match scope {
            ReplaceScope::Dashboard => Self::Dashboard,
            ReplaceScope::Pages { dashboard_pageids } => Self::Pages {
                dashboard_pageids: dashboard_pageids.clone(),
            },
            ReplaceScope::Widgets { widgetids } => Self::Widgets {
                widgetids: widgetids.clone(),
            },
        }
    }
}

#[derive(Serialize)]
struct OperationBody {
    seq: u64,
    /// RFC 3339 UTC.
    recorded_at: Option<String>,
    source_host: HostBody,
    target_host: Option<HostBody>,
    scope: ScopeOut,
    calculated_at: Option<String>,
    server_version: String,
}

#[derive(Serialize)]
struct RowBody {
    key: FieldKey,
    dashboard_pageid: Option<String>,
    widgetid: Option<String>,
    widget_type: String,
    source: FieldValue,
    target: Option<FieldValue>,
    #[serde(rename = "match")]
    match_status: Option<&'static str>,
    applicability: Option<&'static str>,
    reasons: Vec<&'static str>,
    differences: Vec<&'static str>,
    source_item: Option<ItemFacts>,
    target_item: Option<ItemFacts>,
    candidates: Vec<ItemFacts>,
    pattern_coverage: Vec<PatternCount>,
    state: &'static str,
    kept_reason: Option<&'static str>,
    decision: Option<RowDecision>,
    decision_seq: Option<u64>,
    decision_state: &'static str,
    allowed_decisions: Vec<&'static str>,
    unverified: bool,
    check: &'static str,
    check_reason: Option<&'static str>,
}

impl From<&RowView> for RowBody {
    fn from(row: &RowView) -> Self {
        Self {
            key: row.key.clone(),
            dashboard_pageid: row.dashboard_pageid.clone(),
            widgetid: row.widgetid.clone(),
            widget_type: row.widget_type.clone(),
            source: row.source.clone(),
            target: row.target.clone(),
            match_status: row.match_status.map(|status| status.as_str()),
            applicability: row
                .applicability
                .map(|applicability| applicability.as_str()),
            reasons: row.reasons.iter().map(|reason| reason.as_str()).collect(),
            differences: row
                .differences
                .iter()
                .map(|difference| difference.as_str())
                .collect(),
            source_item: row.source_item.clone(),
            target_item: row.target_item.clone(),
            candidates: row.candidates.clone(),
            pattern_coverage: row.pattern_coverage.clone(),
            state: row.state.as_str(),
            kept_reason: row.kept_reason.map(|reason| reason.as_str()),
            decision: row.decision.clone(),
            decision_seq: row.decision_seq,
            decision_state: row.decision_state.as_str(),
            allowed_decisions: row
                .allowed_decisions
                .iter()
                .map(|decision| decision.as_str())
                .collect(),
            unverified: row.unverified,
            check: row.check.as_str(),
            check_reason: row.check_reason.map(|reason| reason.as_str()),
        }
    }
}

#[derive(Serialize)]
struct FieldCountsBody {
    changed: u32,
    unresolved: u32,
    kept_on_source: u32,
}

#[derive(Serialize)]
struct StateCountsBody {
    total: u32,
    changed: u32,
    untouched: u32,
    unresolved: u32,
    kept_on_source: u32,
}

impl From<StateCounts> for StateCountsBody {
    fn from(counts: StateCounts) -> Self {
        Self {
            total: counts.total,
            changed: counts.changed,
            untouched: counts.untouched,
            unresolved: counts.unresolved,
            kept_on_source: counts.kept_on_source,
        }
    }
}

#[derive(Serialize)]
struct SeriesBody {
    dataset_index: u32,
    state: &'static str,
    fields: FieldCountsBody,
    field_names: Vec<String>,
    host_patterns: Vec<String>,
    item_patterns: Vec<String>,
    itemids: Vec<String>,
}

#[derive(Serialize)]
struct WidgetDiffBody {
    widget_index: usize,
    widgetid: Option<String>,
    widget_type: String,
    name: String,
    in_scope: bool,
    state: &'static str,
    fields: FieldCountsBody,
    /// Field names of the rows of this widget (`rows` key with the page and
    /// widget index).
    row_fields: Vec<String>,
    series: Vec<SeriesBody>,
}

#[derive(Serialize)]
struct PageDiffBody {
    page_index: usize,
    dashboard_pageid: Option<String>,
    name: String,
    state: &'static str,
    widget_counts: StateCountsBody,
    widgets: Vec<WidgetDiffBody>,
}

#[derive(Serialize)]
struct DiffSummaryBody {
    pages: StateCountsBody,
    widgets: StateCountsBody,
    series: StateCountsBody,
    fields: FieldCountsBody,
}

#[derive(Serialize)]
struct DiffBody {
    pages: Vec<PageDiffBody>,
    summary: DiffSummaryBody,
}

#[derive(Serialize)]
struct KeptBody {
    entries: Vec<KeptEntry>,
    /// Pass back to confirm exactly this list.
    digest: String,
    confirmed: bool,
    confirmation_seq: Option<u64>,
}

#[derive(Serialize)]
struct ChangeBody {
    key: FieldKey,
    widgetid: Option<String>,
    widget_type: String,
    source: FieldValue,
    target: FieldValue,
}

#[derive(Serialize)]
struct CheckBody {
    name: &'static str,
    state: &'static str,
    blocking: bool,
}

#[derive(Serialize)]
struct SubjectBody {
    page_index: usize,
    widget_index: usize,
    widgetid: Option<String>,
    field: Option<String>,
}

#[derive(Serialize)]
struct FindingBody {
    code: &'static str,
    severity: &'static str,
    group: &'static str,
    subject: Option<SubjectBody>,
    reason: Option<&'static str>,
    params: BTreeMap<&'static str, String>,
}

#[derive(Serialize)]
pub(super) struct ReportBody {
    allows_publication: bool,
    checks: Vec<CheckBody>,
    findings: Vec<FindingBody>,
}

impl From<&CompatibilityReport> for ReportBody {
    fn from(report: &CompatibilityReport) -> Self {
        Self {
            allows_publication: report.allows_publication(),
            checks: report
                .checks
                .iter()
                .map(|check| CheckBody {
                    name: check.name,
                    state: capability_state_str(check.state),
                    blocking: check.blocking,
                })
                .collect(),
            findings: report
                .findings
                .iter()
                .map(|finding| FindingBody {
                    code: finding.code.as_str(),
                    severity: finding.severity.as_str(),
                    group: finding.group.as_str(),
                    subject: finding.subject.as_ref().map(|subject| SubjectBody {
                        page_index: subject.page_index,
                        widget_index: subject.widget_index,
                        widgetid: subject.widgetid.clone(),
                        field: subject.field.clone(),
                    }),
                    reason: finding.reason.map(|reason| reason.as_str()),
                    params: finding
                        .params
                        .iter()
                        .map(|param| (param.name, param.value.clone()))
                        .collect(),
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
pub(super) struct PreviewBody {
    local_id: String,
    revision: u64,
    kind: &'static str,
    operation: OperationBody,
    rows: Vec<RowBody>,
    diff: DiffBody,
    kept_on_source: KeptBody,
    changes: Vec<ChangeBody>,
    /// Technical details.
    base_fingerprint: String,
    local_fingerprint: String,
    /// RFC 3339 UTC; set only by a revalidation.
    checked_at: Option<String>,
    report: ReportBody,
}

fn field_counts(counts: zdashstitch_application::replace_host::ChangeCounts) -> FieldCountsBody {
    FieldCountsBody {
        changed: counts.changed,
        unresolved: counts.unresolved,
        kept_on_source: counts.kept_on_source,
    }
}

impl From<&ReplaceHostPreview> for PreviewBody {
    fn from(preview: &ReplaceHostPreview) -> Self {
        let operation = &preview.operation;
        let summary = &preview.diff.summary;
        Self {
            local_id: preview.local_id.clone(),
            revision: preview.revision,
            kind: preview.kind.as_str(),
            operation: OperationBody {
                seq: operation.seq,
                recorded_at: rfc3339_utc_ms(operation.recorded_at_ms),
                source_host: HostBody {
                    hostid: operation.source_host.hostid.clone(),
                    name: operation.source_host.name.clone(),
                },
                target_host: operation.target_host.as_ref().map(|host| HostBody {
                    hostid: host.hostid.clone(),
                    name: host.name.clone(),
                }),
                scope: ScopeOut::from(&operation.scope),
                calculated_at: rfc3339_utc_ms(operation.calculated_at_ms),
                server_version: operation.server_version.clone(),
            },
            rows: preview.rows.iter().map(RowBody::from).collect(),
            diff: DiffBody {
                pages: preview
                    .diff
                    .pages
                    .iter()
                    .map(|page| PageDiffBody {
                        page_index: page.page_index,
                        dashboard_pageid: page.dashboard_pageid.clone(),
                        name: page.name.clone(),
                        state: page.state.as_str(),
                        widget_counts: StateCountsBody::from(page.widget_counts),
                        widgets: page
                            .widgets
                            .iter()
                            .map(|widget| WidgetDiffBody {
                                widget_index: widget.widget_index,
                                widgetid: widget.widgetid.clone(),
                                widget_type: widget.widget_type.clone(),
                                name: widget.name.clone(),
                                in_scope: widget.in_scope,
                                state: widget.state.as_str(),
                                fields: field_counts(widget.fields),
                                row_fields: widget
                                    .rows
                                    .iter()
                                    .map(|row| row.key.field.clone())
                                    .collect(),
                                series: widget
                                    .series
                                    .iter()
                                    .map(|series| SeriesBody {
                                        dataset_index: series.dataset_index,
                                        state: series.state.as_str(),
                                        fields: field_counts(series.fields),
                                        field_names: series.field_names.clone(),
                                        host_patterns: series.host_patterns.clone(),
                                        item_patterns: series.item_patterns.clone(),
                                        itemids: series.itemids.clone(),
                                    })
                                    .collect(),
                            })
                            .collect(),
                    })
                    .collect(),
                summary: DiffSummaryBody {
                    pages: StateCountsBody::from(summary.pages),
                    widgets: StateCountsBody::from(summary.widgets),
                    series: StateCountsBody::from(summary.series),
                    fields: field_counts(summary.fields),
                },
            },
            kept_on_source: KeptBody {
                entries: preview.kept_on_source.entries.clone(),
                digest: preview.kept_on_source.digest.clone(),
                confirmed: preview.kept_on_source.confirmed,
                confirmation_seq: preview.kept_on_source.confirmation_seq,
            },
            changes: preview
                .changes
                .iter()
                .map(|change| ChangeBody {
                    key: change.key.clone(),
                    widgetid: change.widgetid.clone(),
                    widget_type: change.widget_type.clone(),
                    source: change.source.clone(),
                    target: change.target.clone(),
                })
                .collect(),
            base_fingerprint: preview.base_fingerprint.clone(),
            local_fingerprint: preview.local_fingerprint.clone(),
            checked_at: preview.checked_at_ms.and_then(rfc3339_utc_ms),
            report: ReportBody::from(&preview.report),
        }
    }
}

#[derive(Serialize)]
struct InventoryWidgetBody {
    widget_index: usize,
    widgetid: Option<String>,
    name: String,
    #[serde(rename = "type")]
    widget_type: String,
}

#[derive(Serialize)]
struct InventoryPageBody {
    page_index: usize,
    dashboard_pageid: Option<String>,
    name: String,
    widgets: Vec<InventoryWidgetBody>,
}

#[derive(Serialize)]
struct ReferenceBody {
    page_index: usize,
    widget_index: usize,
    field: String,
    widget_type: String,
    kind: &'static str,
    outcome: &'static str,
    reason: Option<&'static str>,
}

#[derive(Serialize)]
struct InventoryHostBody {
    hostid: String,
    name: String,
    host: String,
    widgets: u32,
    references: Vec<ReferenceBody>,
}

#[derive(Serialize)]
struct UnattributedBody {
    page_index: usize,
    widget_index: usize,
    field: String,
    widget_type: String,
    kind: &'static str,
}

#[derive(Serialize)]
struct InventoryBody {
    local_id: String,
    kind: &'static str,
    pages: Vec<InventoryPageBody>,
    hosts: Vec<InventoryHostBody>,
    unattributed: Vec<UnattributedBody>,
}

impl From<ReplaceHostInventory> for InventoryBody {
    fn from(inventory: ReplaceHostInventory) -> Self {
        Self {
            local_id: inventory.local_id,
            kind: inventory.kind.as_str(),
            pages: inventory
                .pages
                .into_iter()
                .map(|page| InventoryPageBody {
                    page_index: page.page_index,
                    dashboard_pageid: page.dashboard_pageid,
                    name: page.name,
                    widgets: page
                        .widgets
                        .into_iter()
                        .map(|widget| InventoryWidgetBody {
                            widget_index: widget.widget_index,
                            widgetid: widget.widgetid,
                            name: widget.name,
                            widget_type: widget.widget_type,
                        })
                        .collect(),
                })
                .collect(),
            hosts: inventory
                .hosts
                .into_iter()
                .map(|host| InventoryHostBody {
                    hostid: host.hostid,
                    name: host.name,
                    host: host.host,
                    widgets: host.widgets,
                    references: host
                        .references
                        .into_iter()
                        .map(|reference| ReferenceBody {
                            page_index: reference.page_index,
                            widget_index: reference.widget_index,
                            field: reference.field,
                            widget_type: reference.widget_type,
                            kind: reference.kind.as_str(),
                            outcome: reference.outcome.as_str(),
                            reason: reference.reason.map(|reason| reason.as_str()),
                        })
                        .collect(),
                })
                .collect(),
            unattributed: inventory
                .unattributed
                .into_iter()
                .map(|source| UnattributedBody {
                    page_index: source.page_index,
                    widget_index: source.widget_index,
                    field: source.field,
                    widget_type: source.widget_type,
                    kind: source.kind,
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct CandidateBody {
    hostid: String,
    name: String,
    host: String,
    groups: Vec<String>,
    templates: Vec<String>,
    source_templates_found: u32,
}

#[derive(Serialize)]
struct HostSearchBody {
    hosts: Vec<CandidateBody>,
    truncated: bool,
    source_templates: Option<Vec<String>>,
    empty_reason: Option<&'static str>,
}

impl From<TargetHostSearch> for HostSearchBody {
    fn from(search: TargetHostSearch) -> Self {
        Self {
            hosts: search
                .hosts
                .into_iter()
                .map(|host| CandidateBody {
                    hostid: host.hostid,
                    name: host.name,
                    host: host.host,
                    groups: host.groups,
                    templates: host.templates,
                    source_templates_found: host.source_templates_found,
                })
                .collect(),
            truncated: search.truncated,
            source_templates: search.source_templates,
            empty_reason: search.empty_reason,
        }
    }
}

#[derive(Serialize)]
struct RemovedBody {
    removed: Vec<u64>,
    /// The preview after the removal; `null` when no operation is left.
    preview: Option<PreviewBody>,
}

#[derive(Serialize)]
struct HistoryEntryBody {
    seq: u64,
    op: &'static str,
    op_version: u32,
    recorded_at: Option<String>,
    replace_seq: Option<u64>,
}

#[derive(Serialize)]
struct HistoryBody {
    operations: Vec<HistoryEntryBody>,
}

// ---------------------------------------------------------------- handlers

/// `publication.preview` for ReplaceHost: the stored operation, offline.
async fn preview(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(local_id): PathParams<String>,
) -> Response {
    local(
        connections,
        drafts,
        local_id,
        |workcopies, local_id, session| {
            let preview = workcopies.replace_host_preview(local_id, session)?;
            Ok(PreviewBody::from(&preview))
        },
    )
    .await
}

/// `bindings.inventory` (09-01): hosts of the draft and their references.
async fn inventory(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(local_id): PathParams<String>,
) -> Response {
    let result = run(connections, drafts, move |workcopies, service| async move {
        let draft = workcopies.draft_connection_id(&local_id)?;
        let service = service.ok_or_else(not_connected)?;
        let guard = service.lock().await;
        ensure_draft_connection(&guard, &draft)?;
        let reader = guard.reader().ok_or_else(not_connected)?;
        let inventory = workcopies.replace_host_inventory(reader, &local_id).await?;
        Ok(InventoryBody::from(inventory))
    })
    .await;
    respond(StatusCode::OK, result)
}

/// `bindings.targets.search` (09-02, single choice).
async fn search_hosts(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(local_id): PathParams<String>,
    uri: Uri,
) -> Response {
    let query = match Query::<HostQuery>::try_from_uri(&uri) {
        Ok(Query(query)) => query,
        Err(_) => return Failure::Code(StatusCode::BAD_REQUEST, "request_invalid").into_response(),
    };
    let result = run(connections, drafts, move |workcopies, service| async move {
        let draft = workcopies.draft_connection_id(&local_id)?;
        let service = service.ok_or_else(not_connected)?;
        let guard = service.lock().await;
        ensure_draft_connection(&guard, &draft)?;
        let reader = guard.reader().ok_or_else(not_connected)?;
        let found = workcopies
            .search_replace_host_targets(
                reader,
                &local_id,
                &query.source_hostid,
                query.search.as_deref().unwrap_or_default(),
            )
            .await?;
        Ok(HostSearchBody::from(found))
    })
    .await;
    respond(StatusCode::OK, result)
}

/// `bindings.resolve` (P2-03A): read-only calculation stored as an operation.
async fn calculate(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(local_id): PathParams<String>,
    body: Bytes,
) -> Response {
    let request: CalculateBody = match parse(&body) {
        Ok(request) => request,
        Err(failure) => return failure.into_response(),
    };
    let scope = match request.scope {
        ScopeBody::Dashboard => ResolveScope::Dashboard,
        ScopeBody::Pages { dashboard_pageids } => {
            ResolveScope::Pages(dashboard_pageids.into_iter().collect::<BTreeSet<_>>())
        }
        ScopeBody::Widgets { widgetids } => {
            ResolveScope::Widgets(widgetids.into_iter().collect::<BTreeSet<_>>())
        }
    };
    let request = ReplaceHostRequest {
        source_hostid: request.source_hostid,
        target_hostid: request.target_hostid,
        scope,
    };
    let result = run(connections, drafts, move |workcopies, service| async move {
        let draft = workcopies.draft_connection_id(&local_id)?;
        let service = service.ok_or_else(not_connected)?;
        let guard = service.lock().await;
        ensure_draft_connection(&guard, &draft)?;
        let reader = guard.reader().ok_or_else(not_connected)?;
        let session = session_of(Some(&guard), &draft);
        let preview = workcopies
            .calculate_replace_host(reader, &local_id, &request, session)
            .await?;
        Ok(PreviewBody::from(&preview))
    })
    .await;
    respond(StatusCode::OK, result)
}

/// `bindings.recheck` («Проверить все»): fresh read, nothing is written.
async fn revalidate(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(local_id): PathParams<String>,
    body: Bytes,
) -> Response {
    if let Err(failure) = parse::<EmptyBody>(&body) {
        return failure.into_response();
    }
    let result = run(connections, drafts, move |workcopies, service| async move {
        let draft = workcopies.draft_connection_id(&local_id)?;
        let service = service.ok_or_else(not_connected)?;
        let guard = service.lock().await;
        ensure_draft_connection(&guard, &draft)?;
        let reader = guard.reader().ok_or_else(not_connected)?;
        let session = session_of(Some(&guard), &draft);
        let preview = workcopies
            .revalidate_replace_host(reader, &local_id, session)
            .await?;
        Ok(PreviewBody::from(&preview))
    })
    .await;
    respond(StatusCode::OK, result)
}

/// `bindings.decide`: one decision as a workcopy operation; local only.
async fn decide(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(local_id): PathParams<String>,
    body: Bytes,
) -> Response {
    let request: DecideBody = match parse(&body) {
        Ok(request) => request,
        Err(failure) => return failure.into_response(),
    };
    local(
        connections,
        drafts,
        local_id,
        move |workcopies, local_id, session| {
            let preview = workcopies.decide_replace_host(
                local_id,
                &request.key,
                &request.decision,
                session,
            )?;
            Ok(PreviewBody::from(&preview))
        },
    )
    .await
}

/// `bindings.confirm_kept` / «Подтвердить список»: local only.
async fn confirm_kept(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(local_id): PathParams<String>,
    body: Bytes,
) -> Response {
    let request: ConfirmBody = match parse(&body) {
        Ok(request) => request,
        Err(failure) => return failure.into_response(),
    };
    local(
        connections,
        drafts,
        local_id,
        move |workcopies, local_id, session| {
            let preview = workcopies.confirm_kept_on_source(local_id, &request.digest, session)?;
            Ok(PreviewBody::from(&preview))
        },
    )
    .await
}

/// Undo of one operation; removing the calculation removes its decisions
/// («Сбросить все привязки»). Local only.
async fn remove_operation(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams((local_id, seq)): PathParams<(String, String)>,
    body: Bytes,
) -> Response {
    if let Err(failure) = parse::<EmptyBody>(&body) {
        return failure.into_response();
    }
    let Ok(seq) = seq.parse::<u64>() else {
        return Failure::Code(StatusCode::BAD_REQUEST, "request_invalid").into_response();
    };
    local(
        connections,
        drafts,
        local_id,
        move |workcopies, local_id, session| {
            let removed = workcopies.remove_operation(local_id, seq)?;
            let preview = match workcopies.replace_host_preview(local_id, session) {
                Ok(preview) => Some(PreviewBody::from(&preview)),
                Err(error) if error.code() == "replace_host_not_calculated" => None,
                Err(error) => return Err(Failure::from(error)),
            };
            Ok(RemovedBody { removed, preview })
        },
    )
    .await
}

/// «История привязок»: the operation log of the draft, oldest first.
async fn history(
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(local_id): PathParams<String>,
) -> Response {
    let result = blocking(move || {
        let operations = drafts.service()?.operation_history(&local_id)?;
        Ok(HistoryBody {
            operations: operations
                .into_iter()
                .map(|operation| HistoryEntryBody {
                    seq: operation.seq,
                    op: operation.op,
                    op_version: operation.op_version,
                    recorded_at: rfc3339_utc_ms(operation.recorded_at_ms),
                    replace_seq: operation.replace_seq,
                })
                .collect(),
        })
    })
    .await;
    respond(StatusCode::OK, result)
}
