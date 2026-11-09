// SPDX-License-Identifier: MIT OR Apache-2.0

//! «Settings → Appearance / Editor» behind the `/api`
//! boundary: session cookie, Origin/Host, CSRF, content type and body limits
//! are enforced by `security::protect_api` before any handler runs.
//!
//! Appearance and editor values are changed independently; the storage root
//! and other sections stay as they are. Responses carry values and stable codes, never prose. The settings
//! file is the only source of truth: the browser keeps nothing.
//!
//! An invalid settings file is never overwritten (P1-09B, decision 3).
//! Appearance and editor changes then live in process memory until restart.

use std::{collections::BTreeMap, sync::Arc};

use axum::{
    Extension, Json, Router,
    body::Bytes,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use zdashstitch_application::{
    AppSettings, EditorSettings, LocalePreference, ReducedMotionPreference, SettingsError,
    SettingsStore, ThemePreference,
};

use crate::security::LocalSecurity;

/// Where the settings file lives. One lock serializes read-modify-write of
/// the file across concurrent requests.
#[derive(Clone, Debug)]
pub(crate) struct SettingsHandle {
    location: Arc<Location>,
    lock: Arc<Mutex<SessionOverrides>>,
}

#[derive(Debug, Default)]
struct SessionOverrides {
    appearance: Option<Appearance>,
    editor: Option<EditorSettings>,
}

#[derive(Debug)]
enum Location {
    /// The platform settings file (`%LOCALAPPDATA%` / XDG), resolved per request.
    Platform,
    /// A fixed directory and default storage root, e.g. in tests.
    #[cfg_attr(not(test), allow(dead_code))]
    Fixed {
        store: SettingsStore,
        defaults: AppSettings,
    },
}

impl SettingsHandle {
    pub(crate) fn platform() -> Self {
        Self::with(Location::Platform)
    }

    #[cfg(test)]
    pub(crate) fn at(directory: std::path::PathBuf, storage_root: std::path::PathBuf) -> Self {
        Self::with(Location::Fixed {
            store: SettingsStore::at(directory),
            defaults: AppSettings::new(storage_root),
        })
    }

    fn with(location: Location) -> Self {
        Self {
            location: Arc::new(location),
            lock: Arc::new(Mutex::new(SessionOverrides::default())),
        }
    }

    fn store(&self) -> Result<(SettingsStore, AppSettings), Failure> {
        match self.location.as_ref() {
            Location::Platform => {
                SettingsStore::platform_default().map_err(|_| Failure::Unavailable)
            }
            Location::Fixed { store, defaults } => Ok((store.clone(), defaults.clone())),
        }
    }
}

pub(crate) fn routes(settings: SettingsHandle) -> Router<LocalSecurity> {
    Router::new()
        .route(
            "/settings/appearance",
            get(read_appearance).put(write_appearance),
        )
        .route("/settings/editor", get(read_editor).put(write_editor))
        .layer(Extension(settings))
}

/// State of the settings file as the UI needs it for this section.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FileState {
    /// The file was read and applied.
    Valid,
    /// No file yet: defaults apply; the first change creates it.
    Missing,
    /// The file has errors: the last valid copy (or defaults) applies and
    /// the file is not overwritten.
    Invalid,
    /// The file could not be safely read; session changes are unavailable.
    Unreadable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Appearance {
    locale: LocalePreference,
    theme: ThemePreference,
    reduced_motion: ReducedMotionPreference,
}

impl Appearance {
    fn of(settings: &AppSettings) -> Self {
        Self {
            locale: settings.locale,
            theme: settings.theme,
            reduced_motion: settings.reduced_motion,
        }
    }
}

#[derive(Debug, Serialize)]
struct AppearanceResponse {
    #[serde(flatten)]
    values: Appearance,
    file_state: FileState,
    fingerprint: String,
    session_only: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppearanceWrite {
    locale: LocalePreference,
    theme: ThemePreference,
    reduced_motion: ReducedMotionPreference,
    fingerprint: String,
}

#[derive(Debug, Serialize)]
struct EditorResponse {
    values: EditorSettings,
    file_state: FileState,
    fingerprint: String,
    session_only: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditorWrite {
    values: EditorSettings,
    fingerprint: String,
}

#[derive(Debug)]
enum Failure {
    /// The platform settings location cannot be determined.
    Unavailable,
    /// The body is not a complete, known appearance value set.
    InvalidValue,
    /// The file could not be written.
    WriteFailed,
    Changed,
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    params: BTreeMap<&'static str, String>,
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "settings_unavailable"),
            Self::InvalidValue => (StatusCode::UNPROCESSABLE_ENTITY, "settings_invalid_value"),
            Self::WriteFailed => (StatusCode::INTERNAL_SERVER_ERROR, "settings_write_failed"),
            Self::Changed => (StatusCode::CONFLICT, "settings_changed"),
        };
        (
            status,
            Json(ErrorBody {
                code,
                params: BTreeMap::new(),
            }),
        )
            .into_response()
    }
}

