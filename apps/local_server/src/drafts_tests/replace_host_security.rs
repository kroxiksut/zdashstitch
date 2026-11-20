// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;
use tokio::time::{Duration, timeout};

#[derive(Default)]
struct Upstream {
    pause_editable: AtomicBool,
    revoke_during_inventory: AtomicBool,
    revoked: AtomicBool,
    rename_during_inventory: AtomicBool,
    renamed: AtomicBool,
    editable_entered: Notify,
    resume_editable: Notify,
    methods: StdMutex<Vec<String>>,
}

async fn rpc(State(state): State<Arc<Upstream>>, Json(request): Json<Value>) -> Json<Value> {
    let method = request["method"].as_str().unwrap_or_default();
    state
        .methods
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(method.to_owned());
    let params = &request["params"];
    if method == "item.get" && params["itemids"] == json!(["30001", "30002"]) {
        if state.revoke_during_inventory.swap(false, Ordering::SeqCst) {
            state.revoked.store(true, Ordering::SeqCst);
        }
        if state.rename_during_inventory.swap(false, Ordering::SeqCst) {
            state.renamed.store(true, Ordering::SeqCst);
        }
    }
    if method == "dashboard.get"
        && params["editable"] == true
        && state.pause_editable.swap(false, Ordering::SeqCst)
    {
        state.editable_entered.notify_one();
        state.resume_editable.notified().await;
    }
    let result = match method {
        "apiinfo.version" => json!("8.0.0"),
        "user.checkAuthentication" => json!({"userid":"3",
            "username":if state.renamed.load(Ordering::SeqCst) {"renamed"} else {"synthetic"},
            "roleid":"2","type":"2"}),
        "role.get" => json!([{"roleid":"2","type":"2","rules":{
            "api.access":if state.revoked.load(Ordering::SeqCst) {"0"} else {"1"},
            "api.mode":"0","api":[],"actions":[{"name":"edit_dashboards","status":"1"}]
        }}]),
        "dashboard.get" if params["editable"] == true => json!([{"dashboardid":"501"}]),
        "dashboard.get" => {
            let mut source = dashboard("Identical synthetic snapshot");
            source["pages"][0]["widgets"][0]["fields"] = json!([
                {"type":"4","name":"itemid.0","value":"30001"}
            ]);
            source["pages"][1]["widgets"][0]["type"] = json!("clock");
            source["pages"][1]["widgets"][0]["fields"] = json!([]);
            json!([source])
        }
        "host.get" => json!([("40001", "Test source"), ("40002", "Test target")].into_iter()
            .filter(|(id, _)| params["hostids"].as_array().is_none_or(|ids| ids.contains(&json!(id))))
            .map(|(id, name)| json!({"hostid":id,"host":name,"name":name,"status":"0"})).collect::<Vec<_>>()),
        "item.get" => json!([("30001", "40001"), ("30002", "40002")].into_iter()
            .filter(|(id, host)| params["itemids"].as_array().is_none_or(|ids| ids.contains(&json!(id)))
                && params["hostids"].as_array().is_none_or(|ids| ids.contains(&json!(host))))
            .map(|(id, host)| json!({"itemid":id,"hostid":host,"key_":"test.value","name":"Value",
                "flags":"0","templateid":"0","value_type":"3","units":"","status":"0","valuemapid":"0","valuemap":null,"discoveryData":[]}))
            .collect::<Vec<_>>()),
        "dashboard.update" => json!({"dashboardids":["501"]}),
        _ => json!([]),
    };
    Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
}

async fn start_upstream() -> (String, Arc<Upstream>) {
    let upstream = Arc::new(Upstream::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|_| panic!("bind"));
    let address = format!(
        "http://{}/zabbix",
        listener.local_addr().unwrap_or_else(|_| panic!("address"))
    );
    let router = Router::new()
        .route("/zabbix/api_jsonrpc.php", post(rpc))
        .with_state(Arc::clone(&upstream));
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .unwrap_or_else(|_| panic!("serve"));
    });
    (address, upstream)
}

fn update_request(preview: &Value) -> Value {
    json!({"action":"update_source","revision":preview["revision"],"base_fingerprint":preview["base_fingerprint"],
        "local_fingerprint":preview["local_fingerprint"]})
}

