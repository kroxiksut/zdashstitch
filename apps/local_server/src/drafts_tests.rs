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
use sha2::{Digest, Sha256};
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

#[path = "drafts_tests/clone_8_0.rs"]
mod clone_8_0;

#[path = "drafts_tests/portable_8_0.rs"]
mod portable_8_0;

#[path = "drafts_tests/lab_p5_04.rs"]
mod lab_p5_04;

#[path = "drafts_tests/lab_clone_8_0.rs"]
mod lab_clone_8_0;

#[path = "drafts_tests/lab_z80_e2e.rs"]
mod lab_z80_e2e;

#[path = "drafts_tests/replace_host_security.rs"]
mod replace_host_security;

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
        "apiinfo.version" if script.list["clone_8_0"] == true => json!("8.0.2"),
        "apiinfo.version" => json!("7.4.16"),
        "user.checkAuthentication" if body["params"]["token"] == TOKEN => {
            json!({"userid": "3", "username": "synthetic", "roleid":"9", "type":"3"})
        }
        _ if !authorized => {
            return Json(json!({"jsonrpc": "2.0", "id": id, "error": {
                "code": -32602, "message": "Invalid params.", "data": "Not authorized."
            }}))
            .into_response();
        }
        "dashboard.get"
            if script.list["lld_fixture"] == true
                && body["params"]["dashboardids"] == json!(["99999"]) =>
        {
            json!([script.list["created"]])
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
        "dashboard.get" if script.list["lld_fixture"] == true => json!([]),
        "dashboard.get" => script.list.clone(),
        "host.get" | "item.get" | "itemprototype.get" if script.list["clone_8_0"] == true => {
            clone_8_0::lookup(&script.list, &method, &body["params"])
        }
        "dashboard.create"
            if script.list["lld_fixture"] == true && script.list["verified_create"] == true =>
        {
            let mut created = body["params"].clone();
            created["dashboardid"] = json!("99999");
            created["userid"] = json!("3");
            script.list["created"] = created;
            json!({"dashboardids":["99999"]})
        }
        "role.get" if script.list["lld_fixture"] == true => {
            json!([{"roleid":"9","type":"3","rules":{"api.access":"1","api.mode":"0","api":[],"actions":[{"name":"edit_dashboards","status":if script.list["deny_create"] == true { "0" } else { "1" }}]}}])
        }
        "item.get"
            if body["params"].get("selectItemDiscovery").is_some()
                && body["params"]["itemids"] == json!(["30002"]) =>
        {
            json!([
                {"itemid":"30002","hostid":"40001","key_":"synthetic.value[db-a]","name":"Value db-a","value_type":"0","units":"","flags":"4","templateid":"0","itemDiscovery":{"parent_itemid":"50001","key_":"synthetic.value[{#ID}]"},"hosts":[{"hostid":"40001","name":"Synthetic host"}]}
            ])
        }
        "item.get"
            if script.list["lld_fixture"] == true && body["params"].get("hostids").is_some() =>
        {
            json!([
                {"itemid":"30003","hostid":"40002","key_":"synthetic.value[db-b]","name":"Value db-b","value_type":"0","units":"","flags":"4","templateid":"0","itemDiscovery":{"parent_itemid":"50003","key_":"synthetic.value[{#ID}]"},"hosts":[{"hostid":"40002","name":"Synthetic target"}]}
            ])
        }
        "itemprototype.get"
            if script.list["lld_fixture"] == true
                && body["params"]["itemids"] == json!(["50003"])
                && body["params"]["output"].to_string().contains("uuid") =>
        {
            json!([{"itemid":"50003","hostid":"40002","templateid":"50002","uuid":""}])
        }
        "itemprototype.get"
            if script.list["lld_fixture"] == true
                && body["params"]["itemids"] == json!(["50003"]) =>
        {
            json!([{"itemid":"50003","hostid":"40002","key_":"synthetic.value[{#ID}]","name":"Value {#ID}","value_type":"0","units":"","templateid":"50002","discoveryRule":[]}])
        }
        "host.get"
            if script.list["lld_fixture"] == true
                && body["params"]["hostids"] == json!(["40002"]) =>
        {
            json!([{"hostid":"40002","host":"synthetic-target","name":"Synthetic target"}])
        }
        "itemprototype.get"
            if script.list["lld_fixture"] == true
                && body["params"]["itemids"] == json!(["50001"])
                && body["params"]["output"].to_string().contains("uuid") =>
        {
            json!([
                {"itemid":"50001","hostid":"40001","templateid":"50002","uuid":""}
            ])
        }
        "itemprototype.get"
            if script.list["lld_fixture"] == true
                && body["params"]["itemids"] == json!(["50002"]) =>
        {
            json!([
                {"itemid":"50002","hostid":"60001","templateid":"0","uuid":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}
            ])
        }
        "template.get"
            if script.list["lld_fixture"] == true
                && body["params"]["templateids"] == json!(["60001"]) =>
        {
            json!([
                {"templateid":"60001","uuid":script.list["template_uuid"]}
            ])
        }
        "itemprototype.get" if body["params"]["itemids"] == json!(["50001"]) => json!([
            {"itemid":"50001","hostid":"40001","key_":"synthetic.value[{#ID}]","name":"Value {#ID}","value_type":"0","units":"","templateid":"0","discoveryRule":[]}
        ]),
        "item.get"
            if script.list["template_metric"] == true
                && body["params"]["itemids"] == json!(["30001"])
                && body["params"]["output"].to_string().contains("uuid") =>
        {
            json!([
                {"itemid":"30001","hostid":"40001","templateid":"30010","uuid":""}
            ])
        }
        "item.get"
            if script.list["template_metric"] == true
                && body["params"]["itemids"] == json!(["30010"]) =>
        {
            json!([
                {"itemid":"30010","hostid":"60001","templateid":"0","uuid":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}
            ])
        }
        "template.get"
            if script.list["template_metric"] == true
                && body["params"]["templateids"] == json!(["60001"]) =>
        {
            json!([
                {"templateid":"60001","uuid":script.list["template_uuid"]}
            ])
        }
        "item.get" if script.list["pattern_global"] == true && body["params"]["hostids"] == json!(["40001"]) => json!([
            {"itemid":"30001","hostid":"40001","key_":"cpu","name":"CPU load","value_type":"0","units":"","flags":"0","templateid":"0","itemDiscovery":[],"hosts":[{"hostid":"40001","name":"Synthetic host"}]}
        ]),
        "item.get" if script.list["pattern_global"] == true && body["params"]["hostids"] == json!(["40002"]) => json!([
            {"itemid":"30002","hostid":"40002","key_":"cpu","name":"CPU load","value_type":"0","units":"","flags":"0","templateid":"0","itemDiscovery":[],"hosts":[{"hostid":"40002","name":"db-two"}]}
        ]),
        "item.get" if body["params"].get("selectItemDiscovery").is_some() => json!([
            {"itemid":"30001","hostid":"40001","key_":"synthetic.rate","name":"Rate","value_type":"0","units":if script.list["metric_units_changed"] == true { "changed" } else { "" },"flags":"0","templateid":if script.list["template_metric"] == true { "30010" } else { "0" },"itemDiscovery":[],"hosts":[{"hostid":"40001","name":"Synthetic host"}]}
        ]),
        "host.get" if script.list["pattern_truncated"] == true && body["params"].get("selectParentTemplates").is_some() =>
            json!((0..101).map(|n| json!({"hostid": format!("{}", 40000+n), "host": "node", "name": "node"})).collect::<Vec<_>>()),
        "host.get"
            if script.list["pattern_global"] == true
                && body["params"].get("selectParentTemplates").is_some() =>
        {
            json!([
                {"hostid":"40001","host":"synthetic-host","name":if script.list["pattern_host_changed"] == true { "db-new" } else { "Synthetic host" }},
                {"hostid":"40002","host":"db-two","name":"db-two"}
            ])
        }
        "host.get" if script.list["pattern_global"] == true && body["params"]["hostids"] == json!(["40002"]) => json!([
            {"hostid":"40002","host":"db-two","name":"db-two"}
        ]),
        "host.get" if body["params"]["hostids"] == json!(["40001", "40002"]) => json!([
            {"hostid":"40001","host":"synthetic-host","name":"Synthetic host"},
            {"hostid":"40002","host":"db-two","name":"db-two"}
        ]),
        "host.get" if body["params"]["hostids"] == json!(["40001"]) => json!([
            {"hostid":"40001","host":"synthetic-host","name":if script.list["pattern_host_changed"] == true { "db-new" } else { "Synthetic host" }}
        ]),
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
    connections: ConnectionsHandle,
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
        let drafts = DraftsHandle::at(storage.path().to_path_buf());
        let api = connections::routes(handle.clone())
            .merge(routes(handle.clone(), drafts.clone()))
            .merge(super::publication_routes(
                handle.clone(),
                drafts,
                Arc::new(super::PlanRegistry::default()),
            ));
        let router = build_router_with_api("missing-ui", initial.security, api);
        let mut app = Self {
            router,
            cookie: String::new(),
            csrf: String::new(),
            address,
            script,
            connections: handle,
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
    let mut no_cookie = app.request("GET", "/api/library/snapshots", None);
    no_cookie.headers_mut().remove(header::COOKIE);
    assert_eq!(app.send(no_cookie).await.0, StatusCode::UNAUTHORIZED);

    let mut foreign = app.request("GET", &format!("/api/server/{id}/dashboards"), None);
    foreign.headers_mut().insert(
        header::ORIGIN,
        HeaderValue::from_static("http://attacker.example.invalid"),
    );
    assert_eq!(app.send(foreign).await.0, StatusCode::FORBIDDEN);
    let mut foreign_snapshots = app.request("GET", "/api/library/snapshots", None);
    foreign_snapshots.headers_mut().insert(
        header::ORIGIN,
        HeaderValue::from_static("http://attacker.example.invalid"),
    );
    assert_eq!(app.send(foreign_snapshots).await.0, StatusCode::FORBIDDEN);

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
async fn library_index_route_rebuilds_cache_and_requires_session() {
    let app = App::start(default_script()).await;
    let unauthorized = Request::builder()
        .uri("/api/library/index")
        .header(header::HOST, format!("127.0.0.1:{PORT}"))
        .body(Body::empty())
        .unwrap_or_else(|error| panic!("{error}"));
    let (unauthorized_status, _) = app.send(unauthorized).await;
    assert_ne!(unauthorized_status, StatusCode::OK);
    let (status, empty) = app.call("GET", "/api/library/index", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        empty["catalogs"]
            .as_array()
            .unwrap_or_else(|| panic!("catalogs array"))
            .len(),
        0
    );
    let imported = inspect_template_file(
        "synthetic.json",
        br#"{"zabbix_export":{"version":"7.4","templates":[{"template":"Synthetic metrics","items":[{"name":"Rate","key":"synthetic.rate","value_type":"FLOAT"}]}]}}"#,
    ).unwrap_or_else(|| panic!("known extension")).unwrap_or_else(|error| panic!("{error}"));
    CatalogLibrary::new(app.storage.path().to_owned())
        .save(imported)
        .unwrap_or_else(|error| panic!("{error}"));
    let (status, indexed) = app.call("GET", "/api/library/index", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        indexed["catalogs"]
            .as_array()
            .unwrap_or_else(|| panic!("catalogs array"))
            .len(),
        1
    );
    assert_eq!(
        indexed["pages"]
            .as_array()
            .unwrap_or_else(|| panic!("pages array"))
            .len(),
        0
    );
    std::fs::write(app.storage.path().join("library-index.json"), b"{partial")
        .unwrap_or_else(|error| panic!("{error}"));
    let requests = tokio::join!(
        app.call("GET", "/api/library/index", None),
        app.call("GET", "/api/library/index", None),
        app.call("GET", "/api/library/index", None),
        app.call("GET", "/api/library/index", None)
    );
    for (status, rebuilt) in [requests.0, requests.1, requests.2, requests.3] {
        assert_eq!(status, StatusCode::OK);
        assert_eq!(rebuilt, indexed);
    }
    let entry_id = indexed["catalogs"][0]["library_id"]
        .as_str()
        .unwrap_or_else(|| panic!("catalog entry id"));
    let (status, renamed) = app
        .call(
            "POST",
            &format!("/api/library/catalogs/{entry_id}/rename"),
            Some(json!({"expected_revision":1,"display_name":"Alias"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["name"], "Alias");
    let (status, duplicated) = app
        .call(
            "POST",
            &format!("/api/library/catalogs/{entry_id}/duplicate"),
            Some(json!({"expected_revision":2})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{duplicated}");
    assert_eq!(duplicated["version"], 2);
    assert_ne!(
        duplicated["library_id"],
        indexed["catalogs"][0]["library_id"]
    );
    let (status, still_original) = app
        .call(
            "GET",
            &format!(
                "/api/library/catalogs/{}/1",
                indexed["catalogs"][0]["catalog_id"]
                    .as_str()
                    .unwrap_or_else(|| panic!("catalog id"))
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{still_original}");
    let (status, refreshed) = app.call("GET", "/api/library/index", None).await;
    assert_eq!(status, StatusCode::OK, "{refreshed}");
    assert_eq!(refreshed["catalogs"].as_array().map(Vec::len), Some(2));
    assert_eq!(refreshed["catalogs"][0]["name"], "Alias");
    assert!(app.methods().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_8_0_page_keeps_profile_and_multi_host_rows_inside_dashboard() {
    let app = App::start(default_script()).await;
    let mut imported = inspect_template_file(
        "synthetic-74.json",
        br#"{"zabbix_export":{"version":"7.4","templates":[{"template":"Synthetic rate","items":[{"name":"Rate","key":"synthetic.rate","value_type":"FLOAT"}]}]}}"#,
    ).unwrap_or_else(|| panic!("known extension"))
        .unwrap_or_else(|error| panic!("fixture: {error}"));
    imported.extend(inspect_template_file(
        "synthetic-80.json",
        br#"{"zabbix_export":{"version":"8.0","templates":[{"template":"Synthetic count","items":[{"name":"Count","key":"synthetic.count","value_type":"UNSIGNED"},{"name":"JSON payload","key":"synthetic.payload","value_type":"JSON"}]}]}}"#,
    ).unwrap_or_else(|| panic!("known extension"))
        .unwrap_or_else(|error| panic!("fixture: {error}")));
    let first_catalog = imported[0].catalog.catalog_id.clone();
    let second_catalog = imported[1].catalog.catalog_id.clone();
    let first_metric = imported[0].catalog.metrics[0].id.clone();
    let second_metric = imported[1]
        .catalog
        .metrics
        .iter()
        .find(|metric| metric.name == "Count")
        .unwrap_or_else(|| panic!("count metric"))
        .id
        .clone();
    CatalogLibrary::new(app.storage.path().to_owned())
        .save(imported)
        .unwrap_or_else(|error| panic!("save: {error}"));
    let request = json!({
        "name":"Synthetic page",
        "evaluation_profile":"8.0",
        "catalogs":[{"catalog_id":first_catalog,"version":1},{"catalog_id":second_catalog,"version":1}],
        "roles":{"source":{"cardinality":"many","slots":["node-b","node-a"]}},
        "widgets":[{"kind":"graph","title":"Rate and count","metrics":[
            {"catalog_id":first_catalog,"metric_id":first_metric,"hosts":{"role":"source","selection":"all"}},
            {"catalog_id":second_catalog,"metric_id":second_metric,"hosts":{"role":"source","selection":"all"}}
        ]}]
    });
    let mut invalid = request.clone();
    invalid["evaluation_profile"] = json!("8.1");
    let (status, _) = app
        .call("POST", "/api/library/offline-pages", Some(invalid))
        .await;
    assert!(status.is_client_error());
    let (status, page) = app
        .call("POST", "/api/library/offline-pages", Some(request))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{page}");
    assert_eq!(
        page["widget_evaluation"],
        json!({"profile":"8.0","unrecognized":[]})
    );
    assert_eq!(
        page["bundle"]["project"]["declared_compatibility"],
        json!({"minimum":"8.0.0","maximum_exclusive":"8.1.0"})
    );
    assert_eq!(page["composition"]["instance_count"], 1);
    assert_eq!(
        page["composition"]["instances"][0]["sources"],
        json!([
            {"catalog_id":first_catalog,"metric_id":first_metric,"role":"source","slot_id":"node-b"},
            {"catalog_id":first_catalog,"metric_id":first_metric,"role":"source","slot_id":"node-a"},
            {"catalog_id":second_catalog,"metric_id":second_metric,"role":"source","slot_id":"node-b"},
            {"catalog_id":second_catalog,"metric_id":second_metric,"role":"source","slot_id":"node-a"}
        ])
    );
    let id = page["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("page id"));
    let (status, reopened) = app
        .call("GET", &format!("/api/library/offline-pages/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reopened, page);
    let (status, dashboard) = app
        .call(
            "POST",
            "/api/library/offline-dashboards",
            Some(json!({"name":"Synthetic dashboard","page_ids":[id]})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{dashboard}");
    let dashboard_id = dashboard["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("dashboard id"));
    let (status, embedded) = app
        .call(
            "GET",
            &format!("/api/library/offline-dashboards/{dashboard_id}/pages/{id}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(embedded["widget_evaluation"], page["widget_evaluation"]);
    assert_eq!(embedded["composition"], page["composition"]);
    assert!(app.methods().is_empty());
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
    assert_eq!(created["widget_evaluation"]["profile"], "7.4");
    assert_eq!(created["widget_evaluation"]["unrecognized"], json!([]));
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
    let mut ids = Vec::new();
    for name in ["One", "Two", "Three"] {
        let export = format!(
            r#"{{"zabbix_export":{{"version":"7.4","templates":[{{"template":"{name} metrics","items":[{{"name":"Rate {name}","key":"synthetic.rate.{name}","value_type":"FLOAT"}}]}}]}}}}"#
        );
        let imported = inspect_template_file("synthetic.json", export.as_bytes())
            .unwrap_or_else(|| panic!("known extension"))
            .unwrap_or_else(|error| panic!("fixture: {error}"));
        let catalog_id = imported[0].catalog.catalog_id.clone();
        let metric_id = imported[0].catalog.metrics[0].id.clone();
        CatalogLibrary::new(app.storage.path().to_owned())
            .save(imported)
            .unwrap_or_else(|error| panic!("save catalog: {error}"));
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
    assert_eq!(
        dashboard["catalog_versions"].as_array().map(Vec::len),
        Some(3)
    );
    assert_eq!(
        dashboard["bundle"]["catalogs"]
            .as_object()
            .map(|catalogs| catalogs.len()),
        Some(3)
    );
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
    let (status, library_page) = app
        .call(
            "POST",
            &format!("/api/library/pages/from-draft/{}", ids[0]),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{library_page}");
    let library_id = library_page["library_id"]
        .as_str()
        .unwrap_or_else(|| panic!("library id"));
    let source_snapshot =
        zdashstitch_application::page_library::PageLibrary::new(app.storage.path().to_owned())
            .get(library_id)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("library snapshot"));
    let (status, taken) = app
        .call(
            "POST",
            &format!("/api/library/pages/{library_id}/take"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{taken}");
    let taken_id = taken["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("taken id"));
    assert_ne!(taken_id, ids[0]);
    let taken_widgets = &taken["bundle"]["project"]["root"]["widgets"];
    assert_ne!(taken_widgets[0]["id"], "provider");
    assert_ne!(taken_widgets[1]["id"], "consumer");
    assert_ne!(taken_widgets[0]["id"], taken_widgets[1]["id"]);
    assert_eq!(taken_widgets[0]["fields"][0]["value"]["value"], "ABCDE");
    assert_eq!(
        taken_widgets[1]["fields"][0]["value"]["value"],
        "ABCDE._hostid"
    );
    let (status, edit) = app
        .call(
            "POST",
            &format!("/api/library/offline-pages/{taken_id}/edit-session"),
            Some(json!({"take_over": false})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{edit}");
    let token = edit["token"]
        .as_str()
        .unwrap_or_else(|| panic!("editor token"));
    let (status, changed) = app
        .editor_call(
            &format!("/api/library/offline-pages/{taken_id}"),
            token,
            json!({
                "expected_revision": 1,
                "page": {
                    "name": "Edited taken page",
                    "catalogs": taken["catalog_versions"],
                    "roles": taken["bundle"]["project"]["roles"],
                    "widgets": [
                        {"kind":"preserved", "widget_id":taken_widgets[0]["id"]},
                        {"kind":"preserved", "widget_id":taken_widgets[1]["id"]}
                    ]
                }
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(
        changed["bundle"]["project"]["root"]["name"],
        "Edited taken page"
    );
    assert_eq!(
        changed["bundle"]["project"]["root"]["widgets"],
        *taken_widgets
    );
    assert_eq!(
        zdashstitch_application::page_library::PageLibrary::new(app.storage.path().to_owned())
            .get(library_id)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("library snapshot"))
            .bundle,
        source_snapshot.bundle
    );
    assert_eq!(
        app.call(
            "GET",
            &format!("/api/library/offline-dashboards/{id}"),
            None
        )
        .await
        .1,
        dashboard
    );
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
    assert_eq!(export["sha256"], format!("{:x}", Sha256::digest(&raw)));
    assert_eq!(export["size_bytes"], raw.len());
    let entries = zdashstitch_core::formats::package_entry_hashes(&raw)
        .unwrap_or_else(|error| panic!("entry hashes: {error}"));
    assert_eq!(
        export["entry_hashes"],
        serde_json::to_value(entries).unwrap_or_else(|error| panic!("entry hashes JSON: {error}"))
    );
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
    let current_source_file: Value = serde_json::from_slice(
        &std::fs::read(&source_file).unwrap_or_else(|error| panic!("read sources: {error}")),
    )
    .unwrap_or_else(|error| panic!("parse sources: {error}"));
    let original_source_file: Value = serde_json::from_slice(&source_raw)
        .unwrap_or_else(|error| panic!("parse original sources: {error}"));
    assert_eq!(
        &current_source_file["pages"]
            .as_array()
            .unwrap_or_else(|| panic!("current pages"))[..3],
        original_source_file["pages"]
            .as_array()
            .unwrap_or_else(|| panic!("original pages"))
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
async fn library_page_is_independent_of_draft_and_assembled_dashboard() {
    let app = App::start(default_script()).await;
    let imported = inspect_template_file("synthetic.json", br#"{"zabbix_export":{"version":"7.4","templates":[{"template":"Reusable metrics","items":[{"name":"Rate","key":"synthetic.rate","value_type":"FLOAT"}]}]}}"#)
        .unwrap_or_else(|| panic!("extension")).unwrap_or_else(|error| panic!("{error}"));
    let catalog_id = imported[0].catalog.catalog_id.clone();
    let metric_id = imported[0].catalog.metrics[0].id.clone();
    CatalogLibrary::new(app.storage.path().to_owned())
        .save(imported)
        .unwrap_or_else(|error| panic!("{error}"));
    let mut request = json!({"name":"Original","catalogs":[{"catalog_id":catalog_id,"version":1}],
        "roles":{"source":{"cardinality":"one","slots":["node-a"]}},
        "widgets":[{"kind":"item_value","title":"Original widget","metric":{"catalog_id":catalog_id,"metric_id":metric_id},"hosts":{"role":"source","selection":"one"},"expansion":"single"}]});
    let (status, draft) = app
        .call("POST", "/api/library/offline-pages", Some(request.clone()))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{draft}");
    let draft_id = draft["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("draft id"));
    let save_path = format!("/api/library/pages/from-draft/{draft_id}");
    let (status, first) = app.call("POST", &save_path, Some(json!({}))).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    let (status, second) = app.call("POST", &save_path, Some(json!({}))).await;
    assert_eq!(status, StatusCode::CREATED, "{second}");
    let library_id = first["library_id"]
        .as_str()
        .unwrap_or_else(|| panic!("library id"));
    assert_ne!(first["library_id"], second["library_id"]);
    let (status, dashboard) = app
        .call(
            "POST",
            "/api/library/offline-dashboards",
            Some(json!({"name":"Assembled","page_ids":[library_id]})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{dashboard}");
    let dashboard_id = dashboard["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("dashboard id"));
    let catalog_entry = CatalogLibrary::new(app.storage.path().to_owned())
        .list()
        .unwrap_or_else(|error| panic!("{error}"))[0]
        .library_id
        .clone()
        .unwrap_or_else(|| panic!("catalog entry"));
    let (status, usage) = app
        .call(
            "GET",
            &format!("/api/library/catalogs/{catalog_entry}/delete-preview"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{usage}");
    let consumers = usage["used_by"]
        .as_array()
        .unwrap_or_else(|| panic!("consumers"));
    assert!(consumers.iter().any(|value| value == draft_id));
    assert!(consumers.iter().any(|value| value == library_id));
    assert!(consumers.iter().any(|value| value == dashboard_id));
    let (status, blocked) = app
        .call(
            "POST",
            &format!("/api/library/catalogs/{catalog_entry}/delete"),
            Some(json!({"expected_revision":1,"confirmed_name":usage["name"]})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{blocked}");
    assert_eq!(blocked["code"], "catalog_in_use");
    let original_page = dashboard["bundle"]["project"]["root"]["pages"][0].clone();
    let embedded_id = original_page["id"]
        .as_str()
        .unwrap_or_else(|| panic!("page id"));
    let (status, dashboard_copy) = app
        .call(
            "POST",
            &format!("/api/library/pages/from-dashboard/{dashboard_id}/{embedded_id}"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{dashboard_copy}");
    let dashboard_copy_id = dashboard_copy["library_id"]
        .as_str()
        .unwrap_or_else(|| panic!("library id"));
    let (status, missing) = app
        .call(
            "POST",
            &format!("/api/library/pages/from-dashboard/{dashboard_id}/missing"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
    request["name"] = json!("Changed draft");
    let update = zdashstitch_application::offline_pages::UpdatePage::parse_json(
        &serde_json::to_vec(&json!({"expected_revision":1,"page":request}))
            .unwrap_or_else(|error| panic!("{error}")),
    )
    .unwrap_or_else(|| panic!("update"));
    zdashstitch_application::offline_pages::OfflinePageStore::new(app.storage.path().to_owned())
        .update(draft_id, update)
        .unwrap_or_else(|error| panic!("{error}"));
    let reopened =
        zdashstitch_application::page_library::PageLibrary::new(app.storage.path().to_owned());
    assert_eq!(
        reopened
            .list()
            .unwrap_or_else(|error| panic!("{error}"))
            .len(),
        3
    );
    let copied = reopened
        .get(dashboard_copy_id)
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("dashboard page"));
    assert_eq!(
        serde_json::to_value(&copied.bundle.project.root).unwrap_or_else(|error| panic!("{error}")),
        original_page
    );
    let saved = reopened
        .get(library_id)
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("saved page"));
    assert_eq!(
        serde_json::to_value(&saved.bundle.project.root).unwrap_or_else(|error| panic!("{error}")),
        draft["bundle"]["project"]["root"]
    );
    let (status, taken) = app
        .call(
            "POST",
            &format!("/api/library/pages/{library_id}/take"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{taken}");
    let taken_id = taken["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("taken id"));
    assert_ne!(taken_id, draft_id);
    assert_eq!(taken["bundle"]["project"]["project_id"], taken_id);
    assert_eq!(taken["bundle"]["project"]["root"]["id"], taken_id);
    assert_ne!(
        taken["bundle"]["project"]["root"]["widgets"][0]["id"],
        draft["bundle"]["project"]["root"]["widgets"][0]["id"]
    );
    assert_eq!(taken["catalog_versions"], draft["catalog_versions"]);
    assert_eq!(taken["bundle"]["catalogs"], draft["bundle"]["catalogs"]);
    assert_eq!(
        taken["bundle"]["project"]["roles"],
        draft["bundle"]["project"]["roles"]
    );
    let (status, reopened_taken) = app
        .call(
            "GET",
            &format!("/api/library/offline-pages/{taken_id}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reopened_taken}");
    assert_eq!(reopened_taken["bundle"], taken["bundle"]);
    let (status, missing_take) = app
        .call(
            "POST",
            "/api/library/pages/lp-missing/take",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing_take}");
    assert_eq!(
        reopened
            .get(library_id)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("library page"))
            .bundle,
        saved.bundle
    );
    assert_eq!(
        app.call(
            "GET",
            &format!("/api/library/offline-dashboards/{dashboard_id}"),
            None
        )
        .await
        .1["bundle"]["project"]["root"]["pages"][0],
        original_page
    );
    let mut no_csrf = app.request(
        "POST",
        &format!("/api/library/pages/{library_id}/delete"),
        Some(r#"{"expected_revision":1,"confirmed_name":"Original"}"#),
    );
    no_csrf.headers_mut().remove(CSRF_HEADER);
    let (blocked_status, _) = app.send(no_csrf).await;
    assert_ne!(blocked_status, StatusCode::OK);
    let (status, duplicate) = app
        .call(
            "POST",
            &format!("/api/library/pages/{library_id}/duplicate"),
            Some(json!({"expected_revision":1})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{duplicate}");
    assert_ne!(duplicate["library_id"], first["library_id"]);
    let (status, renamed) = app
        .call(
            "POST",
            &format!("/api/library/pages/{library_id}/rename"),
            Some(json!({"expected_revision":1,"display_name":"Display only"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["name"], "Display only");
    assert_eq!(renamed["revision"], 2);
    let (status, conflict) = app
        .call(
            "POST",
            &format!("/api/library/pages/{library_id}/rename"),
            Some(json!({"expected_revision":1,"display_name":"Stale"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    let (status, mismatch) = app
        .call(
            "POST",
            &format!("/api/library/pages/{library_id}/delete"),
            Some(json!({"expected_revision":2,"confirmed_name":"Original"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{mismatch}");
    let (status, deleted) = app
        .call(
            "POST",
            &format!("/api/library/pages/{library_id}/delete"),
            Some(json!({"expected_revision":2,"confirmed_name":"Display only"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{deleted}");
    assert!(
        reopened
            .get(library_id)
            .unwrap_or_else(|error| panic!("{error}"))
            .is_none()
    );
    assert!(
        reopened
            .get(
                duplicate["library_id"]
                    .as_str()
                    .unwrap_or_else(|| panic!("duplicate id"))
            )
            .unwrap_or_else(|error| panic!("{error}"))
            .is_some()
    );
    assert_eq!(
        app.call(
            "GET",
            &format!("/api/library/offline-pages/{taken_id}"),
            None
        )
        .await
        .1["bundle"],
        taken["bundle"]
    );
    assert_eq!(
        app.call(
            "GET",
            &format!("/api/library/offline-dashboards/{dashboard_id}"),
            None
        )
        .await
        .1["bundle"]["project"]["root"]["pages"][0],
        original_page
    );
    assert!(app.methods().is_empty());
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

    let (status, library_snapshots) = app.call("GET", "/api/library/snapshots", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        library_snapshots["snapshots"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        library_snapshots["snapshots"][0]["used_by"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    let serialized = library_snapshots.to_string();
    assert!(!serialized.contains("raw_api_document"));
    assert!(!serialized.contains("dashboardid"));
    assert!(!serialized.contains("Synthetic DB"));
    let (status, library_index) = app.call("GET", "/api/library/index", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(library_index["snapshots"], library_snapshots["snapshots"]);
    let snapshot_id = library_snapshots["snapshots"][0]["library_id"]
        .as_str()
        .unwrap_or_else(|| panic!("snapshot id: {library_snapshots}"));
    let (status, comparison) = app
        .call(
            "POST",
            "/api/library/snapshots/compare",
            Some(json!({"left": snapshot_id, "right": snapshot_id})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(comparison["identical"], true);
    assert!(comparison.get("raw_api_document").is_none());
    let (status, missing) = app
        .call(
            "POST",
            "/api/library/snapshots/compare",
            Some(json!({"left": snapshot_id, "right": "snap-invalid"})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing["code"], "snapshot_library_not_found");
    let (status, deletion) = app
        .call(
            "GET",
            &format!("/api/library/snapshots/{snapshot_id}/delete-preview"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(deletion["deletable"], false);
    assert_eq!(deletion["workcopy_count"], 2);
    let (status, refused) = app
        .call(
            "POST",
            &format!("/api/library/snapshots/{snapshot_id}/delete"),
            Some(json!({"confirmed": true})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["code"], "snapshot_delete_blocked");

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
    let (blocked_status, blocked) = app
        .call(
            "POST",
            &format!("/api/drafts/{local_id}/make-portable"),
            Some(json!({})),
        )
        .await;
    assert_eq!(blocked_status, StatusCode::CONFLICT, "{blocked}");
    assert_eq!(blocked["code"], portable["blocker"]);
    let (status, blocked_library) = app
        .call(
            "POST",
            &format!("/api/library/pages/from-workcopy/{local_id}"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{blocked_library}");
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
async fn selected_dashboard_workcopy_page_is_independent_and_protected() {
    let source = json!({
        "dashboardid": "801", "name": "Synthetic", "userid": "3", "private": "1",
        "display_period": "30", "auto_start": "1", "users": [], "userGroups": [],
        "pages": [
            {"dashboard_pageid": "8010", "name": "Portable", "sortorder": "0", "display_period": "0",
             "widgets": [{"widgetid": "80100", "type": "clock", "name": "Clock", "x": "0", "y": "0", "width": "12", "height": "4", "view_mode": "0", "fields": []}]},
            {"dashboard_pageid": "8020", "name": "Unsupported sibling", "sortorder": "1", "display_period": "0",
             "widgets": [{"widgetid": "80200", "type": "vendor_unknown", "name": "Unknown", "x": "0", "y": "0", "width": "12", "height": "4", "view_mode": "0", "fields": []}]}
        ]
    });
    let app = App::start(Script {
        snapshots: VecDeque::from([json!([source])]),
        list: json!([]),
        methods: Vec::new(),
    })
    .await;
    let connection = app.connect().await;
    let (status, imported) = app
        .call(
            "POST",
            &format!("/api/server/{connection}/dashboards/801/import"),
            Some(json!({"scope":"dashboard"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{imported}");
    let id = imported["local_id"].as_str().unwrap_or_default();
    let before_methods = app.methods();
    let (_, before) = app.call("GET", &format!("/api/drafts/{id}"), None).await;
    let preview_path = format!("/api/drafts/{id}/pages/8010/portable-preview");
    let (status, preview) = app.call("GET", &preview_path, None).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["ready"], true);
    assert_eq!(preview["inventory"]["pages"], 1);
    assert_eq!(preview["inventory"]["widgets"], 1);
    let (_, sibling) = app
        .call(
            "GET",
            &format!("/api/drafts/{id}/pages/8020/portable-preview"),
            None,
        )
        .await;
    assert_eq!(sibling["ready"], false);
    let save_path = format!("/api/library/pages/from-workcopy/{id}/pages/8010");
    let (status, first) = app.call("POST", &save_path, Some(json!({}))).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    assert_eq!(first["name"], "Portable");
    let (_, second) = app.call("POST", &save_path, Some(json!({}))).await;
    assert_ne!(first["library_id"], second["library_id"]);
    let library =
        zdashstitch_application::page_library::PageLibrary::new(app.storage.path().to_owned());
    let saved = library
        .get(first["library_id"].as_str().unwrap_or_default())
        .unwrap_or_else(|error| panic!("library read: {error}"))
        .unwrap_or_else(|| panic!("library page missing"));
    assert_eq!(saved.source_local_id, id);
    let saved_json =
        serde_json::to_value(&saved.bundle).unwrap_or_else(|error| panic!("bundle: {error}"));
    assert_eq!(saved_json["project"]["root"]["name"], "Portable");
    assert_eq!(
        saved_json["project"]["root"]["widgets"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    let (status, blocked) = app
        .call(
            "POST",
            &format!("/api/library/pages/from-workcopy/{id}/pages/8020"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{blocked}");
    let (status, missing) = app
        .call(
            "POST",
            &format!("/api/library/pages/from-workcopy/{id}/pages/missing"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
    let mut no_csrf = app.request("POST", &save_path, Some("{}"));
    no_csrf.headers_mut().remove(CSRF_HEADER);
    assert_eq!(app.send(no_csrf).await.0, StatusCode::FORBIDDEN);
    let mut compare_no_csrf = app.request("POST", "/api/library/snapshots/compare", Some("{}"));
    compare_no_csrf.headers_mut().remove(CSRF_HEADER);
    assert_eq!(app.send(compare_no_csrf).await.0, StatusCode::FORBIDDEN);
    let mut delete_no_csrf = app.request(
        "POST",
        "/api/library/snapshots/snap-invalid/delete",
        Some("{}"),
    );
    delete_no_csrf.headers_mut().remove(CSRF_HEADER);
    assert_eq!(app.send(delete_no_csrf).await.0, StatusCode::FORBIDDEN);
    let mut no_session = app.request("GET", &preview_path, None);
    no_session.headers_mut().remove(header::COOKIE);
    assert_eq!(app.send(no_session).await.0, StatusCode::UNAUTHORIZED);
    let (_, after) = app.call("GET", &format!("/api/drafts/{id}"), None).await;
    assert_eq!(before, after);
    assert_eq!(app.methods(), before_methods);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn portable_bindings_http_reads_selects_saves_and_reopens() {
    let source = json!({"dashboardid":"601","name":"Source","userid":"3","private":"1","display_period":"30","auto_start":"1","users":[],"userGroups":[],"pages":[{"dashboard_pageid":"6010","name":"Page","display_period":"0","sortorder":"0","widgets":[{"widgetid":"60100","type":"item","name":"Rate","x":"0","y":"0","width":"12","height":"4","view_mode":"0","fields":[{"type":"4","name":"itemid.0","value":"30001"}]}]}]});
    let app = App::start(Script {
        snapshots: VecDeque::from([json!([source])]),
        list: json!([]),
        methods: Vec::new(),
    })
    .await;
    let imported = inspect_template_file("synthetic.json", br#"{"zabbix_export":{"version":"7.4","templates":[{"template":"Synthetic metrics","items":[{"name":"Rate","key":"synthetic.rate","value_type":"FLOAT"}]}]}}"#).unwrap_or_else(|| panic!("extension")).unwrap_or_else(|e| panic!("{e}"));
    CatalogLibrary::new(app.storage.path().to_owned())
        .save(imported)
        .unwrap_or_else(|e| panic!("{e}"));
    let connection = app.connect().await;
    let (status, imported) = app
        .call(
            "POST",
            &format!("/api/server/{connection}/dashboards/601/import"),
            Some(json!({"scope":"page","dashboard_pageid":"6010"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{imported}");
    let id = imported["local_id"].as_str().unwrap_or_default();
    let (_, before) = app.call("GET", &format!("/api/drafts/{id}"), None).await;
    let path = format!("/api/drafts/{id}/portable-bindings");
    let (status, setup) = app.call("GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{setup}");
    let candidate = &setup["items"][0]["candidates"][0];
    assert_eq!(setup["items"][0]["name"], "Rate");
    let payload = json!({"revision":setup["revision"],"catalogs":setup["catalogs"],"choices":[{"itemid":"30001","catalog":candidate["catalog"],"metric":candidate["metric"],"role":"hosts","slot":"first"}]});
    let mut no_csrf = app.request("POST", &path, Some(&payload.to_string()));
    no_csrf.headers_mut().remove(CSRF_HEADER);
    assert_eq!(app.send(no_csrf).await.0, StatusCode::FORBIDDEN);
    let mut stale = payload.clone();
    stale["revision"] = json!(999);
    let (status, _) = app.call("POST", &path, Some(stale)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let mut invalid = payload.clone();
    invalid["choices"][0]["metric"] = json!("wrong");
    let (status, _) = app.call("POST", &path, Some(invalid)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, saved) = app.call("POST", &path, Some(payload)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let saved_id = saved["local_id"].as_str().unwrap_or_default();
    let (status, reopened) = app
        .call("GET", &format!("/api/drafts/{saved_id}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{reopened}");
    assert_eq!(reopened["storage_kind"], "offline_page");
    let bundle = &reopened["page"]["bundle"];
    assert_eq!(
        bundle["project"]["root"]["widgets"][0]["fields"][0]["value"]["metric"],
        candidate["metric"]
    );
    assert!(!bundle.to_string().contains("30001"));
    let (_, after) = app.call("GET", &format!("/api/drafts/{id}"), None).await;
    assert_eq!(before, after);
    assert!(
        !app.methods()
            .iter()
            .any(|method| method == "dashboard.update" || method == "dashboard.create")
    );

    let (status, dashboard) = app
        .call(
            "POST",
            &format!("/api/server/{connection}/dashboards/601/import"),
            Some(json!({"scope":"dashboard"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{dashboard}");
    let dashboard_id = dashboard["local_id"].as_str().unwrap_or_default();
    let dashboard_path = format!("/api/drafts/{dashboard_id}/portable-bindings");
    let (status, setup) = app.call("GET", &dashboard_path, None).await;
    assert_eq!(status, StatusCode::OK, "{setup}");
    let candidate = &setup["items"][0]["candidates"][0];
    let payload = json!({"revision":setup["revision"],"catalogs":setup["catalogs"],"choices":[{"itemid":"30001","catalog":candidate["catalog"],"metric":candidate["metric"],"role":"hosts","slot":"first"}]});
    let (status, saved) = app.call("POST", &dashboard_path, Some(payload)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["kind"], "dashboard");
    let saved_id = saved["local_id"].as_str().unwrap_or_default();
    let (status, reopened) = app
        .call("GET", &format!("/api/drafts/{saved_id}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{reopened}");
    assert!(!reopened.to_string().contains("30001"));

    // An ordinary host-owned metric needs an explicit target review too;
    // the candidate and the materialized ID must come from fresh server reads.
    let mut bundle: zdashstitch_core::formats::TypedPortableBundle =
        serde_json::from_value(reopened["dashboard"]["bundle"].clone())
            .unwrap_or_else(|error| panic!("bundle: {error}"));
    for catalog in bundle.catalogs.values_mut() {
        for metric in &mut catalog.metrics {
            metric.origin = zdashstitch_core::formats::MetricOrigin::HostItem;
        }
    }
    let dashboard = zdashstitch_application::offline_dashboards::OfflineDashboardStore::new(
        app.storage.path().to_path_buf(),
    )
    .save_imported(bundle)
    .unwrap_or_else(|error| panic!("save: {error}"));
    let local = dashboard.local_id;
    let request = json!({"revision": dashboard.revision, "page_index": 0, "widget_index": 0,
        "field_index": 0, "hostid": "40001"});
    let (status, review) = app
        .call(
            "POST",
            &format!("/api/drafts/{local}/portable-lld-review"),
            Some(request.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{review}");
    assert_eq!(review["candidates"][0]["itemid"], "30001");
    let (status, confirmed) = app
        .call(
            "POST",
            &format!("/api/drafts/{local}/portable-lld-confirm"),
            Some(json!({"review":request, "itemid":"30001", "fingerprint":review["fingerprint"]})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{confirmed}");
    let service =
        zdashstitch_application::workcopy::WorkcopyService::new(app.storage.path().to_path_buf());
    let active = app
        .connections
        .service()
        .unwrap_or_else(|| panic!("connection"));
    let guard = active.lock().await;
    let reader = guard.reader().unwrap_or_else(|| panic!("reader"));
    let evidence = service
        .verify_portable_lld_for_plan(reader, &local, dashboard.revision)
        .await
        .unwrap_or_else(|error| panic!("ordinary metric proof: {error}"));
    let materialized = service
        .materialize_portable_lld_dashboard(&evidence)
        .unwrap_or_else(|error| panic!("ordinary metric materialization: {error}"));
    assert_eq!(
        materialized.as_document()["pages"][0]["widgets"][0]["fields"][0]["value"],
        "30001"
    );
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list = json!({"metric_units_changed":true});
    assert!(
        service
            .revalidate_portable_lld_for_plan(reader, &evidence)
            .await
            .is_err(),
        "changed units must invalidate the proof"
    );
    drop(guard);
    let mut bundle: zdashstitch_core::formats::TypedPortableBundle =
        serde_json::from_value(reopened["dashboard"]["bundle"].clone())
            .unwrap_or_else(|error| panic!("bundle: {error}"));
    for catalog in bundle.catalogs.values_mut() {
        catalog.source.template_uuid = Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into());
        for metric in &mut catalog.metrics {
            metric.origin = zdashstitch_core::formats::MetricOrigin::TemplateItem;
            metric.uuid = Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into());
        }
    }
    let dashboard = zdashstitch_application::offline_dashboards::OfflineDashboardStore::new(
        app.storage.path().to_path_buf(),
    )
    .save_imported(bundle)
    .unwrap_or_else(|error| panic!("save: {error}"));
    let local = dashboard.local_id;
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list = json!({"template_metric":true,"template_uuid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"});
    let request = json!({"revision":dashboard.revision,"page_index":0,"widget_index":0,"field_index":0,"hostid":"40001"});
    let path = format!("/api/drafts/{local}/portable-lld-review");
    let (status, review) = app.call("POST", &path, Some(request.clone())).await;
    assert_eq!(status, StatusCode::OK, "{review}");
    assert_eq!(review["candidates"][0]["itemid"], "30001");
    let (status, confirmed) = app
        .call(
            "POST",
            &format!("/api/drafts/{local}/portable-lld-confirm"),
            Some(json!({"review":request,"itemid":"30001","fingerprint":review["fingerprint"]})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{confirmed}");
    let guard = active.lock().await;
    let reader = guard.reader().unwrap_or_else(|| panic!("reader"));
    let evidence = service
        .verify_portable_lld_for_plan(reader, &local, dashboard.revision)
        .await
        .unwrap_or_else(|error| panic!("template metric proof: {error}"));
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list["template_uuid"] = json!("cccccccccccccccccccccccccccccccc");
    assert!(
        service
            .revalidate_portable_lld_for_plan(reader, &evidence)
            .await
            .is_err(),
        "changed template identity must invalidate the proof"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn portable_dynamic_masks_persist_and_revalidate_confirmation() {
    let source = json!({"dashboardid":"604","name":"Source","userid":"3","private":"1","display_period":"30","auto_start":"1","users":[],"userGroups":[],"pages":[{"dashboard_pageid":"6040","name":"Page","display_period":"0","sortorder":"0","widgets":[{"widgetid":"60400","type":"svggraph","name":"CPU","x":"0","y":"0","width":"12","height":"4","view_mode":"0","fields":[{"type":"1","name":"ds.0.hosts.0","value":"db-*"},{"type":"1","name":"ds.0.items.0","value":"CPU*"}]}]}]});
    let app = App::start(Script {
        snapshots: VecDeque::from([json!([source])]),
        list: json!({"pattern_global":true}),
        methods: Vec::new(),
    })
    .await;
    let connection = app.connect().await;
    let (status, imported) = app
        .call(
            "POST",
            &format!("/api/server/{connection}/dashboards/604/import"),
            Some(json!({"scope":"page","dashboard_pageid":"6040"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{imported}");
    let id = imported["local_id"].as_str().unwrap_or_default();
    let path = format!("/api/drafts/{id}/portable-bindings");
    let (status, setup) = app.call("GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{setup}");
    assert_eq!(setup["patterns"].as_array().map(Vec::len), Some(2));
    let (status, _) = app
        .call(
            "POST",
            &path,
            Some(json!({"revision":setup["revision"],"catalogs":{},"choices":[]})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let patterns: Vec<_> = setup["patterns"].as_array().unwrap_or_else(|| panic!("patterns")).iter().map(|p| json!({"page_index":p["page_index"],"widget_index":p["widget_index"],"field_name":p["field_name"],"role":"hosts"})).collect();
    let (status, saved) = app.call("POST", &path, Some(json!({"revision":setup["revision"],"catalogs":{},"choices":[],"patterns":patterns}))).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let (status, reopened) = app
        .call(
            "GET",
            &format!(
                "/api/drafts/{}",
                saved["local_id"].as_str().unwrap_or_default()
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reopened}");
    let fields = &reopened["page"]["bundle"]["project"]["root"]["widgets"][0]["fields"];
    assert_eq!(
        fields[0]["value"],
        json!({"kind":"dynamic_pattern","value":"db-*","role":"hosts"})
    );
    assert_eq!(fields[1]["value"]["kind"], "dynamic_pattern");
    let review_path = format!(
        "/api/drafts/{}/portable-pattern-review",
        saved["local_id"].as_str().unwrap_or_default()
    );
    let review = json!({"revision":reopened["page"]["revision"],"page_index":0,"widget_index":0,"field_index":0,"targets":[{"slot_id":"first","host":{"hostid":"40001","host":"not-trusted","name":"not-trusted"}}]});
    let (status, preview) = app.call("POST", &review_path, Some(review.clone())).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["preview"]["match_count"], 0);
    assert_eq!(preview["visible_matches"][0]["hostid"], "40002");
    assert_eq!(preview["visible_matches"][0]["selected"], false);
    let mut item_review = review.clone();
    item_review["field_index"] = json!(1);
    let (status, item_preview) = app.call("POST", &review_path, Some(item_review)).await;
    assert_eq!(status, StatusCode::OK, "{item_preview}");
    assert_eq!(item_preview["preview"]["match_count"], 1);
    assert_eq!(
        item_preview["visible_matches"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(item_preview["visible_matches"][1]["selected"], false);
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list = json!({"pattern_truncated":true});
    assert_ne!(
        app.call("POST", &review_path, Some(review.clone())).await.0,
        StatusCode::OK
    );
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list = json!({"pattern_global":true});
    assert_eq!(preview["fingerprint"].as_str().map(str::len), Some(64));
    let mut multi = review.clone();
    multi["targets"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("targets"))
        .push(
            json!({"slot_id":"second","host":{"hostid":"40002","host":"ignored","name":"ignored"}}),
        );
    let (status, multi_preview) = app.call("POST", &review_path, Some(multi.clone())).await;
    assert_eq!(status, StatusCode::OK, "{multi_preview}");
    assert_eq!(multi_preview["preview"]["match_count"], 1);
    assert_eq!(
        multi_preview["preview"]["checked_slots"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    multi["targets"][1]["slot_id"] = json!("first");
    assert_ne!(
        app.call("POST", &review_path, Some(multi)).await.0,
        StatusCode::OK
    );
    let confirm_path = review_path.replace("portable-pattern-review", "portable-pattern-confirm");
    let unacknowledged = json!({"review": review.clone(), "fingerprint": preview["fingerprint"]});
    assert_eq!(
        app.call("POST", &confirm_path, Some(unacknowledged))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let confirmation = json!({"review": review.clone(), "fingerprint": preview["fingerprint"], "acknowledge_visible_outside":true});
    assert_eq!(
        app.call("POST", &confirm_path, Some(confirmation.clone()))
            .await
            .0,
        StatusCode::OK
    );
    let reopen_path = review_path.replace("portable-pattern-review", "portable-pattern-reopen");
    let reopen = json!({"revision": review["revision"], "page_index": 0, "widget_index": 0, "field_index": 0});
    let (status, reopened_review) = app.call("POST", &reopen_path, Some(reopen.clone())).await;
    assert_eq!(status, StatusCode::OK, "{reopened_review}");
    assert_eq!(reopened_review["fingerprint"], preview["fingerprint"]);
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list = json!({"pattern_global":true,"pattern_host_changed": true});
    assert_ne!(
        app.call("POST", &reopen_path, Some(reopen.clone())).await.0,
        StatusCode::OK
    );
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list = json!({"pattern_global":true});
    let mut stale_reopen = reopen;
    stale_reopen["revision"] = json!(999);
    assert_ne!(
        app.call("POST", &reopen_path, Some(stale_reopen)).await.0,
        StatusCode::OK
    );
    let mut changed = confirmation;
    changed["fingerprint"] = json!("different");
    assert_eq!(
        app.call("POST", &confirm_path, Some(changed)).await.0,
        StatusCode::CONFLICT
    );
    let mut stale = review;
    stale["revision"] = json!(999);
    assert_eq!(
        app.call("POST", &review_path, Some(stale)).await.0,
        StatusCode::CONFLICT
    );
    // A dashboard proof must bind every saved pattern to the same targets and
    // invalidate when the target inventory changes after proof creation.
    let mut bundle: zdashstitch_core::formats::TypedPortableBundle =
        serde_json::from_value(reopened["page"]["bundle"].clone())
            .unwrap_or_else(|error| panic!("bundle: {error}"));
    let zdashstitch_core::formats::ProjectRoot::Page(page) = bundle.project.root.clone() else {
        panic!("page")
    };
    bundle.project.kind = zdashstitch_core::formats::ProjectKind::Dashboard;
    bundle.project.root =
        zdashstitch_core::formats::ProjectRoot::Dashboard(zdashstitch_core::formats::Dashboard {
            id: bundle.project.project_id.clone(),
            name: "Synthetic".into(),
            kind: zdashstitch_core::formats::DashboardKind::Global,
            pages: vec![page],
        });
    let dashboard = zdashstitch_application::offline_dashboards::OfflineDashboardStore::new(
        app.storage.path().to_path_buf(),
    )
    .save_imported(bundle)
    .unwrap_or_else(|error| panic!("dashboard: {error}"));
    let local = dashboard.local_id;
    for index in 0..2 {
        let request = json!({"revision":dashboard.revision,"page_index":0,"widget_index":0,"field_index":index,
            "targets":[{"slot_id":"first","host":{"hostid":"40001","host":"ignored","name":"ignored"}}]});
        let (status, preview) = app
            .call(
                "POST",
                &format!("/api/drafts/{local}/portable-pattern-review"),
                Some(request.clone()),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{preview}");
        let (status, confirmed) = app.call("POST", &format!("/api/drafts/{local}/portable-pattern-confirm"),
            Some(json!({"review":request,"fingerprint":preview["fingerprint"],"acknowledge_visible_outside":true}))).await;
        assert_eq!(status, StatusCode::OK, "{confirmed}");
    }
    let service =
        zdashstitch_application::workcopy::WorkcopyService::new(app.storage.path().to_path_buf());
    let active = app
        .connections
        .service()
        .unwrap_or_else(|| panic!("connection"));
    let guard = active.lock().await;
    let reader = guard.reader().unwrap_or_else(|| panic!("reader"));
    let proof = service
        .verify_portable_lld_for_plan(reader, &local, dashboard.revision)
        .await
        .unwrap_or_else(|error| panic!("pattern proof: {error}"));
    service
        .revalidate_portable_lld_for_plan(reader, &proof)
        .await
        .unwrap_or_else(|error| panic!("recheck: {error}"));
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list = json!({"pattern_global":true,"pattern_host_changed":true});
    assert!(
        service
            .revalidate_portable_lld_for_plan(reader, &proof)
            .await
            .is_err(),
        "changed matches invalidate the proof"
    );
    drop(guard);
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list = json!({"pattern_global":true});
    let request = json!({"revision":dashboard.revision,"page_index":0,"widget_index":0,"field_index":1,
        "targets":[{"slot_id":"first","host":{"hostid":"40002","host":"ignored","name":"ignored"}}]});
    let (status, preview) = app
        .call(
            "POST",
            &format!("/api/drafts/{local}/portable-pattern-review"),
            Some(request.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    let (status, confirmed) = app.call("POST", &format!("/api/drafts/{local}/portable-pattern-confirm"),
        Some(json!({"review":request,"fingerprint":preview["fingerprint"],"acknowledge_visible_outside":true}))).await;
    assert_eq!(status, StatusCode::OK, "{confirmed}");
    let guard = active.lock().await;
    let reader = guard.reader().unwrap_or_else(|| panic!("reader"));
    assert!(
        service
            .verify_portable_lld_for_plan(reader, &local, dashboard.revision)
            .await
            .is_err(),
        "fields using one role must not disagree on targets"
    );
    assert!(
        !app.methods()
            .iter()
            .any(|method| method == "dashboard.update" || method == "dashboard.create")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn portable_lld_prototype_http_preserves_selected_instance_key() {
    let source = json!({"dashboardid":"603","name":"Source","userid":"3","private":"1","display_period":"30","auto_start":"1","users":[],"userGroups":[],"pages":[{"dashboard_pageid":"6030","name":"Page","display_period":"0","sortorder":"0","widgets":[{"widgetid":"60300","type":"item","name":"Value","x":"0","y":"0","width":"12","height":"4","view_mode":"0","fields":[{"type":"4","name":"itemid.0","value":"30002"}]}]}]});
    let app = App::start(Script {
        snapshots: VecDeque::from([json!([source])]),
        list: json!({"lld_fixture":true,"template_uuid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}),
        methods: Vec::new(),
    })
    .await;
    let imported = inspect_template_file("synthetic.json", br#"{"zabbix_export":{"version":"7.4","templates":[{"uuid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","template":"LLD","discovery_rules":[{"name":"Discovery","key":"synthetic.discovery","item_prototypes":[{"uuid":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","name":"Value {#ID}","key":"synthetic.value[{#ID}]","value_type":"FLOAT"}]}]}]}}"#).unwrap_or_else(|| panic!("extension")).unwrap_or_else(|e| panic!("{e}"));
    CatalogLibrary::new(app.storage.path().to_owned())
        .save(imported)
        .unwrap_or_else(|e| panic!("{e}"));
    let connection = app.connect().await;
    let (status, imported) = app
        .call(
            "POST",
            &format!("/api/server/{connection}/dashboards/603/import"),
            Some(json!({"scope":"dashboard"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{imported}");
    let id = imported["local_id"].as_str().unwrap_or_default();
    let path = format!("/api/drafts/{id}/portable-bindings");
    let (status, setup) = app.call("GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{setup}");
    let candidate = &setup["items"][0]["candidates"][0];
    assert_eq!(candidate["lld_prototype"], true, "{setup}");
    let payload = json!({"revision":setup["revision"],"catalogs":setup["catalogs"],"choices":[{"itemid":"30002","catalog":candidate["catalog"],"metric":candidate["metric"],"role":"hosts","slot":"first"}]});
    let (status, saved) = app.call("POST", &path, Some(payload)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let (status, reopened) = app
        .call(
            "GET",
            &format!(
                "/api/drafts/{}",
                saved["local_id"].as_str().unwrap_or_default()
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reopened}");
    let bundle = &reopened["dashboard"]["bundle"];
    assert!(bundle.to_string().contains("synthetic.value[db-a]"));
    assert!(!bundle.to_string().contains("30002") && !bundle.to_string().contains("50001"));
    assert_eq!(
        bundle["project"]["root"]["pages"][0]["widgets"][0]["fields"][0]["value"]["kind"],
        "lld_instance"
    );
    let target = saved["local_id"].as_str().unwrap_or_default();
    let proof_service =
        zdashstitch_application::workcopy::WorkcopyService::new(app.storage.path().to_path_buf());
    let active = app
        .connections
        .service()
        .unwrap_or_else(|| panic!("connections"));
    let revision = reopened["dashboard"]["revision"]
        .as_u64()
        .unwrap_or_default();
    {
        let guard = active.lock().await;
        let reader = guard.reader().unwrap_or_else(|| panic!("reader"));
        assert!(
            proof_service
                .verify_portable_lld_for_plan(reader, target, revision)
                .await
                .is_err()
        );
    }
    let request = json!({"revision":reopened["dashboard"]["revision"],"page_index":0,"widget_index":0,"field_index":0,"hostid":"40002"});
    let (status, review) = app
        .call(
            "POST",
            &format!("/api/drafts/{target}/portable-lld-review"),
            Some(request.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{review}");
    assert_eq!(review["candidates"][0]["itemid"], "30003");
    let confirm = json!({"review":request,"itemid":"30003","fingerprint":review["fingerprint"]});
    let path = format!("/api/drafts/{target}/portable-lld-confirm");
    assert_eq!(
        app.call("POST", &path, Some(confirm.clone())).await.0,
        StatusCode::OK
    );
    let proof = {
        let guard = active.lock().await;
        let reader = guard.reader().unwrap_or_else(|| panic!("reader"));
        let proof = proof_service
            .verify_portable_lld_for_plan(reader, target, revision)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(proof.selection_count(), 1);
        let materialized = proof_service
            .materialize_portable_lld_dashboard(&proof)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(materialized.page_count(), 1);
        assert_eq!(
            materialized.as_document()["pages"][0]["widgets"][0]["fields"][0]["value"],
            "30003"
        );
        assert!(materialized.as_document().get("dashboardid").is_none());
        let name = materialized.as_document()["name"]
            .as_str()
            .unwrap_or_default();
        let requirements = zdashstitch_application::portable_create_requirements(
            materialized.as_document(),
            &reader.capabilities().version,
            name,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(requirements.page_count, 1);
        assert_eq!(requirements.widget_count, 1);
        let mut unknown = materialized.as_document().clone();
        unknown["pages"][0]["widgets"][0]["type"] = json!("vendor.unknown");
        assert!(
            zdashstitch_application::portable_create_requirements(
                &unknown,
                &reader.capabilities().version,
                name
            )
            .is_err()
        );
        proof_service
            .revalidate_portable_lld_for_plan(reader, &proof)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        proof
    };
    let draft_path = app
        .storage
        .path()
        .join("drafts/offline-dashboards/dashboards.json");
    let original_draft = std::fs::read(&draft_path).unwrap_or_else(|e| panic!("draft: {e}"));
    let mut changed_draft: Value = serde_json::from_slice(&original_draft).unwrap_or_default();
    changed_draft["dashboards"][0]["bundle"]["project"]["root"]["name"] =
        json!("Changed without revision");
    std::fs::write(
        &draft_path,
        serde_json::to_vec(&changed_draft).unwrap_or_default(),
    )
    .unwrap_or_else(|e| panic!("fixture write: {e}"));
    assert!(
        proof_service
            .materialize_portable_lld_dashboard(&proof)
            .is_err()
    );
    std::fs::write(&draft_path, original_draft).unwrap_or_else(|e| panic!("fixture restore: {e}"));
    let reopen_path = format!("/api/drafts/{target}/portable-lld-reopen");
    let reopen = json!({"revision":reopened["dashboard"]["revision"],"page_index":0,"widget_index":0,"field_index":0});
    let (status, restored) = app.call("POST", &reopen_path, Some(reopen.clone())).await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["itemid"], "30003");
    let (status, portable_again) = app
        .call("GET", &format!("/api/drafts/{target}"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let portable_text = portable_again["dashboard"]["bundle"].to_string();
    assert!(!portable_text.contains("30003") && !portable_text.contains("40002"));
    let binding_path = app.storage.path().join("bindings/portable-lld-v1.json");
    let original = std::fs::read(&binding_path).unwrap_or_else(|e| panic!("binding: {e}"));
    let mut changed_binding: Value =
        serde_json::from_slice(&original).unwrap_or_else(|e| panic!("binding JSON: {e}"));
    assert!(!String::from_utf8_lossy(&original).contains("synthetic.value"));
    changed_binding["bindings"][0]["draft_fingerprint"] =
        json!("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
    std::fs::write(
        &binding_path,
        serde_json::to_vec(&changed_binding).unwrap_or_default(),
    )
    .unwrap_or_else(|e| panic!("fixture write: {e}"));
    assert_eq!(
        app.call("POST", &reopen_path, Some(reopen.clone())).await.0,
        StatusCode::CONFLICT
    );
    std::fs::write(&binding_path, original).unwrap_or_else(|e| panic!("fixture restore: {e}"));
    let original = std::fs::read(&binding_path).unwrap_or_default();
    std::fs::write(&binding_path, b"{invalid").unwrap_or_else(|e| panic!("fixture write: {e}"));
    assert_eq!(
        app.call("POST", &reopen_path, Some(reopen.clone())).await.0,
        StatusCode::CONFLICT
    );
    std::fs::write(&binding_path, original).unwrap_or_else(|e| panic!("fixture restore: {e}"));
    let original = std::fs::read(&binding_path).unwrap_or_default();
    let mut other_server: Value = serde_json::from_slice(&original).unwrap_or_default();
    other_server["bindings"][0]["server"] =
        json!("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee");
    std::fs::write(
        &binding_path,
        serde_json::to_vec(&other_server).unwrap_or_default(),
    )
    .unwrap_or_else(|e| panic!("fixture write: {e}"));
    assert_eq!(
        app.call("POST", &reopen_path, Some(reopen.clone())).await.0,
        StatusCode::CONFLICT
    );
    std::fs::write(&binding_path, original).unwrap_or_else(|e| panic!("fixture restore: {e}"));
    let mut missing = reopen.clone();
    missing["revision"] = json!(999);
    assert_eq!(
        app.call("POST", &reopen_path, Some(missing)).await.0,
        StatusCode::CONFLICT
    );
    let mut bad = confirm.clone();
    bad["itemid"] = json!("30001");
    assert_eq!(
        app.call("POST", &path, Some(bad)).await.0,
        StatusCode::CONFLICT
    );
    let mut stale = confirm;
    stale["review"]["revision"] = json!(999);
    assert_eq!(
        app.call("POST", &path, Some(stale)).await.0,
        StatusCode::CONFLICT
    );
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list["template_uuid"] = json!("cccccccccccccccccccccccccccccccc");
    {
        let guard = active.lock().await;
        let reader = guard.reader().unwrap_or_else(|| panic!("reader"));
        assert!(
            proof_service
                .revalidate_portable_lld_for_plan(reader, &proof)
                .await
                .is_err()
        );
    }
    assert_eq!(
        app.call("POST", &reopen_path, Some(reopen)).await.0,
        StatusCode::CONFLICT
    );
    let changed = json!({"review":request,"itemid":"30003","fingerprint":review["fingerprint"]});
    assert_eq!(
        app.call("POST", &path, Some(changed)).await.0,
        StatusCode::CONFLICT
    );
    assert!(
        !app.methods()
            .iter()
            .any(|method| method == "dashboard.update" || method == "dashboard.create")
    );
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list["template_uuid"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

    let mut guard = active.lock().await;
    guard
        .enable_writes(&connection, true)
        .unwrap_or_else(|e| panic!("enable: {e}"));
    let session = zdashstitch_application::LocalSession::new();
    let journal = zdashstitch_application::PublishJournal::new(app.storage.path().to_path_buf());
    let request = zdashstitch_application::PortableCreateRequest {
        local_id: target.into(),
        connection_id: connection.clone(),
        revision,
        target_name: "Synthetic new dashboard".into(),
        create_another: false,
        session: session.current(),
    };
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list["deny_create"] = json!(true);
    assert!(
        zdashstitch_application::plan_portable_dashboard(
            &proof_service,
            &journal,
            &session,
            &guard,
            &request
        )
        .await
        .is_err()
    );
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list["deny_create"] = json!(false);
    let wrong_plan = zdashstitch_application::plan_portable_dashboard(
        &proof_service,
        &journal,
        &session,
        &guard,
        &request,
    )
    .await
    .unwrap_or_else(|e| panic!("plan: {e}"));
    let wrong_id = wrong_plan.preview().plan_id;
    let wrong_confirmed = wrong_plan
        .confirm(&wrong_id)
        .unwrap_or_else(|e| panic!("confirm: {e}"));
    let wrong_stage = session
        .begin_write(session.current())
        .unwrap_or_else(|_| panic!("stage unavailable"));
    let wrong_report = zdashstitch_application::publish_save_as_copy_plan(
        &proof_service,
        guard.writer().unwrap_or_else(|| panic!("writer")),
        &journal,
        wrong_confirmed,
        &wrong_stage,
    )
    .await;
    assert!(!wrong_report.sent());
    drop(wrong_stage);
    let permission_plan = zdashstitch_application::plan_portable_dashboard(
        &proof_service,
        &journal,
        &session,
        &guard,
        &request,
    )
    .await
    .unwrap_or_else(|e| panic!("plan: {e}"));
    let permission_id = permission_plan.preview().plan_id;
    let permission_confirmed = permission_plan
        .confirm(&permission_id)
        .unwrap_or_else(|e| panic!("confirm: {e}"));
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list["deny_create"] = json!(true);
    let permission_stage = session
        .begin_write(session.current())
        .unwrap_or_else(|_| panic!("stage unavailable"));
    let permission_report = zdashstitch_application::publish_portable_dashboard_plan(
        &proof_service,
        guard.writer().unwrap_or_else(|| panic!("writer")),
        &journal,
        permission_confirmed,
        &permission_stage,
    )
    .await;
    assert!(!permission_report.sent());
    assert!(format!("{:?}", permission_report.outcome).contains("CreatePermissionUnverified"));
    assert!(permission_report.journal_error.is_none());
    drop(permission_stage);
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list["deny_create"] = json!(false);
    let plan = zdashstitch_application::plan_portable_dashboard(
        &proof_service,
        &journal,
        &session,
        &guard,
        &request,
    )
    .await
    .unwrap_or_else(|e| panic!("plan: {e}"));
    let plan_id = plan.preview().plan_id;
    let confirmed = plan
        .confirm(&plan_id)
        .unwrap_or_else(|e| panic!("confirm: {e}"));
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list["template_uuid"] = json!("cccccccccccccccccccccccccccccccc");
    let stage = session
        .begin_write(session.current())
        .unwrap_or_else(|_| panic!("stage unavailable"));
    let report = zdashstitch_application::publish_portable_dashboard_plan(
        &proof_service,
        guard.writer().unwrap_or_else(|| panic!("writer")),
        &journal,
        confirmed,
        &stage,
    )
    .await;
    assert!(!report.sent(), "changed prototype must prevent the write");
    assert!(
        report.journal_error.is_none(),
        "journal must admit the plan: {report:?}"
    );
    assert_eq!(
        report.outcome.as_ref().map(|outcome| outcome.as_str()),
        Some("failed_before_send")
    );
    assert!(format!("{:?}", report.outcome).contains("BindingsChanged"));
    assert!(
        !app.methods()
            .iter()
            .any(|method| method == "dashboard.create")
    );
    drop(stage);
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list["template_uuid"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list["verified_create"] = json!(true);
    let verified_request = zdashstitch_application::PortableCreateRequest {
        local_id: target.into(),
        connection_id: connection.clone(),
        revision,
        target_name: "Synthetic verified dashboard".into(),
        create_another: false,
        session: session.current(),
    };
    let verified_plan = zdashstitch_application::plan_portable_dashboard(
        &proof_service,
        &journal,
        &session,
        &guard,
        &verified_request,
    )
    .await
    .unwrap_or_else(|e| panic!("verified plan: {e}"));
    let verified_id = verified_plan.preview().plan_id;
    let verified_confirmed = verified_plan
        .confirm(&verified_id)
        .unwrap_or_else(|e| panic!("confirm: {e}"));
    let verified_stage = session
        .begin_write(session.current())
        .unwrap_or_else(|_| panic!("stage unavailable"));
    let verified_report = zdashstitch_application::publish_portable_dashboard_plan(
        &proof_service,
        guard.writer().unwrap_or_else(|| panic!("writer")),
        &journal,
        verified_confirmed,
        &verified_stage,
    )
    .await;
    assert_eq!(
        verified_report.journal_state,
        Some(zdashstitch_application::JournalState::Verified),
        "{verified_report:?}"
    );
    assert!(verified_report.record_error.is_none());
    drop(verified_stage);
    drop(guard);
    let plan_path = format!("/api/drafts/{target}/portable-publication/plan");
    let confirm_path = format!("/api/drafts/{target}/publication/confirm");
    let (status, planned) = app.call("POST", &plan_path, Some(json!({
        "connection_id": connection, "revision": revision, "name": "Synthetic stale dashboard"
    }))).await;
    assert_eq!(status, StatusCode::OK, "{planned}");
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list["template_uuid"] = json!("cccccccccccccccccccccccccccccccc");
    let (status, stale) = app
        .call(
            "POST",
            &confirm_path,
            Some(json!({
                "handle": planned["handle"], "plan_id": planned["plan"]["plan_id"],
                "confirmed": true, "name": "Synthetic stale dashboard"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{stale}");
    assert_eq!(stale["state"], "failed_before_send", "{stale}");
    assert_eq!(
        app.methods()
            .iter()
            .filter(|method| *method == "dashboard.create")
            .count(),
        1
    );
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list["template_uuid"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let (status, planned) = app.call("POST", &plan_path, Some(json!({
        "connection_id": connection, "revision": revision, "name": "Synthetic HTTP dashboard"
    }))).await;
    assert_eq!(status, StatusCode::OK, "{planned}");
    assert_eq!(planned["plan"]["action"], "create_dashboard");
    let confirmation = json!({"handle": planned["handle"], "plan_id": planned["plan"]["plan_id"],
        "confirmed": true, "name": "Synthetic HTTP dashboard"});
    let (status, result) = app
        .call("POST", &confirm_path, Some(confirmation.clone()))
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["state"], "verified", "{result}");
    let (status, _) = app.call("POST", &confirm_path, Some(confirmation)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "plan must be one-shot");
    let guard = active.lock().await;
    app.script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .list["verified_create"] = json!(false);
    let plan = zdashstitch_application::plan_portable_dashboard(
        &proof_service,
        &journal,
        &session,
        &guard,
        &request,
    )
    .await
    .unwrap_or_else(|e| panic!("second plan: {e}"));
    let plan_id = plan.preview().plan_id;
    let confirmed = plan
        .confirm(&plan_id)
        .unwrap_or_else(|e| panic!("confirm: {e}"));
    let stage = session
        .begin_write(session.current())
        .unwrap_or_else(|_| panic!("stage unavailable"));
    let report = zdashstitch_application::publish_portable_dashboard_plan(
        &proof_service,
        guard.writer().unwrap_or_else(|| panic!("writer")),
        &journal,
        confirmed,
        &stage,
    )
    .await;
    assert!(
        report.sent(),
        "one create should be dispatched to the mock: {report:?}"
    );
    assert_eq!(
        app.methods()
            .iter()
            .filter(|method| *method == "dashboard.create")
            .count(),
        3
    );
    assert_eq!(
        report.journal_state,
        Some(zdashstitch_application::JournalState::UnknownOutcome)
    );
    assert!(journal.entry(&report.plan_id).is_ok());
    assert!(app.methods().iter().all(|method| {
        !(method.ends_with(".create") || method.ends_with(".update") || method.ends_with(".delete"))
            || method == "dashboard.create"
    }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn portable_host_only_http_requires_explicit_visible_host_slot() {
    let source = json!({"dashboardid":"602","name":"Source","userid":"3","private":"1","display_period":"30","auto_start":"1","users":[],"userGroups":[],"pages":[{"dashboard_pageid":"6020","name":"Page","display_period":"0","sortorder":"0","widgets":[{"widgetid":"60200","type":"problems","name":"Problems","x":"0","y":"0","width":"12","height":"4","view_mode":"0","fields":[{"type":"3","name":"hostids.0","value":"40001"}]}]}]});
    let app = App::start(Script {
        snapshots: VecDeque::from([json!([source])]),
        list: json!([]),
        methods: Vec::new(),
    })
    .await;
    let connection = app.connect().await;
    let (_, imported) = app
        .call(
            "POST",
            &format!("/api/server/{connection}/dashboards/602/import"),
            Some(json!({"scope":"page","dashboard_pageid":"6020"})),
        )
        .await;
    let id = imported["local_id"].as_str().unwrap_or_default();
    let path = format!("/api/drafts/{id}/portable-bindings");
    let (status, setup) = app.call("GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{setup}");
    assert_eq!(setup["hosts"][0]["name"], "Synthetic host");
    let missing = json!({"revision":setup["revision"],"catalogs":{},"choices":[]});
    let (status, _) = app.call("POST", &path, Some(missing)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let payload = json!({"revision":setup["revision"],"catalogs":{},"choices":[],"hosts":[{"hostid":"40001","role":"hosts","slot":"first"}]});
    let (status, saved) = app.call("POST", &path, Some(payload)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let (status, reopened) = app
        .call(
            "GET",
            &format!(
                "/api/drafts/{}",
                saved["local_id"].as_str().unwrap_or_default()
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reopened}");
    assert_eq!(
        reopened["page"]["bundle"]["project"]["root"]["widgets"][0]["fields"][0]["value"]["slot_id"],
        "first"
    );
    assert!(!reopened["page"]["bundle"].to_string().contains("40001"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn portable_page_preview_creates_a_separate_local_draft_without_zabbix_calls() {
    let source = json!({
        "dashboardid": "601", "name": "Clock source", "userid": "3", "private": "1",
        "display_period": "30", "auto_start": "1", "users": [], "userGroups": [],
        "pages": [{
            "dashboard_pageid": "6010", "name": "Clock page", "display_period": "0",
            "sortorder": "0", "widgets": [{
                "widgetid": "60100", "type": "clock", "name": "Clock",
                "x": "0", "y": "0", "width": "12", "height": "4",
                "view_mode": "0", "fields": []
            }]
        }]
    });
    let app = App::start(Script {
        snapshots: VecDeque::from([json!([source])]),
        list: json!([]),
        methods: Vec::new(),
    })
    .await;
    let connection = app.connect().await;
    let (status, imported) = app
        .call(
            "POST",
            &format!("/api/server/{connection}/dashboards/601/import"),
            Some(json!({"scope": "page", "dashboard_pageid": "6010"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{imported}");
    let workcopy = imported["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("workcopy: {imported}"));
    let methods_after_import = app.methods();
    let (status, preview) = app
        .call(
            "GET",
            &format!("/api/drafts/{workcopy}/portable-preview"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["ready"], true);
    assert_eq!(preview["blockers"], json!([]));
    assert_eq!(preview["inventory"]["kind"], "page");
    let (status, library_page) = app
        .call(
            "POST",
            &format!("/api/library/pages/from-workcopy/{workcopy}"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{library_page}");
    assert_eq!(library_page["name"], "Clock page");
    let (status, assembled) = app
        .call(
            "POST",
            "/api/library/offline-dashboards",
            Some(json!({"name":"From workcopy", "page_ids":[library_page["library_id"]]})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{assembled}");
    assert_eq!(
        assembled["bundle"]["project"]["root"]["pages"][0]["name"],
        "Clock page"
    );
    let (status, created) = app
        .call(
            "POST",
            &format!("/api/drafts/{workcopy}/make-portable"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["kind"], "page");
    let local_id = created["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("local draft: {created}"));
    assert!(local_id.starts_with("pg-"));
    let (status, reopened) = app
        .call("GET", &format!("/api/drafts/{local_id}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{reopened}");
    assert_eq!(reopened["storage_kind"], "offline_page");
    assert_eq!(
        reopened["page"]["bundle"]["project"]["root"]["name"],
        "Clock page"
    );
    let (status, source_reopened) = app
        .call("GET", &format!("/api/drafts/{workcopy}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{source_reopened}");
    assert_eq!(source_reopened["name"], "Clock page");
    assert_eq!(
        app.methods(),
        methods_after_import,
        "conversion is local-only"
    );
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