/// Current settings and the file state; synchronous file I/O.
fn current(store: &SettingsStore, defaults: AppSettings) -> (AppSettings, FileState) {
    match store.load() {
        Ok(Some(settings)) => (settings, FileState::Valid),
        Ok(None) => (defaults, FileState::Missing),
        Err(error) => {
            let state = match error.error.as_ref() {
                SettingsError::InvalidFormat
                | SettingsError::StorageRootNotAbsolute
                | SettingsError::StorageRootDevicePath
                | SettingsError::FileTooLarge
                | SettingsError::InvalidYaml { .. }
                | SettingsError::InterruptedSave => FileState::Invalid,
                SettingsError::PathUnavailable
                | SettingsError::LinkedPath
                | SettingsError::Io(_) => FileState::Unreadable,
            };
            (error.last_valid.unwrap_or(defaults), state)
        }
    }
}

async fn blocking<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, Failure> + Send + 'static,
) -> Result<T, Failure> {
    tokio::task::spawn_blocking(task)
        .await
        .unwrap_or(Err(Failure::Unavailable))
}

/// `GET /api/settings/appearance`.
async fn read_appearance(Extension(settings): Extension<SettingsHandle>) -> Response {
    let (store, defaults) = match settings.store() {
        Ok(found) => found,
        Err(failure) => return failure.into_response(),
    };
    let mut guard = settings.lock.lock().await;
    let session_appearance = guard.appearance;
    let result = blocking(move || {
        let first = store.fingerprint().map_err(|_| Failure::Unavailable)?;
        let (settings, file_state) = current(&store, defaults);
        let fingerprint = store.fingerprint().map_err(|_| Failure::Unavailable)?;
        if first != fingerprint {
            return Err(Failure::Changed);
        }
        let session_only = file_state == FileState::Invalid && session_appearance.is_some();
        Ok(AppearanceResponse {
            values: if session_only {
                session_appearance.unwrap_or_else(|| Appearance::of(&settings))
            } else {
                Appearance::of(&settings)
            },
            file_state,
            fingerprint,
            session_only,
        })
    })
    .await;
    match result {
        Ok(response) => {
            if response.file_state != FileState::Invalid {
                guard.appearance = None;
            }
            Json(response).into_response()
        }
        Err(failure) => failure.into_response(),
    }
}