async fn enable_writes(app: &App, connection: &str) {
    let (status, body) = app
        .call(
            "POST",
            &format!("/api/connections/{connection}/enable-writes"),
            Some(json!({"confirmed":true})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable writes: {}", body["code"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn eight_zero_replace_host_rejects_late_rename_and_identical_dashboard_on_another_endpoint() {
    let (address_a, upstream_a) = start_upstream().await;
    let (address_b, upstream_b) = start_upstream().await;
    let mut app = App::start(default_script()).await;
    app.address = address_a;
    let connection = app.connect().await;
    let connection_path = format!("/api/connections/{connection}");
    let (status, imported) = app
        .call(
            "POST",
            &format!("/api/server/{connection}/dashboards/501/import"),
            Some(json!({"scope":"dashboard"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "import: {}", imported["code"]);
    let local = imported["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("workcopy ID"));
    let path = format!("/api/drafts/{local}");
    let publication = format!("{path}/publication/plan");
    let (status, preview) = app
        .call(
            "POST",
            &format!("{path}/replace-host/calculate"),
            Some(json!({
                "source_hostid":"40001","target_hostid":"40002","scope":{"kind":"dashboard"}
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "calculate: {}", preview["code"]);
    assert_eq!(preview["changes"].as_array().map(Vec::len), Some(1));
    enable_writes(&app, &connection).await;
    let (status, ready) = app
        .call("POST", &publication, Some(update_request(&preview)))
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "plan: {} {:?}",
        ready["code"],
        ready["params"]
    );
    let confirm = format!("{path}/publication/confirm");
    let confirmation = |ready: &Value| json!({"handle":ready["handle"],"plan_id":ready["plan"]["plan_id"],"confirmed":true});
    upstream_a
        .revoke_during_inventory
        .store(true, Ordering::SeqCst);
    let (status, refused) = app.call("POST", &confirm, Some(confirmation(&ready))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(refused["state"], "failed_before_send");
    assert_eq!(refused["sent"], false);
    assert!(upstream_a.revoked.swap(false, Ordering::SeqCst));
    let (status, ready) = app
        .call("POST", &publication, Some(update_request(&preview)))
        .await;
    assert_eq!(status, StatusCode::OK);
    upstream_a
        .rename_during_inventory
        .store(true, Ordering::SeqCst);
    let (status, refused) = app.call("POST", &confirm, Some(confirmation(&ready))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(refused["state"], "failed_before_send");
    assert_eq!(refused["sent"], false);
    assert!(upstream_a.renamed.swap(false, Ordering::SeqCst));
    let (status, ready) = app
        .call("POST", &publication, Some(update_request(&preview)))
        .await;
    assert_eq!(status, StatusCode::OK);
    upstream_a.pause_editable.store(true, Ordering::SeqCst);
    let rename = async {
        timeout(
            Duration::from_secs(5),
            upstream_a.editable_entered.notified(),
        )
        .await
        .unwrap_or_else(|_| panic!("editable check did not start"));
        let result = timeout(
            Duration::from_secs(5),
            app.call(
                "POST",
                &format!("{path}/rename"),
                Some(json!({"name":"Renamed during preflight"})),
            ),
        )
        .await;
        upstream_a.resume_editable.notify_one();
        assert_eq!(
            result.unwrap_or_else(|_| panic!("rename stalled")).0,
            StatusCode::OK
        );
    };
    let ((status, refused), ()) = tokio::join!(
        app.call("POST", &confirm, Some(confirmation(&ready))),
        rename
    );
    assert_eq!(status, StatusCode::OK);
    assert_eq!(refused["state"], "failed_before_send");
    assert_eq!(refused["invalidated"], "draft_changed");
    assert_eq!(refused["sent"], false);
    let (status, preview) = app.call("GET", &format!("{path}/replace-host"), None).await;
    assert_eq!(status, StatusCode::OK);

    let file = app
        .storage
        .path()
        .join("workcopies")
        .join(local)
        .join("workcopy.json");
    let saved = std::fs::read(&file).unwrap_or_else(|_| panic!("local file"));
    let mut legacy: Value =
        serde_json::from_slice(&saved).unwrap_or_else(|_| panic!("local document"));
    let original_identity = legacy["source"]
        .as_object_mut()
        .and_then(|source| source.remove("server_identity_fingerprint"));
    assert!(
        original_identity
            .as_ref()
            .and_then(Value::as_str)
            .is_some_and(|hash| hash.len() == 64)
    );
    std::fs::write(
        &file,
        serde_json::to_vec(&legacy).unwrap_or_else(|_| panic!("encode")),
    )
    .unwrap_or_else(|_| panic!("legacy fixture"));
    assert_eq!(
        app.call("GET", &format!("{path}/replace-host"), None)
            .await
            .0,
        StatusCode::OK
    );
    let (status, refusal) = app
        .call("POST", &publication, Some(update_request(&preview)))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refusal["params"]["reason"], "connection_changed");
    std::fs::write(&file, saved).unwrap_or_else(|_| panic!("restore fixture"));

    let (status, profile) = app
        .call(
            "PUT",
            &connection_path,
            Some(json!({"name":"Synthetic lab", "address":address_b,
        "auth":{"method":"api_token","store_token":false},"trust":{"kind":"system"}})),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "change endpoint: {}",
        profile["code"]
    );
    assert_eq!(profile["context_changed"], true);
    assert_eq!(
        app.call(
            "POST",
            &format!("{connection_path}/connect"),
            Some(json!({"secret":TOKEN}))
        )
        .await
        .0,
        StatusCode::OK
    );
    enable_writes(&app, &connection).await;
    upstream_b
        .methods
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    let (status, refusal) = app
        .call("POST", &publication, Some(update_request(&preview)))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refusal["params"]["reason"], "connection_changed");
    let mut copy = update_request(&preview);
    copy["action"] = json!("save_as_copy");
    copy["name"] = json!("Synthetic copy");
    let (status, refusal) = app.call("POST", &publication, Some(copy)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refusal["params"]["reason"], "connection_changed");
    assert!(
        upstream_b
            .methods
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    );
    assert!(
        !upstream_a
            .methods
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .any(|method| method == "dashboard.update")
    );
}
