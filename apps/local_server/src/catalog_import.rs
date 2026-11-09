// SPDX-License-Identifier: MIT OR Apache-2.0

//! Protected file import. Both selectors use the same quota-bound upload set.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::Path,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use zdashstitch_application::catalog_library::{
    CatalogLibrary, CatalogSummary, MAX_TEMPLATE_FILE_BYTES, inspect_template_file,
};
use zdashstitch_application::offline_dashboards::{
    CreateDashboard, OfflineDashboardStore, UpdateDashboard,
};
use zdashstitch_application::offline_page_sessions::PageEditSessions;
use zdashstitch_application::offline_pages::{CreatePage, OfflinePageStore, UpdatePage};

use super::{DraftsHandle, Failure, blocking, respond};
use crate::{
    BrowserTransferService, DialogCancellation, FileDialogService, SelectionError, SelectionKind,
    SelectionRegistry, SelectionRegistryLimits,
    browser_transfer::{MAX_FILES, transfer_error},
    security::LocalSecurity,
};

pub(super) fn routes() -> Router<LocalSecurity> {
    Router::new()
        .route("/library/catalogs", get(list))
        .route("/library/catalogs/{id}/{version}", get(catalog))
        .route("/library/offline-pages", get(list_pages).post(create_page))
        .route(
            "/library/offline-dashboards",
            get(list_dashboards).post(create_dashboard),
        )
        .route(
            "/library/offline-dashboards/{id}",
            get(open_dashboard).post(update_dashboard),
        )
        .route(
            "/library/offline-dashboards/{id}/export",
            post(export_dashboard),
        )
        .route(
            "/library/offline-dashboards/{id}/pages/{page_id}",
            get(open_dashboard_page).post(update_dashboard_page),
        )
        .route(
            "/library/offline-dashboards/{id}/pages/{page_id}/edit-session",
            post(dashboard_page_session),
        )
        .route(
            "/library/offline-dashboards/{id}/pages/{page_id}/edit-session/renew",
            post(renew_dashboard_page_session),
        )
        .route(
            "/library/offline-dashboards/{id}/pages/{page_id}/edit-session/release",
            post(release_dashboard_page_session),
        )
        .route(
            "/library/offline-pages/{id}",
            get(open_page).post(update_page),
        )
        .route(
            "/library/offline-pages/{id}/edit-session",
            post(edit_session),
        )
        .route(
            "/library/offline-pages/{id}/edit-session/renew",
            post(renew_session),
        )
        .route(
            "/library/offline-pages/{id}/edit-session/release",
            post(release_session),
        )
        .route("/library/template-import/choose", post(choose))
        .route("/library/template-import/{id}/inspect", post(inspect))
        .route("/library/template-import/{id}/run", post(run))
        .layer(Extension(FileDialogService::default()))
        .layer(Extension(Arc::new(PageEditSessions::default())))
}

#[derive(Serialize)]
struct FilePreview {
    file: String,
    #[serde(flatten)]
    outcome: PreviewOutcome,
}

impl FilePreview {
    fn rejected(file: String, code: &'static str) -> Self {
        Self {
            file,
            outcome: PreviewOutcome::Rejected {
                code,
                line: None,
                column: None,
            },
        }
    }
}

