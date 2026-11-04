// SPDX-License-Identifier: MIT OR Apache-2.0

//! Copies of server dashboards and local drafts (P2-05D, P1-03H) behind the
//! `/api` boundary: session cookie, Origin/Host, CSRF, content type and body
//! limits are enforced by `security::protect_api` before any handler runs.
//!
//! Every server read goes through the read-only gateway of the one active
//! connection; no route here can reach a write gateway. Responses carry
//! stable codes, safe parameters and data (dashboard, page and widget names
//! are server data and are never translated); times are RFC 3339 UTC
//! strings (O-035). Zabbix IDs appear only in API bodies and API paths,
//! never in interface routes.
//!
//! Workcopy storage is synchronous file I/O, so every use case runs on the
//! blocking pool. The connection service lock is held for the whole server
//! read: the read-only gateway is owned by the service and cannot be
//! borrowed past the lock. Other connection commands wait for the read,
//! exactly as they wait for each other.

use std::{collections::BTreeMap, future::Future, path::PathBuf, sync::Arc};

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{FromRequestParts, Path, Query},
    http::{StatusCode, Uri, request::Parts},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::sync::Mutex;
use zdashstitch_application::{
    AppSettings, PublishJournal, SettingsStore,
    connections::ConnectionService,
    workcopy::{
        AccessFilter, DraftDetail, DraftList, DraftSummary, ExistingCopy, ImportOutcome,
        ImportPreview, ImportRequest, ImportScope, ServerDashboardList, ServerDashboardQuery,
        WorkcopyError, WorkcopyService,
    },
};

use crate::connections::ConnectionsHandle;
use crate::security::LocalSecurity;

/// Default page of «Панели в Zabbix» when the UI sends no `limit`.
const DEFAULT_LIST_PAGE: usize = 50;

/// Where workcopies live. The settings file is read for every request, so a
/// storage root changed in «Settings → Workspace» applies without restart.
#[derive(Clone, Debug)]
pub(crate) struct DraftsHandle {
    storage: StorageRoot,
}

#[derive(Clone, Debug)]
enum StorageRoot {
    /// `storage_root` of the platform settings file, or its default.
    Settings,
    /// A fixed root, e.g. a temporary directory in tests.
    #[cfg_attr(not(test), allow(dead_code))]
    Fixed(PathBuf),
}

impl DraftsHandle {
    pub(crate) fn from_settings() -> Self {
        Self {
            storage: StorageRoot::Settings,
        }
    }

    #[cfg(test)]
    pub(crate) fn at(root: PathBuf) -> Self {
        Self {
            storage: StorageRoot::Fixed(root),
        }
    }

    /// Synchronous: may read the settings file.
    fn service(&self) -> Result<WorkcopyService, Failure> {
        Ok(WorkcopyService::new(self.root()?))
    }

    /// Workcopies and the publication journal of the same storage root
    /// (P2-04F). Synchronous: may read the settings file.
    fn stores(&self) -> Result<(WorkcopyService, PublishJournal), Failure> {
        let root = self.root()?;
        Ok((
            WorkcopyService::new(root.clone()),
            PublishJournal::new(root),
        ))
    }

    fn root(&self) -> Result<PathBuf, Failure> {
        match &self.storage {
            StorageRoot::Fixed(root) => Ok(root.clone()),
            StorageRoot::Settings => settings_storage_root(),
        }
    }
}

/// The configured storage root. A damaged settings file falls back to the
/// last valid copy; without one the root is unknown and nothing is read or
/// written, so drafts never land in an unexpected folder.
fn settings_storage_root() -> Result<PathBuf, Failure> {
    let unavailable = || Failure::Code(StatusCode::SERVICE_UNAVAILABLE, "settings_unavailable");
    let (store, defaults) = SettingsStore::platform_default().map_err(|_| unavailable())?;
    let settings: AppSettings = match store.load() {
        Ok(Some(settings)) => settings,
        Ok(None) => defaults,
        Err(error) => error.last_valid.ok_or_else(unavailable)?,
    };
    Ok(settings.storage_root)
}

