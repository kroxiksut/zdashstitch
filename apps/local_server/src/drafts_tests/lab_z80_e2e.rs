// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};

struct Wire {
    upstream: reqwest::Url,
    client: reqwest::Client,
    title: String,
    items: [String; 3],
    source: StdMutex<Option<String>>,
    update_pages: StdMutex<Option<Value>>,
    creates: AtomicUsize,
    updates: AtomicUsize,
    stale: AtomicBool,
    lost: AtomicBool,
}

async fn forward(
    State(wire): State<Arc<Wire>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let method = string(&body["method"]);
    let params = &body["params"];
    let source = wire
        .source
        .lock()
        .unwrap_or_else(|_| panic!("source lock"))
        .clone();
    match method {
        "dashboard.create" => {
            assert_eq!(params["name"], wire.title);
            assert!(params.get("userid").is_none());
            assert_eq!(params["private"], "1");
            for list in ["users", "userGroups"] {
                assert!(params.get(list).is_none_or(|rows| rows == &json!([])));
            }
            assert_eq!(params["pages"].as_array().map(Vec::len), Some(1));
            assert_eq!(
                params["pages"][0]["widgets"].as_array().map(Vec::len),
                Some(1)
            );
            let widget = &params["pages"][0]["widgets"][0];
            assert_eq!(widget["type"], "svggraph");
            let fields = widget["fields"]
                .as_array()
                .unwrap_or_else(|| panic!("create fields"));
            assert_eq!(fields.len(), 7);
            for (index, id) in wire.items.iter().enumerate() {
                let name = format!("ds.0.itemids.{index}");
                assert!(
                    fields
                        .iter()
                        .any(|field| field["name"] == name && field["value"] == *id)
                );
            }
            assert!(source.is_none());
            assert_eq!(wire.creates.fetch_add(1, SeqCst), 0);
        }
        "dashboard.update" => {
            assert_eq!(Some(string(&params["dashboardid"])), source.as_deref());
            assert_eq!(params.as_object().map(serde_json::Map::len), Some(2));
            let expected = wire
                .update_pages
                .lock()
                .unwrap_or_else(|_| panic!("expected update lock"));
            comparable(
                &params["pages"],
                expected
                    .as_ref()
                    .unwrap_or_else(|| panic!("approved update pages")),
            );
            assert_eq!(wire.updates.fetch_add(1, SeqCst), 0);
        }
        _ => assert!(zdashstitch_zabbix::read_only_allowlist().contains(&method)),
    }

    let mut request = wire.client.post(wire.upstream.clone()).json(&body);
    if let Some(auth) = headers.get(header::AUTHORIZATION) {
        request = request.header(header::AUTHORIZATION, auth);
    }
    let mut reply: Value = request
        .send()
        .await
        .unwrap_or_else(|_| panic!("lab transport"))
        .json()
        .await
        .unwrap_or_else(|_| panic!("lab JSON"));
    if method == "dashboard.create" {
        *wire.source.lock().unwrap_or_else(|_| panic!("source lock")) =
            Some(string(&reply["result"]["dashboardids"][0]).to_owned());
    } else if method == "dashboard.update" && wire.lost.load(SeqCst) {
        assert_eq!(reply["result"]["dashboardids"][0], params["dashboardid"]);
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    } else if method == "dashboard.get"
        && source
            .as_ref()
            .is_some_and(|id| params["dashboardids"] == json!([id]))
    {
        if wire.lost.load(SeqCst) && wire.updates.load(SeqCst) != 0 {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        if wire.stale.load(SeqCst)
            && let Some(row) = reply["result"].get_mut(0)
        {
            row["name"] = json!("Changed after preview");
        }
    }
    Json(reply).into_response()
}

// Only JSON scalar representation differs between the update encoder and a read response.
fn comparable(got: &Value, expected: &Value) {
    match (got, expected) {
        (Value::Object(got), Value::Object(expected)) => {
            assert_eq!(got.len(), expected.len());
            for (key, value) in expected {
                comparable(&got[key], value);
            }
        }
        (Value::Array(got), Value::Array(expected)) => {
            assert_eq!(got.len(), expected.len());
            for (got, expected) in got.iter().zip(expected) {
                comparable(got, expected);
            }
        }
        (Value::Number(got), Value::String(expected)) => assert_eq!(&got.to_string(), expected),
        _ => assert_eq!(got, expected),
    }
}

impl Wire {
    async fn audit(&self, id: &str) -> Value {
        let response: Value = self
            .client
            .post(self.upstream.clone())
            .bearer_auth(env("ZDS_E2E_AUDIT"))
            .json(
                &json!({"jsonrpc":"2.0", "id":1, "method":"dashboard.get", "params":{
                    "dashboardids":[id], "output":"extend", "selectPages":"extend",
                    "selectUsers":"extend", "selectUserGroups":"extend"
                }}),
            )
            .send()
            .await
            .unwrap_or_else(|_| panic!("audit transport"))
            .json()
            .await
            .unwrap_or_else(|_| panic!("audit JSON"));
        assert_eq!(response["result"].as_array().map(Vec::len), Some(1));
        response["result"][0].clone()
    }
}

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("missing {key}"))
}

