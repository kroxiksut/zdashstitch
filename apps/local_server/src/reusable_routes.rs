// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{sync::Arc, time::Duration};

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::Path,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, de::DeserializeOwned};
use zdashstitch_application::{
    offline_dashboards::OfflineDashboardStore,
    offline_page_sessions::PageEditSessions,
    offline_pages::{OfflinePage, OfflinePageStore},
    reusable_library::{ImportMode, Metadata, ReusableDocument, ReusableLibrary},
    reusable_widgets::{
        self, Extraction, ReusableAction, ReusableActionResult, ReusableHistoryAction,
    },
    workcopy::{CopyBlueprintRequest, CopyStyleRequest},
};
use zdashstitch_core::formats::{MAX_FILE_BYTES, strict};

use super::{DraftsHandle, Failure, blocking, dashboard_page_key, editor_token, respond};
use crate::{
    BrowserTransferService, ConnectionsHandle, DialogCancellation, DialogErrorCode, DialogIntent,
    FileDialogService, LocalSecurity, SelectionKind, SelectionRegistry, SelectionRegistryLimits,
    browser_transfer::transfer_error, selections::save_new_file_before_commit,
};

pub(super) fn routes() -> Router<LocalSecurity> {
    Router::new()
        .route("/library/reusable", get(list).post(create))
        .route("/library/reusable/import-choose", post(choose))
        .route("/library/reusable/import/{id}/inspect", post(inspect))
        .route("/library/reusable/import/{id}/run", post(import))
        .route("/library/reusable/{id}/style", post(update_style))
        .route("/library/reusable/{id}/metadata", post(edit_metadata))
        .route("/library/reusable/{id}/duplicate", post(duplicate))
        .route("/library/reusable/{id}/delete", post(delete))
        .route("/library/reusable/{id}/export", post(export))
        .route("/library/reusable/{id}/save", post(save))
        .route("/library/reusable/{id}/scan", get(scan))
        .route(
            "/drafts/{id}/reusable-style",
            get(copy_style_state).post(copy_style),
        )
        .route("/drafts/{id}/reusable-style-undo", post(copy_style_undo))
        .route(
            "/drafts/{id}/reusable-blueprint",
            get(copy_blueprint_state).post(copy_blueprint),
        )
        .route(
            "/drafts/{id}/reusable-blueprint-hosts",
            post(copy_blueprint_hosts),
        )
        .route("/library/offline-pages/{id}/reusable", post(page_action))
        .route(
            "/library/offline-pages/{id}/reusable-history",
            post(page_history),
        )
        .route(
            "/library/offline-pages/{id}/reusable-extract",
            post(extract_page),
        )
        .route(
            "/library/offline-dashboards/{id}/pages/{page}/reusable",
            post(dashboard_page_action),
        )
        .route(
            "/library/offline-dashboards/{id}/pages/{page}/reusable-history",
            post(dashboard_page_history),
        )
        .route(
            "/library/offline-dashboards/{id}/pages/{page}/reusable-extract",
            post(extract_dashboard_page),
        )
}

fn request<T: DeserializeOwned>(raw: &[u8]) -> Result<T, Failure> {
    std::str::from_utf8(raw)
        .ok()
        .and_then(|text| strict::json_from_str(text).ok())
        .ok_or_else(super::super::request_invalid)
}

fn invalid(code: &'static str) -> Failure {
    Failure::Code(StatusCode::UNPROCESSABLE_ENTITY, code)
}

async fn list(Extension(handle): Extension<DraftsHandle>) -> Response {
    respond(
        StatusCode::OK,
        blocking(move || Ok(ReusableLibrary::new(handle.root()?).list()?)).await,
    )
}

async fn copy_style_state(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
) -> Response {
    respond(
        StatusCode::OK,
        blocking(move || Ok(handle.service()?.workcopy_style_state(&id)?)).await,
    )
}

