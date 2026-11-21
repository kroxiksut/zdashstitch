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
use sha2::{Digest, Sha256};
use zdashstitch_application::catalog_library::{
    CatalogLibrary, CatalogSummary, MAX_TEMPLATE_FILE_BYTES, inspect_template_file,
};
use zdashstitch_application::library_index::LibraryIndex;
use zdashstitch_application::offline_dashboards::{
    CreateDashboard, OfflineDashboardStore, UpdateDashboard,
};
use zdashstitch_application::offline_page_sessions::PageEditSessions;
use zdashstitch_application::offline_pages::{CreatePage, OfflinePageStore, UpdatePage};
use zdashstitch_application::page_library::PageLibrary;
use zdashstitch_application::portable_import::{InspectedPackage, inspect_package};
use zdashstitch_application::snapshot_library::SnapshotLibrary;
use zdashstitch_core::formats::{
    LegacyRootSelection, ProjectRoot, TypedPortableBundle, build_typed_package_bytes,
    build_typed_yaml_bytes, package_entry_hashes,
};

use super::{DraftsHandle, Failure, blocking, respond};
use crate::{
    BrowserTransferService, DialogCancellation, FileDialogService, SelectionError, SelectionKind,
    SelectionRegistry, SelectionRegistryLimits,
    browser_transfer::{MAX_FILES, transfer_error},
    export_jobs::{ExportJob, ExportJobs},
    security::LocalSecurity,
};

#[path = "reusable_routes.rs"]
mod reusable_routes;

