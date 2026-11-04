// SPDX-License-Identifier: MIT OR Apache-2.0

//! ReplaceHost routes (P2-03D/E) over a mock Zabbix JSON-RPC server on
//! loopback: the `/api` boundary, read-only server access only through the
//! draft's own connection, local decisions and undo, the offline preview,
//! RFC 3339 times and that no response echoes a secret or an address. All
//! data is synthetic.

use std::sync::{Arc, Mutex as StdMutex};

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
use zdashstitch_application::connections::{ConnectionService, MemoryCredentialStore};

use super::{DraftsHandle, routes};
use crate::{CSRF_HEADER, ConnectionsHandle, LocalSecurity, build_router_with_api, connections};

const PORT: u16 = 43179;
const TOKEN: &str = "p203de-token-secret-value";

type Shared = Arc<StdMutex<Vec<String>>>;

/// Two pages: an `item` widget and an `svggraph` data set on the replaced
/// host (both change), and a `problems` widget naming the host (kept).
fn dashboard() -> Value {
    json!({
        "dashboardid": "601", "name": "Synthetic DB", "userid": "3", "private": "1",
        "display_period": "30", "auto_start": "1", "users": [], "userGroups": [],
        "pages": [
            {
                "dashboard_pageid": "6010", "name": "Databases", "display_period": "0",
                "widgets": [
                    {
                        "widgetid": "60100", "type": "item", "name": "Ping",
                        "x": "0", "y": "0", "width": "12", "height": "3", "view_mode": "0",
                        "fields": [{"type": "4", "name": "itemid.0", "value": "30001"}]
                    },
                    {
                        "widgetid": "60101", "type": "svggraph", "name": "CPU",
                        "x": "12", "y": "0", "width": "36", "height": "5", "view_mode": "0",
                        "fields": [
                            {"type": "1", "name": "ds.0.hosts.0", "value": "syn-pg-a"},
                            {"type": "1", "name": "ds.0.items.0", "value": "CPU*"}
                        ]
                    }
                ]
            },
            {
                "dashboard_pageid": "6011", "name": "Problems", "display_period": "0",
                "widgets": [{
                    "widgetid": "60110", "type": "problems", "name": "Open problems",
                    "x": "0", "y": "0", "width": "72", "height": "5", "view_mode": "0",
                    "fields": [{"type": "3", "name": "hostids.0", "value": "10"}]
                }]
            }
        ]
    })
}

fn host(hostid: &str, name: &str) -> Value {
    json!({"hostid": hostid, "host": name, "name": name})
}

fn listing(hostid: &str, name: &str) -> Value {
    json!({
        "hostid": hostid, "host": name, "name": name,
        "hostgroups": [{"name": "Synthetic/DB"}],
        "parentTemplates": [{"templateid": "900", "name": "Synthetic PostgreSQL"}]
    })
}

fn item(itemid: &str, hostid: &str, key: &str, name: &str) -> Value {
    json!({
        "itemid": itemid, "hostid": hostid, "key_": key, "name": name,
        "value_type": "3", "units": "", "flags": "0", "templateid": "0",
        "valuemapid": "0", "valuemap": [], "itemDiscovery": [],
        "hosts": [{"hostid": hostid, "name": if hostid == "10" { "syn-pg-a" } else { "syn-pg-b" }}]
    })
}

fn items() -> Vec<Value> {
    vec![
        item("30001", "10", "pgsql.ping", "Ping"),
        item("30002", "10", "system.cpu.load", "CPU load"),
        item("40001", "20", "pgsql.ping", "Ping"),
        item("40002", "20", "system.cpu.load", "CPU load"),
    ]
}