async fn copy_blueprint_state(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
) -> Response {
    respond(
        StatusCode::OK,
        blocking(move || Ok(handle.service()?.workcopy_blueprint_state(&id)?)).await,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyStyleAction {
    apply: bool,
    action: CopyStyleRequest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyBlueprintAction {
    approved_fingerprint: Option<String>,
    #[serde(default)]
    allow_unbound: bool,
    action: CopyBlueprintRequest,
}

async fn copy_blueprint(
    Extension(handle): Extension<DraftsHandle>,
    Extension(connections): Extension<ConnectionsHandle>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, Failure> {
    let input: CopyBlueprintAction = request(&body)?;
    let runtime = tokio::runtime::Handle::current();
    let result = blocking(move || {
        let workcopies = handle.service()?;
        let connection_id = workcopies.draft_connection_id(&id)?;
        runtime.block_on(async {
            let service = connections.service();
            let guard = match &service {
                Some(service) => Some(service.lock().await),
                None => None,
            };
            let reader = guard.as_ref().and_then(|guard| {
                guard
                    .active()
                    .filter(|(active, _, _)| *active == connection_id)
                    .and_then(|_| guard.reader())
            });
            Ok(workcopies
                .insert_workcopy_blueprint(
                    &id,
                    &input.action,
                    reader,
                    input.approved_fingerprint.as_deref(),
                    input.allow_unbound,
                )
                .await?)
        })
    })
    .await;
    Ok(respond(StatusCode::OK, result))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BlueprintHostQuery {
    search: String,
}

async fn copy_blueprint_hosts(
    Extension(handle): Extension<DraftsHandle>,
    Extension(connections): Extension<ConnectionsHandle>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, Failure> {
    let input: BlueprintHostQuery = request(&body)?;
    let runtime = tokio::runtime::Handle::current();
    let result = blocking(move || {
        let workcopies = handle.service()?;
        let connection_id = workcopies.draft_connection_id(&id)?;
        let service = connections.service().ok_or_else(super::super::not_connected)?;
        runtime.block_on(async {
            let guard = service.lock().await;
            super::super::ensure_active(&guard, &connection_id)?;
            let reader = guard.reader().ok_or_else(super::super::not_connected)?;
            let found = workcopies
                .search_workcopy_blueprint_hosts(&id, reader, &input.search)
                .await?;
            Ok(serde_json::json!({
                "hosts": found.objects.into_iter().map(|host| serde_json::json!({"hostid": host.hostid, "name": host.name})).collect::<Vec<_>>(),
                "truncated": found.truncated
            }))
        })
    }).await;
    Ok(respond(StatusCode::OK, result))
}

async fn copy_style(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, Failure> {
    let input: CopyStyleAction = request(&body)?;
    let result = blocking(move || {
        Ok(handle
            .service()?
            .workcopy_style(&id, &input.action, input.apply)?)
    })
    .await;
    Ok(respond(StatusCode::OK, result))
}

async fn copy_style_undo(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, Failure> {
    let input: Revision = request(&body)?;
    let result = blocking(move || {
        Ok(handle
            .service()?
            .undo_workcopy_style(&id, input.expected_revision)?)
    })
    .await;
    Ok(respond(StatusCode::OK, result))
}

async fn create(
    Extension(handle): Extension<DraftsHandle>,
    body: Bytes,
) -> Result<Response, Failure> {
    let document: ReusableDocument = request(&body)?;
    let result = blocking(move || Ok(ReusableLibrary::new(handle.root()?).create(document)?)).await;
    Ok(respond(StatusCode::CREATED, result))
}

async fn inspect(
    Extension(handle): Extension<DraftsHandle>,
    Extension(transfers): Extension<BrowserTransferService>,
    Path(id): Path<String>,
) -> Response {
    inspect_upload(handle, &transfers, &id).await
}

async fn choose(
    Extension(handle): Extension<DraftsHandle>,
    Extension(transfers): Extension<BrowserTransferService>,
    Extension(dialog): Extension<FileDialogService>,
) -> Result<Response, Failure> {
    let path = match dialog
        .choose(
            DialogIntent::OpenFile,
            DialogCancellation::default(),
            Duration::from_secs(120),
        )
        .await
    {
        Ok(path) => path,
        Err(DialogErrorCode::UserCancelled) => {
            return Ok(Json(serde_json::json!({"id": null})).into_response());
        }
        Err(error) => {
            return Err(Failure::Code(
                StatusCode::SERVICE_UNAVAILABLE,
                error.as_str(),
            ));
        }
    };
    let bytes = blocking(move || {
        let registry = SelectionRegistry::new(SelectionRegistryLimits {
            maximum_file_bytes: MAX_FILE_BYTES,
            ..SelectionRegistryLimits::default()
        });
        registry
            .register(path, SelectionKind::File)
            .and_then(|selected| registry.read_file(&selected.id))
            .map_err(|_| invalid("reusable_file_unreadable"))
    })
    .await?;
    let set = match transfers.create_upload_set() {
        Ok(set) => set,
        Err(error) => return Ok(transfer_error(error)),
    };
    if let Err(error) = transfers.add_file(&set.id, "library-asset.yaml", Bytes::from(bytes)) {
        let _ = transfers.take_upload_set(&set.id);
        return Ok(transfer_error(error));
    }
    Ok(inspect_upload(handle, &transfers, &set.id).await)
}

async fn inspect_upload(
    handle: DraftsHandle,
    transfers: &BrowserTransferService,
    id: &str,
) -> Response {
    let files = match transfers.seal_upload_set(id) {
        Ok(files) => files,
        Err(error) => return transfer_error(error),
    };
    let result = blocking(move || {
        let [file] = files.as_slice() else {
            return Err(invalid("reusable_one_file"));
        };
        let asset = ReusableDocument::parse_yaml(&file.bytes)?;
        let library = ReusableLibrary::new(handle.root()?);
        let existing = match library.get(asset.id()) {
            Ok(existing) => Some(existing),
            Err(error) if error.code() == "reusable_not_found" => None,
            Err(error) => return Err(error.into()),
        };
        Ok(serde_json::json!({"name": asset.name(), "kind": asset.kind(), "adapter": asset.adapter(), "version": asset.version(), "existing": existing.map(|item| (item.revision, item.origin))}))
    }).await;
    if result.is_ok() {
        if let Err(error) = transfers.complete_inspection(id) {
            return transfer_error(error);
        }
    } else {
        // Free quota on error. Nothing can be applied from this rejected preview.
        if transfers.complete_inspection(id).is_ok() {
            let _ = transfers.take_sealed_upload_set(id);
        }
    }
    respond(
        StatusCode::OK,
        result.map(|preview| serde_json::json!({"id": id, "preview": preview})),
    )
}

async fn import(
    Extension(handle): Extension<DraftsHandle>,
    Extension(transfers): Extension<BrowserTransferService>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, Failure> {
    let mode: ImportMode = request(&body)?;
    let files = match transfers.take_sealed_upload_set(&id) {
        Ok(files) => files,
        Err(error) => return Ok(transfer_error(error)),
    };
    let result = blocking(move || {
        let [file] = files.as_slice() else {
            return Err(invalid("reusable_one_file"));
        };
        let asset = ReusableDocument::parse_yaml(&file.bytes)?;
        let library = ReusableLibrary::new(handle.root()?);
        Ok(library.import(asset, mode)?)
    })
    .await;
    Ok(respond(StatusCode::CREATED, result))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeStyle {
    expected_revision: u64,
    document: zdashstitch_core::formats::StyleProfile,
}

async fn update_style(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, Failure> {
    let input: ChangeStyle = request(&body)?;
    if id != input.document.profile_id {
        return Err(super::super::request_invalid());
    }
    let result = blocking(move || {
        Ok(ReusableLibrary::new(handle.root()?)
            .update_style(input.document, input.expected_revision)?)
    })
    .await;
    Ok(respond(StatusCode::OK, result))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeMetadata {
    expected_revision: u64,
    metadata: Metadata,
}

async fn edit_metadata(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, Failure> {
    let edit: ChangeMetadata = request(&body)?;
    let result = blocking(move || {
        Ok(ReusableLibrary::new(handle.root()?).edit_metadata(
            &id,
            edit.expected_revision,
            edit.metadata,
        )?)
    })
    .await;
    Ok(respond(StatusCode::OK, result))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Revision {
    expected_revision: u64,
}

async fn duplicate(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, Failure> {
    let revision: Revision = request(&body)?;
    let result = blocking(move || {
        Ok(ReusableLibrary::new(handle.root()?).duplicate(&id, revision.expected_revision)?)
    })
    .await;
    Ok(respond(StatusCode::CREATED, result))
}

async fn delete(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, Failure> {
    let revision: Revision = request(&body)?;
    blocking(move || {
        Ok(ReusableLibrary::new(handle.root()?).delete(&id, revision.expected_revision)?)
    })
    .await?;
    Ok(Json(serde_json::json!({})).into_response())
}

async fn scan(Extension(handle): Extension<DraftsHandle>, Path(id): Path<String>) -> Response {
    let result = blocking(move || {
        let entry = ReusableLibrary::new(handle.root()?).get(&id)?;
        Ok(serde_json::json!({"revision": entry.revision, "scan": entry.asset.scan()?}))
    })
    .await;
    respond(StatusCode::OK, result)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportRequest {
    expected_revision: u64,
    acknowledged: bool,
}

async fn export(
    Extension(handle): Extension<DraftsHandle>,
    Extension(transfers): Extension<BrowserTransferService>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, Failure> {
    let input: ExportRequest = request(&body)?;
    let (name, bytes) = blocking(move || prepare_export(&handle, &id, input)).await?;
    Ok(match transfers.register_download(&name, bytes) {
        Ok(download) => (StatusCode::CREATED, Json(download)).into_response(),
        Err(error) => transfer_error(error),
    })
}

fn prepare_export(
    handle: &DraftsHandle,
    id: &str,
    input: ExportRequest,
) -> Result<(String, Vec<u8>), Failure> {
    let entry = ReusableLibrary::new(handle.root()?).get(id)?;
    if entry.revision != input.expected_revision || !input.acknowledged {
        return Err(Failure::Code(StatusCode::CONFLICT, "reusable_conflict"));
    }
    if entry.asset.scan()?.credential_candidates != 0 {
        return Err(Failure::Code(
            StatusCode::CONFLICT,
            "redaction_credentials_present",
        ));
    }
    let suffix = entry.asset.kind().replace('_', "-");
    let mut slug = String::new();
    for ch in entry.asset.name().chars() {
        if slug.len() >= 48 {
            break;
        }
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = if slug.trim_end_matches('-').is_empty() {
        entry.asset.id()
    } else {
        slug.trim_end_matches('-')
    };
    Ok((format!("{slug}.{suffix}.yaml"), entry.asset.export()?))
}

async fn save(
    Extension(handle): Extension<DraftsHandle>,
    Extension(dialog): Extension<FileDialogService>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, Failure> {
    let input: ExportRequest = request(&body)?;
    let destination = match dialog
        .choose(
            DialogIntent::SaveFile,
            DialogCancellation::default(),
            Duration::from_secs(120),
        )
        .await
    {
        Ok(path) => path,
        Err(DialogErrorCode::UserCancelled) => {
            return Ok(Json(serde_json::json!({"cancelled": true})).into_response());
        }
        Err(error) => {
            return Err(Failure::Code(
                StatusCode::SERVICE_UNAVAILABLE,
                error.as_str(),
            ));
        }
    };
    if !destination
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml"))
    {
        return Err(invalid("reusable_format_unsupported"));
    }
    blocking(move || {
        let (_, bytes) = prepare_export(&handle, &id, input)?;
        save_new_file_before_commit(&destination, &bytes, || true)
            .map_err(|_| Failure::Code(StatusCode::CONFLICT, "portable_save_failed"))
    })
    .await?;
    Ok(Json(serde_json::json!({"cancelled": false})).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftAction {
    apply: bool,
    action: ReusableAction,
}

async fn page_action(
    Extension(handle): Extension<DraftsHandle>,
    Extension(sessions): Extension<Arc<PageEditSessions>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Failure> {
    let input: DraftAction = request(&body)?;
    let token = editor_token(&headers);
    let result: Result<ReusableActionResult, Failure> = blocking(move || {
        let root = handle.root()?;
        Ok(
            sessions.with_owner(root.clone(), id.clone(), token.as_deref(), || {
                OfflinePageStore::new(root).reusable_action(&id, &input.action, input.apply)
            })?,
        )
    })
    .await;
    Ok(respond(StatusCode::OK, result))
}

async fn dashboard_page_action(
    Extension(handle): Extension<DraftsHandle>,
    Extension(sessions): Extension<Arc<PageEditSessions>>,
    Path((id, page)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Failure> {
    let input: DraftAction = request(&body)?;
    let token = editor_token(&headers);
    let result: Result<ReusableActionResult, Failure> = blocking(move || {
        let root = handle.root()?;
        Ok(sessions.with_owner(
            root.clone(),
            dashboard_page_key(&id, &page),
            token.as_deref(),
            || {
                OfflineDashboardStore::new(root).reusable_action(
                    &id,
                    &page,
                    &input.action,
                    input.apply,
                )
            },
        )?)
    })
    .await;
    Ok(respond(StatusCode::OK, result))
}

async fn extract_page(
    Extension(handle): Extension<DraftsHandle>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, Failure> {
    let input: Extraction = request(&body)?;
    let result = blocking(move || {
        let root = handle.root()?;
        let page = OfflinePageStore::new(root.clone()).get(&id)?;
        save_extracted(root, page, &input)
    })
    .await;
    Ok(respond(StatusCode::CREATED, result))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryRequest {
    expected_revision: u64,
    action: ReusableHistoryAction,
}

async fn page_history(
    Extension(handle): Extension<DraftsHandle>,
    Extension(sessions): Extension<Arc<PageEditSessions>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Failure> {
    let input: HistoryRequest = request(&body)?;
    let token = editor_token(&headers);
    let result = blocking(move || {
        let root = handle.root()?;
        Ok(
            sessions.with_owner(root.clone(), id.clone(), token.as_deref(), || {
                OfflinePageStore::new(root).reusable_history(
                    &id,
                    input.expected_revision,
                    &input.action,
                )
            })?,
        )
    })
    .await;
    Ok(respond(StatusCode::OK, result))
}

async fn dashboard_page_history(
    Extension(handle): Extension<DraftsHandle>,
    Extension(sessions): Extension<Arc<PageEditSessions>>,
    Path((id, page)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Failure> {
    let input: HistoryRequest = request(&body)?;
    let token = editor_token(&headers);
    let result = blocking(move || {
        let root = handle.root()?;
        Ok(sessions.with_owner(
            root.clone(),
            dashboard_page_key(&id, &page),
            token.as_deref(),
            || {
                OfflineDashboardStore::new(root).reusable_history(
                    &id,
                    &page,
                    input.expected_revision,
                    &input.action,
                )
            },
        )?)
    })
    .await;
    Ok(respond(StatusCode::OK, result))
}

async fn extract_dashboard_page(
    Extension(handle): Extension<DraftsHandle>,
    Path((id, page)): Path<(String, String)>,
    body: Bytes,
) -> Result<Response, Failure> {
    let input: Extraction = request(&body)?;
    let result = blocking(move || {
        let root = handle.root()?;
        let page = OfflineDashboardStore::new(root.clone()).page(&id, &page)?;
        save_extracted(root, page.map(|view| view.page), &input)
    })
    .await;
    Ok(respond(StatusCode::CREATED, result))
}

fn save_extracted(
    root: std::path::PathBuf,
    page: Option<OfflinePage>,
    input: &Extraction,
) -> Result<zdashstitch_application::reusable_library::ReusableEntry, Failure> {
    let page = page.ok_or(Failure::Code(
        StatusCode::NOT_FOUND,
        "offline_page_not_found",
    ))?;
    if page.revision != input.expected_revision {
        return Err(Failure::Code(StatusCode::CONFLICT, "offline_page_conflict"));
    }
    let asset = reusable_widgets::extract(&page.bundle, input)?;
    Ok(ReusableLibrary::new(root).create(asset)?)
}