fn string(value: &Value) -> &str {
    value
        .as_str()
        .unwrap_or_else(|| panic!("string response field"))
}

async fn request(
    app: &App,
    method: &str,
    path: &str,
    input: Option<Value>,
    status: StatusCode,
) -> Value {
    let (actual, response) = app.call(method, path, input).await;
    assert_eq!(actual, status, "{path}: {}", response["code"]);
    response
}

async fn connect(app: &App, connection: &str) {
    let connected = request(
        app,
        "POST",
        &format!("/api/connections/{connection}/connect"),
        Some(json!({"secret":env("ZDS_E2E_PASSWORD")})),
        StatusCode::OK,
    )
    .await;
    assert_eq!(connected["profile"]["mode"], "read_only");
}

async fn restart(app: &mut App, connection: &str) {
    let handle = ConnectionsHandle::new(ConnectionService::new(
        app._config.path().join("config"),
        Box::new(MemoryCredentialStore::default()),
    ));
    let initial = LocalSecurity::initialize(PORT).unwrap_or_else(|_| panic!("fresh local session"));
    let drafts = DraftsHandle::at(app.storage.path().to_path_buf());
    let api = connections::routes(handle.clone())
        .merge(routes(handle.clone(), drafts.clone()))
        .merge(super::super::publication_routes(
            handle.clone(),
            drafts,
            Arc::new(super::super::PlanRegistry::default()),
        ));
    app.router = build_router_with_api("missing-ui", initial.security, api);
    app.connections = handle;
    app.enter(&initial.code).await;
    connect(app, connection).await;
}

