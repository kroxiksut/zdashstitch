// SPDX-License-Identifier: MIT OR Apache-2.0

//! «Settings → Appearance» routes (P2-00G): the `/api` boundary, values
//! persisted in the settings file only, unknown values and fields refused,
//! and an invalid file never overwritten.

use std::path::PathBuf;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use zdashstitch_application::{SettingsStore, ThemePreference};

use super::{SettingsHandle, routes};
use crate::{CSRF_HEADER, LocalSecurity, build_router_with_api};

const PORT: u16 = 43179;
const ROUTE: &str = "/api/settings/appearance";
const EDITOR_ROUTE: &str = "/api/settings/editor";

struct App {
    router: Router,
    cookie: String,
    csrf: String,
    config: PathBuf,
    storage: PathBuf,
    _temporary: tempfile::TempDir,
}

impl App {
    async fn start() -> Self {
        let temporary = tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"));
        let config = temporary.path().join("config");
        let storage = temporary.path().join("data");
        let initial = LocalSecurity::initialize(PORT)
            .unwrap_or_else(|error| panic!("security must initialize: {error}"));
        let api = routes(SettingsHandle::at(config.clone(), storage.clone()));
        let router = build_router_with_api("missing-ui", initial.security, api);
        let mut app = Self {
            router,
            cookie: String::new(),
            csrf: String::new(),
            config,
            storage,
            _temporary: temporary,
        };
        let response = app
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/?bootstrap={}", initial.code))
                    .header(header::HOST, format!("127.0.0.1:{PORT}"))
                    .body(Body::empty())
                    .unwrap_or_else(|error| panic!("request: {error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("router: {error}"));
        app.cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .unwrap_or_else(|| panic!("bootstrap must set a cookie"))
            .to_owned();
        let (status, session) = app.call("GET", "/api/session", None, true).await;
        assert_eq!(status, StatusCode::OK);
        app.csrf = session["csrf_token"]
            .as_str()
            .unwrap_or_else(|| panic!("CSRF token"))
            .to_owned();
        app
    }

    async fn call(
        &self,
        method: &str,
        uri: &str,
        body: Option<Value>,
        with_csrf: bool,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, format!("127.0.0.1:{PORT}"))
            .header(header::ORIGIN, format!("http://127.0.0.1:{PORT}"))
            .header(header::COOKIE, &self.cookie)
            .header(header::CONTENT_TYPE, "application/json");
        if with_csrf {
            request = request.header(CSRF_HEADER, &self.csrf);
        }
        let request = request
            .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
            .unwrap_or_else(|error| panic!("request: {error}"));
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .unwrap_or_else(|error| panic!("router: {error}"));
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|error| panic!("body: {error}"))
            .to_bytes();
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }

    fn file(&self) -> Option<String> {
        std::fs::read_to_string(self.config.join("settings.yaml")).ok()
    }
}

#[tokio::test]
async fn missing_file_reads_as_system_defaults() {
    let app = App::start().await;
    let (status, body) = app.call("GET", ROUTE, None, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["locale"], "system");
    assert_eq!(body["theme"], "system");
    assert_eq!(body["reduced_motion"], "system");
    assert_eq!(body["file_state"], "missing");
    assert_eq!(body["fingerprint"], "missing");
    assert_eq!(body["session_only"], false);
    assert_eq!(app.file(), None, "reading never creates the file");
}

#[tokio::test]
async fn editor_settings_use_fingerprint_and_preserve_appearance() {
    let app = App::start().await;
    let (_, initial) = app.call("GET", EDITOR_ROUTE, None, true).await;
    assert_eq!(initial["file_state"], "missing");
    assert_eq!(initial["session_only"], false);
    assert_eq!(initial["values"]["autosave"], "on");
    assert_eq!(initial["values"]["autosave_max_interval_s"], 30);
    let mut change = initial["values"].clone();
    change["autosave"] = json!("off");
    change["autosave_max_interval_s"] = json!(60);
    let request = json!({"values": change, "fingerprint": initial["fingerprint"]});
    let (status, updated) = app
        .call("PUT", EDITOR_ROUTE, Some(request.clone()), true)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["values"]["autosave"], "off");
    assert_eq!(updated["session_only"], false);
    assert_eq!(updated["values"]["autosave_max_interval_s"], 60);
    assert!(
        app.file()
            .unwrap_or_else(|| panic!("settings file missing"))
            .contains("format: zdashstitch-settings/v1")
    );
    let (status, conflict) = app.call("PUT", EDITOR_ROUTE, Some(request), true).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["code"], "settings_changed");
    let (status, appearance) = app.call("GET", ROUTE, None, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(appearance["theme"], "system");
}

