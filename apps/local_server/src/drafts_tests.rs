// SPDX-License-Identifier: MIT OR Apache-2.0

//! Draft and server-dashboard routes over a mock Zabbix JSON-RPC server on
//! loopback: the `/api` boundary, read-only server access, RFC 3339 times
//! and that no response echoes a secret, an address or a raw document.
//! All data is synthetic.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex as StdMutex},
};

use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use zdashstitch_application::catalog_library::{CatalogLibrary, inspect_template_file};
use zdashstitch_application::connections::{ConnectionService, MemoryCredentialStore};
use zdashstitch_core::formats::{TypedPortableBundle, read_typed_package_bytes};

use super::{DraftsHandle, routes};
use crate::{CSRF_HEADER, ConnectionsHandle, LocalSecurity, build_router_with_api, connections};

const PORT: u16 = 43178;
const TOKEN: &str = "p205d-token-secret-value";
/// Appears only in the raw snapshot (a widget field value); never in a response.
const RAW_MARKER: &str = "p205d-raw-field-marker";

#[derive(Default)]
struct Script {
    /// `dashboard.get` with `dashboardids`: consumed in order, the last repeats.
    snapshots: VecDeque<Value>,
    /// `dashboard.get` without `dashboardids`.
    list: Value,
    methods: Vec<String>,
}

type Shared = Arc<StdMutex<Script>>;

async fn rpc(
    State(script): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let id = body["id"].clone();
    let method = body["method"].as_str().unwrap_or_default().to_owned();
    let mut script = script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    script.methods.push(method.clone());
    let authorized = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        == Some(format!("Bearer {TOKEN}").as_str());
    let result = match method.as_str() {
        "apiinfo.version" => json!("7.4.16"),
        "user.checkAuthentication" if body["params"]["token"] == TOKEN => {
            json!({"userid": "3", "username": "synthetic"})
        }
        _ if !authorized => {
            return Json(json!({"jsonrpc": "2.0", "id": id, "error": {
                "code": -32602, "message": "Invalid params.", "data": "Not authorized."
            }}))
            .into_response();
        }
        "dashboard.get" if body["params"].get("dashboardids").is_some() => {
            if script.snapshots.len() > 1 {
                script.snapshots.pop_front().unwrap_or(Value::Null)
            } else {
                script
                    .snapshots
                    .front()
                    .cloned()
                    .unwrap_or_else(|| json!([]))
            }
        }
        "dashboard.get" => script.list.clone(),
        "item.get" => json!([
            {"itemid": "30001", "value_type": "3", "units": "", "valuemapid": "0", "valuemap": []}
        ]),
        _ => json!([]),
    };
    Json(json!({"jsonrpc": "2.0", "id": id, "result": result})).into_response()
}

async fn zabbix(script: Script) -> (String, Shared) {
    let script = Arc::new(StdMutex::new(script));
    let app = Router::new()
        .route("/zabbix/api_jsonrpc.php", post(rpc))
        .with_state(Arc::clone(&script));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("address: {error}"));
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            panic!("mock failed: {error}");
        }
    });
    (format!("http://{address}/zabbix"), script)
}

/// Two pages: a known widget on the first, an unknown type on the second.
fn dashboard(name: &str) -> Value {
    json!({
        "dashboardid": "501",
        "name": name,
        "userid": "3",
        "private": "1",
        "display_period": "30",
        "auto_start": "1",
        "users": [],
        "userGroups": [],
        "pages": [
            {
                "dashboard_pageid": "5010",
                "name": "Overview",
                "display_period": "0",
                "widgets": [{
                    "widgetid": "50100", "type": "item", "name": "Synthetic value",
                    "x": "0", "y": "0", "width": "12", "height": "3", "view_mode": "0",
                    "fields": [
                        {"type": "4", "name": "itemid.0", "value": "30001"},
                        {"type": "1", "name": "reference", "value": "AAAAA"}
                    ]
                }]
            },
            {
                "dashboard_pageid": "5011",
                "name": "Vendor",
                "display_period": "120",
                "widgets": [{
                    "widgetid": "50110", "type": "vendor.heatmap", "name": "",
                    "x": "0", "y": "0", "width": "72", "height": "8", "view_mode": "0",
                    "fields": [{"type": "1", "name": "vendor.palette", "value": RAW_MARKER}]
                }]
            }
        ]
    })
}

fn listing(id: &str, name: &str, userid: &str, private: &str) -> Value {
    json!({
        "dashboardid": id, "name": name, "userid": userid, "private": private,
        "pages": [{"dashboard_pageid": format!("{id}0"), "name": "", "widgets": [
            {"type": "item", "fields": [{"type": "4", "name": "itemid.0", "value": "1"}]},
            {"type": "vendor.x", "fields": []}
        ]}]
    })
}

fn default_script() -> Script {
    Script {
        snapshots: VecDeque::from([json!([dashboard("Synthetic DB")])]),
        list: json!([
            listing("501", "Synthetic DB", "3", "1"),
            listing("502", "beta shared", "8", "0")
        ]),
        methods: Vec::new(),
    }
}

struct App {
    router: Router,
    cookie: String,
    csrf: String,
    address: String,
    script: Shared,
    storage: tempfile::TempDir,
    _config: tempfile::TempDir,
}

impl App {
    async fn start(script: Script) -> Self {
        let (address, script) = zabbix(script).await;
        let config = tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"));
        let storage = tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"));
        let handle = ConnectionsHandle::new(ConnectionService::new(
            config.path().join("config"),
            Box::new(MemoryCredentialStore::default()),
        ));
        let initial = LocalSecurity::initialize(PORT)
            .unwrap_or_else(|error| panic!("security must initialize: {error}"));
        let api = connections::routes(handle.clone()).merge(routes(
            handle,
            DraftsHandle::at(storage.path().to_path_buf()),
        ));
        let router = build_router_with_api("missing-ui", initial.security, api);
        let mut app = Self {
            router,
            cookie: String::new(),
            csrf: String::new(),
            address,
            script,
            storage,
            _config: config,
        };
        app.enter(&initial.code).await;
        app
    }

