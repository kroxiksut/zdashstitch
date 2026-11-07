// SPDX-License-Identifier: MIT OR Apache-2.0

//! Publication routes (P2-04E/F) over a stateful mock Zabbix JSON-RPC
//! server on loopback: the `/api` boundary, read-write mode, the one-shot
//! plan handle, exactly one write, the new base after a verified update,
//! a conflict with zero writes, `already exists` after a create, and the
//! journal (recovery listing, «Убрать в карантин»). All data is synthetic.

use std::sync::{Arc, Mutex as StdMutex};

use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use zdashstitch_application::connections::{ConnectionService, MemoryCredentialStore};

use super::{DraftsHandle, PlanRegistry, publication_routes, routes};
use crate::{CSRF_HEADER, ConnectionsHandle, LocalSecurity, build_router_with_api, connections};

const PORT: u16 = 43181;
const TOKEN: &str = "p204f-token-secret-value";

#[derive(Default)]
struct Mock {
    dashboard: Value,
    methods: Vec<String>,
    clone_created: Option<Value>,
    accept_clone: bool,
    extra_clone_hosts: bool,
    item_override: Option<Vec<Value>>,
    item_prototypes: Vec<Value>,
    /// `dashboard.create` answers `already exists` for this name.
    taken_name: Option<String>,
}

type Shared = Arc<StdMutex<Mock>>;

fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, Mock> {
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

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

/// `dashboard.update` replaces the pages it sends with widgets.
fn apply_update(dashboard: &mut Value, params: &Value) {
    let Some(pages) = dashboard["pages"].as_array_mut() else {
        return;
    };
    for incoming in params["pages"].as_array().into_iter().flatten() {
        let Some(current) = pages
            .iter_mut()
            .find(|page| page["dashboard_pageid"] == incoming["dashboard_pageid"])
        else {
            continue;
        };
        if incoming.get("widgets").is_some() {
            *current = incoming.clone();
        }
    }
}

async fn rpc(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let id = body["id"].clone();
    let method = body["method"].as_str().unwrap_or_default().to_owned();
    let params = &body["params"];
    let mut mock = lock(&shared);
    mock.methods.push(method.clone());
    let authorized = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        == Some(format!("Bearer {TOKEN}").as_str());
    let mut hosts = vec![("10", "syn-pg-a"), ("20", "syn-pg-b")];
    if mock.extra_clone_hosts {
        hosts.extend([("30", "syn-pg-c"), ("40", "syn-pg-d"), ("50", "syn-pg-e")]);
    }
    let error = |data: String| {
        Json(json!({"jsonrpc": "2.0", "id": id, "error": {
            "code": -32602, "message": "Invalid params.", "data": data
        }}))
        .into_response()
    };
    let result = match method.as_str() {
        "apiinfo.version" => json!("7.4.16"),
        "user.checkAuthentication" if params["token"] == TOKEN => {
            json!({"userid": "3", "username": "synthetic", "roleid": "7", "type": 2})
        }
        _ if !authorized => return error("Not authorized.".to_owned()),
        "role.get" => {
            assert_eq!(params["roleids"], json!(["7"]));
            json!([{"roleid":"7","type":"2","rules":{
                "api.access":"1","api.mode":"0","api":[],
                "actions":[{"name":"edit_dashboards","status":"1"}]
            }}])
        }
        "dashboard.get" => {
            let names = ids(&params["filter"], "name");
            let selected = ids(params, "dashboardids");
            Value::Array(
                std::iter::once(&mock.dashboard)
                    .chain(&mock.clone_created)
                    .filter(|dashboard| {
                        (names.is_empty()
                            || names.iter().any(|name| dashboard["name"] == name.as_str()))
                            && (selected.is_empty()
                                || selected
                                    .iter()
                                    .any(|id| dashboard["dashboardid"] == id.as_str()))
                    })
                    .cloned()
                    .collect(),
            )
        }
        "dashboard.update" => {
            let mut current = mock.dashboard.clone();
            apply_update(&mut current, params);
            mock.dashboard = current;
            json!({"dashboardids": ["601"]})
        }
        "dashboard.create" => {
            let name = params["name"].as_str().unwrap_or_default().to_owned();
            if mock.taken_name.as_deref() == Some(name.as_str()) {
                return error(format!("Dashboard \"{name}\" already exists."));
            }
            if mock.accept_clone {
                assert!(mock.clone_created.is_none(), "a second create was sent");
                let mut created = params.clone();
                created["dashboardid"] = json!("602");
                created["userid"] = json!("3");
                created["private"] = json!("1");
                let mut next = 6100;
                for page in created["pages"].as_array_mut().into_iter().flatten() {
                    next += 1;
                    page["dashboard_pageid"] = json!(next.to_string());
                    for widget in page["widgets"].as_array_mut().into_iter().flatten() {
                        next += 1;
                        widget["widgetid"] = json!(next.to_string());
                    }
                }
                mock.clone_created = Some(created);
                json!({"dashboardids": ["602"]})
            } else {
                return error("Unexpected create.".to_owned());
            }
        }
        "host.get" if params.get("selectParentTemplates").is_some() => Value::Array(
            hosts
                .iter()
                .filter(|(hostid, _)| ids(params, "hostids").iter().any(|id| id == hostid))
                .map(|(hostid, name)| {
                    json!({
                        "hostid": hostid, "host": name, "name": name,
                        "hostgroups": [{"name": "Synthetic/DB"}],
                        "parentTemplates": [{"templateid": "900", "name": "Synthetic PostgreSQL"}]
                    })
                })
                .collect(),
        ),
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
            let mut items = mock.item_override.clone().unwrap_or_else(items);
            if mock.extra_clone_hosts {
                items.extend([
                    item("50001", "30", "pgsql.ping", "Ping"),
                    item("60001", "40", "pgsql.ping", "Ping"),
                    item("70001", "50", "pgsql.ping", "Ping"),
                ]);
            }
            Value::Array(
                items
                    .into_iter()
                    .filter(|item| {
                        itemids.iter().any(|id| item["itemid"] == id.as_str())
                            || hostids.iter().any(|id| item["hostid"] == id.as_str())
                    })
                    .collect(),
            )
        }
        "itemprototype.get" => {
            let wanted = ids(params, "itemids");
            Value::Array(
                mock.item_prototypes
                    .iter()
                    .filter(|item| wanted.iter().any(|id| item["itemid"] == id.as_str()))
                    .cloned()
                    .collect(),
            )
        }
        _ => json!([]),
    };
    Json(json!({"jsonrpc": "2.0", "id": id, "result": result})).into_response()
}

async fn zabbix() -> (String, Shared) {
    let shared: Shared = Arc::new(StdMutex::new(Mock {
        dashboard: dashboard(),
        ..Mock::default()
    }));
    let app = Router::new()
        .route("/zabbix/api_jsonrpc.php", post(rpc))
        .with_state(Arc::clone(&shared));
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
    (format!("http://{address}/zabbix"), shared)
}

