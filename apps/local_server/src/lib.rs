// SPDX-License-Identifier: MIT OR Apache-2.0

//! Loopback-only HTTP shell for the local GUI.

use std::path::{Path, PathBuf};

use axum::{Extension, Json, Router, extract::DefaultBodyLimit, middleware, routing::get};
use serde::Serialize;
use tower_http::services::{ServeDir, ServeFile};

mod about;
mod browser_transfer;
mod connections;
mod drafts;
mod file_dialog;
pub mod instance;
mod security;
mod selections;
mod session_end;
mod settings;
pub mod startup;

pub use browser_transfer::{
    BrowserTransferService, DownloadHandle, TransferError, UploadReceipt, UploadSetHandle,
    UploadedFile,
};
pub use connections::ConnectionsHandle;
pub use file_dialog::{
    DialogCancellation, DialogErrorCode, DialogIntent, FileDialogService, NativeDialogBackend,
};
pub use security::{
    AttachmentResponse, BootstrapIssueError, CSRF_HEADER, InitialBootstrap, JSON_BODY_LIMIT,
    LocalSecurity, SecurityInitError, UPLOAD_BODY_LIMIT, UPLOAD_COUNT_LIMIT,
};
pub use selections::{
    OpaqueSelection, SelectionError, SelectionKind, SelectionRegistry, SelectionRegistryLimits,
};
pub use session_end::{EraseFuture, SessionMemory, end_session_on_close, end_session_on_exit};

/// Non-sensitive health response consumed by the bundled UI.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub product_stage: &'static str,
    pub target_zabbix_family: &'static str,
    pub zabbix_writes_enabled: bool,
}

/// Build the HTTP application. Every route nested under `/api` is protected by
/// the same session, origin, content-type, body-limit and CSRF boundary.
pub fn build_router(ui_dir: impl AsRef<Path>, security: LocalSecurity) -> Router {
    build_router_with_api(ui_dir, security, Router::new())
}

/// The HTTP application with the connection routes (P2-01G) over `connections`.
pub fn build_router_with_connections(
    ui_dir: impl AsRef<Path>,
    security: LocalSecurity,
    connections: ConnectionsHandle,
) -> Router {
    // «End session» (P2-SEC-01E) erases the active connection's secrets.
    if let Some(service) = connections.service() {
        security.register_session_memory(service);
    }
    // Copies of server dashboards and local drafts (P2-05D).
    let api = connections::routes(connections.clone()).merge(drafts::routes(
        connections.clone(),
        drafts::DraftsHandle::from_settings(),
    ));
    // Publication (P2-04E/F): plans live in memory only and are erased with
    // the session.
    let plans = std::sync::Arc::new(drafts::PlanRegistry::default());
    security.register_session_memory(plans.clone());
    let api = api.merge(drafts::publication_routes(
        connections,
        drafts::DraftsHandle::from_settings(),
        plans,
    ));
    // «Settings → Appearance» (P2-00G): language, theme, reduced motion.
    let api = api.merge(settings::routes(settings::SettingsHandle::platform()));
    build_router_with_api(ui_dir, security, api)
}

pub fn build_router_with_transfer_service(
    ui_dir: impl AsRef<Path>,
    security: LocalSecurity,
    transfers: BrowserTransferService,
) -> Router {
    build_router_with_api_and_transfers(ui_dir, security, Router::new(), transfers)
}

fn build_router_with_api(
    ui_dir: impl AsRef<Path>,
    security: LocalSecurity,
    additional_api: Router<LocalSecurity>,
) -> Router {
    build_router_with_api_and_transfers(
        ui_dir,
        security,
        additional_api,
        BrowserTransferService::new(),
    )
}

fn build_router_with_api_and_transfers(
    ui_dir: impl AsRef<Path>,
    security: LocalSecurity,
    additional_api: Router<LocalSecurity>,
    transfers: BrowserTransferService,
) -> Router {
    let ui_dir = ui_dir.as_ref().to_path_buf();
    let index = ui_dir.join("index.html");
    let files = ServeDir::new(ui_dir);
    let index = ServeFile::new(index);
    let api = Router::new()
        .route("/health", get(health))
        .route("/session", get(security::session))
        .route("/about", get(about::about))
        .merge(session_end::routes())
        .merge(browser_transfer::routes())
        .merge(additional_api)
        .fallback(security::api_not_found)
        .layer(Extension(transfers.clone()))
        // Body limits are enforced by `protect_api` before buffering: per
        // method, per route and, for uploads, per upload-set quota.
        .layer(DefaultBodyLimit::disable())
        .layer(middleware::from_fn_with_state(
            security::ApiGuard {
                security: security.clone(),
                transfers,
            },
            security::protect_api,
        ));

    Router::new()
        .nest("/api", api)
        .fallback(move |request: axum::extract::Request| {
            let (files, index) = (files.clone(), index.clone());
            async move { serve_ui(files, index, request).await }
        })
        .layer(middleware::from_fn_with_state(
            security.clone(),
            security::host_and_security_headers,
        ))
        .with_state(security)
}

