// SPDX-License-Identifier: MIT OR Apache-2.0

//! «Settings → Appearance / Editor» behind the `/api`
//! boundary: session cookie, Origin/Host, CSRF, content type and body limits
//! are enforced by `security::protect_api` before any handler runs.
//!
//! Appearance and editor share one file observation and last valid snapshot.
//! Responses carry values and stable codes, never prose.
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
    lock: Arc<Mutex<SettingsSession>>,
}

#[derive(Debug, Default)]
struct SettingsSession {
    last_valid: Option<AppSettings>,
    appearance: Option<Appearance>,
    editor: Option<EditorSettings>,
}

impl SettingsSession {
    fn observe(&mut self, read: &mut SettingsRead) {
        match read.file_state {
            FileState::Valid => {
                self.last_valid = Some(read.settings.clone());
                self.appearance = None;
                self.editor = None;
            }
            FileState::Missing => {
                *self = Self::default();
            }
            FileState::Invalid | FileState::Unreadable => {
                if let Some(last_valid) = &self.last_valid {
                    read.settings = last_valid.clone();
                }
            }
        }
    }
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
            lock: Arc::new(Mutex::new(SettingsSession::default())),
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
    issue: Option<FileIssue>,
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
    issue: Option<FileIssue>,
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

#[derive(Serialize, Debug)]
struct FileIssue {
    code: &'static str,
    line: Option<usize>,
    column: Option<usize>,
}

impl FileIssue {
    fn from_error(error: &SettingsError) -> (FileState, Self) {
        let (state, code, line, column) = match error {
            SettingsError::InvalidFormat => (FileState::Invalid, "invalid_format", None, None),
            SettingsError::StorageRootNotAbsolute | SettingsError::StorageRootDevicePath => {
                (FileState::Invalid, "invalid_storage_root", None, None)
            }
            SettingsError::FileTooLarge => (FileState::Invalid, "file_too_large", None, None),
            // serde's error and the extracted YAML key may contain untrusted
            // content and are intentionally omitted from the response.
            SettingsError::InvalidYaml { line, column, .. } => {
                (FileState::Invalid, "invalid_yaml", *line, *column)
            }
            SettingsError::InterruptedSave => (FileState::Invalid, "interrupted_save", None, None),
            SettingsError::PathUnavailable | SettingsError::LinkedPath | SettingsError::Io(_) => {
                (FileState::Unreadable, "unreadable", None, None)
            }
        };
        (state, Self { code, line, column })
    }
}

struct SettingsRead {
    settings: AppSettings,
    file_state: FileState,
    fingerprint: String,
    issue: Option<FileIssue>,
}

fn bounded_fingerprint(store: &SettingsStore) -> String {
    match store.fingerprint() {
        Ok(value) => value,
        // These markers never authorize a file write. In particular, an
        // oversized file is not read past the configured size limit.
        Err(SettingsError::FileTooLarge) => "oversized".to_owned(),
        Err(_) => "unreadable".to_owned(),
    }
}

fn read_current(store: &SettingsStore, defaults: AppSettings) -> Result<SettingsRead, Failure> {
    let before = bounded_fingerprint(store);
    let loaded = store.load();
    let fingerprint = bounded_fingerprint(store);
    if before != fingerprint {
        return Err(Failure::Changed);
    }
    let (settings, file_state, issue) = match loaded {
        Ok(Some(settings)) => (settings, FileState::Valid, None),
        Ok(None) => (defaults, FileState::Missing, None),
        Err(error) => {
            let (state, issue) = FileIssue::from_error(&error.error);
            (error.last_valid.unwrap_or(defaults), state, Some(issue))
        }
    };
    if matches!(file_state, FileState::Valid | FileState::Missing)
        && matches!(fingerprint.as_str(), "unreadable" | "oversized")
    {
        return Err(Failure::Changed);
    }
    Ok(SettingsRead {
        settings,
        file_state,
        fingerprint,
        issue,
    })
}

async fn blocking<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, Failure> + Send + 'static,
) -> Result<T, Failure> {
    tokio::task::spawn_blocking(task)
        .await
        .unwrap_or(Err(Failure::Unavailable))
}

async fn observe(
    store: &SettingsStore,
    defaults: AppSettings,
    session: &mut SettingsSession,
) -> Result<SettingsRead, Failure> {
    let store = store.clone();
    let mut read = blocking(move || read_current(&store, defaults)).await?;
    session.observe(&mut read);
    Ok(read)
}