pub(crate) fn routes(
    connections: ConnectionsHandle,
    drafts: DraftsHandle,
) -> Router<LocalSecurity> {
    Router::new()
        .route("/server/{connection}/dashboards", get(list_dashboards))
        .route(
            "/server/{connection}/dashboards/{dashboard}",
            get(preview_dashboard),
        )
        .route(
            "/server/{connection}/dashboards/{dashboard}/import",
            post(import_dashboard),
        )
        .route("/drafts", get(list_drafts))
        .route("/drafts/{local_id}", get(draft))
        .route("/drafts/{local_id}/rename", post(rename_draft))
        .merge(replace_host::routes())
        .layer(Extension(drafts))
        .layer(Extension(connections))
}

// ---------------------------------------------------------------- errors

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    params: BTreeMap<&'static str, String>,
}

/// A failed request: refused before the use case, or a use-case error.
enum Failure {
    Code(StatusCode, &'static str),
    Workcopy(WorkcopyError),
}

impl From<WorkcopyError> for Failure {
    fn from(error: WorkcopyError) -> Self {
        Self::Workcopy(error)
    }
}

fn request_invalid() -> Failure {
    Failure::Code(StatusCode::BAD_REQUEST, "request_invalid")
}

fn not_connected() -> Failure {
    Failure::Code(StatusCode::CONFLICT, "not_connected")
}

/// HTTP status of a workcopy code. Zabbix failures (`session_expired`,
/// `api_method_denied`, `host_unreachable_or_timeout`, …) are upstream
/// errors: 502, never 401/403, which belong to the local session.
fn workcopy_status(code: &str) -> StatusCode {
    if let Some(status) = replace_host::replace_host_status(code) {
        return status;
    }
    match code {
        "workcopy_not_found" | "not_visible_or_denied" | "import_page_not_found" => {
            StatusCode::NOT_FOUND
        }
        "import_inconsistent" => StatusCode::CONFLICT,
        "connection_id_invalid"
        | "dashboard_id_invalid"
        | "page_id_invalid"
        | "workcopy_id_invalid"
        | "list_limit_invalid"
        | "search_invalid"
        | "draft_name_invalid"
        | "snapshot_too_large" => StatusCode::UNPROCESSABLE_ENTITY,
        "storage_limit_reached" => StatusCode::INSUFFICIENT_STORAGE,
        "workcopy_corrupt"
        | "workcopy_too_large"
        | "workcopy_unsupported_operation"
        | "snapshot_corrupt"
        | "snapshot_missing"
        | "snapshot_unsupported_adapter"
        | "storage_unavailable"
        | "storage_linked_path"
        | "storage_file_too_large"
        | "storage_write_failed"
        | "random_source_unavailable" => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::BAD_GATEWAY,
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        match self {
            Self::Code(status, code) => (
                status,
                Json(ErrorBody {
                    code,
                    params: BTreeMap::new(),
                }),
            )
                .into_response(),
            Self::Workcopy(error) => {
                // A repeated parameter (`change` of `import_inconsistent`)
                // keeps every value, comma-separated.
                let mut params: BTreeMap<&'static str, String> = BTreeMap::new();
                for param in error.params() {
                    params
                        .entry(param.name)
                        .and_modify(|value| {
                            value.push(',');
                            value.push_str(&param.value);
                        })
                        .or_insert_with(|| param.value.clone());
                }
                (
                    workcopy_status(error.code()),
                    Json(ErrorBody {
                        code: error.code(),
                        params,
                    }),
                )
                    .into_response()
            }
        }
    }
}

/// Path parameters of these routes. A segment that is not valid
/// percent-encoded UTF-8 is refused with JSON `request_invalid`, not with
/// axum's plain-text rejection.
struct PathParams<T>(T);

impl<S: Send + Sync, T: DeserializeOwned + Send> FromRequestParts<S> for PathParams<T> {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Path::<T>::from_request_parts(parts, state)
            .await
            .map(|Path(value)| Self(value))
            .map_err(|_| request_invalid().into_response())
    }
}