struct App {
    router: Router,
    cookie: String,
    csrf: String,
    address: String,
    mock: Shared,
    plans: Arc<PlanRegistry>,
    connections: ConnectionsHandle,
    storage: tempfile::TempDir,
    _config: tempfile::TempDir,
}

impl App {
    async fn start() -> Self {
        Self::start_with_ui("missing-ui").await.0
    }

    async fn start_with_ui(ui: impl AsRef<std::path::Path>) -> (Self, LocalSecurity) {
        let (address, mock) = zabbix().await;
        let config = tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"));
        let storage = tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"));
        let handle = ConnectionsHandle::new(ConnectionService::new(
            config.path().join("config"),
            Box::new(MemoryCredentialStore::default()),
        ));
        let initial = LocalSecurity::initialize(PORT)
            .unwrap_or_else(|error| panic!("security must initialize: {error}"));
        let plans = Arc::new(PlanRegistry::default());
        initial.security.register_session_memory(plans.clone());
        let drafts = DraftsHandle::at(storage.path().to_path_buf());
        let api = connections::routes(handle.clone())
            .merge(routes(handle.clone(), drafts.clone()))
            .merge(publication_routes(handle.clone(), drafts, plans.clone()));
        let router = build_router_with_api(ui, initial.security.clone(), api);
        let mut app = Self {
            router,
            cookie: String::new(),
            csrf: String::new(),
            address,
            mock,
            plans,
            connections: handle,
            storage,
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
        (app, initial.security)
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
            assert!(!text.contains("api_jsonrpc"), "{text}");
        }
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

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
        id
    }