pub(super) fn routes() -> Router<LocalSecurity> {
    Router::new()
        .merge(reusable_routes::routes())
        .route("/library/export-jobs", post(create_export_job))
        .route("/library/export-jobs/{id}", get(export_job_status))
        .route("/library/export-jobs/{id}/cancel", post(cancel_export_job))
        .route("/library/index", get(library_index))
        .route(
            "/library/package-import/{id}/inspect",
            post(inspect_packages),
        )
        .route("/library/package-import/{id}/run", post(import_packages))
        .route("/library/snapshots", get(list_snapshots))
        .route("/library/snapshots/compare", post(compare_snapshots))
        .route(
            "/library/snapshots/{id}/delete-preview",
            get(snapshot_delete_preview),
        )
        .route("/library/snapshots/{id}/delete", post(delete_snapshot))
        .route("/library/catalogs", get(list))
        .route(
            "/library/catalogs/{library_id}/duplicate",
            post(duplicate_catalog),
        )
        .route(
            "/library/catalogs/{library_id}/rename",
            post(rename_catalog),
        )
        .route(
            "/library/catalogs/{library_id}/delete-preview",
            get(catalog_delete_preview),
        )
        .route(
            "/library/catalogs/{library_id}/delete",
            post(delete_catalog),
        )
        .route("/library/catalogs/{id}/{version}", get(catalog))
        .route("/library/offline-pages", get(list_pages).post(create_page))
        .route("/library/pages", get(list_library_pages))
        .route(
            "/library/pages/{id}/duplicate",
            post(duplicate_library_page),
        )
        .route("/library/pages/{id}/rename", post(rename_library_page))
        .route("/library/pages/{id}/delete", post(delete_library_page))
        .route("/library/pages/{id}/take", post(take_library_page))
        .route("/library/pages/from-draft/{id}", post(save_library_page))
        .route(
            "/library/pages/from-workcopy/{id}",
            post(save_workcopy_library_page),
        )
        .route(
            "/library/pages/from-workcopy/{id}/pages/{page_id}",
            post(save_dashboard_workcopy_library_page),
        )
        .route(
            "/library/pages/from-dashboard/{dashboard}/{page}",
            post(save_dashboard_library_page),
        )
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
        .route("/library/offline-pages/{id}/export", post(export_page))
        .route(
            "/library/offline-pages/{id}/save-export",
            post(save_page_export),
        )
        .route(
            "/library/offline-dashboards/{id}/save-export",
            post(save_dashboard_export),
        )
        .route(
            "/library/offline-pages/{id}/export-yaml",
            post(export_page_yaml),
        )
        .route(
            "/library/offline-dashboards/{id}/export-yaml",
            post(export_dashboard_yaml),
        )
        .route(
            "/library/offline-pages/{id}/sensitive-preview",
            get(scan_page),
        )
        .route(
            "/library/offline-dashboards/{id}/sensitive-preview",
            get(scan_dashboard),
        )
        .route(
            "/library/offline-pages/{id}/redaction-preview",
            post(redaction_page_preview),
        )
        .route(
            "/library/offline-dashboards/{id}/redaction-preview",
            post(redaction_dashboard_preview),
        )
        .route(
            "/library/offline-pages/{id}/redaction-export",
            post(redaction_page_export),
        )
        .route(
            "/library/offline-dashboards/{id}/redaction-export",
            post(redaction_dashboard_export),
        )
        .route(
            "/library/offline-pages/{id}/redaction-save",
            post(redaction_page_save),
        )
        .route(
            "/library/offline-dashboards/{id}/redaction-save",
            post(redaction_dashboard_save),
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
        .layer(Extension(ExportJobs::default()))
        .layer(Extension(Arc::new(PageEditSessions::default())))
}

async fn create_export_job(Extension(jobs): Extension<ExportJobs>) -> Response {
    match jobs.create() {
        Some(id) => (StatusCode::CREATED, Json(serde_json::json!({"id": id}))).into_response(),
        None => Failure::Code(StatusCode::SERVICE_UNAVAILABLE, "internal_error").into_response(),
    }
}
async fn export_job_status(
    Extension(jobs): Extension<ExportJobs>,
    Path(id): Path<String>,
) -> Response {
    match jobs.get(&id).and_then(|job| job.status()) {
        Some(status) => Json(status).into_response(),
        None => Failure::Code(StatusCode::NOT_FOUND, "request_invalid").into_response(),
    }
}
async fn cancel_export_job(
    Extension(jobs): Extension<ExportJobs>,
    Path(id): Path<String>,
) -> Response {
    match jobs.get(&id) {
        Some(job) => Json(serde_json::json!({"cancelled": job.cancel()})).into_response(),
        None => Failure::Code(StatusCode::NOT_FOUND, "request_invalid").into_response(),
    }
}
struct ExportGuard(Arc<ExportJob>);
impl Drop for ExportGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn library_index(Extension(handle): Extension<DraftsHandle>) -> Response {
    let result = blocking(move || Ok(LibraryIndex::open(handle.root()?)?)).await;
    respond(StatusCode::OK, result)
}

async fn list_snapshots(Extension(handle): Extension<DraftsHandle>) -> Response {
    let result = blocking(move || Ok(SnapshotLibrary::open(handle.root()?)?)).await;
    respond(StatusCode::OK, result)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompareSnapshotsRequest {
    left: String,
    right: String,
}

async fn compare_snapshots(
    Extension(handle): Extension<DraftsHandle>,
    Json(request): Json<CompareSnapshotsRequest>,
) -> Response {
    let result = blocking(move || {
        Ok(SnapshotLibrary::compare(
            handle.root()?,
            &request.left,
            &request.right,
        )?)
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn snapshot_delete_preview(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
) -> Response {
    let result =
        blocking(move || Ok(SnapshotLibrary::deletion_preview(handle.root()?, &id)?)).await;
    respond(StatusCode::OK, result)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteSnapshotRequest {
    confirmed: bool,
}

async fn delete_snapshot(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
    Json(request): Json<DeleteSnapshotRequest>,
) -> Response {
    let result = blocking(move || {
        SnapshotLibrary::delete(handle.root()?, &id, request.confirmed)?;
        Ok(serde_json::json!({"deleted": true}))
    })
    .await;
    respond(StatusCode::OK, result)
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

async fn duplicate_catalog(
    Extension(handle): Extension<DraftsHandle>,
    Path(library_id): Path<String>,
    body: Bytes,
) -> Response {
    let Some(request) = parse_page_mutation(&body) else {
        return super::request_invalid().into_response();
    };
    let result =
        blocking(move || {
            Ok(CatalogLibrary::new(handle.root()?)
                .duplicate(&library_id, request.expected_revision)?)
        })
        .await;
    respond(StatusCode::CREATED, result)
}

async fn rename_catalog(
    Extension(handle): Extension<DraftsHandle>,
    Path(library_id): Path<String>,
    body: Bytes,
) -> Response {
    let Some(request) = parse_page_mutation(&body) else {
        return super::request_invalid().into_response();
    };
    let Some(name) = request.display_name else {
        return super::request_invalid().into_response();
    };
    let result = blocking(move || {
        Ok(CatalogLibrary::new(handle.root()?).rename(
            &library_id,
            request.expected_revision,
            &name,
        )?)
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn catalog_delete_preview(
    Extension(handle): Extension<DraftsHandle>,
    Path(library_id): Path<String>,
) -> Response {
    let result =
        blocking(move || Ok(CatalogLibrary::new(handle.root()?).deletion_preview(&library_id)?))
            .await;
    respond(StatusCode::OK, result)
}

async fn delete_catalog(
    Extension(handle): Extension<DraftsHandle>,
    Path(library_id): Path<String>,
    body: Bytes,
) -> Response {
    let Some(request) = parse_page_mutation(&body) else {
        return super::request_invalid().into_response();
    };
    let Some(name) = request.confirmed_name else {
        return super::request_invalid().into_response();
    };
    let result = blocking(move || {
        CatalogLibrary::new(handle.root()?).delete(
            &library_id,
            request.expected_revision,
            &name,
        )?;
        Ok(serde_json::json!({}))
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn list_pages(Extension(handle): Extension<DraftsHandle>) -> Response {
    let result = blocking(move || Ok(OfflinePageStore::new(handle.root()?).summaries()?)).await;
    respond(StatusCode::OK, result)
}

async fn list_library_pages(Extension(handle): Extension<DraftsHandle>) -> Response {
    let result = blocking(move || Ok(PageLibrary::new(handle.root()?).list()?)).await;
    respond(StatusCode::OK, result)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageMutation {
    expected_revision: u64,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    confirmed_name: Option<String>,
}

fn parse_page_mutation(body: &[u8]) -> Option<PageMutation> {
    (body.len() <= 4096)
        .then(|| serde_json::from_slice(body).ok())
        .flatten()
}

async fn duplicate_library_page(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let Some(request) = parse_page_mutation(&body) else {
        return super::request_invalid().into_response();
    };
    let result = blocking(move || {
        Ok(PageLibrary::new(handle.root()?).duplicate(&id, request.expected_revision)?)
    })
    .await;
    respond(StatusCode::CREATED, result)
}

async fn rename_library_page(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let Some(request) = parse_page_mutation(&body) else {
        return super::request_invalid().into_response();
    };
    let Some(name) = request.display_name else {
        return super::request_invalid().into_response();
    };
    let result = blocking(move || {
        Ok(PageLibrary::new(handle.root()?).rename(&id, request.expected_revision, &name)?)
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn delete_library_page(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let Some(request) = parse_page_mutation(&body) else {
        return super::request_invalid().into_response();
    };
    let Some(name) = request.confirmed_name else {
        return super::request_invalid().into_response();
    };
    let result = blocking(move || {
        PageLibrary::new(handle.root()?).delete(&id, request.expected_revision, &name)?;
        Ok(serde_json::json!({}))
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn save_library_page(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
) -> Response {
    let result = blocking(move || Ok(PageLibrary::new(handle.root()?).save_draft(&id)?)).await;
    respond(StatusCode::CREATED, result)
}

async fn take_library_page(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
) -> Response {
    let result =
        blocking(move || Ok(PageLibrary::new(handle.root()?).take_page(&id)?.view()?)).await;
    respond(StatusCode::CREATED, result)
}

async fn save_dashboard_library_page(
    Extension(handle): Extension<DraftsHandle>,
    Path((dashboard, page)): Path<(String, String)>,
) -> Response {
    let result = blocking(move || {
        Ok(PageLibrary::new(handle.root()?).save_dashboard_page(&dashboard, &page)?)
    })
    .await;
    respond(StatusCode::CREATED, result)
}

async fn save_workcopy_library_page(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
) -> Response {
    let result =
        blocking(move || Ok(PageLibrary::new(handle.root()?).save_workcopy_page(&id)?)).await;
    respond(StatusCode::CREATED, result)
}

async fn save_dashboard_workcopy_library_page(
    Extension(handle): Extension<DraftsHandle>,
    Path((id, page_id)): Path<(String, String)>,
) -> Response {
    let result = blocking(move || {
        Ok(PageLibrary::new(handle.root()?).save_dashboard_workcopy_page(&id, &page_id)?)
    })
    .await;
    respond(StatusCode::CREATED, result)
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
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageExportRequest {
    #[serde(default)]
    expected_revision: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveExportRequest {
    expected_revision: u64,
    yaml: bool,
    job_id: Option<String>,
}

async fn save_page_export(
    Extension(handle): Extension<DraftsHandle>,
    Extension(dialog): Extension<FileDialogService>,
    Extension(jobs): Extension<ExportJobs>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    save_export(handle, dialog, jobs, id, body, false).await
}

async fn save_dashboard_export(
    Extension(handle): Extension<DraftsHandle>,
    Extension(dialog): Extension<FileDialogService>,
    Extension(jobs): Extension<ExportJobs>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    save_export(handle, dialog, jobs, id, body, true).await
}

async fn save_export(
    handle: DraftsHandle,
    dialog: FileDialogService,
    jobs: ExportJobs,
    id: String,
    body: Bytes,
    dashboard: bool,
) -> Response {
    let Some(request) = std::str::from_utf8(&body).ok().and_then(|s| {
        zdashstitch_core::formats::strict::json_from_str::<SaveExportRequest>(s).ok()
    }) else {
        return super::request_invalid().into_response();
    };
    let Some(job) = request.job_id.as_deref().and_then(|id| jobs.get(id)) else {
        return super::request_invalid().into_response();
    };
    if !job.claim() {
        return if job.is_cancelled() {
            Json(serde_json::json!({"cancelled": true})).into_response()
        } else {
            super::request_invalid().into_response()
        };
    }
    let _guard = ExportGuard(job.clone());
    if !job.stage("choosing") {
        return Json(serde_json::json!({"cancelled": true})).into_response();
    }
    let selected = match dialog
        .choose(
            crate::DialogIntent::SaveFile,
            job.dialog.clone(),
            Duration::from_secs(120),
        )
        .await
    {
        Ok(path) => path,
        Err(crate::DialogErrorCode::UserCancelled) => {
            job.cancel();
            return Json(serde_json::json!({"cancelled": true})).into_response();
        }
        Err(error) => {
            return Failure::Code(StatusCode::SERVICE_UNAVAILABLE, error.as_str()).into_response();
        }
    };
    let extension_ok = selected.extension().is_some_and(|ext| {
        if request.yaml {
            ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml")
        } else {
            ext.eq_ignore_ascii_case("zip")
        }
    });
    if !extension_ok {
        return Failure::Code(StatusCode::BAD_REQUEST, "request_invalid").into_response();
    }
    if !job.stage("preparing") {
        return Json(serde_json::json!({"cancelled": true})).into_response();
    }
    let worker_job = job.clone();
    let result = blocking(move || {
        let root = handle.root()?;
        let prepared_result = if dashboard {
            let item = OfflineDashboardStore::new(root).get(&id)?.ok_or(Failure::Code(StatusCode::NOT_FOUND, "offline_dashboard_not_found"))?;
            prepare_checked_package_with_progress(&item.bundle, item.revision, Some(request.expected_revision), request.yaml, "offline_dashboard_conflict", "offline_dashboard_export_failed", Some(&worker_job))
        } else {
            let item = OfflinePageStore::new(root).get(&id)?.ok_or(Failure::Code(StatusCode::NOT_FOUND, "offline_page_not_found"))?;
            prepare_checked_package_with_progress(&item.bundle, item.revision, Some(request.expected_revision), request.yaml, "offline_page_conflict", "offline_page_export_failed", Some(&worker_job))
        };
        let prepared = match prepared_result { Ok(value) => value, Err(_) if worker_job.is_cancelled() => return Ok(serde_json::json!({"cancelled": true})), Err(error) => return Err(error) };
        if !worker_job.stage("writing") { return Ok(serde_json::json!({"cancelled": true})); }
        match crate::selections::save_new_file_before_commit(&selected, &prepared.bytes, || worker_job.begin_commit()) {
            Ok(()) => {}, Err(SelectionError::Cancelled) => return Ok(serde_json::json!({"cancelled": true})), Err(_) => return Err(Failure::Code(StatusCode::CONFLICT, "portable_save_failed")),
        }
        worker_job.finish(true);
        Ok(serde_json::json!({"cancelled": false, "path": selected.to_string_lossy(), "sha256": prepared.sha256, "size_bytes": prepared.bytes.len(), "entry_hashes": prepared.entries}))
    }).await;
    respond(StatusCode::CREATED, result)
}

fn parse_export_request(body: &[u8]) -> Option<PackageExportRequest> {
    if body.is_empty() {
        return Some(PackageExportRequest::default());
    }
    zdashstitch_core::formats::strict::json_from_str(std::str::from_utf8(body).ok()?).ok()
}

async fn export_dashboard(
    Extension(handle): Extension<DraftsHandle>,
    Extension(transfers): Extension<BrowserTransferService>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let Some(request) = parse_export_request(&body) else {
        return super::request_invalid().into_response();
    };
    let display_name = format!("{id}.zip");
    let result = blocking(move || {
        let dashboard =
            OfflineDashboardStore::new(handle.root()?)
                .get(&id)?
                .ok_or(Failure::Code(
                    StatusCode::NOT_FOUND,
                    "offline_dashboard_not_found",
                ))?;
        prepare_checked_package(
            &dashboard.bundle,
            dashboard.revision,
            request.expected_revision,
            false,
            "offline_dashboard_conflict",
            "offline_dashboard_export_failed",
        )
    })
    .await;
    let prepared = match result {
        Ok(prepared) => prepared,
        Err(error) => return error.into_response(),
    };
    register_package_download(&transfers, &display_name, prepared)
}

async fn export_page(
    Extension(handle): Extension<DraftsHandle>,
    Extension(transfers): Extension<BrowserTransferService>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let Some(request) = parse_export_request(&body) else {
        return super::request_invalid().into_response();
    };
    let display_name = format!("{id}.zip");
    let result = blocking(move || {
        let page = OfflinePageStore::new(handle.root()?)
            .get(&id)?
            .ok_or(Failure::Code(
                StatusCode::NOT_FOUND,
                "offline_page_not_found",
            ))?;
        prepare_checked_package(
            &page.bundle,
            page.revision,
            request.expected_revision,
            false,
            "offline_page_conflict",
            "offline_page_export_failed",
        )
    })
    .await;
    let prepared = match result {
        Ok(prepared) => prepared,
        Err(error) => return error.into_response(),
    };
    register_package_download(&transfers, &display_name, prepared)
}

fn package_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

struct PreparedPackage {
    bytes: Vec<u8>,
    sha256: String,
    entries: BTreeMap<String, String>,
}

fn prepare_checked_package(
    bundle: &TypedPortableBundle,
    revision: u64,
    expected: Option<u64>,
    yaml: bool,
    conflict: &'static str,
    failure: &'static str,
) -> Result<PreparedPackage, Failure> {
    prepare_checked_package_with_progress(bundle, revision, expected, yaml, conflict, failure, None)
}

fn progress(job: Option<&ExportJob>, phase: &'static str) -> Result<(), Failure> {
    if job.is_some_and(|job| !job.stage(phase)) {
        return Err(Failure::Code(StatusCode::CONFLICT, "user_cancelled"));
    }
    Ok(())
}

fn prepare_checked_package_with_progress(
    bundle: &TypedPortableBundle,
    revision: u64,
    expected: Option<u64>,
    yaml: bool,
    conflict: &'static str,
    failure: &'static str,
    job: Option<&ExportJob>,
) -> Result<PreparedPackage, Failure> {
    if expected.is_some_and(|value| value != revision) {
        return Err(Failure::Code(StatusCode::CONFLICT, conflict));
    }
    progress(job, "scanning")?;
    let scan = zdashstitch_application::portable_scan::scan(bundle)
        .map_err(|_| Failure::Code(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"))?;
    if scan.credential_candidates > 0 {
        return Err(Failure::Code(
            StatusCode::CONFLICT,
            "redaction_credentials_present",
        ));
    }
    progress(job, "validating")?;
    bundle
        .validate()
        .map_err(|_| Failure::Code(StatusCode::CONFLICT, failure))?;
    progress(job, "packaging")?;
    let bytes = if yaml {
        build_typed_yaml_bytes(bundle)
    } else {
        build_typed_package_bytes(bundle)
    }
    .map_err(|_| Failure::Code(StatusCode::CONFLICT, failure))?;
    progress(job, "verifying")?;
    prepare_package_download(bytes)
}

#[cfg(test)]
mod package_export_safety_tests {
    use super::*;

    #[test]
    fn ordinary_exports_also_reject_recognized_credentials() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap_or_else(|| panic!("workspace"));
        let mut bundle = zdashstitch_core::formats::load_typed_project_file(
            &root.join("examples/clock-page-v2.yaml"),
        )
        .unwrap_or_else(|error| panic!("fixture: {error}"));
        let ProjectRoot::Page(page) = &mut bundle.project.root else {
            panic!("page")
        };
        page.name = "password=synthetic-test-value".into();
        assert!(matches!(
            prepare_checked_package(&bundle, 1, Some(1), false, "conflict", "failed"),
            Err(Failure::Code(
                StatusCode::CONFLICT,
                "redaction_credentials_present"
            ))
        ));
    }

    #[test]
    fn save_preparation_tracks_verification_and_obeys_cancellation() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap_or_else(|| panic!("workspace"));
        let bundle = zdashstitch_core::formats::load_typed_project_file(
            &root.join("examples/clock-page-v2.yaml"),
        )
        .unwrap_or_else(|error| panic!("fixture: {error}"));
        for yaml in [false, true] {
            let jobs = ExportJobs::default();
            let id = jobs.create().unwrap_or_else(|| panic!("job"));
            let job = jobs.get(&id).unwrap_or_else(|| panic!("job"));
            assert!(job.claim());
            let prepared = prepare_checked_package_with_progress(
                &bundle,
                1,
                Some(1),
                yaml,
                "conflict",
                "failed",
                Some(&job),
            )
            .unwrap_or_else(|_| panic!("prepare"));
            assert!(!prepared.bytes.is_empty());
            assert_eq!(
                serde_json::to_value(job.status()).unwrap_or_default()["phase"],
                "verifying"
            );
            assert!(job.cancel());
            assert!(matches!(
                prepare_checked_package_with_progress(
                    &bundle,
                    1,
                    Some(1),
                    yaml,
                    "conflict",
                    "failed",
                    Some(&job)
                ),
                Err(Failure::Code(StatusCode::CONFLICT, "user_cancelled"))
            ));
        }
    }
}

fn prepare_package_download(bytes: Vec<u8>) -> Result<PreparedPackage, Failure> {
    let sha256 = package_sha256(&bytes);
    let entries = if bytes.starts_with(b"PK") {
        package_entry_hashes(&bytes)
            .map_err(|_| Failure::Code(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"))?
    } else {
        BTreeMap::new()
    };
    Ok(PreparedPackage {
        bytes,
        sha256,
        entries,
    })
}

async fn export_page_yaml(
    Extension(handle): Extension<DraftsHandle>,
    Extension(transfers): Extension<BrowserTransferService>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let Some(request) = parse_export_request(&body) else {
        return super::request_invalid().into_response();
    };
    let name = format!("{id}.yaml");
    let result = blocking(move || {
        let page = OfflinePageStore::new(handle.root()?)
            .get(&id)?
            .ok_or(Failure::Code(
                StatusCode::NOT_FOUND,
                "offline_page_not_found",
            ))?;
        prepare_checked_package(
            &page.bundle,
            page.revision,
            request.expected_revision,
            true,
            "offline_page_conflict",
            "offline_page_export_failed",
        )
    })
    .await;
    match result {
        Ok(prepared) => register_package_download(&transfers, &name, prepared),
        Err(error) => error.into_response(),
    }
}

async fn export_dashboard_yaml(
    Extension(handle): Extension<DraftsHandle>,
    Extension(transfers): Extension<BrowserTransferService>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let Some(request) = parse_export_request(&body) else {
        return super::request_invalid().into_response();
    };
    let name = format!("{id}.yaml");
    let result = blocking(move || {
        let dashboard =
            OfflineDashboardStore::new(handle.root()?)
                .get(&id)?
                .ok_or(Failure::Code(
                    StatusCode::NOT_FOUND,
                    "offline_dashboard_not_found",
                ))?;
        prepare_checked_package(
            &dashboard.bundle,
            dashboard.revision,
            request.expected_revision,
            true,
            "offline_dashboard_conflict",
            "offline_dashboard_export_failed",
        )
    })
    .await;
    match result {
        Ok(prepared) => register_package_download(&transfers, &name, prepared),
        Err(error) => error.into_response(),
    }
}

fn register_package_download(
    transfers: &BrowserTransferService,
    name: &str,
    prepared: PreparedPackage,
) -> Response {
    let size_bytes = prepared.bytes.len();
    match transfers.register_download(name, prepared.bytes) {
        Ok(download) => (StatusCode::CREATED, Json(serde_json::json!({
            "id": download.id, "display_name": download.display_name,
            "expires_in_seconds": download.expires_in_seconds, "sha256": prepared.sha256, "size_bytes": size_bytes, "entry_hashes": prepared.entries
        }))).into_response(),
        Err(error) => transfer_error(error),
    }
}

#[derive(Serialize)]
struct PackageScanResponse {
    revision: u64,
    #[serde(flatten)]
    scan: zdashstitch_application::portable_scan::CandidateScan,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RedactionPreviewRequest {
    expected_revision: u64,
    changes: Vec<zdashstitch_application::portable_redaction::RedactionChange>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RedactionExportRequest {
    expected_revision: u64,
    changes: Vec<zdashstitch_application::portable_redaction::RedactionChange>,
    preview_sha256: String,
    acknowledged: bool,
    #[serde(default)]
    job_id: Option<String>,
}

async fn redaction_page_save(
    Extension(handle): Extension<DraftsHandle>,
    Extension(dialog): Extension<FileDialogService>,
    Extension(jobs): Extension<ExportJobs>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    redaction_save(handle, dialog, jobs, id, body, false).await
}

async fn redaction_dashboard_save(
    Extension(handle): Extension<DraftsHandle>,
    Extension(dialog): Extension<FileDialogService>,
    Extension(jobs): Extension<ExportJobs>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    redaction_save(handle, dialog, jobs, id, body, true).await
}

async fn redaction_save(
    handle: DraftsHandle,
    dialog: FileDialogService,
    jobs: ExportJobs,
    id: String,
    body: Bytes,
    dashboard: bool,
) -> Response {
    let Some(request) = std::str::from_utf8(&body).ok().and_then(|s| {
        zdashstitch_core::formats::strict::json_from_str::<RedactionExportRequest>(s).ok()
    }) else {
        return super::request_invalid().into_response();
    };
    let Some(job) = request.job_id.as_deref().and_then(|id| jobs.get(id)) else {
        return super::request_invalid().into_response();
    };
    if !job.claim() {
        return if job.is_cancelled() {
            Json(serde_json::json!({"cancelled": true})).into_response()
        } else {
            super::request_invalid().into_response()
        };
    }
    let _guard = ExportGuard(job.clone());
    if !job.stage("choosing") {
        return Json(serde_json::json!({"cancelled": true})).into_response();
    }
    let selected = match dialog
        .choose(
            crate::DialogIntent::SaveFile,
            job.dialog.clone(),
            Duration::from_secs(120),
        )
        .await
    {
        Ok(path) => path,
        Err(crate::DialogErrorCode::UserCancelled) => {
            job.cancel();
            return Json(serde_json::json!({"cancelled": true})).into_response();
        }
        Err(error) => {
            return Failure::Code(StatusCode::SERVICE_UNAVAILABLE, error.as_str()).into_response();
        }
    };
    if !selected
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
    {
        return Failure::Code(StatusCode::BAD_REQUEST, "request_invalid").into_response();
    }
    if !job.stage("preparing") {
        return Json(serde_json::json!({"cancelled": true})).into_response();
    }
    let worker_job = job.clone();
    let result = blocking(move || {
        let root = handle.root()?;
        let prepared_result = if dashboard {
            let item = OfflineDashboardStore::new(root).get(&id)?.ok_or(Failure::Code(StatusCode::NOT_FOUND, "offline_dashboard_not_found"))?;
            prepare_redacted_export_with_progress(&item.bundle, item.revision, request, "offline_dashboard_conflict", Some(&worker_job))
        } else {
            let item = OfflinePageStore::new(root).get(&id)?.ok_or(Failure::Code(StatusCode::NOT_FOUND, "offline_page_not_found"))?;
            prepare_redacted_export_with_progress(&item.bundle, item.revision, request, "offline_page_conflict", Some(&worker_job))
        };
        let prepared = match prepared_result { Ok(value) => value, Err(_) if worker_job.is_cancelled() => return Ok(serde_json::json!({"cancelled": true})), Err(error) => return Err(error) };
        if !worker_job.stage("writing") { return Ok(serde_json::json!({"cancelled": true})); }
        match crate::selections::save_new_file_before_commit(&selected, &prepared.bytes, || worker_job.begin_commit()) {
            Ok(()) => {}, Err(SelectionError::Cancelled) => return Ok(serde_json::json!({"cancelled": true})), Err(_) => return Err(Failure::Code(StatusCode::CONFLICT, "portable_save_failed")),
        }
        worker_job.finish(true);
        Ok(serde_json::json!({"cancelled":false,"path":selected.to_string_lossy(),"sha256":prepared.sha256,"size_bytes":prepared.bytes.len()}))
    }).await;
    respond(StatusCode::CREATED, result)
}

#[derive(Serialize)]
struct RedactionPreviewResponse {
    revision: u64,
    sha256: String,
    changes: Vec<zdashstitch_application::portable_redaction::ChangeSummary>,
    remaining: zdashstitch_application::portable_scan::CandidateScan,
}

fn redaction_preview(
    bundle: &TypedPortableBundle,
    revision: u64,
    request: RedactionPreviewRequest,
    conflict: &'static str,
) -> Result<RedactionPreviewResponse, Failure> {
    if revision != request.expected_revision {
        return Err(Failure::Code(StatusCode::CONFLICT, conflict));
    }
    let preview = zdashstitch_application::portable_redaction::preview(bundle, &request.changes)
        .map_err(redaction_failure)?;
    let bytes = build_typed_package_bytes(&preview.bundle)
        .map_err(|_| Failure::Code(StatusCode::CONFLICT, "request_invalid"))?;
    Ok(RedactionPreviewResponse {
        revision,
        sha256: package_sha256(&bytes),
        changes: preview.changes,
        remaining: preview.remaining,
    })
}

fn redaction_failure(
    error: zdashstitch_application::portable_redaction::RedactionError,
) -> Failure {
    use zdashstitch_application::portable_redaction::RedactionError;
    Failure::Code(
        StatusCode::CONFLICT,
        match error {
            RedactionError::Invalid => "redaction_invalid",
            RedactionError::Protected => "redaction_protected",
        },
    )
}

async fn redaction_page_preview(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let Some(request) = std::str::from_utf8(&body).ok().and_then(|text| {
        zdashstitch_core::formats::strict::json_from_str::<RedactionPreviewRequest>(text).ok()
    }) else {
        return super::request_invalid().into_response();
    };
    let result = blocking(move || {
        let page = OfflinePageStore::new(handle.root()?)
            .get(&id)?
            .ok_or(Failure::Code(
                StatusCode::NOT_FOUND,
                "offline_page_not_found",
            ))?;
        redaction_preview(
            &page.bundle,
            page.revision,
            request,
            "offline_page_conflict",
        )
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn redaction_dashboard_preview(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let Some(request) = std::str::from_utf8(&body).ok().and_then(|text| {
        zdashstitch_core::formats::strict::json_from_str::<RedactionPreviewRequest>(text).ok()
    }) else {
        return super::request_invalid().into_response();
    };
    let result = blocking(move || {
        let dashboard =
            OfflineDashboardStore::new(handle.root()?)
                .get(&id)?
                .ok_or(Failure::Code(
                    StatusCode::NOT_FOUND,
                    "offline_dashboard_not_found",
                ))?;
        redaction_preview(
            &dashboard.bundle,
            dashboard.revision,
            request,
            "offline_dashboard_conflict",
        )
    })
    .await;
    respond(StatusCode::OK, result)
}

fn prepare_redacted_export(
    bundle: &TypedPortableBundle,
    revision: u64,
    request: RedactionExportRequest,
    conflict: &'static str,
) -> Result<PreparedPackage, Failure> {
    prepare_redacted_export_with_progress(bundle, revision, request, conflict, None)
}

fn prepare_redacted_export_with_progress(
    bundle: &TypedPortableBundle,
    revision: u64,
    request: RedactionExportRequest,
    conflict: &'static str,
    job: Option<&ExportJob>,
) -> Result<PreparedPackage, Failure> {
    if revision != request.expected_revision {
        return Err(Failure::Code(StatusCode::CONFLICT, conflict));
    }
    if !request.acknowledged
        || request.preview_sha256.len() != 64
        || !request
            .preview_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(Failure::Code(StatusCode::BAD_REQUEST, "request_invalid"));
    }
    progress(job, "scanning")?;
    zdashstitch_application::portable_scan::scan(bundle)
        .map_err(|_| Failure::Code(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"))?;
    progress(job, "transforming")?;
    let preview = zdashstitch_application::portable_redaction::preview(bundle, &request.changes)
        .map_err(redaction_failure)?;
    if preview.remaining.credential_candidates > 0 {
        return Err(Failure::Code(
            StatusCode::CONFLICT,
            "redaction_credentials_present",
        ));
    }
    progress(job, "validating")?;
    preview
        .bundle
        .validate()
        .map_err(|_| Failure::Code(StatusCode::CONFLICT, "redaction_invalid"))?;
    progress(job, "packaging")?;
    let bytes = build_typed_package_bytes(&preview.bundle)
        .map_err(|_| Failure::Code(StatusCode::CONFLICT, "request_invalid"))?;
    progress(job, "verifying")?;
    let prepared = prepare_package_download(bytes)?;
    if prepared.sha256 != request.preview_sha256 {
        return Err(Failure::Code(StatusCode::CONFLICT, "request_invalid"));
    }
    Ok(prepared)
}

async fn redaction_page_export(
    Extension(handle): Extension<DraftsHandle>,
    Extension(transfers): Extension<BrowserTransferService>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let Some(request) = std::str::from_utf8(&body).ok().and_then(|text| {
        zdashstitch_core::formats::strict::json_from_str::<RedactionExportRequest>(text).ok()
    }) else {
        return super::request_invalid().into_response();
    };
    let name = format!("{id}-copy.zip");
    let result = blocking(move || {
        let page = OfflinePageStore::new(handle.root()?)
            .get(&id)?
            .ok_or(Failure::Code(
                StatusCode::NOT_FOUND,
                "offline_page_not_found",
            ))?;
        prepare_redacted_export(
            &page.bundle,
            page.revision,
            request,
            "offline_page_conflict",
        )
    })
    .await;
    match result {
        Ok(prepared) => register_package_download(&transfers, &name, prepared),
        Err(error) => error.into_response(),
    }
}

async fn redaction_dashboard_export(
    Extension(handle): Extension<DraftsHandle>,
    Extension(transfers): Extension<BrowserTransferService>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let Some(request) = std::str::from_utf8(&body).ok().and_then(|text| {
        zdashstitch_core::formats::strict::json_from_str::<RedactionExportRequest>(text).ok()
    }) else {
        return super::request_invalid().into_response();
    };
    let name = format!("{id}-copy.zip");
    let result = blocking(move || {
        let dashboard =
            OfflineDashboardStore::new(handle.root()?)
                .get(&id)?
                .ok_or(Failure::Code(
                    StatusCode::NOT_FOUND,
                    "offline_dashboard_not_found",
                ))?;
        prepare_redacted_export(
            &dashboard.bundle,
            dashboard.revision,
            request,
            "offline_dashboard_conflict",
        )
    })
    .await;
    match result {
        Ok(prepared) => register_package_download(&transfers, &name, prepared),
        Err(error) => error.into_response(),
    }
}

async fn scan_page(Extension(handle): Extension<DraftsHandle>, Path(id): Path<String>) -> Response {
    let result = blocking(move || {
        let page = OfflinePageStore::new(handle.root()?)
            .get(&id)?
            .ok_or(Failure::Code(
                StatusCode::NOT_FOUND,
                "offline_page_not_found",
            ))?;
        zdashstitch_application::portable_scan::scan(&page.bundle)
            .map(|scan| PackageScanResponse {
                revision: page.revision,
                scan,
            })
            .map_err(|_| Failure::Code(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"))
    })
    .await;
    respond(StatusCode::OK, result)
}

async fn scan_dashboard(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
) -> Response {
    let result = blocking(move || {
        let dashboard =
            OfflineDashboardStore::new(handle.root()?)
                .get(&id)?
                .ok_or(Failure::Code(
                    StatusCode::NOT_FOUND,
                    "offline_dashboard_not_found",
                ))?;
        zdashstitch_application::portable_scan::scan(&dashboard.bundle)
            .map(|scan| PackageScanResponse {
                revision: dashboard.revision,
                scan,
            })
            .map_err(|_| Failure::Code(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"))
    })
    .await;
    respond(StatusCode::OK, result)
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
        sessions
            .with_owner(root.clone(), id.clone(), token.as_deref(), || {
                let store = OfflinePageStore::new(root);
                store.update(&id, request)?;
                store.open(&id)
            })?
            .ok_or(Failure::Code(
                StatusCode::NOT_FOUND,
                "offline_page_not_found",
            ))
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageSelection {
    file: String,
    dashboard_id: Option<String>,
    page_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageImportRequest {
    selections: Vec<PackageSelection>,
}

async fn inspect_packages(
    Extension(transfers): Extension<BrowserTransferService>,
    Path(id): Path<String>,
) -> Response {
    let files = match transfers.seal_upload_set(&id) {
        Ok(files) => files,
        Err(error) => return transfer_error(error),
    };
    let result = blocking(move || Ok(files.into_iter().map(|file| {
        let outcome = match inspect_package(&file.bytes) {
            Ok(InspectedPackage::Typed(bundle)) => {
                let (kind, name, pages, widgets) = match &bundle.project.root {
                    ProjectRoot::Page(page) => ("page", &page.name, 1, page.widgets.len()),
                    ProjectRoot::Dashboard(dashboard) => ("dashboard", &dashboard.name, dashboard.pages.len(), dashboard.pages.iter().map(|page| page.widgets.len()).sum()),
                };
                let compatibility = zdashstitch_application::portable_import::compatibility(&bundle.project);
                serde_json::json!({"status":"ready", "version":"v2", "kind":kind, "name":name,"pages":pages,"widgets":widgets,"catalogs":bundle.catalogs.len(),"declared_compatibility":bundle.project.declared_compatibility,"registry_compatibility":compatibility})
            }
            Ok(InspectedPackage::Legacy(bundle)) => {
                let dashboards: Vec<_> = bundle.project.dashboards.iter().map(|dashboard| {
                    let pages: Vec<_> = dashboard.pages.iter().map(|page| serde_json::json!({"id":page.id,"name":page.name})).collect();
                    serde_json::json!({"id":dashboard.id,"name":dashboard.name,"pages":pages})
                }).collect();
                serde_json::json!({"status":"ready", "version":"v1", "dashboards":dashboards,"catalogs":bundle.catalogs.len()})
            }
            Err(_) => serde_json::json!({"status":"invalid"}),
        };
        let entries = if outcome["status"] == "ready" { package_entry_hashes(&file.bytes).ok() } else { None };
        serde_json::json!({"file":file.relative_path,"sha256":package_sha256(&file.bytes),"size_bytes":file.bytes.len(),"entry_hashes":entries,"outcome":outcome})
    }).collect::<Vec<_>>())).await;
    match result {
        Ok(preview) => match transfers.complete_inspection(&id) {
            Ok(()) => Json(preview).into_response(),
            Err(error) => transfer_error(error),
        },
        Err(error) => error.into_response(),
    }
}

async fn import_packages(
    Extension(handle): Extension<DraftsHandle>,
    Extension(transfers): Extension<BrowserTransferService>,
    Path(id): Path<String>,
    Json(request): Json<PackageImportRequest>,
) -> Response {
    let files = match transfers.take_sealed_upload_set(&id) {
        Ok(files) => files,
        Err(error) => return transfer_error(error),
    };
    blocking(move || {
        let root = handle.root()?;
        let mut results = Vec::new();
        for file in files {
            let selection = request.selections.iter().filter(|entry| entry.file == file.relative_path).collect::<Vec<_>>();
            let outcome = if selection.len() != 1 {
                None
            } else {
                match inspect_package(&file.bytes) {
                    Ok(InspectedPackage::Typed(bundle)) if selection[0].dashboard_id.is_none() && selection[0].page_id.is_none() => Some(bundle),
                    Ok(InspectedPackage::Legacy(bundle)) => selection[0].dashboard_id.as_ref().and_then(|dashboard_id| {
                        let selected = match &selection[0].page_id {
                            Some(page_id) => LegacyRootSelection::Page { dashboard_id: dashboard_id.clone(), page_id: page_id.clone() },
                            None => LegacyRootSelection::Dashboard { dashboard_id: dashboard_id.clone() },
                        };
                        bundle.migrate(&selected).ok()
                    }),
                    _ => None,
                }
            };
            let result = match outcome {
                Some(bundle) => match &bundle.project.root {
                    ProjectRoot::Page(_) => OfflinePageStore::new(root.clone()).save_imported(bundle).map(|page| ("page", page.local_id)),
                    ProjectRoot::Dashboard(_) => OfflineDashboardStore::new(root.clone()).save_imported(bundle).map(|dashboard| ("dashboard", dashboard.local_id)),
                }.ok().map(|(kind, local_id)| serde_json::json!({"status":"saved", "kind":kind,"local_id":local_id})),
                None => None,
            }.unwrap_or_else(|| serde_json::json!({"status":"failed"}));
            results.push(serde_json::json!({"file":file.relative_path,"outcome":result}));
        }
        Ok(Json(results))
    }).await.into_response()
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
