// SPDX-License-Identifier: MIT OR Apache-2.0

//! Loopback-only HTTP shell for the local GUI.

use std::path::{Path, PathBuf};

use axum::{Json, Router, routing::get};
use serde::Serialize;
use tower_http::services::{ServeDir, ServeFile};

/// Non-sensitive health response consumed by the bundled UI.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub product_stage: &'static str,
    pub target_zabbix_family: &'static str,
    pub zabbix_writes_enabled: bool,
}

/// Build the HTTP application. P0 deliberately exposes no mutating or Zabbix
/// route; verified plans live only inside the trusted Rust application layer.
pub fn build_router(ui_dir: impl AsRef<Path>) -> Router {
    let ui_dir = ui_dir.as_ref().to_path_buf();
    let index = ui_dir.join("index.html");
    let ui = ServeDir::new(ui_dir).not_found_service(ServeFile::new(index));

    Router::new()
        .route("/api/health", get(health))
        .fallback_service(ui)
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
    use std::fs;

    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tempfile::tempdir;
    use tower::ServiceExt;

    use super::build_router;

    #[tokio::test]
    async fn health_is_read_only_and_honest() {
        let response = build_router("missing-ui")
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap_or_else(|error| panic!("request must be valid: {error}")),
            )
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
    }

    #[tokio::test]
    async fn built_ui_is_served_by_the_same_origin() {
        let dir = tempdir().unwrap_or_else(|error| panic!("temp dir must exist: {error}"));
        fs::write(
            dir.path().join("index.html"),
            "<main>ZDashstitch UI smoke</main>",
        )
        .unwrap_or_else(|error| panic!("fixture must be writable: {error}"));

        let response = build_router(dir.path())
            .oneshot(
                Request::builder()
                    .uri("/")
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

    #[tokio::test]
    async fn forged_verified_plan_cannot_enter_through_http() {
        let response = build_router("missing-ui")
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/publish")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"plan_id":"forged","update_params":{"method":"dashboard.update"}}"#,
                    ))
                    .unwrap_or_else(|error| panic!("request must be valid: {error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("router must respond: {error}"));

        assert!(matches!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_FOUND
        ));
    }
}
