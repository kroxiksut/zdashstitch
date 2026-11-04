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
    let hosts = [("10", "syn-pg-a"), ("20", "syn-pg-b")];
    let error = |data: String| {
        Json(json!({"jsonrpc": "2.0", "id": id, "error": {
            "code": -32602, "message": "Invalid params.", "data": data
        }}))
        .into_response()
    };
    let result = match method.as_str() {
        "apiinfo.version" => json!("7.4.16"),
        "user.checkAuthentication" if params["token"] == TOKEN => {
            json!({"userid": "3", "username": "synthetic"})
        }
        _ if !authorized => return error("Not authorized.".to_owned()),
        "dashboard.get" => {
            let names = ids(&params["filter"], "name");
            let current = mock.dashboard.clone();
            if names.is_empty() || names.iter().any(|name| current["name"] == name.as_str()) {
                json!([current])
            } else {
                json!([])
            }
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
            return error("Unexpected create.".to_owned());
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
    storage: tempfile::TempDir,
    _config: tempfile::TempDir,
}

impl App {
    async fn start() -> Self {
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
            .merge(publication_routes(handle, drafts, plans.clone()));
        let router = build_router_with_api("missing-ui", initial.security, api);
        let mut app = Self {
            router,
            cookie: String::new(),
            csrf: String::new(),
            address,
            mock,
            plans,
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