fn ids(params: &Value, key: &str) -> Vec<String> {
    params[key]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

async fn rpc(
    State(methods): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let id = body["id"].clone();
    let method = body["method"].as_str().unwrap_or_default().to_owned();
    let params = &body["params"];
    methods
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(method.clone());
    let authorized = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        == Some(format!("Bearer {TOKEN}").as_str());
    let hosts = [("10", "syn-pg-a"), ("20", "syn-pg-b")];
    let result = match method.as_str() {
        "apiinfo.version" => json!("7.4.16"),
        "user.checkAuthentication" if params["token"] == TOKEN => {
            json!({"userid": "3", "username": "synthetic"})
        }
        _ if !authorized => {
            return Json(json!({"jsonrpc": "2.0", "id": id, "error": {
                "code": -32602, "message": "Invalid params.", "data": "Not authorized."
            }}))
            .into_response();
        }
        "dashboard.get" => json!([dashboard()]),
        "host.get" if params.get("selectParentTemplates").is_some() => {
            let wanted = ids(params, "hostids");
            let search = params["search"]["name"].as_str().unwrap_or_default();
            Value::Array(
                hosts
                    .iter()
                    .filter(|(hostid, name)| {
                        if wanted.is_empty() {
                            name.contains(search)
                        } else {
                            wanted.iter().any(|id| id == hostid)
                        }
                    })
                    .map(|(hostid, name)| listing(hostid, name))
                    .collect(),
            )
        }
        "host.get" if params.get("hostids").is_some() => {
            let wanted = ids(params, "hostids");
            Value::Array(
                hosts
                    .iter()
                    .filter(|(hostid, _)| wanted.iter().any(|id| id == hostid))
                    .map(|(hostid, name)| host(hostid, name))
                    .collect(),
            )
        }
        "host.get" => {
            let names = ids(&params["search"], "name");
            Value::Array(
                hosts
                    .iter()
                    .filter(|(_, name)| {
                        names.iter().any(|wanted| wanted.eq_ignore_ascii_case(name))
                    })
                    .map(|(hostid, name)| host(hostid, name))
                    .collect(),
            )
        }
        "item.get" => {
            let itemids = ids(params, "itemids");
            let hostids = ids(params, "hostids");
            Value::Array(
                items()
                    .into_iter()
                    .filter(|item| {
                        itemids.iter().any(|id| item["itemid"] == id.as_str())
                            || hostids.iter().any(|id| item["hostid"] == id.as_str())
                    })
                    .collect(),
            )
        }
        _ => json!([]),
    };
    Json(json!({"jsonrpc": "2.0", "id": id, "result": result})).into_response()
}

async fn zabbix() -> (String, Shared) {
    let methods: Shared = Arc::new(StdMutex::new(Vec::new()));
    let app = Router::new()
        .route("/zabbix/api_jsonrpc.php", post(rpc))
        .with_state(Arc::clone(&methods));
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
    (format!("http://{address}/zabbix"), methods)
}

struct App {
    router: Router,
    cookie: String,
    csrf: String,
    address: String,
    methods: Shared,
    _storage: tempfile::TempDir,
    _config: tempfile::TempDir,
}

impl App {
    async fn start() -> Self {
        let (address, methods) = zabbix().await;
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
            methods,
            _storage: storage,
            _config: config,
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
        let (_, session) = app.call("GET", "/api/session", None).await;
        app.csrf = session["csrf_token"]
            .as_str()
            .unwrap_or_else(|| panic!("CSRF token"))
            .to_owned();
        app
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
        if !uri.starts_with("/api/connections") {
            assert!(!text.contains(TOKEN), "response echoes the token: {text}");
            let host = self
                .address
                .trim_start_matches("http://")
                .trim_end_matches("/zabbix");
            assert!(!text.contains(host), "response names the server: {text}");
            assert!(!text.contains("http://") && !text.contains("https://"));
            assert!(!text.contains("api_jsonrpc"), "{text}");
        }
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    async fn connect(&self) -> String {
        self.connect_named("Synthetic lab").await
    }

    async fn connect_named(&self, name: &str) -> String {
        let (status, profile) = self
            .call(
                "POST",
                "/api/connections",
                Some(json!({
                    "name": name,
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
        self.reconnect(&id).await;
        id
    }

    async fn reconnect(&self, id: &str) {
        let (status, connected) = self
            .call(
                "POST",
                &format!("/api/connections/{id}/connect"),
                Some(json!({"secret": TOKEN})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{connected}");
        self.methods
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    async fn import(&self, id: &str) -> String {
        let (status, imported) = self
            .call(
                "POST",
                &format!("/api/server/{id}/dashboards/601/import"),
                Some(json!({"scope": "dashboard"})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{imported}");
        imported["local_id"]
            .as_str()
            .unwrap_or_else(|| panic!("local id: {imported}"))
            .to_owned()
    }

    fn methods(&self) -> Vec<String> {
        self.methods
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

fn assert_reads_only(methods: &[String]) {
    assert!(!methods.is_empty());
    for method in methods {
        assert!(
            method.ends_with(".get")
                || method == "apiinfo.version"
                || method == "user.checkAuthentication",
            "{method} is not a read"
        );
    }
}

fn assert_rfc3339_utc(value: &Value) {
    let text = value.as_str().unwrap_or_default();
    assert!(text.ends_with('Z'), "not a UTC timestamp: {value}");
    assert!(
        time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339).is_ok(),
        "not RFC 3339: {value}"
    );
}

fn codes(preview: &Value) -> Vec<String> {
    preview["report"]["findings"]
        .as_array()
        .map(|findings| {
            findings
                .iter()
                .filter_map(|finding| finding["code"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn calculate_body() -> Value {
    json!({"source_hostid": "10", "target_hostid": "20", "scope": {"kind": "dashboard"}})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn routes_stay_behind_session_origin_csrf_and_content_type() {
    let app = App::start().await;
    let draft = "/api/drafts/wc-00000000000000000000000000000000/replace-host";
    let body = calculate_body().to_string();

    let mut no_cookie = app.request("GET", draft, None);
    no_cookie.headers_mut().remove(header::COOKIE);
    assert_eq!(app.send(no_cookie).await.0, StatusCode::UNAUTHORIZED);

    let mut foreign = app.request("GET", &format!("{draft}/inventory"), None);
    foreign.headers_mut().insert(
        header::ORIGIN,
        HeaderValue::from_static("http://attacker.example.invalid"),
    );
    assert_eq!(app.send(foreign).await.0, StatusCode::FORBIDDEN);

    for action in [
        "calculate",
        "revalidate",
        "decide",
        "confirm-kept",
        "operations/1/remove",
    ] {
        let mut no_csrf = app.request("POST", &format!("{draft}/{action}"), Some(&body));
        no_csrf.headers_mut().remove(CSRF_HEADER);
        assert_eq!(app.send(no_csrf).await.0, StatusCode::FORBIDDEN, "{action}");
    }

    let mut form = app.request("POST", &format!("{draft}/calculate"), Some(&body));
    form.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-www-form-urlencoded"),
    );
    assert_eq!(app.send(form).await.0, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(app.methods().is_empty(), "{:?}", app.methods());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn requests_are_validated_and_server_reads_need_the_draft_connection() {
    let app = App::start().await;
    let (status, body) = app
        .call("GET", "/api/drafts/..%2Fconfig/replace-host", None)
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "workcopy_id_invalid");

    let id = app.connect().await;
    let local_id = app.import(&id).await;
    let base = format!("/api/drafts/{local_id}/replace-host");

    let (status, body) = app.call("GET", &base, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        body,
        json!({"code": "replace_host_not_calculated", "params": {}})
    );

    for bad in [
        json!({}),
        json!({"source_hostid": "10", "target_hostid": "20"}),
        json!({"source_hostid": "10", "target_hostid": "20", "scope": {"kind": "everything"}}),
        json!({"source_hostid": "10", "target_hostid": "20", "scope": {"kind": "dashboard"}, "x": 1}),
    ] {
        let (status, body) = app
            .call("POST", &format!("{base}/calculate"), Some(bad))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], "request_invalid");
    }
    let (status, body) = app
        .call(
            "POST",
            &format!("{base}/calculate"),
            Some(json!({"source_hostid": "10", "target_hostid": "10", "scope": {"kind": "dashboard"}})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "replace_host_same_host");
    let (status, body) = app
        .call(
            "POST",
            &format!("{base}/calculate"),
            Some(json!({"source_hostid": "10", "target_hostid": "20",
                        "scope": {"kind": "pages", "dashboard_pageids": ["999"]}})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "replace_host_scope_invalid");
    let (status, body) = app
        .call("GET", &format!("{base}/hosts?source_hostid=10&x=1"), None)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "request_invalid");
    let (status, body) = app
        .call(
            "POST",
            &format!("{base}/operations/abc/remove"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "request_invalid");

    // Without a connection, server reads are refused; nothing is read.
    let (status, _) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/disconnect"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    app.methods
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    for (method, path, body) in [
        ("GET", format!("{base}/inventory"), None),
        ("GET", format!("{base}/hosts?source_hostid=10"), None),
        ("POST", format!("{base}/calculate"), Some(calculate_body())),
        ("POST", format!("{base}/revalidate"), Some(json!({}))),
    ] {
        let (status, body) = app.call(method, &path, body).await;
        assert_eq!(status, StatusCode::CONFLICT, "{path}");
        assert_eq!(body["code"], "not_connected", "{path}");
    }
    assert!(app.methods().is_empty(), "{:?}", app.methods());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wizard_calculation_decisions_undo_and_offline_preview() {
    let app = App::start().await;
    let id = app.connect().await;
    let local_id = app.import(&id).await;
    let base = format!("/api/drafts/{local_id}/replace-host");

    // 09-01: hosts of the draft with every reference and its outcome.
    let (status, inventory) = app.call("GET", &format!("{base}/inventory"), None).await;
    assert_eq!(status, StatusCode::OK, "{inventory}");
    assert_eq!(inventory["kind"], "dashboard");
    assert_eq!(inventory["pages"].as_array().map(Vec::len), Some(2));
    assert_eq!(inventory["pages"][0]["widgets"][1]["type"], "svggraph");
    assert_eq!(inventory["pages"][1]["dashboard_pageid"], "6011");
    let source = &inventory["hosts"][0];
    assert_eq!(source["hostid"], "10");
    assert_eq!(source["name"], "syn-pg-a");
    assert_eq!(source["widgets"], 3);
    let outcomes: Vec<(&str, &str)> = source["references"]
        .as_array()
        .map(|references| {
            references
                .iter()
                .map(|reference| {
                    (
                        reference["kind"].as_str().unwrap_or_default(),
                        reference["outcome"].as_str().unwrap_or_default(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        outcomes,
        [
            ("item", "changes"),
            ("host_pattern", "changes"),
            ("host", "kept_on_source")
        ]
    );
    assert!(source["references"][2]["reason"].is_string());

    // 09-02: target search without the replaced host, with template coverage.
    let (status, found) = app
        .call(
            "GET",
            &format!("{base}/hosts?source_hostid=10&search=syn"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(found["hosts"].as_array().map(Vec::len), Some(1));
    assert_eq!(found["hosts"][0]["hostid"], "20");
    assert_eq!(found["hosts"][0]["groups"], json!(["Synthetic/DB"]));
    assert_eq!(found["hosts"][0]["source_templates_found"], 1);
    assert_eq!(found["source_templates"], json!(["Synthetic PostgreSQL"]));
    assert_eq!(found["truncated"], false);
    let (_, none) = app
        .call(
            "GET",
            &format!("{base}/hosts?source_hostid=10&search=zzz"),
            None,
        )
        .await;
    assert_eq!(none["hosts"], json!([]));
    assert_eq!(none["empty_reason"], "not_visible_or_denied");

    // 09-03 / preview: read-only calculation stored as an operation.
    let (status, preview) = app
        .call("POST", &format!("{base}/calculate"), Some(calculate_body()))
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["operation"]["source_host"]["name"], "syn-pg-a");
    assert_eq!(preview["operation"]["target_host"]["hostid"], "20");
    assert_eq!(preview["operation"]["scope"], json!({"kind": "dashboard"}));
    assert_rfc3339_utc(&preview["operation"]["calculated_at"]);
    assert_eq!(preview["checked_at"], Value::Null);
    let rows = preview["rows"].as_array().cloned().unwrap_or_default();
    assert_eq!(rows.len(), 3, "{preview}");
    assert_eq!(rows[0]["match"], "exact");
    assert_eq!(rows[0]["state"], "changed");
    assert_eq!(
        rows[0]["target"],
        json!({"kind": "item", "itemid": "40001"})
    );
    assert_eq!(
        rows[1]["target"],
        json!({"kind": "host_pattern", "pattern": "syn-pg-b"})
    );
    assert_eq!(rows[2]["state"], "kept_on_source");
    assert_eq!(preview["changes"].as_array().map(Vec::len), Some(2));
    assert_eq!(preview["diff"]["summary"]["widgets"]["changed"], 2);
    assert_eq!(preview["diff"]["summary"]["widgets"]["kept_on_source"], 1);
    assert_eq!(
        preview["diff"]["pages"][0]["widgets"][1]["series"][0]["state"],
        "changed"
    );
    assert_eq!(preview["report"]["allows_publication"], false);
    let found_codes = codes(&preview);
    assert!(
        found_codes.contains(&"write_mode_required".to_owned()),
        "{found_codes:?}"
    );
    assert!(found_codes.contains(&"kept_on_source_unconfirmed".to_owned()));
    assert_eq!(
        preview["kept_on_source"]["entries"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    let digest = preview["kept_on_source"]["digest"]
        .as_str()
        .unwrap_or_default()
        .to_owned();

    // A decision is a local operation; undo restores the calculated row.
    let key = rows[0]["key"].clone();
    let (status, decided) = app
        .call(
            "POST",
            &format!("{base}/decide"),
            Some(json!({"key": key, "decision": {"kind": "keep_on_source"}})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{decided}");
    assert_eq!(decided["rows"][0]["state"], "kept_on_source");
    assert_eq!(decided["rows"][0]["decision_state"], "applied");
    let decision_seq = decided["rows"][0]["decision_seq"]
        .as_u64()
        .unwrap_or_default();
    let (status, refused) = app
        .call(
            "POST",
            &format!("{base}/decide"),
            Some(json!({"key": key, "decision": {"kind": "choose_candidate", "itemid": "40001"}})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused["code"], "decision_not_applicable");
    let (status, undone) = app
        .call(
            "POST",
            &format!("{base}/operations/{decision_seq}/remove"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{undone}");
    assert_eq!(undone["removed"], json!([decision_seq]));
    assert_eq!(undone["preview"]["rows"][0]["state"], "changed");

    // «Подтвердить список» needs exactly the shown list.
    let (status, refused) = app
        .call(
            "POST",
            &format!("{base}/confirm-kept"),
            Some(json!({"digest": "0"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["code"], "kept_on_source_list_changed");
    let (status, confirmed) = app
        .call(
            "POST",
            &format!("{base}/confirm-kept"),
            Some(json!({"digest": digest})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{confirmed}");
    assert_eq!(confirmed["kept_on_source"]["confirmed"], true);
    assert!(!codes(&confirmed).contains(&"kept_on_source_unconfirmed".to_owned()));

    // «Проверить все»: a fresh read, nothing written.
    let (status, checked) = app
        .call("POST", &format!("{base}/revalidate"), Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::OK, "{checked}");
    assert_rfc3339_utc(&checked["checked_at"]);
    assert_eq!(checked["rows"][0]["check"], "checked");

    let (status, history) = app.call("GET", &format!("{base}/history"), None).await;
    assert_eq!(status, StatusCode::OK);
    let operations = history["operations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        operations
            .iter()
            .map(|op| op["op"].clone())
            .collect::<Vec<_>>(),
        [json!("replace_host"), json!("replace_host_confirm_kept")]
    );
    assert_rfc3339_utc(&operations[0]["recorded_at"]);
    assert_reads_only(&app.methods());

    // Offline the preview still works and says a connection is required.
    let (status, _) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/disconnect"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, offline) = app.call("GET", &base, None).await;
    assert_eq!(status, StatusCode::OK, "{offline}");
    assert!(codes(&offline).contains(&"connection_required".to_owned()));
    assert_eq!(offline["rows"].as_array().map(Vec::len), Some(3));

    // «Сбросить все привязки»: removing the calculation removes its entries.
    let seq = offline["operation"]["seq"].as_u64().unwrap_or_default();
    let (status, reset) = app
        .call(
            "POST",
            &format!("{base}/operations/{seq}/remove"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reset}");
    assert_eq!(reset["removed"].as_array().map(Vec::len), Some(2));
    assert_eq!(reset["preview"], Value::Null);
    let (status, _) = app.call("GET", &base, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_reads_use_only_the_draft_connection() {
    let app = App::start().await;
    let first = app.connect().await;
    let local_id = app.import(&first).await;
    let base = format!("/api/drafts/{local_id}/replace-host");
    // Another profile becomes the active connection.
    let second = app.connect_named("Synthetic lab 2").await;
    assert_ne!(first, second);
    for (method, path, body) in [
        ("GET", format!("{base}/inventory"), None),
        ("GET", format!("{base}/hosts?source_hostid=10"), None),
        ("POST", format!("{base}/calculate"), Some(calculate_body())),
    ] {
        let (status, body) = app.call(method, &path, body).await;
        assert_eq!(status, StatusCode::CONFLICT, "{path}");
        assert_eq!(body["code"], "draft_other_connection", "{path}");
    }
    assert!(app.methods().is_empty(), "{:?}", app.methods());
    // Local routes still answer.
    let (status, body) = app.call("GET", &base, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "replace_host_not_calculated");
}