    async fn enable_writes(&self, id: &str) {
        let (status, body) = self
            .call(
                "POST",
                &format!("/api/connections/{id}/enable-writes"),
                Some(json!({"confirmed": true})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    /// Imports the dashboard, calculates the replacement and confirms the
    /// kept list; returns the draft and its preview.
    async fn prepared_draft(&self, id: &str) -> (String, Value) {
        let (status, imported) = self
            .call(
                "POST",
                &format!("/api/server/{id}/dashboards/601/import"),
                Some(json!({"scope": "dashboard"})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{imported}");
        let local_id = imported["local_id"]
            .as_str()
            .unwrap_or_else(|| panic!("local id: {imported}"))
            .to_owned();
        let base = format!("/api/drafts/{local_id}/replace-host");
        let (status, preview) = self
            .call(
                "POST",
                &format!("{base}/calculate"),
                Some(json!({"source_hostid": "10", "target_hostid": "20",
                            "scope": {"kind": "dashboard"}})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{preview}");
        let digest = preview["kept_on_source"]["digest"].clone();
        let (status, preview) = self
            .call(
                "POST",
                &format!("{base}/confirm-kept"),
                Some(json!({"digest": digest})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{preview}");
        (local_id, preview)
    }

    fn methods(&self) -> Vec<String> {
        lock(&self.mock).methods.clone()
    }

    fn clear_methods(&self) {
        lock(&self.mock).methods.clear();
    }

    fn writes(&self) -> usize {
        self.methods()
            .iter()
            .filter(|method| !method.ends_with(".get") && method.as_str() != "apiinfo.version")
            .filter(|method| method.as_str() != "user.checkAuthentication")
            .count()
    }
}

fn update_request(preview: &Value) -> Value {
    json!({
        "action": "update_source",
        "revision": preview["revision"],
        "base_fingerprint": preview["base_fingerprint"],
        "local_fingerprint": preview["local_fingerprint"],
        "kept_digest": preview["kept_on_source"]["digest"],
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publication_routes_stay_behind_the_api_boundary() {
    let app = App::start().await;
    let draft = "/api/drafts/wc-00000000000000000000000000000000/publication";
    for action in ["check", "plan", "confirm", "discard", "refetch"] {
        let mut no_cookie = app.request("POST", &format!("{draft}/{action}"), Some("{}"));
        no_cookie.headers_mut().remove(header::COOKIE);
        assert_eq!(
            app.send(no_cookie).await.0,
            StatusCode::UNAUTHORIZED,
            "{action}"
        );
        let mut no_csrf = app.request("POST", &format!("{draft}/{action}"), Some("{}"));
        no_csrf.headers_mut().remove(CSRF_HEADER);
        assert_eq!(app.send(no_csrf).await.0, StatusCode::FORBIDDEN, "{action}");
    }
    let clone = "/api/drafts/wc-00000000000000000000000000000000/clone";
    let mut no_cookie = app.request("GET", clone, None);
    no_cookie.headers_mut().remove(header::COOKIE);
    assert_eq!(app.send(no_cookie).await.0, StatusCode::UNAUTHORIZED);
    for uri in [
        clone.to_owned(),
        format!("{clone}/wc-00000000000000000000000000000001/plan"),
    ] {
        let mut no_csrf = app.request("POST", &uri, Some("{}"));
        no_csrf.headers_mut().remove(CSRF_HEADER);
        assert_eq!(app.send(no_csrf).await.0, StatusCode::FORBIDDEN);
    }
    let journal = "/api/publication/journal/zds-0123-0123-00000001-0123456789abcdef";
    for action in ["reconcile", "close", "quarantine"] {
        let mut no_csrf = app.request("POST", &format!("{journal}/{action}"), Some("{}"));
        no_csrf.headers_mut().remove(CSRF_HEADER);
        assert_eq!(app.send(no_csrf).await.0, StatusCode::FORBIDDEN, "{action}");
    }
    let (status, body) = app.call("GET", "/api/publication/journal", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"unfinished": [], "damaged": []}));
    assert!(app.methods().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_plan_without_read_write_mode_and_no_write_without_a_live_plan() {
    let app = App::start().await;
    let id = app.connect().await;
    let (local_id, preview) = app.prepared_draft(&id).await;
    let base = format!("/api/drafts/{local_id}/publication");
    app.clear_methods();

    // Read-only: the checks read, the plan is refused before any read.
    let (status, check) = app
        .call("POST", &format!("{base}/check"), Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::OK, "{check}");
    assert_eq!(check["source"]["state"], "current");
    assert_eq!(check["mode"], "read_only");
    assert!(check["remote"].is_null());
    let (status, body) = app
        .call(
            "POST",
            &format!("{base}/plan"),
            Some(update_request(&preview)),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "write_mode_required");

    // A forged plan or payload never reaches a write.
    let forged = json!({
        "handle": "plan-00000000000000000000000000000000",
        "plan_id": "zds-forged", "confirmed": true,
        "update_params": {"method": "dashboard.update"}
    });
    let (status, body) = app
        .call("POST", &format!("{base}/confirm"), Some(forged))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "request_invalid");
    let (status, body) = app
        .call(
            "POST",
            &format!("{base}/confirm"),
            Some(json!({"handle": "plan-00000000000000000000000000000000",
                        "plan_id": "zds-forged", "confirmed": true})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "plan_not_found");
    let (status, body) = app
        .call(
            "POST",
            &format!("{base}/confirm"),
            Some(json!({"handle": "x", "plan_id": "y", "confirmed": false})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "confirmation_required");
    assert_eq!(app.writes(), 0, "{:?}", app.methods());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_verified_update_is_sent_once_and_becomes_the_new_base() {
    let app = App::start().await;
    let id = app.connect().await;
    let (local_id, preview) = app.prepared_draft(&id).await;
    app.enable_writes(&id).await;
    let base = format!("/api/drafts/{local_id}/publication");
    app.clear_methods();

    let (status, planned) = app
        .call(
            "POST",
            &format!("{base}/plan"),
            Some(update_request(&preview)),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{planned}");
    assert_eq!(app.writes(), 0, "planning only reads: {:?}", app.methods());
    let handle = planned["handle"].as_str().unwrap_or_default().to_owned();
    let plan = &planned["plan"];
    assert!(handle.starts_with("plan-") && handle.len() == 37);
    assert_eq!(plan["action"], "update_source");
    assert_eq!(plan["api_method"], "dashboard.update");
    assert_eq!(plan["write_requests"], 1);
    assert_eq!(plan["replacement_count"], 2);
    let plan_id = plan["plan_id"].as_str().unwrap_or_default().to_owned();
    assert_ne!(plan_id, handle);
    assert_eq!(app.plans.count(), 1);

    // The confirmation must name exactly what the dialog showed.
    let (status, body) = app
        .call(
            "POST",
            &format!("{base}/confirm"),
            Some(
                json!({"handle": handle, "plan_id": plan_id, "confirmed": true,
                        "kept_digest": "other"}),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "plan_mismatch");
    // ...and the mismatch consumed the plan.
    assert_eq!(app.plans.count(), 0);
    assert_eq!(app.writes(), 0);

    let (_, planned) = app
        .call(
            "POST",
            &format!("{base}/plan"),
            Some(update_request(&preview)),
        )
        .await;
    let handle = planned["handle"].clone();
    let confirm = json!({
        "handle": handle, "plan_id": planned["plan"]["plan_id"], "confirmed": true,
        "kept_digest": preview["kept_on_source"]["digest"],
    });
    app.clear_methods();
    let (status, outcome) = app
        .call("POST", &format!("{base}/confirm"), Some(confirm.clone()))
        .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["state"], "verified", "{outcome}");
    assert_eq!(outcome["kind"], "update");
    assert_eq!(outcome["sent"], true);
    assert_eq!(outcome["entry"]["sends"], 1);
    assert!(outcome["base_advanced"].is_object(), "{outcome}");
    let updates = app
        .methods()
        .iter()
        .filter(|method| method.as_str() == "dashboard.update")
        .count();
    assert_eq!(updates, 1);
    assert_eq!(app.writes(), 1, "{:?}", app.methods());

    // One-shot: the same confirmation again finds nothing and writes nothing.
    app.clear_methods();
    let (status, body) = app
        .call("POST", &format!("{base}/confirm"), Some(confirm))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "plan_not_found");
    assert!(app.methods().is_empty());

    // The draft now has the snapshot after writing as its base.
    let (status, body) = app
        .call("GET", &format!("/api/drafts/{local_id}/replace-host"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "replace_host_not_calculated");
    let written = lock(&app.mock).dashboard.clone();
    assert_eq!(
        written["pages"][0]["widgets"][0]["fields"][0]["value"],
        "40001"
    );
    assert_eq!(
        written["pages"][1],
        dashboard()["pages"][1],
        "kept page untouched"
    );
    let (status, journal) = app.call("GET", "/api/publication/journal", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(journal["unfinished"], json!([]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_conflict_builds_no_plan_and_writes_nothing() {
    let app = App::start().await;
    let id = app.connect().await;
    let (local_id, preview) = app.prepared_draft(&id).await;
    app.enable_writes(&id).await;
    // Someone renames the graph on the server after the copy.
    lock(&app.mock).dashboard["pages"][0]["widgets"][1]["name"] = json!("CPU (edited)");
    let base = format!("/api/drafts/{local_id}/publication");
    app.clear_methods();

    let (status, check) = app
        .call("POST", &format!("{base}/check"), Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::OK, "{check}");
    assert_eq!(check["source"]["state"], "conflict");
    assert_eq!(check["source"]["invalidated"], "source_changed");
    assert_eq!(check["remote"]["changes"][0]["kind"], "widget_changed");
    assert_eq!(check["remote"]["changes"][0]["overlaps"], true);
    assert_eq!(check["remote"]["overlapping_widgets"], 1);

    let (status, body) = app
        .call(
            "POST",
            &format!("{base}/plan"),
            Some(update_request(&preview)),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "plan_invalidated");
    assert_eq!(body["params"]["reason"], "source_changed");
    assert_eq!(app.writes(), 0, "{:?}", app.methods());

    // «Скопировать заново»: a new copy with the replacement replayed; this
    // draft keeps its base and its operations.
    let (status, refetched) = app
        .call(
            "POST",
            &format!("{base}/refetch"),
            Some(json!({"name": "Synthetic DB (copy 2)"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{refetched}");
    assert_eq!(refetched["replayed"], true);
    assert_ne!(refetched["local_id"], json!(local_id));
    let (status, old) = app
        .call("GET", &format!("/api/drafts/{local_id}/replace-host"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(old["revision"], preview["revision"]);
    assert_eq!(app.writes(), 0, "{:?}", app.methods());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_taken_name_after_send_is_final_and_never_retried() {
    let app = App::start().await;
    let id = app.connect().await;
    let (local_id, preview) = app.prepared_draft(&id).await;
    app.enable_writes(&id).await;
    let name = "Synthetic DB copy";
    lock(&app.mock).taken_name = Some(name.to_owned());
    let base = format!("/api/drafts/{local_id}/publication");
    let (status, planned) = app
        .call(
            "POST",
            &format!("{base}/plan"),
            Some(json!({
                "action": "save_as_copy",
                "revision": preview["revision"],
                "base_fingerprint": preview["base_fingerprint"],
                "local_fingerprint": preview["local_fingerprint"],
                "name": name,
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{planned}");
    assert_eq!(planned["plan"]["action"], "save_as_copy");
    assert_eq!(planned["plan"]["api_method"], "dashboard.create");
    assert_eq!(planned["plan"]["private"], true);
    assert_eq!(planned["plan"]["name_check"], "not_visible");
    app.clear_methods();
    let (status, outcome) = app
        .call(
            "POST",
            &format!("{base}/confirm"),
            Some(
                json!({"handle": planned["handle"], "plan_id": planned["plan"]["plan_id"],
                        "confirmed": true, "name": name}),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["kind"], "create");
    assert_eq!(outcome["state"], "reconciled_failed", "{outcome}");
    assert_eq!(outcome["evidence"], "dashboard_name_taken");
    assert_eq!(outcome["sent"], true);
    let creates = app
        .methods()
        .iter()
        .filter(|method| method.as_str() == "dashboard.create")
        .count();
    assert_eq!(creates, 1, "{:?}", app.methods());
    let (_, journal) = app.call("GET", "/api/publication/journal", None).await;
    assert_eq!(
        journal["unfinished"],
        json!([]),
        "a final state blocks nothing"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_damaged_journal_file_is_listed_and_quarantined_after_confirmation() {
    let app = App::start().await;
    let directory = app.storage.path().join("journal");
    std::fs::create_dir_all(&directory).unwrap_or_else(|error| panic!("{error}"));
    let id = "zds-0123456789ab-0123456789ab-00000001-0123456789abcdef";
    std::fs::write(directory.join(format!("{id}.jsonl")), b"{broken\n")
        .unwrap_or_else(|error| panic!("{error}"));
    let (status, journal) = app.call("GET", "/api/publication/journal", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(journal["damaged"][0]["operation_id"], id);
    assert!(journal["damaged"][0]["target"].is_null());

    let path = format!("/api/publication/journal/{id}/quarantine");
    let (status, body) = app
        .call("POST", &path, Some(json!({"confirmed": false})))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "quarantine_not_confirmed");
    let (status, body) = app
        .call("POST", &path, Some(json!({"confirmed": true})))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["quarantined"], true);
    let (_, journal) = app.call("GET", "/api/publication/journal", None).await;
    assert_eq!(journal, json!({"unfinished": [], "damaged": []}));
    assert!(
        directory
            .join("quarantine")
            .join(format!("{id}.jsonl"))
            .is_file()
    );
    let (status, body) = app
        .call(
            "POST",
            "/api/publication/journal/..%2Fescape/close",
            Some(json!({"confirmed": true})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "operation_id_invalid");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ending_the_session_erases_every_plan() {
    let app = App::start().await;
    let id = app.connect().await;
    let (local_id, preview) = app.prepared_draft(&id).await;
    app.enable_writes(&id).await;
    let (status, _) = app
        .call(
            "POST",
            &format!("/api/drafts/{local_id}/publication/plan"),
            Some(update_request(&preview)),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.plans.count(), 1);
    let (status, _) = app
        .send(app.request("POST", "/api/session/end", Some(r#"{"confirmed":true}"#)))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    for _ in 0..200 {
        if app.plans.count() == 0 {
            break;
        }
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(app.plans.count(), 0);
    assert_eq!(app.writes(), 0);
}

fn lab_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required for the lab run"))
}

/// Starts an isolated browser instance for the authorized, synthetic lab.
/// A temporary handoff carries a one-use local bootstrap, never lab secrets.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires laboratory credentials, a built UI and a local browser"]
async fn lab_clone_gui_smoke_fixture() {
    assert_eq!(lab_env("ZDASHSTITCH_LAB_PUBLISH_ACK"), "1");
    let marker = std::path::PathBuf::from(lab_env("ZDASHSTITCH_LAB_CLONE_HANDOFF"));
    let (app, security) = App::start_with_ui(lab_env("ZDASHSTITCH_LAB_CLONE_UI")).await;
    let (status, profile) = app
        .call(
            "POST",
            "/api/connections",
            Some(json!({
                "name": "Clone acceptance", "address": lab_env("ZDASHSTITCH_LAB_API_URL"),
                "auth": {"method": "password", "username": lab_env("ZDASHSTITCH_LAB_USERNAME")},
                "trust": {"kind": "system"}
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let connection = profile["local_id"].as_str().unwrap_or_default();
    let (status, _) = app
        .call(
            "POST",
            &format!("/api/connections/{connection}/connect"),
            Some(json!({"secret": lab_env("ZDASHSTITCH_LAB_PASSWORD")})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let source_dashboard = lab_env("ZDASHSTITCH_LAB_SOURCE_DASHBOARD");
    let original = async || {
        let connections = app
            .connections
            .service()
            .unwrap_or_else(|| panic!("connection service"));
        let guard = connections.lock().await;
        let reader = guard.reader().unwrap_or_else(|| panic!("lab reader"));
        reader
            .dashboard_snapshot(connection, &source_dashboard)
            .await
            .unwrap_or_else(|_| panic!("lab source read"))
            .unwrap_or_else(|| panic!("lab source unavailable"))
    };
    let before = original().await;
    assert_eq!(before.server_version().to_string(), "7.4.16");
    assert!(
        before.raw_api_document()["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("ZDashstitch Contract Lab")),
        "expected the synthetic contract fixture"
    );
    let source = lab_import(&app, connection, &source_dashboard).await;
    let (status, setup) = app
        .call("GET", &format!("/api/drafts/{source}/clone"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "clone source setup");
    let sources = setup["hosts"]
        .as_array()
        .unwrap_or_else(|| panic!("source hosts"));
    let first = sources
        .first()
        .and_then(|host| host["hostid"].as_str())
        .unwrap_or_else(|| panic!("source host"));
    let (status, targets) = app
        .call(
            "GET",
            &format!("/api/drafts/{source}/replace-host/hosts?source_hostid={first}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "clone target setup");
    let targets = targets["hosts"]
        .as_array()
        .unwrap_or_else(|| panic!("target hosts"));
    let target = targets
        .iter()
        .filter(|host| host["hostid"] != first)
        .max_by_key(|host| host["source_templates_found"].as_u64().unwrap_or(0))
        .and_then(|host| host["hostid"].as_str())
        .unwrap_or_else(|| panic!("target host"));
    app.enable_writes(connection).await;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", PORT))
        .await
        .unwrap_or_else(|error| panic!("browser listener: {error}"));
    let bootstrap = security
        .issue_bootstrap(std::time::Duration::from_secs(2))
        .await
        .unwrap_or_else(|error| panic!("bootstrap: {error:?}"));
    std::fs::write(
        &marker,
        json!({
            "bootstrap": security.bootstrap_url(&bootstrap),
            "draft": format!("http://127.0.0.1:{PORT}/draft/{source}/bindings"),
            "source": first, "target": target, "source_count": sources.len()
        })
        .to_string(),
    )
    .unwrap_or_else(|error| panic!("handoff: {error}"));
    let server = tokio::spawn(axum::serve(listener, app.router.clone()).into_future());
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(240);
    loop {
        let stored = zdashstitch_application::PublishJournal::new(app.storage.path().to_path_buf())
            .list()
            .unwrap_or_else(|error| panic!("journal: {error}"));
        if let Some(entry) = stored.entries.iter().find(|entry| {
            entry.intent == "clone_dashboard"
                && matches!(
                    entry.state,
                    zdashstitch_application::JournalState::Verified
                        | zdashstitch_application::JournalState::ReconciledVerified
                )
        }) {
            assert_eq!(entry.sends, 1);
            assert_eq!(original().await.fingerprint(), before.fingerprint());
            println!("clone_gui_lab: verified, one create, original fingerprint unchanged");
            // Allow the browser to render and reload the persisted result.
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "browser did not verify Clone"
        );
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
    }
    server.abort();
    let service = app
        .connections
        .service()
        .unwrap_or_else(|| panic!("connection service"));
    service
        .lock()
        .await
        .disconnect()
        .await
        .unwrap_or_else(|_| panic!("lab disconnect"));
}

/// Keeps every unresolved reference on the source host and confirms the
/// kept list; returns the preview after that.
async fn keep_unresolved(app: &App, local_id: &str, mut preview: Value) -> Value {
    let base = format!("/api/drafts/{local_id}/replace-host");
    let rows = preview["rows"].as_array().cloned().unwrap_or_default();
    for row in rows {
        let keep = row["allowed_decisions"]
            .as_array()
            .is_some_and(|allowed| allowed.iter().any(|kind| kind == "keep_on_source"));
        if row["state"] == "unresolved" && keep {
            let (status, next) = app
                .call(
                    "POST",
                    &format!("{base}/decide"),
                    Some(json!({"key": row["key"], "decision": {"kind": "keep_on_source"}})),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{next}");
            preview = next;
        }
    }
    if preview["kept_on_source"]["entries"]
        .as_array()
        .is_some_and(|entries| !entries.is_empty())
    {
        let digest = preview["kept_on_source"]["digest"].clone();
        let (status, next) = app
            .call(
                "POST",
                &format!("{base}/confirm-kept"),
                Some(json!({"digest": digest})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{next}");
        preview = next;
    }
    preview
}

async fn lab_import(app: &App, connection: &str, dashboardid: &str) -> String {
    let (status, imported) = app
        .call(
            "POST",
            &format!("/api/server/{connection}/dashboards/{dashboardid}/import"),
            Some(json!({"scope": "dashboard"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{imported}");
    imported["local_id"].as_str().unwrap_or_default().to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clone_routes_save_reopen_and_require_confirmation() {
    let app = App::start().await;
    lock(&app.mock).accept_clone = true;
    lock(&app.mock).dashboard["pages"][0]["widgets"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("widgets"))
        .push(json!({
            "widgetid":"60102", "type":"item", "name":"Shared",
            "x":"48", "y":"0", "width":"12", "height":"3", "view_mode":"0",
            "fields":[{"type":"4","name":"itemid.0","value":"40001"}]
        }));
    let connection = app.connect().await;
    let source = lab_import(&app, &connection, "601").await;
    let base = format!("/api/drafts/{source}/clone");
    let (status, setup) = app.call("GET", &base, None).await;
    assert_eq!(status, StatusCode::OK, "{setup}");
    let (status, saved) = app
        .call(
            "POST",
            &base,
            Some(json!({
                    "revision":setup["revision"], "source_fingerprint":setup["source_fingerprint"],
            "name":"Synthetic clone", "hosts":{"10":"20","20":null}, "keep_patterns":true
                })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert!(saved["blocked"].is_null(), "{saved}");
    let id = saved["clone_id"].as_str().unwrap_or_default();
    let branch = format!("{base}/{id}");
    let (status, reopened) = app.call("GET", &branch, None).await;
    assert_eq!(status, StatusCode::OK, "{reopened}");
    assert_eq!(reopened, saved);
    assert_eq!(saved["pages"], 2);
    let comparison_uri = format!("{branch}/comparison");
    let (status, comparison) = app.call("GET", &comparison_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{comparison}");
    assert_eq!(comparison["observation"], "pending");
    assert!(comparison["operation_id"].is_null());
    assert!(comparison["access"][2].is_null());
    let rows = comparison["rows"]
        .as_array()
        .unwrap_or_else(|| panic!("comparison rows"));
    assert!(rows.iter().all(|row| row["observed"].is_null()));
    assert!(rows.iter().any(|row| row["field"] == "itemid.0"
        && row["change"] == "changed"
        && row["source"]["label"] == "30001"
        && row["expected"]["label"] == "40001"));
    assert!(
        rows.iter()
            .any(|row| row["field"] == "hostids.0" && row["kept"] == true)
    );
    let plan_uri = format!("{branch}/plan");
    let (status, denied) = app
        .call(
            "POST",
            &plan_uri,
            Some(json!({
                "kept_digest":saved["kept_digest"]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{denied}");
    app.enable_writes(&connection).await;
    let (status, plan) = app
        .call(
            "POST",
            &plan_uri,
            Some(json!({
                "kept_digest":saved["kept_digest"]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert!(plan.get("payload").is_none());
    let (status, refusal) = app
        .call(
            "POST",
            &format!("/api/drafts/{source}/publication/confirm"),
            Some(json!({
                "handle":plan["handle"], "plan_id":plan["plan_id"], "name":plan["name"],
                "kept_digest":saved["kept_digest"], "confirmed":false
            })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refusal}");
    assert!(
        !app.methods()
            .iter()
            .any(|method| method == "dashboard.create" || method == "dashboard.update")
    );
    let confirm_uri = format!("/api/drafts/{source}/publication/confirm");
    let confirmation = json!({
        "handle":plan["handle"], "plan_id":plan["plan_id"], "name":plan["name"],
        "kept_digest":saved["kept_digest"], "confirmed":true
    });
    let (status, outcome) = app
        .call("POST", &confirm_uri, Some(confirmation.clone()))
        .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["state"], "verified", "{outcome}");
    let (status, _) = app.call("POST", &confirm_uri, Some(confirmation)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, reopened) = app.call("GET", &branch, None).await;
    assert_eq!(status, StatusCode::OK, "{reopened}");
    assert_eq!(reopened["last_outcome"]["state"], "verified");
    assert_eq!(reopened["last_outcome"]["created_dashboardid"], "602");
    let (status, comparison) = app.call("GET", &comparison_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{comparison}");
    assert_eq!(comparison["observation"], "verified_read_back");
    assert_eq!(comparison["operation_id"], outcome["operation_id"]);
    assert_eq!(comparison["access"][2]["owner_matches"], true);
    assert!(
        comparison["rows"]
            .as_array()
            .unwrap_or_else(|| panic!("comparison rows"))
            .iter()
            .all(|row| row["observed_differs"] == false)
    );
    // Later edits are visible without rewriting the historical verdict.
    {
        let mut mock = lock(&app.mock);
        let created = mock
            .clone_created
            .as_mut()
            .unwrap_or_else(|| panic!("created clone"));
        created["pages"][0]["widgets"][0]["fields"][0]["value"] = json!("40002");
    }
    let (status, comparison) = app.call("GET", &comparison_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{comparison}");
    assert_eq!(comparison["observation"], "changed_since_read_back");
    assert_eq!(comparison["verdict"], "content_differs");
    assert!(
        comparison["rows"]
            .as_array()
            .unwrap_or_else(|| panic!("comparison rows"))
            .iter()
            .any(|row| row["field"] == "itemid.0"
                && row["unresolved"] == true
                && row["observed"]["label"] == "40002")
    );
    let (status, reopened) = app.call("GET", &branch, None).await;
    assert_eq!(status, StatusCode::OK, "{reopened}");
    assert_eq!(reopened["last_outcome"]["state"], "verified");
    let (status, _) = app
        .call(
            "POST",
            &format!("/api/connections/{connection}/disconnect"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, comparison) = app.call("GET", &comparison_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{comparison}");
    assert_eq!(comparison["observation"], "unavailable");
    assert_eq!(comparison["operation_id"], outcome["operation_id"]);
    assert!(comparison["access"][2].is_null());
    assert_eq!(
        app.methods()
            .iter()
            .filter(|method| *method == "dashboard.create")
            .count(),
        1
    );
    assert!(
        !app.methods()
            .iter()
            .any(|method| method == "dashboard.update")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clone_growth_reviews_and_publishes_only_the_selected_extra_series() {
    let app = App::start().await;
    let original = {
        let mut mock = lock(&app.mock);
        mock.accept_clone = true;
        mock.extra_clone_hosts = true;
        mock.dashboard["pages"][0]["widgets"][1]["fields"] = json!([
            {"type": "0", "name": "ds.0.dataset_type", "value": "0"},
            {"type": "4", "name": "ds.0.itemids.0", "value": "30001"},
            {"type": "4", "name": "ds.0.itemids.1", "value": "40001"},
            {"type": "1", "name": "ds.0.color.0", "value": "FF0000"},
            {"type": "1", "name": "ds.0.color.1", "value": "00FF00"}
        ]);
        mock.dashboard.clone()
    };
    let connection = app.connect().await;
    let source = lab_import(&app, &connection, "601").await;
    let base = format!("/api/drafts/{source}/clone");
    let preview_uri = format!("{base}/preview-growth");
    let (status, setup) = app.call("GET", &base, None).await;
    assert_eq!(status, StatusCode::OK, "{setup}");
    let samples = setup["growth_samples"]
        .as_array()
        .unwrap_or_else(|| panic!("{setup}"));
    assert_eq!(samples.len(), 2);
    let mut request = json!({
        "revision": setup["revision"], "source_fingerprint": setup["source_fingerprint"],
        "name": "Synthetic clone with growth", "hosts": {"10":"30","20":"40"},
        "keep_patterns": false,
        "growth": {"sample": samples[0]["field"], "target_hostid":"50"}
    });
    let (status, blocked) = app.call("POST", &preview_uri, Some(request.clone())).await;
    assert_eq!(status, StatusCode::CONFLICT, "{blocked}");
    assert_eq!(blocked["code"], "clone_growth_single_host_widget");

    request["growth"]["sample"] = samples[1]["field"].clone();
    let (status, preview) = app.call("POST", &preview_uri, Some(request.clone())).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["before"].as_array().map(Vec::len), Some(2));
    assert_eq!(preview["after"].as_array().map(Vec::len), Some(3));
    assert_eq!(preview["after"][2]["host"], "syn-pg-e");
    assert_eq!(preview["after"][2]["key"], "pgsql.ping");

    let (status, blocked) = app.call("POST", &base, Some(request.clone())).await;
    assert_eq!(status, StatusCode::CONFLICT, "{blocked}");
    assert_eq!(blocked["code"], "clone_growth_preview_required");
    request["growth_fingerprint"] = preview["fingerprint"].clone();
    request["name"] = json!("Changed since preview");
    let (status, blocked) = app.call("POST", &base, Some(request.clone())).await;
    assert_eq!(status, StatusCode::CONFLICT, "{blocked}");
    assert_eq!(blocked["code"], "clone_growth_preview_required");
    request["name"] = json!("Synthetic clone with growth");
    let (status, saved) = app.call("POST", &base, Some(request)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert!(saved["blocked"].is_null(), "{saved}");
    let id = saved["clone_id"].as_str().unwrap_or_default();
    let branch = format!("{base}/{id}");
    let (status, reopened) = app.call("GET", &branch, None).await;
    assert_eq!(status, StatusCode::OK, "{reopened}");
    assert_eq!(reopened, saved);
    let (status, comparison) = app.call("GET", &format!("{branch}/comparison"), None).await;
    assert_eq!(status, StatusCode::OK, "{comparison}");
    assert!(
        comparison["rows"]
            .as_array()
            .unwrap_or_else(|| panic!("comparison rows"))
            .iter()
            .any(|row| row["field"] == "ds.0.itemids.2"
                && row["change"] == "added"
                && row["source"].is_null()
                && row["expected"]["label"] == "70001")
    );
    assert!(
        !app.methods()
            .iter()
            .any(|method| matches!(method.as_str(), "dashboard.create" | "dashboard.update"))
    );

    app.enable_writes(&connection).await;
    let (status, plan) = app
        .call(
            "POST",
            &format!("{branch}/plan"),
            Some(json!({"kept_digest": saved["kept_digest"]})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    let (status, outcome) = app
        .call(
            "POST",
            &format!("/api/drafts/{source}/publication/confirm"),
            Some(json!({
                "handle": plan["handle"], "plan_id": plan["plan_id"], "name": plan["name"],
                "kept_digest": saved["kept_digest"], "confirmed": true
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["state"], "verified", "{outcome}");
    let mock = lock(&app.mock);
    assert_eq!(mock.dashboard, original);
    assert_eq!(
        mock.methods
            .iter()
            .filter(|method| *method == "dashboard.create")
            .count(),
        1
    );
    let created = mock
        .clone_created
        .as_ref()
        .unwrap_or_else(|| panic!("clone missing"));
    let graph = &created["pages"][0]["widgets"][1]["fields"];
    let field = |name: &str| {
        graph
            .as_array()
            .into_iter()
            .flatten()
            .find(|field| field["name"] == name)
            .map(|field| &field["value"])
    };
    assert_eq!(field("ds.0.itemids.0"), Some(&json!("50001")));
    assert_eq!(field("ds.0.itemids.1"), Some(&json!("60001")));
    assert_eq!(field("ds.0.itemids.2"), Some(&json!("70001")));
    let color = field("ds.0.color.2").unwrap_or_else(|| panic!("third color missing"));
    assert_ne!(Some(color), field("ds.0.color.0"));
    assert_ne!(Some(color), field("ds.0.color.1"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clone_requires_fresh_explicit_lld_and_pattern_choices() {
    const PROTOTYPE: &str = "pgsql.db.size[{#DBNAME}]";
    let lld_item = |id, host, parent, key, name| {
        let mut metric = item(id, host, key, name);
        metric["flags"] = json!("4");
        metric["units"] = json!("B");
        metric["itemDiscovery"] = json!({"parent_itemid":parent, "key_":PROTOTYPE});
        metric
    };
    let prototype = |id, host| {
        json!({
            "itemid":id, "hostid":host, "templateid":"8001", "name":"Database size",
            "key_":PROTOTYPE, "value_type":"3", "units":"B", "discoveryRule":[]
        })
    };
    let app = App::start().await;
    let original = {
        let mut mock = lock(&app.mock);
        mock.accept_clone = true;
        mock.dashboard["pages"][0]["widgets"][1]["fields"][0]["value"] = json!("syn-pg-a*");
        mock.dashboard["pages"][0]["widgets"][1]["fields"][1]["value"] = json!("Load on source*");
        mock.item_override = Some(vec![
            lld_item(
                "30001",
                "10",
                "31001",
                "pgsql.db.size[prod]",
                "Prod database",
            ),
            lld_item(
                "41001",
                "20",
                "42001",
                "pgsql.db.size[other]",
                "Other database",
            ),
            lld_item(
                "41002",
                "20",
                "42001",
                "pgsql.db.size[pilot]",
                "Pilot database",
            ),
            item("40002", "20", "system.cpu.load", "CPU load"),
        ]);
        mock.item_prototypes = vec![prototype("31001", "10"), prototype("42001", "20")];
        mock.dashboard.clone()
    };
    let connection = app.connect().await;
    let source = lab_import(&app, &connection, "601").await;
    let base = format!("/api/drafts/{source}/clone");
    let review_uri = format!("{base}/review-bindings");
    let pattern_uri = format!("{base}/preview-pattern");
    let (status, setup) = app.call("GET", &base, None).await;
    assert_eq!(status, StatusCode::OK, "{setup}");
    let mut request = json!({
        "revision":setup["revision"], "source_fingerprint":setup["source_fingerprint"],
        "name":"Synthetic decisions clone", "hosts":{"10":"20"}
    });
    let (status, review) = app.call("POST", &review_uri, Some(request.clone())).await;
    assert_eq!(status, StatusCode::OK, "{review}");
    let row = &review["lld_rows"][0];
    assert_eq!(row["prototype_key"], PROTOTYPE);
    assert_eq!(row["candidates"].as_array().map(Vec::len), Some(2));
    assert_eq!(row["candidates"][1]["key"], "pgsql.db.size[pilot]");
    let (status, blocked) = app.call("POST", &base, Some(request.clone())).await;
    assert_eq!(status, StatusCode::CONFLICT, "{blocked}");
    assert_eq!(blocked["code"], "clone_lld_selection_required");
    request["lld_choices"] = json!([{
        "field":row["field"], "slot_id":row["slot_id"], "fingerprint":row["fingerprint"], "itemid":"40002"
    }]);
    let (status, blocked) = app.call("POST", &base, Some(request.clone())).await;
    assert_eq!(status, StatusCode::CONFLICT, "{blocked}");
    assert_eq!(blocked["code"], "clone_lld_candidate_not_visible");
    request["lld_choices"][0]["itemid"] = json!("41002");
    let update_item = |index: usize, name: &str| {
        let mut mock = lock(&app.mock);
        let items = mock
            .item_override
            .as_mut()
            .unwrap_or_else(|| panic!("test catalogue"));
        items[index]["name"] = json!(name);
    };
    update_item(2, "Renamed pilot");
    let (status, blocked) = app.call("POST", &base, Some(request.clone())).await;
    assert_eq!(status, StatusCode::CONFLICT, "{blocked}");
    assert_eq!(blocked["code"], "clone_lld_selection_stale");
    let (status, review) = app.call("POST", &review_uri, Some(request.clone())).await;
    assert_eq!(status, StatusCode::OK, "{review}");
    request["lld_choices"][0]["fingerprint"] = review["lld_rows"][0]["fingerprint"].clone();
    let (status, blocked) = app.call("POST", &base, Some(request.clone())).await;
    assert_eq!(status, StatusCode::CONFLICT, "{blocked}");
    assert_eq!(blocked["code"], "clone_pattern_decision_required");

    let rows = setup["patterns"]
        .as_array()
        .unwrap_or_else(|| panic!("{setup}"));
    assert_eq!(rows.len(), 2);
    let mut decisions = Vec::new();
    for (row, (pattern, expected)) in rows
        .iter()
        .zip([("SYN-PG-*", "syn-pg-b"), ("cPu*", "CPU load")])
    {
        let (status, preview) = app
            .call(
                "POST",
                &pattern_uri,
                Some(json!({
                    "setup":request, "field":row["field"], "target_pattern":pattern
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{preview}");
        assert_eq!(preview["match_count"], 1, "{preview}");
        assert_eq!(preview["matches"][0]["host"], "syn-pg-b");
        assert_eq!(preview["matches"][0]["name"], expected);
        decisions.push(json!({
            "field":row["field"], "decision":{"action":"replace","target_pattern":pattern,
                "fingerprint":preview["fingerprint"]}
        }));
    }
    request["pattern_choices"] = json!(decisions);
    update_item(3, "Memory load");
    let (status, blocked) = app.call("POST", &base, Some(request.clone())).await;
    assert_eq!(status, StatusCode::CONFLICT, "{blocked}");
    assert_eq!(blocked["code"], "clone_pattern_preview_stale");
    let (status, preview) = app
        .call(
            "POST",
            &pattern_uri,
            Some(json!({
                "setup":request, "field":rows[1]["field"], "target_pattern":"cPu*"
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["match_count"], 0);
    update_item(3, "CPU load");
    let (status, saved) = app.call("POST", &base, Some(request)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let clone_id = saved["clone_id"].as_str().unwrap_or_default();
    let branch = format!("{base}/{clone_id}");
    let (status, reopened) = app.call("GET", &branch, None).await;
    assert_eq!(status, StatusCode::OK, "{reopened}");
    assert_eq!(reopened, saved);
    assert!(
        !app.methods()
            .iter()
            .any(|method| matches!(method.as_str(), "dashboard.create" | "dashboard.update"))
    );

    app.enable_writes(&connection).await;
    let (status, plan) = app
        .call(
            "POST",
            &format!("{branch}/plan"),
            Some(json!({
                "kept_digest":saved["kept_digest"]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    let (status, outcome) = app
        .call(
            "POST",
            &format!("/api/drafts/{source}/publication/confirm"),
            Some(json!({
                "handle":plan["handle"], "plan_id":plan["plan_id"], "name":plan["name"],
                "kept_digest":saved["kept_digest"], "confirmed":true
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["state"], "verified", "{outcome}");
    let mock = lock(&app.mock);
    assert_eq!(mock.dashboard, original);
    assert_eq!(
        mock.methods
            .iter()
            .filter(|method| *method == "dashboard.create")
            .count(),
        1
    );
    let created = mock
        .clone_created
        .as_ref()
        .unwrap_or_else(|| panic!("clone missing"));
    let widgets = &created["pages"][0]["widgets"];
    assert_eq!(widgets[0]["fields"][0]["value"], "41002");
    let fields = widgets[1]["fields"]
        .as_array()
        .unwrap_or_else(|| panic!("graph fields"));
    assert!(
        fields
            .iter()
            .any(|field| field["name"] == "ds.0.hosts.0" && field["value"] == "SYN-PG-*")
    );
    assert!(
        fields
            .iter()
            .any(|field| field["name"] == "ds.0.items.0" && field["value"] == "cPu*")
    );
}

async fn lab_calculate(app: &App, local_id: &str, from: &str, to: &str) -> Value {
    let (status, preview) = app
        .call(
            "POST",
            &format!("/api/drafts/{local_id}/replace-host/calculate"),
            Some(
                json!({"source_hostid": from, "target_hostid": to, "scope": {"kind": "dashboard"}}),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    keep_unresolved(app, local_id, preview).await
}

/// P2-04F on the authorized laboratory Zabbix 7.4.16, through these routes
/// only: copy a «ZDashstitch Contract Lab» dashboard, replace a host and
/// «Сохранить копию» (one `dashboard.create`, verified), then replace the
/// host back on that new copy and «Обновить исходную панель» (one
/// `dashboard.update`, verified, base advanced). Only the dashboard this run
/// creates is written. Credentials come from the environment only.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "laboratory Zabbix: ZDASHSTITCH_LAB_PUBLISH_ACK, _API_URL, _USERNAME, _PASSWORD"]
async fn lab_replace_host_copy_then_update_the_copy() {
    assert_eq!(lab_env("ZDASHSTITCH_LAB_PUBLISH_ACK"), "1");
    let source_dashboard =
        std::env::var("ZDASHSTITCH_LAB_SOURCE_DASHBOARD").unwrap_or_else(|_| "386".to_owned());
    let app = App::start().await;
    let (status, profile) = app
        .call(
            "POST",
            "/api/connections",
            Some(json!({
                "name": "Lab 7.4",
                "address": lab_env("ZDASHSTITCH_LAB_API_URL"),
                "auth": {"method": "password", "username": lab_env("ZDASHSTITCH_LAB_USERNAME")},
                "trust": {"kind": "system"}
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{profile}");
    let id = profile["local_id"].as_str().unwrap_or_default().to_owned();
    let (status, connected) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/connect"),
            Some(json!({"secret": lab_env("ZDASHSTITCH_LAB_PASSWORD")})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{connected}");
    app.enable_writes(&id).await;
    let source = lab_import(&app, &id, &source_dashboard).await;

    // Source host: the first one with references that change; target: the
    // visible host with the best template coverage.
    let (status, inventory) = app
        .call(
            "GET",
            &format!("/api/drafts/{source}/replace-host/inventory"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{inventory}");
    let host = inventory["hosts"]
        .as_array()
        .and_then(|hosts| {
            hosts.iter().find(|host| {
                host["references"].as_array().is_some_and(|references| {
                    references
                        .iter()
                        .any(|reference| reference["outcome"] == "changes")
                })
            })
        })
        .cloned()
        .unwrap_or_else(|| panic!("no replaceable host: {inventory}"));
    let from = host["hostid"].as_str().unwrap_or_default().to_owned();
    let (status, found) = app
        .call(
            "GET",
            &format!("/api/drafts/{source}/replace-host/hosts?source_hostid={from}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{found}");
    let to = found["hosts"]
        .as_array()
        .and_then(|hosts| {
            hosts
                .iter()
                .filter(|candidate| candidate["hostid"] != from.as_str())
                .max_by_key(|candidate| candidate["source_templates_found"].as_u64().unwrap_or(0))
        })
        .and_then(|candidate| candidate["hostid"].as_str())
        .unwrap_or_else(|| panic!("no target host: {found}"))
        .to_owned();
    let preview = lab_calculate(&app, &source, &from, &to).await;
    let (status, check) = app
        .call(
            "POST",
            &format!("/api/drafts/{source}/publication/check"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{check}");
    assert_eq!(check["source"]["state"], "current");

    // «Сохранить копию»: one create, verified by reading back.
    let name = format!(
        "ZDashstitch Contract Lab — P2-04F copy {}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs())
    );
    let (status, planned) = app
        .call(
            "POST",
            &format!("/api/drafts/{source}/publication/plan"),
            Some(json!({
                "action": "save_as_copy", "revision": preview["revision"],
                "base_fingerprint": preview["base_fingerprint"],
                "local_fingerprint": preview["local_fingerprint"], "name": name,
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{planned}");
    let (status, created) = app
        .call(
            "POST",
            &format!("/api/drafts/{source}/publication/confirm"),
            Some(
                json!({"handle": planned["handle"], "plan_id": planned["plan"]["plan_id"],
                        "confirmed": true, "name": name}),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(created["state"], "verified", "{created}");
    assert_eq!(created["entry"]["sends"], 1);
    let copy = created["created_dashboardid"]
        .as_str()
        .unwrap_or_else(|| panic!("created dashboard: {created}"))
        .to_owned();
    println!("lab: created copy dashboardid={copy} state=verified sends=1");

    // «Обновить исходную панель» on the new copy only: replace the host back.
    let draft = lab_import(&app, &id, &copy).await;
    let preview = lab_calculate(&app, &draft, &to, &from).await;
    assert_eq!(
        preview["report"]["allows_publication"], true,
        "{}",
        preview["report"]
    );
    let (status, planned) = app
        .call(
            "POST",
            &format!("/api/drafts/{draft}/publication/plan"),
            Some(update_request(&preview)),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{planned}");
    assert_eq!(
        planned["plan"]["dashboardid"],
        copy.as_str(),
        "only the new copy is written"
    );
    let (status, updated) = app
        .call(
            "POST",
            &format!("/api/drafts/{draft}/publication/confirm"),
            Some(
                json!({"handle": planned["handle"], "plan_id": planned["plan"]["plan_id"],
                        "confirmed": true, "kept_digest": planned["plan"]["kept_digest"]}),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["state"], "verified", "{updated}");
    assert_eq!(updated["entry"]["sends"], 1);
    assert!(updated["base_advanced"].is_object(), "{updated}");
    let (status, check) = app
        .call(
            "POST",
            &format!("/api/drafts/{draft}/publication/check"),
            Some(json!({})),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the replacement was consumed: {check}"
    );
    println!(
        "lab: updated copy dashboardid={copy} state=verified sends=1 replacements={}",
        planned["plan"]["replacement_count"]
    );
}