async fn persist(
    store: SettingsStore,
    read: &mut SettingsRead,
    session: &mut SettingsSession,
) -> Result<(), Failure> {
    let settings = read.settings.clone();
    let before = read.fingerprint.clone();
    let fingerprint = blocking(move || {
        if bounded_fingerprint(&store) != before {
            return Err(Failure::Changed);
        }
        store.save(&settings).map_err(|_| Failure::WriteFailed)?;
        store.fingerprint().map_err(|_| Failure::Unavailable)
    })
    .await?;
    read.fingerprint = fingerprint;
    read.file_state = FileState::Valid;
    session.observe(read);
    Ok(())
}

impl SettingsRead {
    fn check_write(&self, expected: &str) -> Result<(), Failure> {
        if self.file_state == FileState::Unreadable {
            return Err(Failure::Unavailable);
        }
        if self.fingerprint != expected {
            return Err(Failure::Changed);
        }
        Ok(())
    }

    fn appearance(self, session: &SettingsSession) -> AppearanceResponse {
        let override_value = session
            .appearance
            .filter(|_| self.file_state == FileState::Invalid);
        AppearanceResponse {
            values: override_value.unwrap_or_else(|| Appearance::of(&self.settings)),
            file_state: self.file_state,
            fingerprint: self.fingerprint,
            session_only: override_value.is_some(),
            issue: self.issue,
        }
    }

    fn editor(self, session: &SettingsSession) -> EditorResponse {
        let override_value = session
            .editor
            .as_ref()
            .filter(|_| self.file_state == FileState::Invalid);
        EditorResponse {
            values: override_value.cloned().unwrap_or(self.settings.editor),
            file_state: self.file_state,
            fingerprint: self.fingerprint,
            session_only: override_value.is_some(),
            issue: self.issue,
        }
    }
}

async fn read_appearance(
    Extension(settings): Extension<SettingsHandle>,
) -> Result<Json<AppearanceResponse>, Failure> {
    let (store, defaults) = settings.store()?;
    let mut session = settings.lock.lock().await;
    let read = observe(&store, defaults, &mut session).await?;
    Ok(Json(read.appearance(&session)))
}

async fn write_appearance(
    Extension(settings): Extension<SettingsHandle>,
    body: Bytes,
) -> Result<Json<AppearanceResponse>, Failure> {
    let request: AppearanceWrite =
        serde_json::from_slice(&body).map_err(|_| Failure::InvalidValue)?;
    let (store, defaults) = settings.store()?;
    let mut session = settings.lock.lock().await;
    let mut read = observe(&store, defaults, &mut session).await?;
    read.check_write(&request.fingerprint)?;
    read.settings.locale = request.locale;
    read.settings.theme = request.theme;
    read.settings.reduced_motion = request.reduced_motion;
    if read.file_state == FileState::Invalid {
        session.appearance = Some(Appearance::of(&read.settings));
    } else {
        persist(store, &mut read, &mut session).await?;
    }
    Ok(Json(read.appearance(&session)))
}

async fn read_editor(
    Extension(settings): Extension<SettingsHandle>,
) -> Result<Json<EditorResponse>, Failure> {
    let (store, defaults) = settings.store()?;
    let mut session = settings.lock.lock().await;
    let read = observe(&store, defaults, &mut session).await?;
    Ok(Json(read.editor(&session)))
}

async fn write_editor(
    Extension(settings): Extension<SettingsHandle>,
    body: Bytes,
) -> Result<Json<EditorResponse>, Failure> {
    let request: EditorWrite = serde_json::from_slice(&body).map_err(|_| Failure::InvalidValue)?;
    if ![30, 60, 120].contains(&request.values.autosave_max_interval_s) {
        return Err(Failure::InvalidValue);
    }
    let (store, defaults) = settings.store()?;
    let mut session = settings.lock.lock().await;
    let mut read = observe(&store, defaults, &mut session).await?;
    read.check_write(&request.fingerprint)?;
    read.settings.editor = request.values;
    if read.file_state == FileState::Invalid {
        session.editor = Some(read.settings.editor.clone());
    } else {
        persist(store, &mut read, &mut session).await?;
    }
    Ok(Json(read.editor(&session)))
}
#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