#[tokio::test]
async fn editor_settings_reject_bad_values_and_keep_corrupt_file() {
    let app = App::start().await;
    let (_, initial) = app.call("GET", EDITOR_ROUTE, None, true).await;
    let mut bad = initial["values"].clone();
    bad["autosave_max_interval_s"] = json!(31);
    let (status, _) = app
        .call(
            "PUT",
            EDITOR_ROUTE,
            Some(json!({"values": bad, "fingerprint": initial["fingerprint"]})),
            true,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(app.file(), None);
    let mut valid = initial["values"].clone();
    valid["autosave_max_interval_s"] = json!(120);
    let (status, _) = app
        .call(
            "PUT",
            EDITOR_ROUTE,
            Some(json!({"values": valid, "fingerprint": initial["fingerprint"]})),
            true,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let good = app
        .file()
        .unwrap_or_else(|| panic!("settings file missing"));
    let broken = good.replace(
        "autosave_max_interval_s: 120",
        "autosave_max_interval_s: 31",
    );
    std::fs::write(app.config.join("settings.yaml"), &broken)
        .unwrap_or_else(|error| panic!("write invalid settings: {error}"));
    let (_, snapshot) = app.call("GET", EDITOR_ROUTE, None, true).await;
    assert_eq!(snapshot["file_state"], "invalid");
    let (status, response) = app
        .call(
            "PUT",
            EDITOR_ROUTE,
            Some(json!({"values": initial["values"], "fingerprint": snapshot["fingerprint"]})),
            true,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["file_state"], "invalid");
    assert_eq!(response["session_only"], true);
    assert_eq!(response["values"]["autosave_max_interval_s"], 30);
    assert_eq!(app.file().as_deref(), Some(broken.as_str()));
    let (_, shared) = app.call("GET", EDITOR_ROUTE, None, true).await;
    assert_eq!(shared["values"]["autosave_max_interval_s"], 30);
    assert_eq!(shared["session_only"], true);
    let fresh = super::read_editor(axum::Extension(SettingsHandle::at(
        app.config.clone(),
        app.storage.clone(),
    )))
    .await;
    let bytes = fresh
        .into_body()
        .collect()
        .await
        .unwrap_or_else(|error| panic!("fresh response: {error}"))
        .to_bytes();
    let restarted: Value =
        serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("fresh JSON: {error}"));
    assert_eq!(restarted["values"]["autosave_max_interval_s"], 120);
    assert_eq!(restarted["session_only"], false);
    assert_eq!(app.file().as_deref(), Some(broken.as_str()));
    std::fs::write(app.config.join("settings.yaml"), &good)
        .unwrap_or_else(|error| panic!("repair settings: {error}"));
    let (_, repaired) = app.call("GET", EDITOR_ROUTE, None, true).await;
    assert_eq!(repaired["file_state"], "valid");
    assert_eq!(repaired["session_only"], false);
    assert_eq!(repaired["values"]["autosave_max_interval_s"], 120);
}

#[tokio::test]
async fn change_is_saved_to_the_settings_file_and_read_back() {
    let app = App::start().await;
    let (_, initial) = app.call("GET", ROUTE, None, true).await;
    let change = json!({"locale": "ru", "theme": "high_contrast", "reduced_motion": "reduce", "fingerprint": initial["fingerprint"]});
    let (status, body) = app.call("PUT", ROUTE, Some(change.clone()), true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["locale"], "ru");
    assert_eq!(body["theme"], "high_contrast");
    assert_eq!(body["file_state"], "valid");
    assert_eq!(body["session_only"], false);
    let (status, stale) = app.call("PUT", ROUTE, Some(change), true).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(stale["code"], "settings_changed");
    let file = app.file().unwrap_or_else(|| panic!("settings file"));
    assert!(file.contains("theme: high_contrast\n"), "{file}");
    assert!(file.contains("locale: ru\n"), "{file}");
    assert!(file.contains("reduced_motion: reduce\n"), "{file}");
    // The storage root is the default one and is kept as is.
    let stored = SettingsStore::at(app.config.clone())
        .load()
        .unwrap_or_else(|error| panic!("load: {error}"))
        .unwrap_or_else(|| panic!("settings must exist"));
    assert_eq!(stored.storage_root, app.storage);
    assert_eq!(stored.theme, ThemePreference::HighContrast);

    let (status, body) = app.call("GET", ROUTE, None, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["theme"], "high_contrast");
    assert_eq!(body["file_state"], "valid");

    let back = json!({"locale": "system", "theme": "dark", "reduced_motion": "no-preference", "fingerprint": body["fingerprint"]});
    let (status, body) = app.call("PUT", ROUTE, Some(back), true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["theme"], "dark");
    assert_eq!(body["reduced_motion"], "no-preference");
}

#[tokio::test]
async fn unknown_values_and_fields_are_refused_without_writing() {
    let app = App::start().await;
    for mut body in [
        json!({"locale": "en", "theme": "sepia", "reduced_motion": "system"}),
        json!({"locale": "en", "theme": "high-contrast", "reduced_motion": "system"}),
        json!({"locale": "de", "theme": "light", "reduced_motion": "system"}),
        json!({"locale": "en", "theme": "light"}),
        json!({"locale": "en", "theme": "light", "reduced_motion": "system", "storage_root": "/tmp"}),
        json!("light"),
    ] {
        if body.is_object() {
            body["fingerprint"] = json!("missing");
        }
        let (status, response) = app.call("PUT", ROUTE, Some(body.clone()), true).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(
            response,
            json!({"code": "settings_invalid_value", "params": {}})
        );
    }
    assert_eq!(app.file(), None);
}

#[tokio::test]
async fn change_without_csrf_is_refused_by_the_api_boundary() {
    let app = App::start().await;
    let change = json!({"locale": "en", "theme": "dark", "reduced_motion": "system"});
    let (status, _) = app.call("PUT", ROUTE, Some(change), false).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (_, editor) = app.call("GET", EDITOR_ROUTE, None, true).await;
    let (status, _) = app
        .call(
            "PUT",
            EDITOR_ROUTE,
            Some(json!({"values": editor["values"], "fingerprint": editor["fingerprint"]})),
            false,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(app.file(), None);
}

#[tokio::test]
async fn invalid_file_is_reported_and_never_overwritten() {
    let app = App::start().await;
    let (_, initial) = app.call("GET", ROUTE, None, true).await;
    let change = json!({"locale": "en", "theme": "dark", "reduced_motion": "system", "fingerprint": initial["fingerprint"]});
    let (status, _) = app.call("PUT", ROUTE, Some(change), true).await;
    assert_eq!(status, StatusCode::OK);
    let valid = app.file().unwrap_or_else(|| panic!("settings file"));
    let broken = valid.replacen("theme: dark\n", "theme: sepia\n", 1);
    std::fs::write(app.config.join("settings.yaml"), &broken)
        .unwrap_or_else(|error| panic!("write fixture: {error}"));

    // The last valid copy applies and the state says the file is invalid.
    let (status, body) = app.call("GET", ROUTE, None, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["theme"], "dark");
    assert_eq!(body["file_state"], "invalid");
    assert_eq!(body["session_only"], false);

    let change = json!({"locale": "en", "theme": "high_contrast", "reduced_motion": "system", "fingerprint": body["fingerprint"]});
    let (status, body) = app.call("PUT", ROUTE, Some(change), true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["theme"], "high_contrast");
    assert_eq!(body["session_only"], true);
    let (_, shared) = app.call("GET", ROUTE, None, true).await;
    assert_eq!(shared["theme"], "high_contrast");
    assert_eq!(shared["session_only"], true);
    assert_eq!(app.file().as_deref(), Some(broken.as_str()));
    let fresh = super::read_appearance(axum::Extension(SettingsHandle::at(
        app.config.clone(),
        app.storage.clone(),
    )))
    .await;
    let bytes = fresh
        .into_body()
        .collect()
        .await
        .unwrap_or_else(|error| panic!("fresh appearance: {error}"))
        .to_bytes();
    let restarted: Value =
        serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("fresh JSON: {error}"));
    assert_eq!(restarted["theme"], "dark");
    assert_eq!(restarted["session_only"], false);
    std::fs::write(app.config.join("settings.yaml"), &valid)
        .unwrap_or_else(|error| panic!("repair settings: {error}"));
    let (_, repaired) = app.call("GET", ROUTE, None, true).await;
    assert_eq!(repaired["theme"], "dark");
    assert_eq!(repaired["session_only"], false);
}