#[derive(Serialize)]
struct NativePreview {
    id: Option<String>,
    files: Vec<FilePreview>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum PreviewOutcome {
    Ready {
        catalogs: Vec<CatalogSummary>,
    },
    Rejected {
        code: &'static str,
        line: Option<u64>,
        column: Option<u64>,
    },
}

async fn list(
    Extension(handle): Extension<DraftsHandle>,
) -> Result<Json<Vec<CatalogSummary>>, Failure> {
    blocking(move || Ok(Json(CatalogLibrary::new(handle.root()?).list()?))).await
}

async fn catalog(
    Extension(handle): Extension<DraftsHandle>,
    Path((id, version)): Path<(String, u32)>,
) -> Response {
    let result = blocking(move || {
        CatalogLibrary::new(handle.root()?)
            .get(&id, version)?
            .map(|template| template.catalog)
            .ok_or(Failure::Code(StatusCode::NOT_FOUND, "catalog_not_found"))
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn list_pages(Extension(handle): Extension<DraftsHandle>) -> Response {
    let result = blocking(move || Ok(OfflinePageStore::new(handle.root()?).summaries()?)).await;
    respond(StatusCode::OK, result)
}

async fn list_dashboards(Extension(handle): Extension<DraftsHandle>) -> Response {
    let result =
        blocking(move || Ok(OfflineDashboardStore::new(handle.root()?).summaries()?)).await;
    respond(StatusCode::OK, result)
}

async fn open_dashboard(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
) -> Response {
    let result = blocking(move || {
        OfflineDashboardStore::new(handle.root()?)
            .get(&id)?
            .ok_or(Failure::Code(
                StatusCode::NOT_FOUND,
                "offline_dashboard_not_found",
            ))
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn create_dashboard(Extension(handle): Extension<DraftsHandle>, body: Bytes) -> Response {
    let Some(request) = CreateDashboard::parse_json(&body) else {
        return super::request_invalid().into_response();
    };
    let result =
        blocking(move || Ok(OfflineDashboardStore::new(handle.root()?).create(request)?)).await;
    respond(StatusCode::CREATED, result)
}

async fn update_dashboard(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let Some(request) = UpdateDashboard::parse_json(&body) else {
        return super::request_invalid().into_response();
    };
    let result =
        blocking(move || Ok(OfflineDashboardStore::new(handle.root()?).update(&id, request)?))
            .await;
    respond(StatusCode::OK, result)
}

/// Registers a one-time, locally authenticated download of the validated project/v2
/// package. Preparing the export never writes to the saved dashboard.
async fn export_dashboard(
    Extension(handle): Extension<DraftsHandle>,
    Extension(transfers): Extension<BrowserTransferService>,
    Path(id): Path<String>,
) -> Response {
    let display_name = format!("{id}.zip");
    let result = blocking(move || {
        OfflineDashboardStore::new(handle.root()?)
            .export_package(&id)?
            .ok_or(Failure::Code(
                StatusCode::NOT_FOUND,
                "offline_dashboard_not_found",
            ))
    })
    .await;
    let bytes = match result {
        Ok(bytes) => bytes,
        Err(error) => return error.into_response(),
    };
    match transfers.register_download(&display_name, bytes) {
        Ok(download) => (StatusCode::CREATED, Json(download)).into_response(),
        Err(error) => transfer_error(error),
    }
}

fn dashboard_page_key(id: &str, page_id: &str) -> String {
    format!("dashboard:{id}:page:{page_id}")
}

async fn open_dashboard_page(
    Extension(handle): Extension<DraftsHandle>,
    Path((id, page_id)): Path<(String, String)>,
) -> Response {
    let result = blocking(move || {
        OfflineDashboardStore::new(handle.root()?)
            .page(&id, &page_id)?
            .ok_or(Failure::Code(
                StatusCode::NOT_FOUND,
                "offline_page_not_found",
            ))
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn update_dashboard_page(
    Extension(handle): Extension<DraftsHandle>,
    Path((id, page_id)): Path<(String, String)>,
    Extension(sessions): Extension<Arc<PageEditSessions>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(request) = UpdatePage::parse_json(&body) else {
        return super::request_invalid().into_response();
    };
    let token = editor_token(&headers);
    let result = blocking(move || {
        let root = handle.root()?;
        Ok(sessions.with_owner(
            root.clone(),
            dashboard_page_key(&id, &page_id),
            token.as_deref(),
            || OfflineDashboardStore::new(root).update_page(&id, &page_id, request),
        )?)
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn dashboard_page_session(
    Extension(handle): Extension<DraftsHandle>,
    Extension(sessions): Extension<Arc<PageEditSessions>>,
    Path((id, page_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let request: AcquireEditor = match super::parse(&body) {
        Ok(request) => request,
        Err(failure) => return failure.into_response(),
    };
    let result = blocking(move || {
        let root = handle.root()?;
        if OfflineDashboardStore::new(root.clone())
            .page(&id, &page_id)?
            .is_none()
        {
            return Err(Failure::Code(
                StatusCode::NOT_FOUND,
                "offline_page_not_found",
            ));
        }
        Ok(sessions.acquire(root, dashboard_page_key(&id, &page_id), request.take_over)?)
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn renew_dashboard_page_session(
    Extension(handle): Extension<DraftsHandle>,
    Extension(sessions): Extension<Arc<PageEditSessions>>,
    Path((id, page_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let token = editor_token(&headers);
    let result = blocking(move || {
        let token = token.ok_or(Failure::Code(
            StatusCode::CONFLICT,
            "offline_page_edit_lost",
        ))?;
        sessions.renew(handle.root()?, dashboard_page_key(&id, &page_id), &token)?;
        Ok(serde_json::json!({"state":"editing"}))
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn release_dashboard_page_session(
    Extension(handle): Extension<DraftsHandle>,
    Extension(sessions): Extension<Arc<PageEditSessions>>,
    Path((id, page_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let token = editor_token(&headers);
    let result = blocking(move || {
        if let Some(token) = token {
            sessions.release(handle.root()?, dashboard_page_key(&id, &page_id), &token)?;
        }
        Ok(serde_json::json!({}))
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn open_page(Extension(handle): Extension<DraftsHandle>, Path(id): Path<String>) -> Response {
    let result = blocking(move || {
        OfflinePageStore::new(handle.root()?)
            .open(&id)?
            .ok_or(Failure::Code(
                StatusCode::NOT_FOUND,
                "offline_page_not_found",
            ))
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn create_page(Extension(handle): Extension<DraftsHandle>, body: Bytes) -> Response {
    let request = match CreatePage::parse_json(&body) {
        Some(request) => request,
        None => return super::request_invalid().into_response(),
    };
    let result = blocking(move || {
        Ok(OfflinePageStore::new(handle.root()?)
            .create(request)?
            .view()?)
    })
    .await;
    respond(StatusCode::CREATED, result)
}

async fn update_page(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
    Extension(sessions): Extension<Arc<PageEditSessions>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(request) = UpdatePage::parse_json(&body) else {
        return super::request_invalid().into_response();
    };
    let token = editor_token(&headers);
    let result = blocking(move || {
        let root = handle.root()?;
        Ok(sessions
            .with_owner(root.clone(), id.clone(), token.as_deref(), || {
                OfflinePageStore::new(root).update(&id, request)
            })?
            .view()?)
    })
    .await;
    respond(StatusCode::OK, result)
}

const EDITOR_HEADER: &str = "x-zdashstitch-editor";
fn editor_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(EDITOR_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|token| token.len() == 64)
        .map(str::to_owned)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AcquireEditor {
    take_over: bool,
}

async fn edit_session(
    Extension(handle): Extension<DraftsHandle>,
    Extension(sessions): Extension<Arc<PageEditSessions>>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let request: AcquireEditor = match super::parse(&body) {
        Ok(request) => request,
        Err(failure) => return failure.into_response(),
    };
    let result = blocking(move || {
        let root = handle.root()?;
        if OfflinePageStore::new(root.clone()).get(&id)?.is_none() {
            return Err(Failure::Code(
                StatusCode::NOT_FOUND,
                "offline_page_not_found",
            ));
        }
        Ok(sessions.acquire(root, id, request.take_over)?)
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn renew_session(
    Extension(handle): Extension<DraftsHandle>,
    Extension(sessions): Extension<Arc<PageEditSessions>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let token = editor_token(&headers);
    let result = blocking(move || {
        let token = token.ok_or(Failure::Code(
            StatusCode::CONFLICT,
            "offline_page_edit_lost",
        ))?;
        sessions.renew(handle.root()?, id, &token)?;
        Ok(serde_json::json!({"state":"editing"}))
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn release_session(
    Extension(handle): Extension<DraftsHandle>,
    Extension(sessions): Extension<Arc<PageEditSessions>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let token = editor_token(&headers);
    let result = blocking(move || {
        if let Some(token) = token {
            sessions.release(handle.root()?, id, &token)?;
        }
        Ok(serde_json::json!({}))
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn inspect(
    Extension(handle): Extension<DraftsHandle>,
    Extension(transfers): Extension<BrowserTransferService>,
    Path(id): Path<String>,
) -> Response {
    inspect_set(handle, &transfers, &id)
        .await
        .map(Json)
        .into_response()
}

async fn choose(
    Extension(handle): Extension<DraftsHandle>,
    Extension(transfers): Extension<BrowserTransferService>,
    Extension(dialog): Extension<FileDialogService>,
) -> Response {
    let paths = match dialog
        .choose_files(DialogCancellation::default(), Duration::from_secs(120))
        .await
    {
        Ok(paths) => paths,
        Err(crate::DialogErrorCode::UserCancelled) => {
            return Json(NativePreview {
                id: None,
                files: Vec::new(),
            })
            .into_response();
        }
        Err(error) => {
            return Failure::Code(StatusCode::SERVICE_UNAVAILABLE, error.as_str()).into_response();
        }
    };
    if paths.len() > MAX_FILES {
        return transfer_error(crate::TransferError::TooManyFiles);
    }
    let set = match transfers.create_upload_set() {
        Ok(set) => set,
        Err(error) => return transfer_error(error),
    };
    let import_transfers = transfers.clone();
    let set_id = set.id.clone();
    let staged = blocking(move || {
        let selections = SelectionRegistry::new(SelectionRegistryLimits {
            maximum_file_bytes: MAX_TEMPLATE_FILE_BYTES,
            ..SelectionRegistryLimits::default()
        });
        let mut rejected = Vec::new();
        let mut uploaded = 0;
        for path in paths {
            let file = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let bytes = selections
                .register(path, SelectionKind::File)
                .and_then(|selection| selections.read_file(&selection.id));
            let code = match bytes {
                Ok(bytes) => match import_transfers.add_file(&set_id, &file, Bytes::from(bytes)) {
                    Ok(_) => {
                        uploaded += 1;
                        continue;
                    }
                    Err(error) => error.message_id().as_str(),
                },
                Err(SelectionError::FileTooLarge) => "template_export_size",
                Err(_) => "template_export_file_read",
            };
            rejected.push(FilePreview::rejected(file, code));
        }
        Ok((uploaded, rejected))
    })
    .await;
    let (uploaded, rejected) = match staged {
        Ok(staged) => staged,
        Err(error) => return error.into_response(),
    };
    if uploaded == 0 {
        let _ = transfers.take_upload_set(&set.id);
        return Json(NativePreview {
            id: None,
            files: rejected,
        })
        .into_response();
    }
    match inspect_set(handle, &transfers, &set.id).await {
        Ok(mut files) => {
            files.extend(rejected);
            Json(NativePreview {
                id: Some(set.id),
                files,
            })
            .into_response()
        }
        Err(error) => error,
    }
}

async fn inspect_set(
    handle: DraftsHandle,
    transfers: &BrowserTransferService,
    id: &str,
) -> Result<Vec<FilePreview>, Response> {
    let files = match transfers.seal_upload_set(id) {
        Ok(files) => files,
        Err(error) => return Err(transfer_error(error)),
    };
    let result = blocking(move || {
        let library = CatalogLibrary::new(handle.root()?);
        let mut versions = BTreeMap::new();
        for summary in library.list()? {
            versions
                .entry(summary.catalog_id)
                .or_insert(summary.version);
        }
        let previews: Vec<_> = files
            .into_iter()
            .map(|file| {
                let outcome = match inspect_template_file(&file.relative_path, &file.bytes) {
                    Some(Ok(templates)) => PreviewOutcome::Ready {
                        catalogs: templates
                            .iter()
                            .map(|template| {
                                let latest = versions
                                    .entry(template.catalog.catalog_id.clone())
                                    .or_insert(0);
                                *latest += 1;
                                CatalogSummary::of(template, *latest)
                            })
                            .collect(),
                    },
                    Some(Err(error)) => PreviewOutcome::Rejected {
                        code: error.code(),
                        line: error.line(),
                        column: error.column(),
                    },
                    None => PreviewOutcome::Rejected {
                        code: "template_export_file_type",
                        line: None,
                        column: None,
                    },
                };
                FilePreview {
                    file: file.relative_path,
                    outcome,
                }
            })
            .collect();
        Ok(previews)
    })
    .await;
    let previews = result.map_err(IntoResponse::into_response)?;
    transfers.complete_inspection(id).map_err(transfer_error)?;
    Ok(previews)
}

async fn run(
    Extension(handle): Extension<DraftsHandle>,
    Extension(transfers): Extension<BrowserTransferService>,
    Path(id): Path<String>,
) -> Response {
    let files = match transfers.take_sealed_upload_set(&id) {
        Ok(files) => files,
        Err(error) => return transfer_error(error),
    };
    blocking(move || {
        let templates = files
            .into_iter()
            .filter_map(|file| inspect_template_file(&file.relative_path, &file.bytes)?.ok())
            .flatten()
            .collect();
        Ok(Json(CatalogLibrary::new(handle.root()?).save(templates)?))
    })
    .await
    .into_response()
}