/// `PUT /api/settings/appearance`: all three values and the read fingerprint.
async fn write_appearance(Extension(settings): Extension<SettingsHandle>, body: Bytes) -> Response {
    let Ok(request) = serde_json::from_slice::<AppearanceWrite>(&body) else {
        return Failure::InvalidValue.into_response();
    };
    let (store, defaults) = match settings.store() {
        Ok(found) => found,
        Err(failure) => return failure.into_response(),
    };
    let mut guard = settings.lock.lock().await;
    let result = blocking(move || {
        let before = store.fingerprint().map_err(|_| Failure::Unavailable)?;
        if before != request.fingerprint {
            return Err(Failure::Changed);
        }
        let (mut current, file_state) = current(&store, defaults);
        if file_state == FileState::Unreadable {
            return Err(Failure::Unavailable);
        }
        if store.fingerprint().map_err(|_| Failure::Unavailable)? != before {
            return Err(Failure::Changed);
        }
        current.locale = request.locale;
        current.theme = request.theme;
        current.reduced_motion = request.reduced_motion;
        if file_state == FileState::Invalid {
            return Ok(AppearanceResponse {
                values: Appearance::of(&current),
                file_state,
                fingerprint: before,
                session_only: true,
            });
        }
        store.save(&current).map_err(|_| Failure::WriteFailed)?;
        let fingerprint = store.fingerprint().map_err(|_| Failure::Unavailable)?;
        Ok(AppearanceResponse {
            values: Appearance::of(&current),
            file_state: FileState::Valid,
            fingerprint,
            session_only: false,
        })
    })
    .await;
    match result {
        Ok(response) => {
            guard.appearance = response.session_only.then_some(response.values);
            Json(response).into_response()
        }
        Err(failure) => failure.into_response(),
    }
}

/// Editor preferences are global; the returned fingerprint must accompany a write.
async fn read_editor(Extension(settings): Extension<SettingsHandle>) -> Response {
    let (store, defaults) = match settings.store() {
        Ok(found) => found,
        Err(failure) => return failure.into_response(),
    };
    let mut guard = settings.lock.lock().await;
    let session_editor = guard.editor.clone();
    match blocking(move || {
        let first = store.fingerprint().map_err(|_| Failure::Unavailable)?;
        let (settings, file_state) = current(&store, defaults);
        let fingerprint = store.fingerprint().map_err(|_| Failure::Unavailable)?;
        if first != fingerprint {
            return Err(Failure::Changed);
        }
        let session_only = file_state == FileState::Invalid && session_editor.is_some();
        Ok(EditorResponse {
            values: if session_only {
                session_editor.unwrap_or(settings.editor)
            } else {
                settings.editor
            },
            file_state,
            fingerprint,
            session_only,
        })
    })
    .await
    {
        Ok(response) => {
            if response.file_state != FileState::Invalid {
                guard.editor = None;
            }
            Json(response).into_response()
        }
        Err(failure) => failure.into_response(),
    }
}

async fn write_editor(Extension(settings): Extension<SettingsHandle>, body: Bytes) -> Response {
    let Ok(request) = serde_json::from_slice::<EditorWrite>(&body) else {
        return Failure::InvalidValue.into_response();
    };
    let (store, defaults) = match settings.store() {
        Ok(found) => found,
        Err(failure) => return failure.into_response(),
    };
    let mut guard = settings.lock.lock().await;
    match blocking(move || {
        let before = store.fingerprint().map_err(|_| Failure::Unavailable)?;
        if before != request.fingerprint {
            return Err(Failure::Changed);
        }
        let (mut current, file_state) = current(&store, defaults);
        if file_state == FileState::Unreadable {
            return Err(Failure::Unavailable);
        }
        if store.fingerprint().map_err(|_| Failure::Unavailable)? != before {
            return Err(Failure::Changed);
        }
        current.editor = request.values;
        // Validation occurs before any disk mutation.
        if ![30, 60, 120].contains(&current.editor.autosave_max_interval_s) {
            return Err(Failure::InvalidValue);
        }
        if file_state == FileState::Invalid {
            return Ok(EditorResponse {
                values: current.editor,
                file_state,
                fingerprint: before,
                session_only: true,
            });
        }
        store.save(&current).map_err(|_| Failure::WriteFailed)?;
        let fingerprint = store.fingerprint().map_err(|_| Failure::Unavailable)?;
        Ok(EditorResponse {
            values: current.editor,
            file_state: FileState::Valid,
            fingerprint,
            session_only: false,
        })
    })
    .await
    {
        Ok(response) => {
            guard.editor = response.session_only.then(|| response.values.clone());
            Json(response).into_response()
        }
        Err(failure) => failure.into_response(),
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