/// Parses a JSON body without echoing parser details. An empty body is `{}`.
fn parse<T: DeserializeOwned>(body: &Bytes) -> Result<T, Failure> {
    let raw: &[u8] = if body.is_empty() { b"{}" } else { body };
    Json::<T>::from_bytes(raw)
        .map(|Json(value)| value)
        .map_err(|_| request_invalid())
}

/// Connection profile ID (`conn-` + 32 lowercase hex).
fn is_connection_id(value: &str) -> bool {
    value.strip_prefix("conn-").is_some_and(|hex| {
        hex.len() == 32
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// RFC 3339 UTC form of Unix epoch milliseconds (O-035). `None` only
/// outside years 0..=9999.
fn rfc3339_utc_ms(milliseconds: u64) -> Option<String> {
    let nanoseconds = i128::from(milliseconds).checked_mul(1_000_000)?;
    OffsetDateTime::from_unix_timestamp_nanos(nanoseconds)
        .ok()?
        .format(&Rfc3339)
        .ok()
}

// ---------------------------------------------------------------- execution

/// Runs a synchronous use case on the blocking pool.
async fn blocking<T, F>(task: F) -> Result<T, Failure>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, Failure> + Send + 'static,
{
    tokio::task::spawn_blocking(task).await.unwrap_or_else(|_| {
        Err(Failure::Code(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
        ))
    })
}

/// A use case that reads the server and the local storage: it runs on the
/// blocking pool and drives its server reads from there. The connection
/// lock is taken inside, after the storage root is known.
async fn with_reader<T, F, Fut>(
    connections: ConnectionsHandle,
    drafts: DraftsHandle,
    connection_id: String,
    use_case: F,
) -> Result<T, Failure>
where
    T: Send + 'static,
    F: FnOnce(WorkcopyService, Arc<Mutex<ConnectionService>>, String) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, Failure>>,
{
    if !is_connection_id(&connection_id) {
        return Err(Failure::Code(
            StatusCode::UNPROCESSABLE_ENTITY,
            "connection_id_invalid",
        ));
    }
    let service = connections.service().ok_or(Failure::Code(
        StatusCode::SERVICE_UNAVAILABLE,
        "connections_unavailable",
    ))?;
    let runtime = tokio::runtime::Handle::current();
    blocking(move || {
        let workcopies = drafts.service()?;
        runtime.block_on(use_case(workcopies, service, connection_id))
    })
    .await
}

/// Server reads go only through the active connection named in the path.
fn ensure_active(service: &ConnectionService, connection_id: &str) -> Result<(), Failure> {
    match service.active() {
        Some((active, _, _)) if active == connection_id => Ok(()),
        _ => Err(not_connected()),
    }
}

// ---------------------------------------------------------------- wire types

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    #[serde(default)]
    search: Option<String>,
    #[serde(default)]
    access: Option<String>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

impl ListQuery {
    fn into_query(self) -> Result<ServerDashboardQuery, Failure> {
        let access = match self.access.as_deref() {
            None | Some("all") => AccessFilter::All,
            Some("mine") => AccessFilter::Mine,
            Some("shared") => AccessFilter::Shared,
            Some(_) => return Err(request_invalid()),
        };
        Ok(ServerDashboardQuery {
            search: self.search,
            access,
            offset: self.offset.unwrap_or(0),
            limit: self.limit.unwrap_or(DEFAULT_LIST_PAGE),
        })
    }
}

/// `name` (P2-08A): the local draft name chosen by the UI, e.g.
/// «… (копия 2)»; absent keeps the source name.
#[derive(Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
enum ImportBody {
    Dashboard {
        #[serde(default)]
        name: Option<String>,
    },
    Page {
        dashboard_pageid: String,
        #[serde(default)]
        name: Option<String>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameBody {
    name: String,
}

#[derive(Serialize)]
struct CopyBody {
    local_id: String,
    kind: &'static str,
    /// RFC 3339 UTC.
    snapshot_at: Option<String>,
}

impl From<&ExistingCopy> for CopyBody {
    fn from(copy: &ExistingCopy) -> Self {
        Self {
            local_id: copy.local_id.clone(),
            kind: copy.kind.as_str(),
            snapshot_at: rfc3339_utc_ms(copy.snapshot_at_ms),
        }
    }
}

#[derive(Serialize)]
struct CountsBody {
    all: usize,
    mine: usize,
    shared: usize,
}

#[derive(Serialize)]
struct DashboardRowBody {
    /// Zabbix ID: used in API paths only.
    dashboardid: String,
    name: String,
    owner: &'static str,
    access: &'static str,
    page_count: u32,
    widget_count: u32,
    editable_widget_count: u32,
    existing_copies: Vec<CopyBody>,
}

#[derive(Serialize)]
struct DashboardListBody {
    rows: Vec<DashboardRowBody>,
    total: usize,
    counts: CountsBody,
    offset: usize,
    limit: usize,
    truncated: bool,
    empty_reason: Option<&'static str>,
    existing_copies_known: bool,
}

impl DashboardListBody {
    fn new(list: &ServerDashboardList, limit: usize) -> Self {
        Self {
            rows: list
                .rows
                .iter()
                .map(|row| DashboardRowBody {
                    dashboardid: row.dashboardid.clone(),
                    name: row.name.clone(),
                    owner: row.owner.as_str(),
                    access: row.access.as_str(),
                    page_count: row.page_count,
                    widget_count: row.widget_count,
                    editable_widget_count: row.editable_widget_count,
                    existing_copies: row.existing_copies.iter().map(CopyBody::from).collect(),
                })
                .collect(),
            total: list.total,
            counts: CountsBody {
                all: list.counts.all,
                mine: list.counts.mine,
                shared: list.counts.shared,
            },
            offset: list.offset,
            limit,
            truncated: list.truncated,
            empty_reason: list.empty_reason,
            existing_copies_known: list.existing_copies_known,
        }
    }
}

#[derive(Serialize)]
struct PreviewWidgetBody {
    /// Zabbix ID: a key of the read-only preview only.
    id: String,
    name: String,
    #[serde(rename = "type")]
    widget_type: String,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    editable: bool,
}

#[derive(Serialize)]
struct PreviewPageBody {
    /// Zabbix ID: used in the import body only.
    dashboard_pageid: String,
    name: String,
    display_period: Option<u32>,
    widget_count: u32,
    editable_widget_count: u32,
    widgets: Vec<PreviewWidgetBody>,
}

#[derive(Serialize)]
struct DependenciesBody {
    hosts: u32,
    host_patterns: u32,
    items: u32,
    item_patterns: u32,
    graphs: u32,
    widget_references: u32,
    unknown: u32,
}

#[derive(Serialize)]
struct PreviewBody {
    dashboardid: String,
    name: String,
    owner: &'static str,
    access: &'static str,
    pages: Vec<PreviewPageBody>,
    properties: PropertiesBody,
    dependencies: DependenciesBody,
    unknown_widget_types: Vec<String>,
    unknown_field_count: u32,
    existing_copies: Vec<CopyBody>,
}

impl From<ImportPreview> for PreviewBody {
    fn from(preview: ImportPreview) -> Self {
        Self {
            existing_copies: preview.existing_copies.iter().map(CopyBody::from).collect(),
            dashboardid: preview.dashboardid,
            name: preview.name,
            owner: preview.owner.as_str(),
            access: preview.access.as_str(),
            pages: preview
                .pages
                .into_iter()
                .map(|page| PreviewPageBody {
                    dashboard_pageid: page.dashboard_pageid,
                    name: page.name,
                    display_period: page.display_period,
                    widget_count: page.widget_count,
                    editable_widget_count: page.editable_widget_count,
                    widgets: page
                        .widgets
                        .into_iter()
                        .map(|widget| PreviewWidgetBody {
                            id: widget.id,
                            name: widget.name,
                            widget_type: widget.widget_type,
                            x: widget.x,
                            y: widget.y,
                            width: widget.width,
                            height: widget.height,
                            editable: widget.editable,
                        })
                        .collect(),
                })
                .collect(),
            properties: PropertiesBody {
                display_period: preview.properties.display_period,
                auto_start: preview.properties.auto_start,
            },
            dependencies: DependenciesBody {
                hosts: preview.dependencies.hosts,
                host_patterns: preview.dependencies.host_patterns,
                items: preview.dependencies.items,
                item_patterns: preview.dependencies.item_patterns,
                graphs: preview.dependencies.graphs,
                widget_references: preview.dependencies.widget_references,
                unknown: preview.dependencies.unknown,
            },
            unknown_widget_types: preview.unknown_widget_types,
            unknown_field_count: preview.unknown_field_count,
        }
    }
}

#[derive(Serialize)]
struct ImportResultBody {
    local_id: String,
    kind: &'static str,
    /// User data: the stored draft name.
    name: String,
    attempts: u8,
    /// Technical detail.
    snapshot_fingerprint: String,
    snapshot_reused: bool,
    item_metadata: &'static str,
}

impl From<ImportOutcome> for ImportResultBody {
    fn from(outcome: ImportOutcome) -> Self {
        Self {
            local_id: outcome.local_id,
            kind: outcome.kind.as_str(),
            name: outcome.name,
            attempts: outcome.attempts,
            snapshot_fingerprint: outcome.snapshot_fingerprint,
            snapshot_reused: outcome.snapshot_reused,
            item_metadata: outcome.item_metadata.as_str(),
        }
    }
}

#[derive(Serialize)]
struct SourceBody {
    dashboard_name: String,
    page_name: Option<String>,
    page_number: Option<u32>,
    /// Opaque profile ID; the interface shows the profile name.
    connection_local_id: String,
    server_version: String,
    /// RFC 3339 UTC.
    snapshot_at: Option<String>,
}

#[derive(Serialize)]
struct FreshnessBody {
    state: &'static str,
    /// RFC 3339 UTC; only for `current`.
    checked_at: Option<String>,
}

#[derive(Serialize)]
struct WidgetCountsBody {
    pages: u32,
    widgets: u32,
}

#[derive(Serialize)]
struct CreatedBody {
    connection_local_id: String,
    name: String,
    /// RFC 3339 UTC.
    created_at: Option<String>,
}

#[derive(Serialize)]
struct SummaryBody {
    local_id: String,
    kind: &'static str,
    origin: &'static str,
    name: String,
    source: SourceBody,
    freshness: FreshnessBody,
    local_state: &'static str,
    /// RFC 3339 UTC.
    updated_at: Option<String>,
    counts: WidgetCountsBody,
    created_dashboards: Vec<CreatedBody>,
}

impl From<&DraftSummary> for SummaryBody {
    fn from(summary: &DraftSummary) -> Self {
        Self {
            local_id: summary.local_id.clone(),
            kind: summary.kind.as_str(),
            origin: summary.origin.as_str(),
            name: summary.name.clone(),
            source: SourceBody {
                dashboard_name: summary.source.dashboard_name.clone(),
                page_name: summary.source.page_name.clone(),
                page_number: summary.source.page_number,
                connection_local_id: summary.source.connection_id.clone(),
                server_version: summary.source.server_version.clone(),
                snapshot_at: rfc3339_utc_ms(summary.source.snapshot_at_ms),
            },
            freshness: FreshnessBody {
                state: summary.freshness.state.as_str(),
                checked_at: summary.freshness.checked_at_ms.and_then(rfc3339_utc_ms),
            },
            local_state: summary.local_state.as_str(),
            updated_at: rfc3339_utc_ms(summary.updated_at_ms),
            counts: WidgetCountsBody {
                pages: summary.counts.pages,
                widgets: summary.counts.widgets,
            },
            created_dashboards: summary
                .created_dashboards
                .iter()
                .map(|created| CreatedBody {
                    connection_local_id: created.connection_id.clone(),
                    name: created.name.clone(),
                    created_at: rfc3339_utc_ms(created.created_at_ms),
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct DamagedBody {
    local_id: String,
    code: &'static str,
}

#[derive(Serialize)]
struct DraftListBody {
    drafts: Vec<SummaryBody>,
    damaged: Vec<DamagedBody>,
}

impl From<&DraftList> for DraftListBody {
    fn from(list: &DraftList) -> Self {
        Self {
            drafts: list.drafts.iter().map(SummaryBody::from).collect(),
            damaged: list
                .damaged
                .iter()
                .map(|damaged| DamagedBody {
                    local_id: damaged.local_id.clone(),
                    code: damaged.code,
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct DraftWidgetBody {
    id: String,
    name: String,
    #[serde(rename = "type")]
    widget_type: String,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    editable: bool,
}

#[derive(Serialize)]
struct DraftPageBody {
    id: String,
    name: String,
    /// Seconds; `null` means the dashboard default.
    display_period: Option<u32>,
    widgets: Vec<DraftWidgetBody>,
}

#[derive(Serialize)]
struct PropertiesBody {
    display_period: u32,
    auto_start: bool,
}

#[derive(Serialize)]
struct DraftDetailBody {
    #[serde(flatten)]
    summary: SummaryBody,
    pages: Vec<DraftPageBody>,
    /// Only for dashboard drafts.
    properties: Option<PropertiesBody>,
}

impl From<&DraftDetail> for DraftDetailBody {
    fn from(detail: &DraftDetail) -> Self {
        Self {
            summary: SummaryBody::from(&detail.summary),
            pages: detail
                .pages
                .iter()
                .map(|page| DraftPageBody {
                    id: page.id.clone(),
                    name: page.name.clone(),
                    display_period: page.display_period,
                    widgets: page
                        .widgets
                        .iter()
                        .map(|widget| DraftWidgetBody {
                            id: widget.id.clone(),
                            name: widget.name.clone(),
                            widget_type: widget.widget_type.clone(),
                            x: widget.x,
                            y: widget.y,
                            width: widget.width,
                            height: widget.height,
                            editable: widget.editable,
                        })
                        .collect(),
                })
                .collect(),
            properties: detail.properties.map(|properties| PropertiesBody {
                display_period: properties.display_period,
                auto_start: properties.auto_start,
            }),
        }
    }
}

fn respond<T: Serialize>(status: StatusCode, result: Result<T, Failure>) -> Response {
    match result {
        Ok(body) => (status, Json(body)).into_response(),
        Err(failure) => failure.into_response(),
    }
}

// ---------------------------------------------------------------- handlers

/// `server.dashboards.list` (P1-03H): visible dashboards of the active
/// connection with owner, access, counts and local copies.
async fn list_dashboards(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(connection_id): PathParams<String>,
    uri: Uri,
) -> Response {
    let query = match Query::<ListQuery>::try_from_uri(&uri) {
        Ok(Query(query)) => query.into_query(),
        Err(_) => Err(request_invalid()),
    };
    let query = match query {
        Ok(query) => query,
        Err(failure) => return failure.into_response(),
    };
    let result = with_reader(
        connections,
        drafts,
        connection_id,
        |workcopies, service, connection_id| async move {
            let guard = service.lock().await;
            ensure_active(&guard, &connection_id)?;
            let reader = guard.reader().ok_or_else(not_connected)?;
            let list = workcopies
                .list_server_dashboards(reader, &connection_id, &query)
                .await?;
            Ok(DashboardListBody::new(&list, query.limit))
        },
    )
    .await;
    respond(StatusCode::OK, result)
}

/// `import.dashboard.preview` (P1-03C): one read, nothing is stored.
async fn preview_dashboard(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams((connection_id, dashboardid)): PathParams<(String, String)>,
) -> Response {
    let result = with_reader(
        connections,
        drafts,
        connection_id,
        |workcopies, service, connection_id| async move {
            let guard = service.lock().await;
            ensure_active(&guard, &connection_id)?;
            let reader = guard.reader().ok_or_else(not_connected)?;
            let preview = workcopies
                .preview_import(reader, &connection_id, &dashboardid)
                .await?;
            Ok(PreviewBody::from(preview))
        },
    )
    .await;
    respond(StatusCode::OK, result)
}

/// `import.dashboard.run` / `import.page.run` (P1-03H): consistent read,
/// snapshot and a new workcopy; `import_inconsistent` stores nothing.
async fn import_dashboard(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams((connection_id, dashboardid)): PathParams<(String, String)>,
    body: Bytes,
) -> Response {
    let (scope, name) = match parse::<ImportBody>(&body) {
        Ok(ImportBody::Dashboard { name }) => (ImportScope::Dashboard, name),
        Ok(ImportBody::Page {
            dashboard_pageid,
            name,
        }) => (ImportScope::Page { dashboard_pageid }, name),
        Err(failure) => return failure.into_response(),
    };
    let request = ImportRequest {
        dashboardid,
        scope,
        name,
    };
    let result = with_reader(
        connections,
        drafts,
        connection_id,
        |workcopies, service, connection_id| async move {
            let guard = service.lock().await;
            ensure_active(&guard, &connection_id)?;
            let reader = guard.reader().ok_or_else(not_connected)?;
            let outcome = workcopies.import(reader, &connection_id, &request).await?;
            Ok(ImportResultBody::from(outcome))
        },
    )
    .await;
    respond(StatusCode::CREATED, result)
}

async fn list_drafts(Extension(drafts): Extension<DraftsHandle>) -> Response {
    let result = blocking(move || {
        let list = drafts.service()?.list_drafts()?;
        Ok(DraftListBody::from(&list))
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn draft(
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(local_id): PathParams<String>,
) -> Response {
    let result = blocking(move || {
        let detail = drafts.service()?.draft(&local_id)?;
        Ok(DraftDetailBody::from(&detail))
    })
    .await;
    respond(StatusCode::OK, result)
}

/// «Переименовать» (P1-03F): the local name only; the server is not touched.
async fn rename_draft(
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(local_id): PathParams<String>,
    body: Bytes,
) -> Response {
    let request: RenameBody = match parse(&body) {
        Ok(request) => request,
        Err(failure) => return failure.into_response(),
    };
    let result = blocking(move || {
        let summary = drafts.service()?.rename_draft(&local_id, &request.name)?;
        Ok(SummaryBody::from(&summary))
    })
    .await;
    respond(StatusCode::OK, result)
}

#[path = "drafts_replace_host.rs"]
mod replace_host;

#[path = "drafts_publication.rs"]
mod publication;

pub(crate) use publication::PlanRegistry;

/// Publication routes (P2-04E/F): read-only checks, the one-shot plan, its
/// confirmation and the single write, and the journal (recovery,
/// reconciliation, «Закрыть без сверки», «Убрать в карантин»). Plans live
/// in `plans` only, keyed by an opaque handle.
pub(crate) fn publication_routes(
    connections: ConnectionsHandle,
    drafts: DraftsHandle,
    plans: Arc<PlanRegistry>,
) -> Router<LocalSecurity> {
    publication::routes()
        .layer(Extension(plans))
        .layer(Extension(drafts))
        .layer(Extension(connections))
}

#[cfg(test)]
#[path = "drafts_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "drafts_replace_host_tests.rs"]
mod replace_host_tests;

#[cfg(test)]
#[path = "drafts_publication_tests.rs"]
mod publication_tests;