    async fn enter(&mut self, code: &str) {
        let response = self
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/?bootstrap={code}"))
                    .header(header::HOST, format!("127.0.0.1:{PORT}"))
                    .body(Body::empty())
                    .unwrap_or_else(|error| panic!("request: {error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("router: {error}"));
        self.cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .unwrap_or_else(|| panic!("bootstrap must set a cookie"))
            .to_owned();
        let (status, session) = self.call("GET", "/api/session", None).await;
        assert_eq!(status, StatusCode::OK);
        self.csrf = session["csrf_token"]
            .as_str()
            .unwrap_or_else(|| panic!("CSRF token"))
            .to_owned();
    }

    fn request(&self, method: &str, uri: &str, body: Option<&str>) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, format!("127.0.0.1:{PORT}"))
            .header(header::ORIGIN, format!("http://127.0.0.1:{PORT}"))
            .header(header::COOKIE, &self.cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .header(CSRF_HEADER, &self.csrf)
            .body(body.map_or_else(Body::empty, |body| Body::from(body.to_owned())))
            .unwrap_or_else(|error| panic!("request: {error}"))
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, String) {
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
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn call(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let raw = body.map(|body| body.to_string());
        let (status, text) = self.send(self.request(method, uri, raw.as_deref())).await;
        // Connection profiles show their own address by design (P1-02H).
        if !uri.starts_with("/api/connections") {
            self.assert_clean(&text);
        }
        let value = serde_json::from_str(&text).unwrap_or(Value::Null);
        (status, value)
    }

    async fn editor_call(&self, uri: &str, token: &str, body: Value) -> (StatusCode, Value) {
        let mut request = self.request("POST", uri, Some(&body.to_string()));
        request.headers_mut().insert(
            "x-zdashstitch-editor",
            HeaderValue::from_str(token).unwrap_or_else(|error| panic!("{error}")),
        );
        let (status, text) = self.send(request).await;
        self.assert_clean(&text);
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    /// No secret, server address, URL or raw server field in any response.
    fn assert_clean(&self, text: &str) {
        assert!(!text.contains(TOKEN), "response echoes the token: {text}");
        assert!(
            !text.contains(RAW_MARKER),
            "response leaks raw fields: {text}"
        );
        let host = self
            .address
            .trim_start_matches("http://")
            .trim_end_matches("/zabbix");
        assert!(!text.contains(host), "response names the server: {text}");
        assert!(!text.contains("http://") && !text.contains("https://"));
        assert!(
            !text.contains("api_jsonrpc"),
            "response names the API: {text}"
        );
    }

    /// Creates a token profile for the mock and connects it read-only.
    async fn connect(&self) -> String {
        let (status, profile) = self
            .call(
                "POST",
                "/api/connections",
                Some(json!({
                    "name": "Synthetic lab",
                    "address": self.address,
                    "auth": {"method": "api_token", "store_token": false},
                    "trust": {"kind": "system"},
                    "confirm_plain_http_token_storage": true
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{profile}");
        let id = profile["local_id"]
            .as_str()
            .unwrap_or_else(|| panic!("profile id: {profile}"))
            .to_owned();
        let (status, connected) = self
            .call(
                "POST",
                &format!("/api/connections/{id}/connect"),
                Some(json!({"secret": TOKEN})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{connected}");
        assert_eq!(connected["profile"]["mode"], "read_only");
        self.script
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .methods
            .clear();
        id
    }

    fn methods(&self) -> Vec<String> {
        self.script
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .methods
            .clone()
    }

    fn stored_files(&self) -> usize {
        fn count(path: &std::path::Path) -> usize {
            std::fs::read_dir(path).map_or(0, |entries| {
                entries
                    .flatten()
                    .map(|entry| {
                        let path = entry.path();
                        if path.is_dir() { count(&path) } else { 1 }
                    })
                    .sum()
            })
        }
        count(self.storage.path())
    }
}

fn row<'a>(list: &'a Value, dashboardid: &str) -> &'a Value {
    list["rows"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["dashboardid"] == dashboardid))
        .unwrap_or_else(|| panic!("row {dashboardid}: {list}"))
}

fn assert_rfc3339_utc(value: &Value) {
    let text = value.as_str().unwrap_or_default();
    assert!(text.ends_with('Z'), "not a UTC timestamp: {value}");
    assert!(
        time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339).is_ok(),
        "not RFC 3339: {value}"
    );
}

fn assert_reads_only(methods: &[String]) {
    for method in methods {
        assert!(
            !method.ends_with(".create")
                && !method.ends_with(".update")
                && !method.ends_with(".delete"),
            "{method} changes the server"
        );
    }
}

#[test]
fn milliseconds_become_rfc3339_utc() {
    assert_eq!(
        super::rfc3339_utc_ms(1_793_000_000_123).as_deref(),
        Some("2026-10-26T07:33:20.123Z")
    );
    assert_eq!(
        super::rfc3339_utc_ms(0).as_deref(),
        Some("1970-01-01T00:00:00Z")
    );
    assert_eq!(super::rfc3339_utc_ms(u64::MAX), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn routes_stay_behind_session_origin_csrf_and_content_type() {
    let app = App::start(default_script()).await;
    let id = app.connect().await;
    let import = format!("/api/server/{id}/dashboards/501/import");
    let body = json!({"scope": "dashboard"}).to_string();

    let mut no_cookie = app.request("GET", "/api/drafts", None);
    no_cookie.headers_mut().remove(header::COOKIE);
    assert_eq!(app.send(no_cookie).await.0, StatusCode::UNAUTHORIZED);

    let mut foreign = app.request("GET", &format!("/api/server/{id}/dashboards"), None);
    foreign.headers_mut().insert(
        header::ORIGIN,
        HeaderValue::from_static("http://attacker.example.invalid"),
    );
    assert_eq!(app.send(foreign).await.0, StatusCode::FORBIDDEN);

    let mut no_csrf = app.request("POST", &import, Some(&body));
    no_csrf.headers_mut().remove(CSRF_HEADER);
    assert_eq!(app.send(no_csrf).await.0, StatusCode::FORBIDDEN);

    let mut form = app.request("POST", &import, Some(&body));
    form.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-www-form-urlencoded"),
    );
    assert_eq!(app.send(form).await.0, StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let mut rename_without_csrf = app.request(
        "POST",
        "/api/drafts/wc-00000000000000000000000000000000/rename",
        Some(&json!({"name": "x"}).to_string()),
    );
    rename_without_csrf.headers_mut().remove(CSRF_HEADER);
    assert_eq!(app.send(rename_without_csrf).await.0, StatusCode::FORBIDDEN);

    // Nothing reached the server or the storage.
    assert!(app.methods().is_empty(), "{:?}", app.methods());
    assert_eq!(app.stored_files(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn requests_are_validated_before_any_read() {
    let app = App::start(default_script()).await;
    let id = app.connect().await;
    let other = "conn-0123456789abcdef0123456789abcdef";

    let (status, body) = app
        .call("GET", "/api/server/not-a-profile/dashboards", None)
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "connection_id_invalid");

    let (status, body) = app
        .call("GET", &format!("/api/server/{other}/dashboards"), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "not_connected");

    for query in ["access=everyone", "unknown=1", "offset=-1", "limit=x"] {
        let (status, body) = app
            .call("GET", &format!("/api/server/{id}/dashboards?{query}"), None)
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(body, json!({"code": "request_invalid", "params": {}}));
    }
    let (status, body) = app
        .call(
            "GET",
            &format!("/api/server/{id}/dashboards?limit=201"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "list_limit_invalid");
    assert_eq!(body["params"]["max"], "200");

    let (status, body) = app
        .call("GET", &format!("/api/server/{id}/dashboards/50x1"), None)
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "dashboard_id_invalid");

    for body in [
        json!({}),
        json!({"scope": "widget"}),
        json!({"scope": "page"}),
    ] {
        let (status, refused) = app
            .call(
                "POST",
                &format!("/api/server/{id}/dashboards/501/import"),
                Some(body),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(refused["code"], "request_invalid");
    }
    let (status, body) = app
        .call(
            "POST",
            &format!("/api/server/{id}/dashboards/501/import"),
            Some(json!({"scope": "page", "dashboard_pageid": "../1"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "page_id_invalid");
    // P2-08A: a chosen draft name is validated like «Rename», before any read.
    for name in [json!("\u{7}"), json!("   "), json!("x".repeat(256))] {
        let (status, body) = app
            .call(
                "POST",
                &format!("/api/server/{id}/dashboards/501/import"),
                Some(json!({"scope": "dashboard", "name": name})),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{name}");
        assert_eq!(body["code"], "draft_name_invalid");
        assert_eq!(body["params"]["max"], "255");
    }
    let (status, body) = app
        .call(
            "POST",
            &format!("/api/server/{id}/dashboards/501/import"),
            Some(json!({"scope": "dashboard", "name": 5})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "request_invalid");

    let (status, body) = app.call("GET", "/api/drafts/%FF", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "request_invalid");
    let (status, body) = app.call("GET", "/api/drafts/..%2Fconfig", None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "workcopy_id_invalid");
    let (status, body) = app
        .call(
            "GET",
            "/api/drafts/wc-00000000000000000000000000000000",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "workcopy_not_found");

    assert!(app.methods().is_empty(), "{:?}", app.methods());
    assert_eq!(app.stored_files(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_page_routes_use_saved_catalog_without_zabbix_calls() {
    let app = App::start(default_script()).await;
    let imported = inspect_template_file(
        "synthetic.json",
        br#"{"zabbix_export":{"version":"7.4","templates":[{"template":"Synthetic metrics","items":[{"name":"Rate","key":"synthetic.rate","value_type":"FLOAT"}]}]}}"#,
    ).unwrap_or_else(|| panic!("known extension"))
        .unwrap_or_else(|error| panic!("fixture: {error}"));
    let catalog_id = imported[0].catalog.catalog_id.clone();
    let metric_id = imported[0].catalog.metrics[0].id.clone();
    CatalogLibrary::new(app.storage.path().to_owned())
        .save(imported)
        .unwrap_or_else(|error| panic!("save: {error}"));

    let (status, detail) = app
        .call(
            "GET",
            &format!("/api/library/catalogs/{catalog_id}/1"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["metrics"][0]["id"], metric_id);

    let (status, created) = app.call("POST", "/api/library/offline-pages", Some(json!({
        "name": "Synthetic page",
        "catalogs": [{"catalog_id": catalog_id, "version": 1}],
        "roles": {"source": {"cardinality": "many", "slots": ["node-b", "node-a"]}},
        "widgets": [
            {"kind": "graph", "title": "Rate graph", "metrics": [{"catalog_id": catalog_id, "metric_id": metric_id, "hosts": {"role": "source", "selection": "all"}}]},
            {"kind": "item_value", "title": "Rate by host", "metric": {"catalog_id": catalog_id, "metric_id": metric_id}, "hosts": {"role": "source", "selection": "all"}, "expansion": "widget_per_host"}
        ]
    }))).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["bundle"]["project"]["kind"], "page");
    assert_eq!(created["composition"]["instance_count"], 3);
    assert_eq!(created["composition"]["instances"][1]["slot_id"], "node-b");
    assert_eq!(created["composition"]["instances"][2]["slot_id"], "node-a");
    let id = created["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("local id"));
    let (status, opened) = app
        .call("GET", &format!("/api/library/offline-pages/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(opened, created);
    let (status, main_list) = app.call("GET", "/api/drafts", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(main_list["drafts"][0]["storage_kind"], "offline_page");
    assert_eq!(main_list["drafts"][0]["origin"], "local");
    assert!(main_list["drafts"][0].get("source").is_none());
    let (status, draft) = app.call("GET", &format!("/api/drafts/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(draft["storage_kind"], "offline_page");
    assert_eq!(draft["page"], created);
    assert!(
        draft.get("pages").is_none(),
        "no invented Workcopy geometry"
    );
    let update = json!({
        "expected_revision": created["revision"],
        "page": {
            "name": "Edited local page", "catalogs": [{"catalog_id": catalog_id, "version": 1}],
            "roles": {"source": {"cardinality": "many", "slots": ["node-b", "node-a"]}},
            "widgets": [{"kind": "item_value", "widget_id": "widget-1", "title": "One selected slot", "metric": {"catalog_id": catalog_id, "metric_id": metric_id}, "hosts": {"role": "source", "selection": "slot", "slot_id": "node-a"}, "expansion": "single", "layout":{"mode":"fixed","x":7,"y":9,"w":12,"h":5}}]
        }
    });
    let (status, access) = app
        .call(
            "POST",
            &format!("/api/library/offline-pages/{id}/edit-session"),
            Some(json!({"take_over":false})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let token = access["token"]
        .as_str()
        .unwrap_or_else(|| panic!("edit token"));
    let (status, updated) = app
        .editor_call(
            &format!("/api/library/offline-pages/{id}"),
            token,
            update.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["local_id"], id);
    assert_eq!(
        updated["bundle"]["project"]["root"]["widgets"][0]["id"],
        "widget-1"
    );
    assert_eq!(
        updated["composition"]["instances"][0]["sources"][0]["slot_id"],
        "node-a"
    );
    assert_eq!(updated["revision"], 2);
    assert_eq!(
        updated["bundle"]["project"]["root"]["widgets"][0]["layout"],
        json!({"mode":"fixed","x":7,"y":9,"w":12,"h":5})
    );
    let mut fractional = update.clone();
    fractional["expected_revision"] = json!(2);
    fractional["page"]["widgets"][0]["layout"]["x"] = json!(0.5);
    assert_eq!(
        app.call(
            "POST",
            &format!("/api/library/offline-pages/{id}"),
            Some(fractional)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (status, stale) = app
        .editor_call(
            &format!("/api/library/offline-pages/{id}"),
            token,
            update.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(stale["code"], "offline_page_conflict");
    let (status, reopened) = app.call("GET", &format!("/api/drafts/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reopened["page"], updated);
    let (_, renamed_list) = app.call("GET", "/api/drafts", None).await;
    assert_eq!(renamed_list["drafts"][0]["name"], "Edited local page");
    let (status, listed) = app.call("GET", "/api/library/offline-pages", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed.as_array().map(Vec::len), Some(1));
    let request = json!({
        "name": "Rejected page", "catalogs": [{"catalog_id": catalog_id, "version": 1}],
        "roles": {"source": {"cardinality": "many", "slots": ["node-b", "node-a"]}},
        "widgets": [{"kind": "item_value", "title": "Value", "metric": {"catalog_id": catalog_id, "metric_id": metric_id}, "hosts": {"role": "source", "selection": "all"}, "expansion": "single"}]
    });
    let (status, rejected) = app
        .call("POST", "/api/library/offline-pages", Some(request.clone()))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(rejected["code"], "offline_page_invalid");
    let mut target = request.clone();
    target["target_dashboard_id"] = json!("12345");
    assert_eq!(
        app.call("POST", "/api/library/offline-pages", Some(target))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let raw = request.to_string();
    let mut no_csrf = app.request("POST", "/api/library/offline-pages", Some(&raw));
    no_csrf.headers_mut().remove(CSRF_HEADER);
    assert_eq!(app.send(no_csrf).await.0, StatusCode::FORBIDDEN);
    let mut no_csrf = app.request(
        "POST",
        &format!("/api/library/offline-pages/{id}"),
        Some(&raw),
    );
    no_csrf.headers_mut().remove(CSRF_HEADER);
    assert_eq!(app.send(no_csrf).await.0, StatusCode::FORBIDDEN);
    let duplicate_role = raw.replace(
        "\"roles\":{",
        "\"roles\":{\"source\":{\"cardinality\":\"one\",\"slots\":[\"first-slot\"]},",
    );
    let (status, rejected) = app
        .send(app.request("POST", "/api/library/offline-pages", Some(&duplicate_role)))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{rejected}");
    let (status, listed) = app.call("GET", "/api/library/offline-pages", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed.as_array().map(Vec::len), Some(1));
    assert!(app.methods().is_empty());

    let session_path = format!("/api/library/offline-pages/{id}/edit-session");
    let (status, second_view) = app
        .call("POST", &session_path, Some(json!({"take_over":false})))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second_view, json!({"state":"read_only"}));
    let (_, takeover) = app
        .call("POST", &session_path, Some(json!({"take_over":true})))
        .await;
    let second_token = takeover["token"]
        .as_str()
        .unwrap_or_else(|| panic!("second editor token"));
    let mut next = update;
    next["expected_revision"] = json!(2);
    next["page"]["name"] = json!("Second tab edit");
    let (status, denied) = app
        .editor_call(
            &format!("/api/library/offline-pages/{id}"),
            token,
            next.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(denied["code"], "offline_page_edit_lost");
    let (_, still_saved) = app
        .call("GET", &format!("/api/library/offline-pages/{id}"), None)
        .await;
    assert_eq!(still_saved["revision"], 2);
    let (status, after_takeover) = app
        .editor_call(
            &format!("/api/library/offline-pages/{id}"),
            second_token,
            next.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(after_takeover["revision"], 3);
    // A clipboard group is one ordinary CAS edit; one invalid member rejects it all.
    next["expected_revision"] = json!(3);
    let mut copied = next["page"]["widgets"][0].clone();
    copied["widget_id"] = json!("widget-copy");
    copied["layout"] = json!({"mode":"fixed","x":30,"y":9,"w":12,"h":5});
    next["page"]["widgets"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("widget array"))
        .push(copied);
    let (status, group) = app
        .editor_call(
            &format!("/api/library/offline-pages/{id}"),
            second_token,
            next.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{group}");
    assert_eq!(group["revision"], 4);
    next["expected_revision"] = json!(4);
    next["page"]["widgets"][1]["layout"]["x"] = json!(8);
    assert_eq!(
        app.editor_call(
            &format!("/api/library/offline-pages/{id}"),
            second_token,
            next.clone()
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    next["page"]["widgets"][1]["layout"]["x"] = json!(30);
    next["page"]["widgets"][1]["reference"] = json!("unknown-producer");
    assert_eq!(
        app.editor_call(
            &format!("/api/library/offline-pages/{id}"),
            second_token,
            next
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, unchanged) = app
        .call("GET", &format!("/api/library/offline-pages/{id}"), None)
        .await;
    assert_eq!(unchanged, group);
    assert_eq!(
        app.editor_call(&format!("{session_path}/renew"), token, json!({}))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        app.editor_call(&format!("{session_path}/release"), token, json!({}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        app.editor_call(&format!("{session_path}/renew"), second_token, json!({}))
            .await
            .0,
        StatusCode::OK
    );
    let mut no_csrf = app.request("POST", &session_path, Some("{\"take_over\":true}"));
    no_csrf.headers_mut().remove(CSRF_HEADER);
    assert_eq!(app.send(no_csrf).await.0, StatusCode::FORBIDDEN);
    assert!(app.methods().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_dashboard_copies_pages_and_reopens_in_order_without_zabbix_calls() {
    let app = App::start(default_script()).await;
    let imported = inspect_template_file(
        "synthetic.json",
        br#"{"zabbix_export":{"version":"7.4","templates":[{"template":"Synthetic metrics","items":[{"name":"Rate","key":"synthetic.rate","value_type":"FLOAT"}]}]}}"#,
    ).unwrap_or_else(|| panic!("known extension"))
        .unwrap_or_else(|error| panic!("fixture: {error}"));
    let catalog_id = imported[0].catalog.catalog_id.clone();
    let metric_id = imported[0].catalog.metrics[0].id.clone();
    CatalogLibrary::new(app.storage.path().to_owned())
        .save(imported)
        .unwrap_or_else(|error| panic!("save: {error}"));
    let mut page_ids = Vec::new();
    for name in ["PostgreSQL", "ClickHouse"] {
        let (status, page) = app
            .call(
                "POST",
                "/api/library/offline-pages",
                Some(json!({
                    "name": name,
                    "catalogs": [{"catalog_id": catalog_id, "version": 1}],
                    "roles": {"source": {"cardinality": "one", "slots": ["node-a"]}},
                    "widgets": [{"kind": "item_value", "title": name,
                        "metric": {"catalog_id": catalog_id, "metric_id": metric_id},
                        "hosts": {"role": "source", "selection": "one"}, "expansion": "single"}]
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{page}");
        page_ids.push(
            page["local_id"]
                .as_str()
                .unwrap_or_else(|| panic!("page id"))
                .to_owned(),
        );
    }
    let (status, dashboard) = app
        .call(
            "POST",
            "/api/library/offline-dashboards",
            Some(json!({
                "name": "DB Pilot", "page_ids": page_ids,
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{dashboard}");
    assert_eq!(dashboard["bundle"]["project"]["kind"], "dashboard");
    assert_eq!(
        dashboard["bundle"]["project"]["root"]["pages"][0]["name"],
        "PostgreSQL"
    );
    let id = dashboard["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("dashboard id"));
    let path = format!("/api/library/offline-dashboards/{id}");
    let (status, reopened) = app.call("GET", &path, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reopened, dashboard);
    let embedded_path = format!("{path}/pages/{}", page_ids[0]);
    let (status, embedded) = app.call("GET", &embedded_path, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(embedded["bundle"]["project"]["kind"], "page");
    assert_eq!(embedded["bundle"]["project"]["root"]["name"], "PostgreSQL");
    let change = json!({
        "expected_revision": 1,
        "page": {
            "name": "PostgreSQL revised",
            "catalogs": [{"catalog_id": catalog_id, "version": 1}],
            "roles": {"source": {"cardinality": "one", "slots": ["node-a"]}},
            "widgets": [{"kind": "item_value",
                "widget_id": embedded["bundle"]["project"]["root"]["widgets"][0]["id"],
                "title": "Rate revised", "layout": {"mode":"fixed", "x": 2, "y": 3, "w": 12, "h": 5},
                "metric": {"catalog_id": catalog_id, "metric_id": metric_id},
                "hosts": {"role": "source", "selection": "one"}, "expansion": "single"}]
        }
    });
    let (status, denied) = app.call("POST", &embedded_path, Some(change.clone())).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(denied["code"], "offline_page_edit_lost");
    let (_, lease) = app
        .call(
            "POST",
            &format!("{embedded_path}/edit-session"),
            Some(json!({"take_over":false})),
        )
        .await;
    let token = lease["token"]
        .as_str()
        .unwrap_or_else(|| panic!("editor token"));
    let (status, edited) = app.editor_call(&embedded_path, token, change.clone()).await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(edited["revision"], 2);
    let (_, after_edit) = app.call("GET", &path, None).await;
    assert_eq!(
        after_edit["bundle"]["project"]["root"]["pages"][0]["name"],
        "PostgreSQL revised"
    );
    assert_eq!(
        after_edit["bundle"]["project"]["root"]["pages"][1]["name"],
        "ClickHouse"
    );
    let (_, original_source) = app
        .call(
            "GET",
            &format!("/api/library/offline-pages/{}", page_ids[0]),
            None,
        )
        .await;
    assert_eq!(
        original_source["bundle"]["project"]["root"]["name"],
        "PostgreSQL"
    );
    let mut altered_sources = change.clone();
    let (status, stale_page) = app.editor_call(&embedded_path, token, change).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(stale_page["code"], "offline_page_conflict");
    altered_sources["expected_revision"] = json!(2);
    altered_sources["page"]["roles"]["source"]["slots"] = json!(["different-node"]);
    let (status, refused) = app
        .editor_call(&embedded_path, token, altered_sources)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["code"], "offline_dashboard_sources_conflict");
    let source_change = json!({
        "expected_revision": 1,
        "page": {
            "name": "Changed source page",
            "catalogs": [{"catalog_id": catalog_id, "version": 1}],
            "roles": {"source": {"cardinality": "one", "slots": ["node-a"]}},
            "widgets": [{"kind": "item_value", "title": "New title",
                "metric": {"catalog_id": catalog_id, "metric_id": metric_id},
                "hosts": {"role": "source", "selection": "one"}, "expansion": "single"}]
        }
    });
    let request = zdashstitch_application::offline_pages::UpdatePage::parse_json(
        &serde_json::to_vec(&source_change).unwrap_or_else(|error| panic!("{error}")),
    )
    .unwrap_or_else(|| panic!("valid page update"));
    zdashstitch_application::offline_pages::OfflinePageStore::new(app.storage.path().to_owned())
        .update(&page_ids[0], request)
        .unwrap_or_else(|error| panic!("{error}"));
    let (_, after_source_change) = app.call("GET", &path, None).await;
    assert_eq!(
        after_source_change["bundle"]["project"]["root"]["pages"][0]["name"],
        "PostgreSQL revised"
    );
    let (_, listed) = app.call("GET", "/api/drafts", None).await;
    assert!(listed["drafts"].as_array().is_some_and(|rows| {
        rows.iter().any(|row| {
            row["local_id"] == id
                && row["kind"] == "dashboard"
                && row["storage_kind"] == "offline_dashboard"
        })
    }));
    let (status, changed) = app
        .call(
            "POST",
            &path,
            Some(json!({
                "expected_revision": 2, "name": "DB Pilot revised",
                "page_order": [page_ids[1], page_ids[0]], "append_page_ids": [],
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(
        changed["bundle"]["project"]["root"]["pages"][0]["name"],
        "ClickHouse"
    );
    let (status, stale) = app.call("POST", &path, Some(json!({
        "expected_revision": 1, "name": "stale", "page_order": page_ids, "append_page_ids": [],
    }))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(stale["code"], "offline_dashboard_conflict");
    let (status, other_page) = app
        .call(
            "POST",
            "/api/library/offline-pages",
            Some(json!({
                "name": "Incompatible source",
                "catalogs": [{"catalog_id": catalog_id, "version": 1}],
                "roles": {"source": {"cardinality": "one", "slots": ["node-b"]}},
                "widgets": [{"kind": "item_value", "title": "Other",
                    "metric": {"catalog_id": catalog_id, "metric_id": metric_id},
                    "hosts": {"role": "source", "selection": "one"}, "expansion": "single"}]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{other_page}");
    let (status, incompatible) = app
        .call(
            "POST",
            "/api/library/offline-dashboards",
            Some(json!({
                "name": "Invalid mix", "page_ids": [page_ids[0], other_page["local_id"]],
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(incompatible["code"], "offline_dashboard_sources_conflict");
    assert!(app.methods().is_empty(), "{:?}", app.methods());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_dashboard_rewrites_colliding_references_without_changing_sources() {
    let app = App::start(default_script()).await;
    let imported = inspect_template_file(
        "synthetic.json",
        br#"{"zabbix_export":{"version":"7.4","templates":[{"template":"Synthetic metrics","items":[{"name":"Rate","key":"synthetic.rate","value_type":"FLOAT"}]}]}}"#,
    ).unwrap_or_else(|| panic!("known extension"))
        .unwrap_or_else(|error| panic!("fixture: {error}"));
    let catalog_id = imported[0].catalog.catalog_id.clone();
    let metric_id = imported[0].catalog.metrics[0].id.clone();
    CatalogLibrary::new(app.storage.path().to_owned())
        .save(imported)
        .unwrap_or_else(|error| panic!("save catalog: {error}"));
    let mut ids = Vec::new();
    for name in ["One", "Two", "Three"] {
        let (status, page) = app
            .call(
                "POST",
                "/api/library/offline-pages",
                Some(json!({
                    "name": name, "catalogs": [{"catalog_id": catalog_id, "version": 1}],
                    "roles": {"source": {"cardinality": "one", "slots": ["node-a"]}},
                    "widgets": [{"kind": "item_value", "title": name,
                        "metric": {"catalog_id": catalog_id, "metric_id": metric_id},
                        "hosts": {"role": "source", "selection": "one"}, "expansion": "single"}]
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{page}");
        ids.push(
            page["local_id"]
                .as_str()
                .unwrap_or_else(|| panic!("page id"))
                .to_owned(),
        );
    }
    // A saved page with a known Zabbix link is a legitimate project/v2
    // source; the page editor currently cannot create these fields directly.
    let source_file = app.storage.path().join("drafts/offline-pages/pages.json");
    let raw = std::fs::read(&source_file).unwrap_or_else(|error| panic!("read pages: {error}"));
    let mut file: Value =
        serde_json::from_slice(&raw).unwrap_or_else(|error| panic!("parse pages: {error}"));
    let pages = file["pages"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("pages array"));
    for page in pages {
        page["bundle"]["project"]["root"]["widgets"] = json!([
            {"type":"zabbix", "id":"provider", "widget_type":"item", "name":"", "registry_state":"known",
                "fields":[{"name":"reference", "field_type":1, "registry_state":"known", "value":{"kind":"string","value":"ABCDE"}}],
                "layout":{"mode":"auto","w":12,"h":5}},
            {"type":"zabbix", "id":"consumer", "widget_type":"svggraph", "name":"", "registry_state":"known",
                "fields":[{"name":"ds.0.override_hostid._reference", "field_type":1, "registry_state":"known", "value":{"kind":"string","value":"ABCDE._hostid"}}],
                "layout":{"mode":"auto","w":12,"h":5}}
        ]);
    }
    let source_raw =
        serde_json::to_vec(&file).unwrap_or_else(|error| panic!("encode pages: {error}"));
    std::fs::write(&source_file, &source_raw)
        .unwrap_or_else(|error| panic!("save fixture: {error}"));
    let (status, first_version) = app
        .call(
            "POST",
            "/api/library/offline-dashboards",
            Some(json!({
                "name": "Assembled", "page_ids": &ids[..2]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{first_version}");
    let id = first_version["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("dashboard id"));
    let (status, dashboard) = app
        .call(
            "POST",
            &format!("/api/library/offline-dashboards/{id}"),
            Some(json!({
                "expected_revision": 1, "name": "Assembled",
                "page_order": &ids[..2], "append_page_ids": [ids[2].clone()]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{dashboard}");
    assert_eq!(
        first_version["bundle"]["project"]["root"]["pages"]
            .as_array()
            .unwrap_or_else(|| panic!("initial pages"))
            .as_slice(),
        &dashboard["bundle"]["project"]["root"]["pages"]
            .as_array()
            .unwrap_or_else(|| panic!("updated pages"))[..2]
    );
    let pages = dashboard["bundle"]["project"]["root"]["pages"]
        .as_array()
        .unwrap_or_else(|| panic!("dashboard pages"));
    for (page, reference) in pages.iter().zip(["ABCDE", "R0000", "R0001"]) {
        assert_eq!(page["widgets"][0]["fields"][0]["value"]["value"], reference);
        assert_eq!(
            page["widgets"][1]["fields"][0]["value"]["value"],
            format!("{reference}._hostid")
        );
    }
    let (_, reopened) = app
        .call(
            "GET",
            &format!("/api/library/offline-dashboards/{id}"),
            None,
        )
        .await;
    assert_eq!(reopened, dashboard);
    let (status, export) = app
        .call(
            "POST",
            &format!("/api/library/offline-dashboards/{id}/export"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{export}");
    assert_eq!(export["display_name"], format!("{id}.zip"));
    let download_id = export["id"]
        .as_str()
        .unwrap_or_else(|| panic!("download id"));
    let download_path = format!("/api/downloads/{download_id}");
    let response = app
        .router
        .clone()
        .oneshot(app.request("GET", &download_path, None))
        .await
        .unwrap_or_else(|error| panic!("download request: {error}"));
    assert_eq!(response.status(), StatusCode::OK);
    let raw = response
        .into_body()
        .collect()
        .await
        .unwrap_or_else(|error| panic!("download body: {error}"))
        .to_bytes();
    let package =
        read_typed_package_bytes(&raw).unwrap_or_else(|error| panic!("exported package: {error}"));
    let expected: TypedPortableBundle = serde_json::from_value(dashboard["bundle"].clone())
        .unwrap_or_else(|error| panic!("dashboard bundle: {error}"));
    assert_eq!(package, expected);
    let (status, duplicate_download) = app.send(app.request("GET", &download_path, None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{duplicate_download}");
    let (status, missing_export) = app
        .call(
            "POST",
            "/api/library/offline-dashboards/db-missing/export",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing_export}");
    assert_eq!(
        std::fs::read(&source_file).unwrap_or_else(|error| panic!("read sources: {error}")),
        source_raw
    );
    assert!(app.methods().is_empty(), "{:?}", app.methods());

    file["pages"][0]["bundle"]["project"]["root"]["widgets"][1]["fields"][0]["value"]["value"] =
        json!("OTHER._hostid");
    std::fs::write(
        &source_file,
        serde_json::to_vec(&file).unwrap_or_else(|error| panic!("encode dangling: {error}")),
    )
    .unwrap_or_else(|error| panic!("write dangling: {error}"));
    let dashboard_file = app
        .storage
        .path()
        .join("drafts/offline-dashboards/dashboards.json");
    let before =
        std::fs::read(&dashboard_file).unwrap_or_else(|error| panic!("read dashboards: {error}"));
    let (status, refused) = app
        .call(
            "POST",
            "/api/library/offline-dashboards",
            Some(json!({
                "name": "Dangling", "page_ids": [ids[0]]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "offline_dashboard_reference_unresolved");
    assert_eq!(
        std::fs::read(&dashboard_file).unwrap_or_else(|error| panic!("reread dashboards: {error}")),
        before
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_page_resolved_paste_uses_target_evidence_and_preserves_source() {
    let app = App::start(default_script()).await;
    let imported = inspect_template_file("paste.json", br#"{"zabbix_export":{"version":"7.4","templates":[{"template":"Synthetic source","items":[{"name":"Source","key":"synthetic.source","value_type":"FLOAT"}]},{"template":"Synthetic target","items":[{"name":"Target","key":"synthetic.target","value_type":"FLOAT"}]}]}}"#)
        .unwrap_or_else(|| panic!("extension")).unwrap_or_else(|error| panic!("{error}"));
    let pins: Vec<_> = imported
        .iter()
        .map(|entry| {
            (
                entry.catalog.catalog_id.clone(),
                entry.catalog.metrics[0].id.clone(),
            )
        })
        .collect();
    CatalogLibrary::new(app.storage.path().to_owned())
        .save(imported)
        .unwrap_or_else(|error| panic!("{error}"));
    let (_, source) = app.call("POST", "/api/library/offline-pages", Some(json!({
        "name":"Source", "catalogs":[{"catalog_id":pins[0].0,"version":1}],
        "roles":{"source":{"cardinality":"many","slots":["source-a","source-b"]}},
        "widgets":[{"kind":"graph","title":"Source graph","metrics":[{"catalog_id":pins[0].0,"metric_id":pins[0].1,"hosts":{"role":"source","selection":"all"}}]}]
    }))).await;
    let target_request = json!({"name":"Target","catalogs":[{"catalog_id":pins[1].0,"version":1}],"roles":{"target":{"cardinality":"many","slots":["target-x","target-y"]}},"widgets":[{"kind":"item_value","title":"Target neighbor","metric":{"catalog_id":pins[1].0,"metric_id":pins[1].1},"hosts":{"role":"target","selection":"slot","slot_id":"target-x"},"expansion":"single","layout":{"mode":"fixed","x":50,"y":0,"w":8,"h":4}}]});
    let (status, target) = app
        .call(
            "POST",
            "/api/library/offline-pages",
            Some(target_request.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = target["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("target id"));
    let path = format!("/api/library/offline-pages/{id}");
    let (_, access) = app
        .call(
            "POST",
            &format!("{path}/edit-session"),
            Some(json!({"take_over":false})),
        )
        .await;
    let token = access["token"]
        .as_str()
        .unwrap_or_else(|| panic!("editor token"));
    let mut resolved = target_request;
    let mut neighbor = resolved["widgets"][0].clone();
    neighbor["widget_id"] = target["bundle"]["project"]["root"]["widgets"][0]["id"].clone();
    resolved["widgets"] = json!([
        neighbor,
        {"kind":"graph","widget_id":"copied-graph","title":"Copied graph","metrics":[{"catalog_id":pins[1].0,"metric_id":pins[1].1,"hosts":{"role":"target","selection":"all"}}],"layout":{"mode":"fixed","x":0,"y":0,"w":8,"h":4}},
        {"kind":"item_value","widget_id":"copied-item","title":"Copied slot","metric":{"catalog_id":pins[1].0,"metric_id":pins[1].1},"hosts":{"role":"target","selection":"slot","slot_id":"target-y"},"expansion":"single","layout":{"mode":"fixed","x":10,"y":0,"w":8,"h":4}}
    ]);
    let (status, saved) = app
        .editor_call(
            &path,
            token,
            json!({"expected_revision":1,"page":resolved.clone()}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        saved["composition"]["instances"][1]["sources"][0]["slot_id"],
        "target-x"
    );
    assert_eq!(
        saved["composition"]["instances"][1]["sources"][1]["slot_id"],
        "target-y"
    );
    assert_eq!(
        saved["composition"]["instances"][2]["sources"][0]["slot_id"],
        "target-y"
    );
    assert_eq!(
        saved["bundle"]["project"]["root"]["widgets"][0],
        target["bundle"]["project"]["root"]["widgets"][0]
    );
    resolved["widgets"][2]["hosts"]["slot_id"] = json!("source-b");
    assert_eq!(
        app.editor_call(&path, token, json!({"expected_revision":2,"page":resolved}))
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(app.call("GET", &path, None).await.1, saved);
    let source_id = source["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("source id"));
    assert_eq!(
        app.call(
            "GET",
            &format!("/api/library/offline-pages/{source_id}"),
            None
        )
        .await
        .1,
        source
    );
    assert!(app.methods().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_page_and_workcopy_share_list_but_keep_distinct_content_models() {
    let app = App::start(default_script()).await;
    let connection = app.connect().await;
    let (status, workcopy) = app
        .call(
            "POST",
            &format!("/api/server/{connection}/dashboards/501/import"),
            Some(json!({"scope":"dashboard"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let imported = inspect_template_file("synthetic.json", br#"{"zabbix_export":{"version":"7.4","templates":[{"template":"Synthetic local","items":[{"name":"Rate","key":"synthetic.rate","value_type":"FLOAT"}]}]}}"#)
        .unwrap_or_else(|| panic!("extension")).unwrap_or_else(|error| panic!("{error}"));
    let catalog_id = imported[0].catalog.catalog_id.clone();
    let metric_id = imported[0].catalog.metrics[0].id.clone();
    CatalogLibrary::new(app.storage.path().to_owned())
        .save(imported)
        .unwrap_or_else(|error| panic!("{error}"));
    let request = json!({"name":"Local page alongside server copy","catalogs":[{"catalog_id":catalog_id,"version":1}],"roles":{"source":{"cardinality":"one","slots":["source-01"]}},"widgets":[{"kind":"item_value","title":"Local value","metric":{"catalog_id":catalog_id,"metric_id":metric_id},"hosts":{"role":"source","selection":"one"},"expansion":"single"}]});
    let (status, local) = app
        .call("POST", "/api/library/offline-pages", Some(request))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let before = app.methods();
    let (status, list) = app.call("GET", "/api/drafts", None).await;
    assert_eq!(status, StatusCode::OK);
    let rows = list["drafts"]
        .as_array()
        .unwrap_or_else(|| panic!("draft list"));
    assert_eq!(rows.len(), 2);
    let server_row = rows
        .iter()
        .find(|row| row["local_id"] == workcopy["local_id"])
        .unwrap_or_else(|| panic!("workcopy row"));
    let local_row = rows
        .iter()
        .find(|row| row["local_id"] == local["local_id"])
        .unwrap_or_else(|| panic!("local row"));
    assert_eq!(server_row["origin"], "server_copy");
    assert!(server_row.get("source").is_some());
    assert_eq!(local_row["storage_kind"], "offline_page");
    assert_eq!(local_row["origin"], "local");
    assert!(local_row.get("source").is_none());
    let local_id = local["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("local id"));
    let (_, detail) = app
        .call("GET", &format!("/api/drafts/{local_id}"), None)
        .await;
    assert_eq!(detail["page"], local);
    let workcopy_id = workcopy["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("workcopy id"));
    let (_, detail) = app
        .call("GET", &format!("/api/drafts/{workcopy_id}"), None)
        .await;
    assert!(detail.get("pages").is_some());
    assert!(detail.get("bundle").is_none());
    assert_eq!(
        app.methods(),
        before,
        "list and local reopen make no server reads"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_routes_need_the_active_connection() {
    let app = App::start(default_script()).await;
    let (status, body) = app
        .call(
            "GET",
            "/api/server/conn-0123456789abcdef0123456789abcdef/dashboards",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body, json!({"code": "not_connected", "params": {}}));

    // Local drafts need no connection.
    let (status, body) = app.call("GET", "/api/drafts", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"drafts": [], "damaged": []}));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_preview_import_and_drafts_round_trip() {
    let app = App::start(default_script()).await;
    let id = app.connect().await;

    let (status, list) = app
        .call("GET", &format!("/api/server/{id}/dashboards"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["total"], 2);
    assert_eq!(list["counts"], json!({"all": 2, "mine": 1, "shared": 1}));
    assert_eq!(list["limit"], 50);
    assert_eq!(list["truncated"], false);
    assert_eq!(list["empty_reason"], Value::Null);
    assert_eq!(list["existing_copies_known"], true);
    // Sorted by name, case-insensitively.
    assert_eq!(list["rows"][0]["name"], "beta shared");
    let mine = row(&list, "501");
    assert_eq!(mine["name"], "Synthetic DB");
    assert_eq!(mine["owner"], "me");
    assert_eq!(mine["access"], "private");
    assert_eq!(
        (
            &mine["page_count"],
            &mine["widget_count"],
            &mine["editable_widget_count"]
        ),
        (&json!(1), &json!(2), &json!(1))
    );
    assert_eq!(mine["existing_copies"], json!([]));
    assert_eq!(row(&list, "502")["owner"], "other");
    assert_eq!(row(&list, "502")["access"], "shared");

    let (_, shared) = app
        .call(
            "GET",
            &format!("/api/server/{id}/dashboards?access=shared&search=beta&limit=10"),
            None,
        )
        .await;
    assert_eq!(shared["total"], 1);
    assert_eq!(shared["rows"][0]["dashboardid"], "502");

    let (status, preview) = app
        .call("GET", &format!("/api/server/{id}/dashboards/501"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["owner"], "me");
    assert_eq!(preview["pages"][0]["dashboard_pageid"], "5010");
    // P2-05E: the read-only preview carries the «Structure» view: geometry,
    // page periods and dashboard properties.
    assert_eq!(
        preview["pages"][0]["widgets"],
        json!([{"id": "50100", "name": "Synthetic value", "type": "item",
                "x": 0, "y": 0, "width": 12, "height": 3, "editable": true}])
    );
    assert_eq!(preview["pages"][0]["display_period"], Value::Null);
    assert_eq!(preview["pages"][1]["display_period"], 120);
    assert_eq!(
        preview["properties"],
        json!({"display_period": 30, "auto_start": true})
    );
    assert_eq!(preview["pages"][1]["editable_widget_count"], 0);
    assert_eq!(preview["unknown_widget_types"], json!(["vendor.heatmap"]));
    assert_eq!(preview["dependencies"]["items"], 1);
    assert_eq!(app.stored_files(), 0, "preview stores nothing");

    let (status, imported) = app
        .call(
            "POST",
            &format!("/api/server/{id}/dashboards/501/import"),
            Some(json!({"scope": "dashboard"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{imported}");
    assert_eq!(imported["kind"], "dashboard");
    assert_eq!(imported["name"], "Synthetic DB");
    assert_eq!(imported["attempts"], 1);
    assert_eq!(imported["item_metadata"], "complete");
    let local_id = imported["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("local id: {imported}"))
        .to_owned();

    let (status, page) = app
        .call(
            "POST",
            &format!("/api/server/{id}/dashboards/501/import"),
            Some(json!({"scope": "page", "dashboard_pageid": "5011"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{page}");
    assert_eq!(page["kind"], "page");
    assert_eq!(page["name"], "Vendor");
    assert_eq!(page["snapshot_reused"], true);

    let (status, missing_page) = app
        .call(
            "POST",
            &format!("/api/server/{id}/dashboards/501/import"),
            Some(json!({"scope": "page", "dashboard_pageid": "999"})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing_page["code"], "import_page_not_found");

    let (_, list) = app
        .call("GET", &format!("/api/server/{id}/dashboards"), None)
        .await;
    let copies = row(&list, "501")["existing_copies"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(copies.len(), 2);
    assert_rfc3339_utc(&copies[0]["snapshot_at"]);

    let (status, drafts) = app.call("GET", "/api/drafts", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(drafts["damaged"], json!([]));
    let summaries = drafts["drafts"].as_array().cloned().unwrap_or_default();
    assert_eq!(summaries.len(), 2);
    let summary = summaries
        .iter()
        .find(|draft| draft["local_id"] == local_id.as_str())
        .unwrap_or_else(|| panic!("imported draft: {drafts}"));
    assert_eq!(summary["origin"], "server_copy");
    assert_eq!(summary["name"], "Synthetic DB");
    assert_eq!(summary["source"]["connection_local_id"], id.as_str());
    assert_eq!(summary["source"]["server_version"], "7.4.16");
    assert_eq!(summary["source"]["page_name"], Value::Null);
    assert_eq!(summary["freshness"]["state"], "current");
    assert_eq!(summary["local_state"], "saved");
    assert_eq!(summary["counts"], json!({"pages": 2, "widgets": 2}));
    assert_rfc3339_utc(&summary["updated_at"]);
    assert_rfc3339_utc(&summary["source"]["snapshot_at"]);
    assert_rfc3339_utc(&summary["freshness"]["checked_at"]);
    assert!(summary.get("dashboardid").is_none());
    assert!(summary["source"].get("dashboardid").is_none());
    let before_preview_methods = app.methods();
    let (status, portable) = app
        .call(
            "GET",
            &format!("/api/drafts/{local_id}/portable-preview"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{portable}");
    assert_eq!(portable["ready"], false);
    assert!(portable["blocker"].is_string());
    assert_eq!(portable["inventory"]["kind"], "dashboard");
    assert_eq!(
        app.methods(),
        before_preview_methods,
        "preview is local-only"
    );
    let page_summary = summaries
        .iter()
        .find(|draft| draft["kind"] == "page")
        .unwrap_or_else(|| panic!("page draft: {drafts}"));
    assert_eq!(page_summary["name"], "Vendor");
    assert_eq!(page_summary["source"]["page_number"], 2);

    let (status, detail) = app
        .call("GET", &format!("/api/drafts/{local_id}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["local_id"], local_id.as_str());
    assert_eq!(
        detail["properties"],
        json!({"display_period": 30, "auto_start": true})
    );
    assert_eq!(detail["pages"][0]["display_period"], Value::Null);
    assert_eq!(detail["pages"][1]["display_period"], 120);
    assert_eq!(
        detail["pages"][0]["widgets"][0],
        json!({"id": "50100", "name": "Synthetic value", "type": "item",
               "x": 0, "y": 0, "width": 12, "height": 3, "editable": true})
    );
    assert_eq!(detail["pages"][1]["widgets"][0]["editable"], false);

    let (status, renamed) = app
        .call(
            "POST",
            &format!("/api/drafts/{local_id}/rename"),
            Some(json!({"name": "  My copy  "})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["name"], "My copy");
    let (status, refused) = app
        .call(
            "POST",
            &format!("/api/drafts/{local_id}/rename"),
            Some(json!({"name": "\u{7}"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused["code"], "draft_name_invalid");
    assert_eq!(refused["params"]["max"], "255");
    let (status, refused) = app
        .call(
            "POST",
            &format!("/api/drafts/{local_id}/rename"),
            Some(json!({"name": "x", "extra": true})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(refused["code"], "request_invalid");

    // P2-08A: another copy of the same dashboard under the name the UI chose.
    let (status, numbered) = app
        .call(
            "POST",
            &format!("/api/server/{id}/dashboards/501/import"),
            Some(json!({"scope": "dashboard", "name": "  Synthetic DB (copy 2) "})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{numbered}");
    assert_eq!(numbered["name"], "Synthetic DB (copy 2)");
    let numbered_id = numbered["local_id"].as_str().unwrap_or_default();
    let (_, drafts) = app.call("GET", "/api/drafts", None).await;
    let stored = drafts["drafts"]
        .as_array()
        .and_then(|drafts| drafts.iter().find(|draft| draft["local_id"] == numbered_id))
        .unwrap_or_else(|| panic!("numbered draft: {drafts}"));
    assert_eq!(stored["name"], "Synthetic DB (copy 2)");
    assert_eq!(stored["source"]["dashboard_name"], "Synthetic DB");

    assert_reads_only(&app.methods());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changing_dashboard_fails_with_import_inconsistent_and_stores_nothing() {
    let mut script = default_script();
    script.snapshots = VecDeque::from([
        json!([dashboard("Synthetic DB")]),
        json!([dashboard("Synthetic DB v2")]),
        json!([dashboard("Synthetic DB v3")]),
        json!([dashboard("Synthetic DB v4")]),
    ]);
    let app = App::start(script).await;
    let id = app.connect().await;
    let (status, body) = app
        .call(
            "POST",
            &format!("/api/server/{id}/dashboards/501/import"),
            Some(json!({"scope": "dashboard"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_inconsistent");
    assert_eq!(body["params"]["attempts"], "2");
    let changes = body["params"]["change"].as_str().unwrap_or_default();
    assert_eq!(changes.split(',').count(), 2, "{body}");
    assert_eq!(app.stored_files(), 0);
    let (_, drafts) = app.call("GET", "/api/drafts", None).await;
    assert_eq!(drafts["drafts"], json!([]));
    assert_reads_only(&app.methods());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hidden_dashboard_is_not_visible_or_denied() {
    let mut script = default_script();
    script.snapshots = VecDeque::from([json!([])]);
    script.list = json!([]);
    let app = App::start(script).await;
    let id = app.connect().await;
    let (status, list) = app
        .call("GET", &format!("/api/server/{id}/dashboards"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["rows"], json!([]));
    assert_eq!(list["empty_reason"], "not_visible_or_denied");
    let (status, body) = app
        .call("GET", &format!("/api/server/{id}/dashboards/501"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_visible_or_denied");
}
