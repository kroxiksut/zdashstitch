// SPDX-License-Identifier: MIT OR Apache-2.0

//! «Settings → Appearance» (P2-00G, P1-09C, O-038) behind the `/api`
//! boundary: session cookie, Origin/Host, CSRF, content type and body limits
//! are enforced by `security::protect_api` before any handler runs.
//!
//! Only the language, the theme and reduced motion are read and changed
//! here; the storage root and the rest of the settings file stay as they
//! are. Responses carry values and stable codes, never prose. The settings
//! file is the only source of truth: the browser keeps nothing.
//!
//! An invalid settings file is never overwritten (P1-09B, decision 3): a
//! change then fails with `settings_file_invalid` and the UI applies it
//! until restart only.

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
    AppSettings, LocalePreference, ReducedMotionPreference, SettingsStore, ThemePreference,
};

use crate::security::LocalSecurity;

/// Where the settings file lives. One lock serializes read-modify-write of
/// the file across concurrent requests.
#[derive(Clone, Debug)]
pub(crate) struct SettingsHandle {
    location: Arc<Location>,
    lock: Arc<Mutex<()>>,
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
            lock: Arc::new(Mutex::new(())),
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
}

#[derive(Debug)]
enum Failure {
    /// The platform settings location cannot be determined.
    Unavailable,
    /// The body is not a complete, known appearance value set.
    InvalidValue,
    /// The settings file has errors and is not overwritten.
    FileInvalid,
    /// The file could not be written.
    WriteFailed,
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
            Self::FileInvalid => (StatusCode::CONFLICT, "settings_file_invalid"),
            Self::WriteFailed => (StatusCode::INTERNAL_SERVER_ERROR, "settings_write_failed"),
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
        Err(error) => (error.last_valid.unwrap_or(defaults), FileState::Invalid),
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
    let _guard = settings.lock.lock().await;
    let result = blocking(move || {
        let (settings, file_state) = current(&store, defaults);
        Ok(AppearanceResponse {
            values: Appearance::of(&settings),
            file_state,
        })
    })
    .await;
    match result {
        Ok(response) => Json(response).into_response(),
        Err(failure) => failure.into_response(),
    }
}

/// `PUT /api/settings/appearance`: all three values, nothing else.
async fn write_appearance(Extension(settings): Extension<SettingsHandle>, body: Bytes) -> Response {
    let Ok(values) = serde_json::from_slice::<Appearance>(&body) else {
        return Failure::InvalidValue.into_response();
    };
    let (store, defaults) = match settings.store() {
        Ok(found) => found,
        Err(failure) => return failure.into_response(),
    };
    let _guard = settings.lock.lock().await;
    let result = blocking(move || {
        let (mut current, file_state) = current(&store, defaults);
        if file_state == FileState::Invalid {
            return Err(Failure::FileInvalid);
        }
        current.locale = values.locale;
        current.theme = values.theme;
        current.reduced_motion = values.reduced_motion;
        store.save(&current).map_err(|_| Failure::WriteFailed)?;
        Ok(AppearanceResponse {
            values: Appearance::of(&current),
            file_state: FileState::Valid,
        })
    })
    .await;
    match result {
        Ok(response) => Json(response).into_response(),
        Err(failure) => failure.into_response(),
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