async fn local_draft(app: &App) -> Value {
    let set = request(
        app,
        "POST",
        "/api/upload-sets",
        Some(json!({})),
        StatusCode::CREATED,
    )
    .await;
    let id = string(&set["id"]);
    let payload = env("ZDS_E2E_TEMPLATE");
    let mut upload = app.request(
        "POST",
        &format!("/api/uploads/{id}/files/e2e.json"),
        Some(&payload),
    );
    upload.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    assert_eq!(app.send(upload).await.0, StatusCode::CREATED);
    let root = format!("/api/library/template-import/{id}");
    let inspected = request(
        app,
        "POST",
        &format!("{root}/inspect"),
        Some(json!({})),
        StatusCode::OK,
    )
    .await;
    assert_eq!(inspected[0]["status"], "ready");
    request(
        app,
        "POST",
        &format!("{root}/run"),
        Some(json!({})),
        StatusCode::OK,
    )
    .await;
    let library = request(app, "GET", "/api/library/index", None, StatusCode::OK).await;
    assert_eq!(library["catalogs"].as_array().map(Vec::len), Some(1));
    let catalog = string(&library["catalogs"][0]["catalog_id"]);
    let details = request(
        app,
        "GET",
        &format!("/api/library/catalogs/{catalog}/1"),
        None,
        StatusCode::OK,
    )
    .await;
    assert_eq!(details["metrics"][0]["name"], "E2E count");
    let mut page = json!({
        "name":"Observed", "evaluation_profile":"8.0", "catalogs":[{"catalog_id":catalog,"version":1}],
        "roles":{"sources":{"cardinality":"many","slots":["first","second"]}},
        "widgets":[{"kind":"graph","title":"E2E count by host","layout":{"mode":"fixed","x":1,"y":2,"w":24,"h":7},
            "metrics":[{"catalog_id":catalog,"metric_id":details["metrics"][0]["id"],"hosts":{"role":"sources","selection":"all"}}]}]
    });
    let created = request(
        app,
        "POST",
        "/api/library/offline-pages",
        Some(page.clone()),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(
        created["composition"]["instances"][0]["sources"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    let page_id = string(&created["local_id"]);
    page["roles"]["sources"]["slots"] = json!(["first", "second", "third"]);
    page["widgets"][0]["widget_id"] =
        created["bundle"]["project"]["root"]["widgets"][0]["id"].clone();
    let path = format!("/api/library/offline-pages/{page_id}");
    let lease = request(
        app,
        "POST",
        &format!("{path}/edit-session"),
        Some(json!({"take_over":false})),
        StatusCode::OK,
    )
    .await;
    let (status, edited) = app
        .editor_call(
            &path,
            string(&lease["token"]),
            json!({"expected_revision":created["revision"],"page":page}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "edit: {}", edited["code"]);
    assert_eq!(
        edited["composition"]["instances"][0]["sources"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );
    request(
        app,
        "POST",
        "/api/library/offline-dashboards",
        Some(json!({"name":"E2E local draft","page_ids":[page_id]})),
        StatusCode::CREATED,
    )
    .await
}

fn confirm(plan: &Value) -> Value {
    json!({"handle":plan["handle"], "plan_id":plan["plan"]["plan_id"], "confirmed":true})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the dedicated 8.0.0 lab account, 4 synthetic hosts and corresponding template export"]
async fn lab_z80_import_graph_replace_and_reconcile_http() {
    assert_eq!(env("ZDS_E2E_ACK"), "private-create-and-replace");
    let upstream = reqwest::Url::parse(&env("ZDS_E2E_URL")).unwrap_or_else(|_| panic!("lab URL"));
    assert_eq!(upstream.host_str(), Some("127.0.0.1"));
    assert_eq!(upstream.path(), "/zabbix/api_jsonrpc.php");
    assert_eq!(upstream.scheme(), "http");
    assert!(upstream.username().is_empty() && upstream.password().is_none());
    assert!(upstream.query().is_none() && upstream.fragment().is_none());
    let title = env("ZDS_E2E_TITLE");
    assert!(title.starts_with("ZDashstitch Z80-E2E "));
    let host_list = env("ZDS_E2E_HOSTS");
    let item_list = env("ZDS_E2E_ITEMS");
    let hosts: Vec<_> = host_list.split(',').collect();
    let items: Vec<_> = item_list.split(',').collect();
    assert_eq!(hosts.len(), 4);
    assert_eq!(items.len(), 4);
    assert!(
        hosts
            .iter()
            .chain(&items)
            .all(|id| zdashstitch_zabbix::is_zabbix_id(id))
    );
    let wire = Arc::new(Wire {
        upstream,
        title: title.clone(),
        items: std::array::from_fn(|index| items[index].to_owned()),
        client: reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(12))
            .build()
            .unwrap_or_else(|_| panic!("lab client")),
        source: StdMutex::new(None),
        update_pages: StdMutex::new(None),
        creates: AtomicUsize::new(0),
        updates: AtomicUsize::new(0),
        stale: AtomicBool::new(false),
        lost: AtomicBool::new(false),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|_| panic!("observer bind"));
    let address = format!(
        "http://{}/zabbix",
        listener
            .local_addr()
            .unwrap_or_else(|_| panic!("observer port"))
    );
    let proxy = Router::new()
        .route("/zabbix/api_jsonrpc.php", post(forward))
        .with_state(wire.clone());
    let proxy_task = tokio::spawn(async move { axum::serve(listener, proxy).await });
    let mut app = App::start(default_script()).await;
    let draft = local_draft(&app).await;
    assert!(app.methods().is_empty());
    println!("Imported template through Axum and extended offline graph from two to three slots");
    let profile = request(
        &app,
        "POST",
        "/api/connections",
        Some(json!({"name":"E2E lab","address":address,
        "auth":{"method":"password","username":env("ZDS_E2E_USER")},"trust":{"kind":"system"}})),
        StatusCode::CREATED,
    )
    .await;
    let connection = string(&profile["local_id"]);
    connect(&app, connection).await;
    let draft_path = format!("/api/drafts/{}", string(&draft["local_id"]));
    for (index, (host, item)) in hosts.iter().zip(&items).take(3).enumerate() {
        let review = json!({"revision":draft["revision"],"page_index":0,"widget_index":0,"field_index":index,"hostid":host});
        let evidence = request(
            &app,
            "POST",
            &format!("{draft_path}/portable-lld-review"),
            Some(review.clone()),
            StatusCode::OK,
        )
        .await;
        request(
            &app,
            "POST",
            &format!("{draft_path}/portable-lld-confirm"),
            Some(json!({"review":review,
            "itemid":item,"fingerprint":evidence["fingerprint"]})),
            StatusCode::OK,
        )
        .await;
    }
    request(
        &app,
        "POST",
        &format!("/api/connections/{connection}/enable-writes"),
        Some(json!({"confirmed":true})),
        StatusCode::OK,
    )
    .await;
    let plan = request(&app, "POST", &format!("{draft_path}/portable-publication/plan"), Some(json!({
        "connection_id":connection,"revision":draft["revision"],"name":title,"create_another":false
    })), StatusCode::OK).await;
    assert_eq!(plan["plan"]["api_method"], "dashboard.create");
    assert_eq!(wire.creates.load(SeqCst), 0);
    let mut create = confirm(&plan);
    create["name"] = json!(title);
    let created = request(
        &app,
        "POST",
        &format!("{draft_path}/publication/confirm"),
        Some(create),
        StatusCode::OK,
    )
    .await;
    assert_eq!(created["state"], "verified", "{}", created["code"]);
    assert!(created["record_error"].is_null());
    let source = wire
        .source
        .lock()
        .unwrap_or_else(|_| panic!("source lock"))
        .clone()
        .unwrap_or_else(|| panic!("created scenario panel"));
    let base = wire.audit(&source).await;
    assert_eq!(base["name"], title);
    assert_eq!(base["userid"], env("ZDS_E2E_OWNER"));
    assert_eq!(base["private"], "1");
    assert_eq!(base["users"], json!([]));
    assert_eq!(base["userGroups"], json!([]));
    let fields = base["pages"][0]["widgets"][0]["fields"]
        .as_array()
        .unwrap_or_else(|| panic!("Graph fields"));
    for (index, item) in items.iter().take(3).enumerate() {
        let name = format!("ds.0.itemids.{index}");
        assert!(
            fields
                .iter()
                .any(|field| field["name"] == name && field["value"] == *item)
        );
    }
    println!("Verified one private create with three bound rows and independent read-back");

    let imported = request(
        &app,
        "POST",
        &format!("/api/server/{connection}/dashboards/{source}/import"),
        Some(json!({"scope":"dashboard"})),
        StatusCode::CREATED,
    )
    .await;
    let workcopy = format!("/api/drafts/{}", string(&imported["local_id"]));
    let preview = request(
        &app,
        "POST",
        &format!("{workcopy}/replace-host/calculate"),
        Some(json!({
            "source_hostid":hosts[2],"target_hostid":hosts[3],"scope":{"kind":"dashboard"}
        })),
        StatusCode::OK,
    )
    .await;
    assert_eq!(preview["changes"].as_array().map(Vec::len), Some(1));
    let input = json!({"action":"update_source", "revision":preview["revision"],
        "base_fingerprint":preview["base_fingerprint"], "local_fingerprint":preview["local_fingerprint"]});
    let url = format!("{workcopy}/publication/plan");
    let submit = format!("{workcopy}/publication/confirm");
    let held = request(&app, "POST", &url, Some(input.clone()), StatusCode::OK).await;
    wire.stale.store(true, SeqCst);
    let blocked = request(&app, "POST", &submit, Some(confirm(&held)), StatusCode::OK).await;
    wire.stale.store(false, SeqCst);
    assert_eq!(blocked["state"], "conflict");
    assert_eq!(blocked["sent"], false);
    assert_eq!(wire.updates.load(SeqCst), 0);
    assert_eq!(wire.audit(&source).await, base);

    let held = request(&app, "POST", &url, Some(input), StatusCode::OK).await;
    assert_eq!(held["plan"]["api_method"], "dashboard.update");
    assert_eq!(held["plan"]["write_requests"], 1);
    assert_eq!(held["plan"]["replacement_count"], 1);
    let mut expected = base.clone();
    let row = expected["pages"][0]["widgets"][0]["fields"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("Graph fields"))
        .iter_mut()
        .find(|field| field["name"] == "ds.0.itemids.2")
        .unwrap_or_else(|| panic!("replaced row"));
    row["value"] = json!(items[3]);
    *wire
        .update_pages
        .lock()
        .unwrap_or_else(|_| panic!("expected update lock")) = Some(expected["pages"].clone());
    let rows = expected["pages"][0]["widgets"][0]["fields"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("Graph fields"));
    let index = rows
        .iter()
        .position(|field| field["name"] == "ds.0.itemids.2")
        .unwrap_or_else(|| panic!("replacement index"));
    let changed = rows.remove(index);
    rows.push(changed);
    wire.lost.store(true, SeqCst);
    let unknown = request(&app, "POST", &submit, Some(confirm(&held)), StatusCode::OK).await;
    assert_eq!(unknown["state"], "unknown_outcome", "{}", unknown["code"]);
    assert_eq!(unknown["sent"], true);
    assert!(unknown["base_advanced"].is_null());
    assert_eq!(wire.updates.load(SeqCst), 1);
    let entry_path = format!(
        "/api/publication/journal/{}",
        string(&held["plan"]["plan_id"])
    );
    let before_restart = request(&app, "GET", &entry_path, None, StatusCode::OK).await;
    assert_eq!(before_restart["state"], "unknown_outcome");
    assert_eq!(before_restart["sends"], 1);
    restart(&mut app, connection).await;
    let reopened = request(&app, "GET", &entry_path, None, StatusCode::OK).await;
    assert_eq!(reopened, before_restart);
    request(
        &app,
        "POST",
        &submit,
        Some(confirm(&held)),
        StatusCode::NOT_FOUND,
    )
    .await;
    wire.lost.store(false, SeqCst);
    let reconcile = format!("{entry_path}/reconcile");
    let recovered = request(&app, "POST", &reconcile, Some(json!({})), StatusCode::OK).await;
    assert_eq!(recovered["state"], "reconciled_verified");
    assert_eq!(
        recovered["base_advanced"]["base_fingerprint"],
        held["plan"]["expected_fingerprint"]
    );
    request(
        &app,
        "POST",
        &reconcile,
        Some(json!({})),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(wire.audit(&source).await, expected);
    assert_eq!(wire.creates.load(SeqCst), 1);
    assert_eq!(wire.updates.load(SeqCst), 1);
    println!(
        "Same panel reopened and replaced: stale blocked, one update, durable read-only recovery, independent full read-back"
    );
    proxy_task.abort();
}
