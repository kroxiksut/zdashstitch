// SPDX-License-Identifier: MIT OR Apache-2.0

//! Protected file import. Both selectors use the same quota-bound upload set.

use std::{collections::BTreeMap, time::Duration};

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;
use zdashstitch_application::catalog_library::{
    CatalogLibrary, CatalogSummary, MAX_TEMPLATE_FILE_BYTES, inspect_template_file,
};

use super::{DraftsHandle, Failure, blocking};
use crate::{
    BrowserTransferService, DialogCancellation, FileDialogService, SelectionError, SelectionKind,
    SelectionRegistry, SelectionRegistryLimits,
    browser_transfer::{MAX_FILES, transfer_error},
    security::LocalSecurity,
};

pub(super) fn routes() -> Router<LocalSecurity> {
    Router::new()
        .route("/library/catalogs", get(list))
        .route("/library/template-import/choose", post(choose))
        .route("/library/template-import/{id}/inspect", post(inspect))
        .route("/library/template-import/{id}/run", post(run))
        .layer(Extension(FileDialogService::default()))
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