/// Static files plus SPA deep links: a missing path without a file extension is a
/// client route and gets `index.html` with 200; a missing asset-like path
/// (`/assets/x.js`, `/favicon.ico`) stays 404. `/api/*` never reaches here.
async fn serve_ui(
    mut files: ServeDir,
    mut index: ServeFile,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::{http::StatusCode, response::IntoResponse};

    let is_read = matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD
    );
    let looks_like_asset = request
        .uri()
        .path()
        .rsplit('/')
        .next()
        .is_some_and(|segment| segment.contains('.'));
    let (method, headers) = (request.method().clone(), request.headers().clone());
    match files.try_call(request).await {
        Ok(response) if response.status() != StatusCode::NOT_FOUND => {
            return response.into_response();
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
    if !is_read || looks_like_asset {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mut fallback = axum::http::Request::new(axum::body::Body::empty());
    *fallback.method_mut() = method;
    *fallback.headers_mut() = headers;
    match index.try_call(fallback).await {
        Ok(response) => response.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Default UI build directory, resolved independently from the process CWD.
#[must_use]
pub fn default_ui_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/dist")
}

async fn health() -> Json<HealthResponse> {
    let status = zdashstitch_application::runtime_status();
    Json(HealthResponse {
        status: "ok",
        product_stage: status.stage.as_str(),
        target_zabbix_family: status.target_adapter_family,
        zabbix_writes_enabled: status.zabbix_writes_enabled,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use axum::{
        Json, Router,
        body::Body,
        http::{Request, Response, StatusCode, header},
        routing::{get, post},
    };
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use tempfile::tempdir;
    use tower::ServiceExt;

    use super::{
        AttachmentResponse, BrowserTransferService, CSRF_HEADER, JSON_BODY_LIMIT, LocalSecurity,
        UPLOAD_BODY_LIMIT, UPLOAD_COUNT_LIMIT, build_router, build_router_with_api,
        build_router_with_transfer_service,
    };

    const PORT: u16 = 43123;

    fn security() -> (LocalSecurity, String) {
        let initial = LocalSecurity::initialize(PORT)
            .unwrap_or_else(|error| panic!("security must initialize: {error}"));
        (initial.security, initial.code)
    }

    async fn session_cookie(router: &Router, code: &str) -> String {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/?bootstrap={code}"))
                    .header(header::HOST, format!("127.0.0.1:{PORT}"))
                    .body(Body::empty())
                    .unwrap_or_else(|error| panic!("request must be valid: {error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let set_cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_else(|| panic!("bootstrap must set a cookie"));
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Strict"));
        assert!(set_cookie.contains("Path=/"));
        assert!(!set_cookie.contains("Domain="));
        set_cookie
            .split(';')
            .next()
            .unwrap_or_else(|| panic!("bootstrap cookie must have a value"))
            .to_owned()
    }

    async fn csrf_token(router: &Router, cookie: &str) -> String {
        let response = router
            .clone()
            .oneshot(api_request(
                "GET",
                "/api/session",
                cookie,
                None,
                "application/json",
                Body::empty(),
            ))
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|error| panic!("body must be readable: {error}"))
            .to_bytes();
        let value: Value = serde_json::from_slice(&body)
            .unwrap_or_else(|error| panic!("session response must be JSON: {error}"));
        value["csrf_token"]
            .as_str()
            .unwrap_or_else(|| panic!("session response must contain a CSRF token"))
            .to_owned()
    }

    fn api_request(
        method: &str,
        uri: &str,
        cookie: &str,
        csrf: Option<&str>,
        content_type: &str,
        body: Body,
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, format!("127.0.0.1:{PORT}"))
            .header(header::ORIGIN, format!("http://127.0.0.1:{PORT}"))
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, content_type);
        if let Some(csrf) = csrf {
            builder = builder.header(CSRF_HEADER, csrf);
        }
        builder
            .body(body)
            .unwrap_or_else(|error| panic!("request must be valid: {error}"))
    }

    async fn status(router: &Router, request: Request<Body>) -> StatusCode {
        router
            .clone()
            .oneshot(request)
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"))
            .status()
    }

    #[tokio::test]
    async fn template_upload_requires_preview_and_commits_valid_files_once() {
        let root = tempdir().unwrap_or_else(|error| panic!("{error}"));
        let ui = tempdir().unwrap_or_else(|error| panic!("{error}"));
        let (security, code) = security();
        let transfers = BrowserTransferService::new();
        let router = super::build_router_with_api_and_transfers(
            ui.path(),
            security,
            super::drafts::routes(
                super::ConnectionsHandle::unavailable(),
                super::drafts::DraftsHandle::at(root.path().to_owned()),
            ),
            transfers.clone(),
        );
        let cookie = session_cookie(&router, &code).await;
        let csrf = csrf_token(&router, &cookie).await;
        let set = transfers
            .create_upload_set()
            .unwrap_or_else(|error| panic!("{error:?}"));
        let inspect = format!("/api/library/template-import/{}/inspect", set.id);
        let run = format!("/api/library/template-import/{}/run", set.id);
        let post = |path: &str, token| {
            api_request(
                "POST",
                path,
                &cookie,
                token,
                "application/json",
                Body::from("{}"),
            )
        };
        let payload = br#"{"zabbix_export":{"version":"7.4","templates":[{"template":"fixture","items":[{"name":"Ping","key":"agent.ping","delay":"30s"}]},{"template":"other"}]}}"#;
        for (name, bytes) in [
            ("good.json", payload.as_slice()),
            (
                "unsafe.xml",
                b"<!DOCTYPE zabbix_export SYSTEM 'file:///private'><zabbix_export/>".as_slice(),
            ),
        ] {
            let upload = format!("/api/uploads/{}/files/{name}", set.id);
            assert_eq!(
                status(
                    &router,
                    api_request(
                        "POST",
                        &upload,
                        &cookie,
                        Some(&csrf),
                        "application/octet-stream",
                        Body::from(bytes)
                    )
                )
                .await,
                StatusCode::CREATED
            );
        }

        assert_eq!(
            status(&router, post(&run, Some(&csrf))).await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            status(&router, post(&inspect, None)).await,
            StatusCode::FORBIDDEN
        );
        let response = router
            .clone()
            .oneshot(post(&inspect, Some(&csrf)))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .to_bytes();
        let result: Value =
            serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(result[0]["status"], "ready");
        assert_eq!(result[0]["catalogs"].as_array().map(Vec::len), Some(2));
        assert_eq!(result[0]["catalogs"][0]["metrics_with_hints"], 1);
        assert_eq!(result[1]["status"], "rejected");
        assert_eq!(result[1]["code"], "xml_dtd");
        assert!(!String::from_utf8_lossy(&bytes).contains("file:///private"));
        assert!(!root.path().join("library").exists());
        assert!(
            transfers
                .add_file(
                    &set.id,
                    "later.json",
                    axum::body::Bytes::from_static(payload)
                )
                .is_err()
        );
        assert_eq!(
            status(&router, post(&run, Some(&csrf))).await,
            StatusCode::OK
        );
        assert_eq!(
            status(&router, post(&run, Some(&csrf))).await,
            StatusCode::NOT_FOUND
        );
        let library =
            zdashstitch_application::catalog_library::CatalogLibrary::new(root.path().to_owned());
        assert_eq!(library.list().ok().map(|catalogs| catalogs.len()), Some(2));
    }

    #[tokio::test]
    async fn health_is_read_only_and_honest() {
        let (security, code) = security();
        let router = build_router("missing-ui", security);
        let cookie = session_cookie(&router, &code).await;
        let response = router
            .oneshot(api_request(
                "GET",
                "/api/health",
                &cookie,
                None,
                "application/json",
                Body::empty(),
            ))
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|error| panic!("body must be readable: {error}"))
            .to_bytes();
        let body: Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|error| panic!("health must be JSON: {error}"));

        assert_eq!(body["product_stage"], "scaffold");
        assert_eq!(body["zabbix_writes_enabled"], false);
        assert!(body.get("csrf_token").is_none());
        assert!(body.get("session").is_none());
    }

    #[tokio::test]
    async fn built_ui_is_served_by_the_same_origin() {
        let dir = tempdir().unwrap_or_else(|error| panic!("temp dir must exist: {error}"));
        fs::write(
            dir.path().join("index.html"),
            "<main>ZDashstitch UI smoke</main>",
        )
        .unwrap_or_else(|error| panic!("fixture must be writable: {error}"));

        let (security, _) = security();
        let router = build_router(dir.path(), security);
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::HOST, format!("localhost:{PORT}"))
                    .body(Body::empty())
                    .unwrap_or_else(|error| panic!("request must be valid: {error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));

        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|error| panic!("body must be readable: {error}"))
            .to_bytes();
        assert!(
            body.windows(b"ZDashstitch UI smoke".len())
                .any(|window| { window == b"ZDashstitch UI smoke" })
        );
    }

    /// The publication routes exist (P2-04F), but there is no generic
    /// publish route, every publication route is behind `protect_api`, and
    /// a write needs a live plan handle that only the server's own planner
    /// issues: an HTTP body can never carry a plan or a payload.
    #[tokio::test]
    async fn forged_verified_plan_cannot_enter_through_http() {
        let (security, code) = security();
        let router = super::build_router_with_connections(
            "missing-ui",
            security,
            super::ConnectionsHandle::unavailable(),
        );
        let cookie = session_cookie(&router, &code).await;
        let csrf = csrf_token(&router, &cookie).await;
        let generic = router
            .clone()
            .oneshot(api_request(
                "POST",
                "/api/publish",
                &cookie,
                Some(&csrf),
                "application/json",
                Body::from(r#"{"plan_id":"forged","update_params":{"method":"dashboard.update"}}"#),
            ))
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        assert!(matches!(
            generic.status(),
            StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_FOUND
        ));

        let confirm = "/api/drafts/wc-00000000000000000000000000000000/publication/confirm";
        let post = |cookie: &str, csrf: Option<&str>, body: &'static str| {
            api_request(
                "POST",
                confirm,
                cookie,
                csrf,
                "application/json",
                Body::from(body),
            )
        };
        let forged = r#"{"handle":"plan-0","plan_id":"forged","confirmed":true,"update_params":{"method":"dashboard.update"}}"#;
        let unknown = r#"{"handle":"plan-0","plan_id":"forged","confirmed":true}"#;
        assert_eq!(
            status(&router, post(&cookie, Some(&csrf), forged)).await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            status(&router, post(&cookie, Some(&csrf), unknown)).await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            status(&router, post("missing=1", Some(&csrf), unknown)).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(&router, post(&cookie, None, unknown)).await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn unknown_api_route_stays_behind_the_api_security_boundary() {
        let dir = tempdir().unwrap_or_else(|error| panic!("temp dir must exist: {error}"));
        fs::write(
            dir.path().join("index.html"),
            "<main>static fallback</main>",
        )
        .unwrap_or_else(|error| panic!("fixture must be writable: {error}"));
        let (security, code) = security();
        let router = build_router(dir.path(), security);
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/unknown")
                    .header(header::HOST, format!("127.0.0.1:{PORT}"))
                    .header(header::ORIGIN, format!("http://127.0.0.1:{PORT}"))
                    .body(Body::empty())
                    .unwrap_or_else(|error| panic!("request must be valid: {error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = response
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|error| panic!("body must be readable: {error}"))
            .to_bytes();
        let value: Value = serde_json::from_slice(&body)
            .unwrap_or_else(|error| panic!("error must be JSON: {error}"));
        assert_eq!(value["code"], "http.security.session_invalid");
        assert_eq!(value["params"], json!({}));

        let cookie = session_cookie(&router, &code).await;
        let authenticated = router
            .oneshot(
                Request::builder()
                    .uri("/api/unknown")
                    .header(header::HOST, format!("127.0.0.1:{PORT}"))
                    .header(header::ORIGIN, format!("http://127.0.0.1:{PORT}"))
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap_or_else(|error| panic!("request must be valid: {error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        assert_eq!(authenticated.status(), StatusCode::NOT_FOUND);
        let body = authenticated
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|error| panic!("body must be readable: {error}"))
            .to_bytes();
        let value: Value = serde_json::from_slice(&body)
            .unwrap_or_else(|error| panic!("error must be JSON: {error}"));
        assert_eq!(value["code"], "http.security.api_not_found");
        assert_eq!(value["params"], json!({}));
    }

    #[tokio::test]
    async fn bootstrap_is_single_use_and_short_lived() {
        let (security, code) = security();
        assert_eq!(code.len(), 64);
        assert!(code.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let router = build_router("missing-ui", security);
        let _ = session_cookie(&router, &code).await;
        assert_eq!(
            status(
                &router,
                Request::builder()
                    .uri(format!("/?bootstrap={code}"))
                    .header(header::HOST, format!("127.0.0.1:{PORT}"))
                    .body(Body::empty())
                    .unwrap_or_else(|error| panic!("request must be valid: {error}")),
            )
            .await,
            StatusCode::UNAUTHORIZED
        );

        let expired = LocalSecurity::expired_for_test(PORT)
            .unwrap_or_else(|error| panic!("security must initialize: {error}"));
        let expired_router = build_router("missing-ui", expired.security);
        assert_eq!(
            status(
                &expired_router,
                Request::builder()
                    .uri(format!("/?bootstrap={}", expired.code))
                    .header(header::HOST, format!("127.0.0.1:{PORT}"))
                    .body(Body::empty())
                    .unwrap_or_else(|error| panic!("request must be valid: {error}")),
            )
            .await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn security_errors_are_data_only_and_do_not_echo_inputs() {
        let (security, _) = security();
        let router = build_router("missing-ui", security);
        let marker = "secret-marker";
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .header(header::HOST, format!("127.0.0.1:{PORT}"))
                    .header(header::ORIGIN, format!("http://127.0.0.1:{PORT}"))
                    .header(header::COOKIE, format!("zdashstitch_session={marker}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::empty())
                    .unwrap_or_else(|error| panic!("request must be valid: {error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = response
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|error| panic!("body must be readable: {error}"))
            .to_bytes();
        let value: Value = serde_json::from_slice(&body)
            .unwrap_or_else(|error| panic!("error must be JSON: {error}"));
        assert_eq!(value.as_object().map(serde_json::Map::len), Some(2));
        assert_eq!(value["code"], "http.security.session_invalid");
        assert_eq!(value["params"], json!({}));
        assert!(!String::from_utf8_lossy(&body).contains(marker));
    }

    #[tokio::test]
    async fn api_boundary_rejects_host_origin_session_csrf_and_body_before_side_effects() {
        let effects = Arc::new(AtomicUsize::new(0));
        let effect_handler = effects.clone();
        let extra = Router::new().route(
            "/mutate",
            post(move || {
                let effects = effect_handler.clone();
                async move {
                    effects.fetch_add(1, Ordering::SeqCst);
                    Json(json!({"accepted": true}))
                }
            }),
        );
        let (download_security, code) = security();
        let router = build_router_with_api("missing-ui", download_security, extra);
        let cookie = session_cookie(&router, &code).await;
        let csrf = csrf_token(&router, &cookie).await;

        let mut foreign_origin = api_request(
            "POST",
            "/api/mutate",
            &cookie,
            Some(&csrf),
            "application/json",
            Body::from("{}"),
        );
        foreign_origin.headers_mut().insert(
            header::ORIGIN,
            header::HeaderValue::from_static("https://evil.invalid"),
        );
        assert_eq!(status(&router, foreign_origin).await, StatusCode::FORBIDDEN);

        let mut cross_site = api_request(
            "POST",
            "/api/mutate",
            &cookie,
            Some(&csrf),
            "text/plain",
            Body::from("{}"),
        );
        cross_site.headers_mut().remove(header::ORIGIN);
        cross_site.headers_mut().insert(
            "sec-fetch-site",
            header::HeaderValue::from_static("cross-site"),
        );
        assert_eq!(status(&router, cross_site).await, StatusCode::FORBIDDEN);

        let mut forged_host = api_request(
            "POST",
            "/api/mutate",
            &cookie,
            Some(&csrf),
            "application/json",
            Body::from("{}"),
        );
        forged_host.headers_mut().insert(
            header::HOST,
            header::HeaderValue::from_static("attacker.invalid"),
        );
        assert_eq!(status(&router, forged_host).await, StatusCode::BAD_REQUEST);

        assert_eq!(
            status(
                &router,
                api_request(
                    "POST",
                    "/api/mutate",
                    "missing=1",
                    Some(&csrf),
                    "application/json",
                    Body::from("{}"),
                ),
            )
            .await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(
                &router,
                api_request(
                    "POST",
                    "/api/mutate",
                    &cookie,
                    None,
                    "application/json",
                    Body::from("{}"),
                ),
            )
            .await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            status(
                &router,
                api_request(
                    "POST",
                    "/api/mutate",
                    &cookie,
                    Some("wrong"),
                    "application/json",
                    Body::from("{}"),
                ),
            )
            .await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            status(
                &router,
                api_request(
                    "POST",
                    "/api/mutate",
                    &cookie,
                    Some(&csrf),
                    "application/json",
                    Body::from(vec![b'x'; JSON_BODY_LIMIT + 1]),
                ),
            )
            .await,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(effects.load(Ordering::SeqCst), 0);

        assert_eq!(
            status(
                &router,
                api_request(
                    "POST",
                    "/api/mutate",
                    &cookie,
                    Some(&csrf),
                    "application/json",
                    Body::from("{}"),
                ),
            )
            .await,
            StatusCode::OK
        );
        assert_eq!(effects.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn stale_process_cookie_is_rejected_as_an_expired_session() {
        let (old_security, old_code) = security();
        let old_router = build_router("missing-ui", old_security);
        let old_cookie = session_cookie(&old_router, &old_code).await;
        let (new_security, _) = security();
        let new_router = build_router("missing-ui", new_security);
        assert_eq!(
            status(
                &new_router,
                api_request(
                    "GET",
                    "/api/health",
                    &old_cookie,
                    None,
                    "application/json",
                    Body::empty(),
                ),
            )
            .await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn upload_boundary_rejects_origin_csrf_types_size_and_excess_count() {
        let effects = Arc::new(AtomicUsize::new(0));
        let effect_handler = effects.clone();
        let extra = Router::new().route(
            "/uploads/file",
            post(move || {
                let effects = effect_handler.clone();
                async move {
                    effects.fetch_add(1, Ordering::SeqCst);
                    StatusCode::NO_CONTENT
                }
            }),
        );
        let (security, code) = security();
        let router = build_router_with_api("missing-ui", security, extra);
        let cookie = session_cookie(&router, &code).await;
        let csrf = csrf_token(&router, &cookie).await;

        for content_type in ["multipart/form-data; boundary=x", "text/plain"] {
            assert_eq!(
                status(
                    &router,
                    api_request(
                        "POST",
                        "/api/uploads/file",
                        &cookie,
                        Some(&csrf),
                        content_type,
                        Body::from("x"),
                    ),
                )
                .await,
                StatusCode::UNSUPPORTED_MEDIA_TYPE
            );
        }
        let mut foreign = api_request(
            "POST",
            "/api/uploads/file",
            &cookie,
            Some(&csrf),
            "application/octet-stream",
            Body::from("x"),
        );
        foreign.headers_mut().insert(
            header::ORIGIN,
            header::HeaderValue::from_static("https://evil.invalid"),
        );
        assert_eq!(status(&router, foreign).await, StatusCode::FORBIDDEN);
        assert_eq!(
            status(
                &router,
                api_request(
                    "POST",
                    "/api/uploads/file",
                    &cookie,
                    None,
                    "application/octet-stream",
                    Body::from("x"),
                ),
            )
            .await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            status(
                &router,
                api_request(
                    "POST",
                    "/api/uploads/file",
                    &cookie,
                    Some(&csrf),
                    "application/octet-stream",
                    Body::from(vec![b'x'; UPLOAD_BODY_LIMIT + 1]),
                ),
            )
            .await,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(effects.load(Ordering::SeqCst), 0);

        for _ in 0..(UPLOAD_COUNT_LIMIT * 2) {
            assert_eq!(
                status(
                    &router,
                    api_request(
                        "POST",
                        "/api/uploads/file",
                        &cookie,
                        Some(&csrf),
                        "application/octet-stream",
                        Body::from("x"),
                    ),
                )
                .await,
                StatusCode::NO_CONTENT
            );
        }
        assert_eq!(effects.load(Ordering::SeqCst), UPLOAD_COUNT_LIMIT * 2);
    }

    #[tokio::test]
    async fn upload_extractor_accepts_a_body_larger_than_the_json_limit() {
        let extra = Router::new().route(
            "/uploads/file",
            post(|body: axum::body::Bytes| async move { body.len().to_string() }),
        );
        let (security, code) = security();
        let router = build_router_with_api("missing-ui", security, extra);
        let cookie = session_cookie(&router, &code).await;
        let csrf = csrf_token(&router, &cookie).await;
        let size = JSON_BODY_LIMIT + 1024 * 1024;
        let response = router
            .oneshot(api_request(
                "POST",
                "/api/uploads/file",
                &cookie,
                Some(&csrf),
                "application/octet-stream",
                Body::from(vec![b'x'; size]),
            ))
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|error| panic!("body must be readable: {error}"))
            .to_bytes();
        assert_eq!(body, size.to_string());
    }

    #[tokio::test]
    async fn browser_upload_sets_and_one_time_downloads_use_the_protected_routes() {
        let transfers = BrowserTransferService::new();
        let (security, code) = security();
        let router = build_router_with_transfer_service("missing-ui", security, transfers.clone());
        let cookie = session_cookie(&router, &code).await;
        let csrf = csrf_token(&router, &cookie).await;

        let response = router
            .clone()
            .oneshot(api_request(
                "POST",
                "/api/upload-sets",
                &cookie,
                Some(&csrf),
                "application/json",
                Body::from("{}"),
            ))
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = response
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|error| panic!("body must be readable: {error}"))
            .to_bytes();
        let value: Value = serde_json::from_slice(&body)
            .unwrap_or_else(|error| panic!("upload set must be JSON: {error}"));
        let set_id = value["id"]
            .as_str()
            .unwrap_or_else(|| panic!("upload set must have an ID"));

        let response = router
            .clone()
            .oneshot(api_request(
                "POST",
                &format!("/api/uploads/{set_id}/files/templates/db.yaml"),
                &cookie,
                Some(&csrf),
                "application/octet-stream",
                Body::from("safe"),
            ))
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        assert_eq!(response.status(), StatusCode::CREATED);
        let files = transfers
            .take_upload_set(set_id)
            .unwrap_or_else(|error| panic!("take upload set: {error:?}"));
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "templates/db.yaml");
        assert_eq!(&files[0].bytes[..], b"safe");

        let download = transfers
            .register_download("draft.zip", b"package".to_vec())
            .unwrap_or_else(|error| panic!("register download: {error:?}"));
        let request = Request::builder()
            .uri(format!("/api/downloads/{}", download.id))
            .header(header::HOST, format!("127.0.0.1:{PORT}"))
            .header(header::ORIGIN, format!("http://127.0.0.1:{PORT}"))
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap_or_else(|error| panic!("request must be valid: {error}"));
        let response = router
            .clone()
            .oneshot(request)
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_DISPOSITION),
            Some(&header::HeaderValue::from_static(
                "attachment; filename=\"draft.zip\"; filename*=UTF-8''draft.zip"
            ))
        );
        let body = response
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|error| panic!("body must be readable: {error}"))
            .to_bytes();
        assert_eq!(body, "package");

        let second = Request::builder()
            .uri(format!("/api/downloads/{}", download.id))
            .header(header::HOST, format!("127.0.0.1:{PORT}"))
            .header(header::ORIGIN, format!("http://127.0.0.1:{PORT}"))
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap_or_else(|error| panic!("request must be valid: {error}"));
        assert_eq!(status(&router, second).await, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn spa_deep_links_serve_index_but_missing_assets_and_api_stay_404() {
        let dir = tempdir().unwrap_or_else(|error| panic!("temp dir must exist: {error}"));
        fs::write(dir.path().join("index.html"), "<main>static</main>")
            .unwrap_or_else(|error| panic!("fixture must be writable: {error}"));
        let (spa_security, _) = security();
        let router = build_router(dir.path(), spa_security);
        let get = |uri: &str| {
            Request::builder()
                .uri(uri)
                .header(header::HOST, format!("127.0.0.1:{PORT}"))
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("request must be valid: {error}"))
        };
        let index_response = router
            .clone()
            .oneshot(get("/"))
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        let expected_csp = index_response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .cloned();
        assert!(expected_csp.is_some());
        for uri in ["/drafts", "/connections/new", "/settings/about"] {
            let response = router
                .clone()
                .oneshot(get(uri))
                .await
                .unwrap_or_else(|error| panic!("router must respond: {error}"));
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            assert_eq!(
                response.headers().get(header::CONTENT_SECURITY_POLICY),
                expected_csp.as_ref()
            );
            assert_eq!(
                response.headers().get(header::X_CONTENT_TYPE_OPTIONS),
                Some(&header::HeaderValue::from_static("nosniff"))
            );
            let body = response
                .into_body()
                .collect()
                .await
                .unwrap_or_else(|error| panic!("body must be readable: {error}"))
                .to_bytes();
            assert_eq!(&body[..], b"<main>static</main>");
        }
        for uri in ["/assets/x.js", "/favicon.ico"] {
            let response = router
                .clone()
                .oneshot(get(uri))
                .await
                .unwrap_or_else(|error| panic!("router must respond: {error}"));
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        }
        let api = router
            .oneshot(get("/api/x"))
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        assert_ne!(api.status(), StatusCode::OK);
        let body = api
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|error| panic!("body must be readable: {error}"))
            .to_bytes();
        assert_ne!(&body[..], b"<main>static</main>");
    }

    #[tokio::test]
    async fn security_headers_cover_api_static_and_download_responses_without_cors() {
        let extra = Router::new().route(
            "/downloads/file",
            get(|| async {
                let mut response = Response::new(Body::from("data"));
                response
                    .extensions_mut()
                    .insert(AttachmentResponse::unnamed());
                response
            }),
        );
        let (download_security, code) = security();
        let router = build_router_with_api("missing-ui", download_security, extra);
        let cookie = session_cookie(&router, &code).await;
        let mut download_request = api_request(
            "GET",
            "/api/downloads/file",
            &cookie,
            None,
            "application/json",
            Body::empty(),
        );
        download_request.headers_mut().remove(header::CONTENT_TYPE);
        let response = router
            .clone()
            .oneshot(download_request)
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers();
        assert_eq!(
            headers.get(header::CACHE_CONTROL),
            Some(&header::HeaderValue::from_static("no-store"))
        );
        assert_eq!(
            headers.get(header::X_CONTENT_TYPE_OPTIONS),
            Some(&header::HeaderValue::from_static("nosniff"))
        );
        assert_eq!(
            headers.get(header::REFERRER_POLICY),
            Some(&header::HeaderValue::from_static("no-referrer"))
        );
        assert_eq!(
            headers.get(header::CONTENT_DISPOSITION),
            Some(&header::HeaderValue::from_static("attachment"))
        );
        assert!(headers.get(header::CONTENT_SECURITY_POLICY).is_some());
        assert!(headers.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());

        let dir = tempdir().unwrap_or_else(|error| panic!("temp dir must exist: {error}"));
        fs::write(dir.path().join("index.html"), "<main>static</main>")
            .unwrap_or_else(|error| panic!("fixture must be writable: {error}"));
        let (static_security, _) = security();
        let static_response = build_router(dir.path(), static_security)
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::HOST, format!("127.0.0.1:{PORT}"))
                    .body(Body::empty())
                    .unwrap_or_else(|error| panic!("request must be valid: {error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        assert_eq!(static_response.status(), StatusCode::OK);
        assert!(
            static_response
                .headers()
                .get(header::CONTENT_SECURITY_POLICY)
                .is_some()
        );
        assert_eq!(
            static_response
                .headers()
                .get(header::X_CONTENT_TYPE_OPTIONS),
            Some(&header::HeaderValue::from_static("nosniff"))
        );

        let (error_security, _) = security();
        let error_response = build_router("missing-ui", error_security)
            .oneshot(
                Request::builder()
                    .uri("/?bootstrap=invalid")
                    .header(header::HOST, format!("127.0.0.1:{PORT}"))
                    .body(Body::empty())
                    .unwrap_or_else(|error| panic!("request must be valid: {error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        assert_eq!(error_response.status(), StatusCode::UNAUTHORIZED);
        assert!(
            error_response
                .headers()
                .get(header::CONTENT_SECURITY_POLICY)
                .is_some()
        );
        assert_eq!(
            error_response.headers().get(header::X_CONTENT_TYPE_OPTIONS),
            Some(&header::HeaderValue::from_static("nosniff"))
        );
    }

    fn download_request(method: &str, id: &str, cookie: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(format!("/api/downloads/{id}"))
            .header(header::HOST, format!("127.0.0.1:{PORT}"))
            .header(header::ORIGIN, format!("http://127.0.0.1:{PORT}"))
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap_or_else(|error| panic!("request must be valid: {error}"))
    }

    #[tokio::test]
    async fn head_does_not_consume_a_one_time_download() {
        let transfers = BrowserTransferService::new();
        let (security, code) = security();
        let router = build_router_with_transfer_service("missing-ui", security, transfers.clone());
        let cookie = session_cookie(&router, &code).await;
        let download = transfers
            .register_download("Панель.zip", b"package".to_vec())
            .unwrap_or_else(|error| panic!("register download: {error:?}"));

        let head = router
            .clone()
            .oneshot(download_request("HEAD", &download.id, &cookie))
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        assert_eq!(head.status(), StatusCode::OK);
        assert_eq!(
            head.headers().get(header::CONTENT_LENGTH),
            Some(&header::HeaderValue::from_static("7"))
        );
        assert_eq!(
            head.headers().get(header::CONTENT_DISPOSITION),
            Some(&header::HeaderValue::from_static(
                "attachment; filename=\"______.zip\"; \
                 filename*=UTF-8''%D0%9F%D0%B0%D0%BD%D0%B5%D0%BB%D1%8C.zip"
            ))
        );

        let get = router
            .clone()
            .oneshot(download_request("GET", &download.id, &cookie))
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        assert_eq!(get.status(), StatusCode::OK);
        let body = get
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|error| panic!("body must be readable: {error}"))
            .to_bytes();
        assert_eq!(body, "package");
        assert_eq!(
            status(&router, download_request("GET", &download.id, &cookie)).await,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn navigation_download_without_origin_is_accepted_only_from_the_same_origin() {
        let transfers = BrowserTransferService::new();
        let (security, code) = security();
        let router = build_router_with_transfer_service("missing-ui", security, transfers.clone());
        let cookie = session_cookie(&router, &code).await;
        let download = transfers
            .register_download("draft.zip", b"package".to_vec())
            .unwrap_or_else(|error| panic!("register download: {error:?}"));

        let mut cross_site = download_request("GET", &download.id, &cookie);
        cross_site.headers_mut().remove(header::ORIGIN);
        cross_site.headers_mut().insert(
            "sec-fetch-site",
            header::HeaderValue::from_static("cross-site"),
        );
        assert_eq!(status(&router, cross_site).await, StatusCode::FORBIDDEN);

        let mut navigation = download_request("GET", &download.id, &cookie);
        navigation.headers_mut().remove(header::ORIGIN);
        navigation.headers_mut().insert(
            "sec-fetch-site",
            header::HeaderValue::from_static("same-origin"),
        );
        navigation.headers_mut().insert(
            "sec-fetch-mode",
            header::HeaderValue::from_static("navigate"),
        );
        let response = router
            .clone()
            .oneshot(navigation)
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_DISPOSITION),
            Some(&header::HeaderValue::from_static(
                "attachment; filename=\"draft.zip\"; filename*=UTF-8''draft.zip"
            ))
        );
    }

    #[tokio::test]
    async fn safe_methods_with_a_body_are_rejected_before_the_handler() {
        let effects = Arc::new(AtomicUsize::new(0));
        let effect_handler = effects.clone();
        let extra = Router::new().route(
            "/read",
            get(move || {
                let effects = effect_handler.clone();
                async move {
                    effects.fetch_add(1, Ordering::SeqCst);
                    StatusCode::NO_CONTENT
                }
            }),
        );
        let (security, code) = security();
        let router = build_router_with_api("missing-ui", security, extra);
        let cookie = session_cookie(&router, &code).await;

        assert_eq!(
            status(
                &router,
                api_request(
                    "GET",
                    "/api/read",
                    &cookie,
                    None,
                    "application/json",
                    Body::from("{}"),
                ),
            )
            .await,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        let mut chunked = api_request(
            "GET",
            "/api/read",
            &cookie,
            None,
            "application/json",
            Body::empty(),
        );
        chunked.headers_mut().insert(
            header::TRANSFER_ENCODING,
            header::HeaderValue::from_static("chunked"),
        );
        assert_eq!(
            status(&router, chunked).await,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        let mut announced = api_request(
            "GET",
            "/api/read",
            &cookie,
            None,
            "application/json",
            Body::empty(),
        );
        announced.headers_mut().insert(
            header::CONTENT_LENGTH,
            header::HeaderValue::from_static("1048576"),
        );
        assert_eq!(
            status(&router, announced).await,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(effects.load(Ordering::SeqCst), 0);

        assert_eq!(
            status(
                &router,
                api_request(
                    "GET",
                    "/api/read",
                    &cookie,
                    None,
                    "application/json",
                    Body::empty(),
                ),
            )
            .await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(effects.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn upload_set_and_quota_are_checked_before_the_body_is_read() {
        let transfers = BrowserTransferService::new();
        let (security, code) = security();
        let router = build_router_with_transfer_service("missing-ui", security, transfers.clone());
        let cookie = session_cookie(&router, &code).await;
        let csrf = csrf_token(&router, &cookie).await;

        // Reading this body would exceed the upload limit; the missing set
        // must be reported first.
        let response = router
            .clone()
            .oneshot(api_request(
                "POST",
                "/api/uploads/missing/files/db.yaml",
                &cookie,
                Some(&csrf),
                "application/octet-stream",
                Body::from(vec![b'x'; UPLOAD_BODY_LIMIT + 1]),
            ))
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|error| panic!("body must be readable: {error}"))
            .to_bytes();
        let value: Value = serde_json::from_slice(&body)
            .unwrap_or_else(|error| panic!("error must be JSON: {error}"));
        assert_eq!(value["code"], "http.transfer.invalid");

        let set = transfers
            .create_upload_set()
            .unwrap_or_else(|error| panic!("create set: {error:?}"));
        let mut announced = api_request(
            "POST",
            &format!("/api/uploads/{}/files/db.yaml", set.id),
            &cookie,
            Some(&csrf),
            "application/octet-stream",
            Body::from("x"),
        );
        announced.headers_mut().insert(
            header::CONTENT_LENGTH,
            header::HeaderValue::from(UPLOAD_BODY_LIMIT + 1),
        );
        assert_eq!(
            status(&router, announced).await,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(
            transfers
                .take_upload_set(&set.id)
                .unwrap_or_else(|error| panic!("take set: {error:?}")),
            Vec::new()
        );
    }
}
