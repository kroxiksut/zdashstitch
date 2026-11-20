// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[path = "lab_clone_8_0/replace_host.rs"]
mod replace_host;

struct Lab {
    upstream: reqwest::Url,
    client: reqwest::Client,
    title: String,
    source: String,
    sends: AtomicUsize,
    drop_response: AtomicBool,
    hide_copy: AtomicBool,
    stale: AtomicBool,
    role_revoked: AtomicBool,
    extended: Option<Value>,
    changed_evidence: AtomicUsize,
}

async fn proxy(
    State(lab): State<Arc<Lab>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let method = body["method"]
        .as_str()
        .unwrap_or_else(|| panic!("API method"));
    let dashboard_create = method == "dashboard.create";
    let dashboard_update = method == "dashboard.update";
    let replace_fixture = lab
        .extended
        .as_ref()
        .filter(|fixture| fixture["replace_host"] == true);
    if dashboard_create {
        let params = &body["params"];
        assert!(
            params["name"]
                .as_str()
                .is_some_and(|name| name.starts_with(&lab.title))
        );
        assert!(params.get("dashboardid").is_none());
        let restoring = params["name"]
            .as_str()
            .is_some_and(|name| name.ends_with("HTTP restored"));
        match &lab.extended {
            Some(fixture) if restoring && fixture["super_admin"] == true => {
                assert_eq!(params["userid"], fixture["owner"]);
                assert_eq!(params["users"].as_array().map(Vec::len), Some(2));
                assert_eq!(params["userGroups"].as_array().map(Vec::len), Some(2));
            }
            _ => assert!(params.get("userid").is_none()),
        }
        let composite = lab
            .extended
            .as_ref()
            .is_some_and(|fixture| fixture.get("mode").is_some());
        assert_eq!(
            params["pages"].as_array().map(Vec::len),
            Some(if restoring && !composite { 1 } else { 2 })
        );
        for page in params["pages"]
            .as_array()
            .unwrap_or_else(|| panic!("pages"))
        {
            assert!(page.get("dashboard_pageid").is_none());
            for widget in page["widgets"]
                .as_array()
                .unwrap_or_else(|| panic!("widgets"))
            {
                assert!(widget.get("widgetid").is_none());
            }
        }
        lab.sends.fetch_add(1, Ordering::SeqCst);
    } else if dashboard_update {
        let fixture = replace_fixture.unwrap_or_else(|| panic!("no authorized update target"));
        let params = &body["params"];
        assert_eq!(params["dashboardid"], lab.source);
        assert!(params.as_object().is_some_and(|params| params.len() == 2));
        let pages = params["pages"]
            .as_array()
            .unwrap_or_else(|| panic!("pages"));
        assert_eq!(Some(pages.len() as u64), fixture["pages"].as_u64());
        for page in &pages[..2] {
            assert!(
                page["dashboard_pageid"]
                    .as_str()
                    .is_some_and(zdashstitch_zabbix::is_zabbix_id)
            );
            let widgets = page["widgets"]
                .as_array()
                .unwrap_or_else(|| panic!("widgets"));
            assert_eq!(widgets.len(), 2);
            assert!(widgets.iter().all(|widget| {
                widget["widgetid"]
                    .as_str()
                    .is_some_and(zdashstitch_zabbix::is_zabbix_id)
            }));
        }
        assert!(pages[2..].iter().all(|page| {
            page.as_object()
                .is_some_and(|page| page.len() == 1 && page.contains_key("dashboard_pageid"))
        }));
        lab.sends.fetch_add(1, Ordering::SeqCst);
    } else {
        assert!(
            zdashstitch_zabbix::read_only_allowlist().contains(&method),
            "unexpected method {method}"
        );
    }
    if method == "dashboard.get"
        && lab.hide_copy.load(Ordering::SeqCst)
        && (body["params"]["filter"]["name"].is_array()
            || (replace_fixture.is_some() && body["params"]["dashboardids"] == json!([lab.source])))
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let mut request = lab.client.post(lab.upstream.clone()).json(&body);
    if let Some(authorization) = headers.get(header::AUTHORIZATION) {
        request = request.header(header::AUTHORIZATION, authorization);
    }
    let mut envelope: Value = request
        .send()
        .await
        .unwrap_or_else(|_| panic!("lab API transport"))
        .json()
        .await
        .unwrap_or_else(|_| panic!("lab JSON-RPC envelope"));
    if (dashboard_create || dashboard_update) && lab.drop_response.swap(false, Ordering::SeqCst) {
        assert_eq!(
            envelope["result"]["dashboardids"].as_array().map(Vec::len),
            Some(1)
        );
        lab.hide_copy.store(true, Ordering::SeqCst);
        return StatusCode::BAD_GATEWAY.into_response();
    }
    if method == "dashboard.get"
        && lab.stale.load(Ordering::SeqCst)
        && (body["params"]["dashboardids"] == json!([lab.source])
            || lab.extended.as_ref().is_some_and(|fixture| {
                body["params"]["dashboardids"] == json!([fixture["simple"]])
            }))
        && let Some(source) = envelope["result"]
            .as_array_mut()
            .and_then(|rows| rows.first_mut())
    {
        if lab
            .extended
            .as_ref()
            .is_some_and(|fixture| body["params"]["dashboardids"] == json!([fixture["simple"]]))
        {
            source["private"] = json!("0");
        } else {
            source["name"] = json!("Changed source observation");
        }
    }
    if method == "role.get" && lab.role_revoked.load(Ordering::SeqCst) {
        for row in envelope["result"].as_array_mut().into_iter().flatten() {
            row["rules"]["api.access"] = json!("0");
        }
    }
    if let Some(fixture) = &lab.extended {
        match (lab.changed_evidence.load(Ordering::SeqCst), method) {
            (1 | 9, "item.get")
                if body["params"]["selectDiscoveryData"]
                    .as_array()
                    .is_some_and(|fields| fields.contains(&json!("status"))) =>
            {
                for item in envelope["result"]
                    .as_array_mut()
                    .into_iter()
                    .flatten()
                    .filter(|item| {
                        let changed = lab.changed_evidence.load(Ordering::SeqCst);
                        if changed == 9 {
                            item["itemid"] == fixture["target_items"][2]
                        } else {
                            item["itemid"] == fixture["lld_target"]
                                || item["itemid"] == fixture["target_items"][1]
                        }
                    })
                {
                    item["discoveryData"]["status"] = json!("1");
                }
            }
            (2, "item.get") if !body["params"]["search"]["name_resolved"].is_null() => {
                for item in envelope["result"]
                    .as_array_mut()
                    .into_iter()
                    .flatten()
                    .filter(|item| item["hostid"] == fixture["external"])
                {
                    item["name_resolved"] = json!("Changed external match");
                }
            }
            (3, "apiinfo.version") => envelope["result"] = json!("8.0.1"),
            (4, "user.checkAuthentication") if envelope["result"].is_object() => {
                envelope["result"]["userid"] = json!("999999999");
            }
            (5, "dashboard.get") if body["params"]["editable"] == true => {
                envelope["result"] = json!([])
            }
            (6, "host.get") if body["params"]["countOutput"] == true => {
                let old = envelope["result"]
                    .as_str()
                    .and_then(|count| count.parse::<u32>().ok())
                    .unwrap_or_else(|| panic!("host count evidence"));
                envelope["result"] = json!((old + 1).to_string());
            }
            (7, "host.get") if body["params"]["limit"] == 501 => {
                envelope["result"] = json!((1..=501).map(|index| json!({
                    "hostid":format!("9000{index}"),"host":"Over-limit synthetic","name":"Over-limit synthetic"
                })).collect::<Vec<_>>());
            }
            (8, "item.get") => {
                if let Some(items) = envelope["result"].as_array_mut() {
                    items.retain(|item| item["itemid"] != fixture["ordinary_item"]);
                }
            }
            (10, "item.get") if body["params"]["countOutput"] == true => {
                let count = envelope["result"]
                    .as_str()
                    .and_then(|count| count.parse::<u32>().ok())
                    .unwrap_or_else(|| panic!("item count evidence"));
                envelope["result"] = json!((count + 1).to_string());
            }
            (11, "item.get") if body["params"]["selectDiscoveryData"].is_array() => {
                lab.role_revoked.store(true, Ordering::SeqCst);
            }
            (13, "item.get") if body["params"]["selectDiscoveryData"].is_array() => {
                lab.changed_evidence.store(14, Ordering::SeqCst);
            }
            (14, "user.checkAuthentication") if envelope["result"].is_object() => {
                envelope["result"]["username"] = json!("Renamed synthetic account");
            }
            (12, "item.get") => {
                for item in envelope["result"].as_array_mut().into_iter().flatten() {
                    if item["itemid"] == fixture["lld_target"] {
                        item["status"] = json!("1");
                    }
                }
            }
            _ => {}
        }
    }
    Json(envelope).into_response()
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("missing lab setting {name}"))
}

async fn restart(app: &mut App, connection: &str) {
    let handle = ConnectionsHandle::new(ConnectionService::new(
        app._config.path().join("config"),
        Box::new(MemoryCredentialStore::default()),
    ));
    let initial = LocalSecurity::initialize(PORT).unwrap_or_else(|_| panic!("new local session"));
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
    let (status, profile) = app
        .call(
            "POST",
            &format!("/api/connections/{connection}/connect"),
            Some(json!({"secret":env("ZDS_CLONE_PASSWORD")})),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "read-only reconnect: {}",
        profile["code"]
    );
    assert_eq!(profile["profile"]["mode"], "read_only");
}

async fn stage(app: &App, path: &str, name: &str, fixture: &Value) -> (String, Value) {
    let (status, setup) = app.call("GET", path, None).await;
    assert_eq!(status, StatusCode::OK, "clone setup: {}", setup["code"]);
    let sample = setup["growth_samples"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|sample| sample["hostid"] == fixture["grow_source"])
        .unwrap_or_else(|| panic!("two-row Graph growth sample"));
    let mut setup_input = json!({
        "revision":setup["revision"], "source_fingerprint":setup["source_fingerprint"],
        "name":name,"hosts":fixture["mapping"],
        "growth":{"sample":sample["field"],"target_hostid":fixture["grow_target"]},
    });
    if fixture.get("lld_target").is_some() {
        let (status, review) = app
            .call(
                "POST",
                &format!("{path}/review-bindings"),
                Some(setup_input.clone()),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "LLD candidates: {}", review["code"]);
        let [row] = review["lld_rows"].as_array().map_or(&[][..], Vec::as_slice) else {
            panic!("one manually selected LLD candidate");
        };
        assert!(row["candidates"].as_array().is_some_and(|candidates| {
            candidates
                .iter()
                .any(|item| item["itemid"] == fixture["lld_target"])
        }));
        setup_input["lld_choices"] = json!([{
            "field":row["field"],"slot_id":row["slot_id"],"itemid":fixture["lld_target"],"fingerprint":row["fingerprint"]
        }]);
        if fixture["super_admin"] == true {
            let mut decisions = vec![];
            for mask in setup["patterns"].as_array().into_iter().flatten() {
                let field_name = mask["field"]["field_name"].as_str().unwrap_or_default();
                let target_pattern = match field_name {
                    "ds.0.hosts.0" => &fixture["target_names"][0],
                    "ds.0.hosts.1" => &fixture["target_names"][1],
                    "ds.0.items.0" => &fixture["item_mask"],
                    _ => panic!("unexpected mask field"),
                };
                let (status, preview) = app.call("POST", &format!("{path}/preview-pattern"),
                    Some(json!({"setup":setup_input,"field":mask["field"],"target_pattern":target_pattern}))).await;
                assert_eq!(status, StatusCode::OK, "mask review: {}", preview["code"]);
                assert_eq!(preview["truncated"], false);
                if field_name == "ds.0.items.0" {
                    assert!(
                        preview["matches"]
                            .as_array()
                            .is_some_and(|rows| rows.iter().any(|row| row["outside"] == true))
                    );
                }
                decisions.push(json!({"field":mask["field"],"decision":{"action":"replace",
                    "target_pattern":target_pattern,"fingerprint":preview["fingerprint"],"acknowledge_visible_outside":true}}));
            }
            assert_eq!(decisions.len(), 3);
            setup_input["pattern_choices"] = json!(decisions);
        }
    }
    let (status, growth) = app
        .call(
            "POST",
            &format!("{path}/preview-growth"),
            Some(setup_input.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "clone growth: {}", growth["code"]);
    assert_eq!(growth["after"].as_array().map(Vec::len), Some(3));
    setup_input["growth_fingerprint"] = growth["fingerprint"].clone();
    let (status, branch) = app.call("POST", path, Some(setup_input)).await;
    assert_eq!(status, StatusCode::OK, "clone save: {}", branch["code"]);
    assert!(
        branch["blocked"].is_null(),
        "clone blocked: {}",
        branch["blocked"]
    );
    assert_eq!(branch["missing_contracts"], json!([]));
    let id = branch["clone_id"]
        .as_str()
        .unwrap_or_else(|| panic!("saved clone identifier"));
    (format!("{path}/{id}/plan"), branch["kept_digest"].clone())
}

#[derive(Clone, Copy)]
enum FixtureKind {
    Basic,
    Extended,
    Composite,
    MultiWorkcopy,
    ReplaceHost,
}

async fn connect_lab(
    kind: FixtureKind,
) -> (
    App,
    Arc<Lab>,
    tokio::task::JoinHandle<std::io::Result<()>>,
    String,
    Value,
) {
    let (ack, title_prefix) = match kind {
        FixtureKind::Extended => (
            "exact-8.0.0-extended-clone-fixture",
            "ZDashstitch Clone Extended ",
        ),
        FixtureKind::Basic => ("exact-8.0.0-clone-fixture", "ZDashstitch clone 8.0 "),
        FixtureKind::Composite => (
            "exact-8.0.0-composite-fixture",
            "ZDashstitch Composite Clone ",
        ),
        FixtureKind::MultiWorkcopy => (
            "exact-8.0.0-workcopy-multi-fixture",
            "ZDashstitch Workcopy Multi ",
        ),
        FixtureKind::ReplaceHost => ("exact-8.0.0-replace-host-fixture", "ZDashstitch Z80-P2-03 "),
    };
    assert_eq!(env("ZDS_CLONE_ACK"), ack);
    let upstream =
        reqwest::Url::parse(&env("ZDS_CLONE_URL")).unwrap_or_else(|_| panic!("lab loopback URL"));
    assert_eq!(upstream.host_str(), Some("127.0.0.1"));
    assert!(upstream.username().is_empty() && upstream.password().is_none());
    assert_eq!(upstream.path(), "/zabbix/api_jsonrpc.php");
    let fixture: Value =
        serde_json::from_str(&env("ZDS_CLONE_FIXTURE")).unwrap_or_else(|_| panic!("lab fixture"));
    let title = fixture["title"]
        .as_str()
        .unwrap_or_else(|| panic!("lab title"))
        .to_owned();
    assert!(title.starts_with(title_prefix));
    let source = fixture["source"]
        .as_str()
        .unwrap_or_else(|| panic!("lab source"))
        .to_owned();
    assert!(zdashstitch_zabbix::is_zabbix_id(&source));
    let lab = Arc::new(Lab {
        upstream,
        client: reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| panic!("lab HTTP client")),
        title: title.clone(),
        source: source.clone(),
        sends: AtomicUsize::new(0),
        drop_response: AtomicBool::new(false),
        hide_copy: AtomicBool::new(false),
        stale: AtomicBool::new(false),
        role_revoked: AtomicBool::new(false),
        extended: (!matches!(kind, FixtureKind::Basic)).then(|| fixture.clone()),
        changed_evidence: AtomicUsize::new(0),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|_| panic!("proxy listener"));
    let address = format!(
        "http://{}/zabbix",
        listener
            .local_addr()
            .unwrap_or_else(|_| panic!("proxy port"))
    );
    let proxy = Router::new()
        .route("/zabbix/api_jsonrpc.php", post(proxy))
        .with_state(lab.clone());
    let proxy_task = tokio::spawn(async move { axum::serve(listener, proxy).await });
    let app = App::start(default_script()).await;
    let (status, profile) = app.call("POST", "/api/connections", Some(json!({
        "name":"Zabbix 8.0.0 Clone laboratory", "address":address,
        "auth":{"method":"password","username":env("ZDS_CLONE_USER")}, "trust":{"kind":"system"}
    }))).await;
    assert_eq!(status, StatusCode::CREATED);
    let connection = profile["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("connection identifier"))
        .to_owned();
    let prefix = format!("/api/connections/{connection}");
    let (status, response) = app
        .call(
            "POST",
            &format!("{prefix}/connect"),
            Some(json!({"secret":env("ZDS_CLONE_PASSWORD")})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "connect: {}", response["code"]);
    (app, lab, proxy_task, connection, fixture)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a dedicated, explicitly authorized Zabbix 8.0.0 full Clone fixture"]
async fn lab_z80_clone_http() {
    let (mut app, lab, proxy_task, connection, fixture) = connect_lab(FixtureKind::Basic).await;
    let connection = connection.as_str();
    let title = &lab.title;
    let source = &lab.source;
    let super_admin = fixture["super_admin"] == true;
    let prefix = format!("/api/connections/{connection}");
    let (status, imported) = app
        .call(
            "POST",
            &format!("/api/server/{connection}/dashboards/{source}/import"),
            Some(json!({"scope":"dashboard"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "import: {}", imported["code"]);
    let local = imported["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("source workcopy"));
    let path = format!("/api/drafts/{local}/clone");
    let confirm = format!("/api/drafts/{local}/publication/confirm");
    let (private_path, kept) = stage(&app, &path, &format!("{title} HTTP private"), &fixture).await;
    let private_input = json!({"kept_digest":kept,"copy_source_access":false});
    assert_ne!(
        app.call("POST", &private_path, Some(private_input.clone()))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(lab.sends.load(Ordering::SeqCst), 0);
    assert_eq!(
        app.call(
            "POST",
            &format!("{prefix}/enable-writes"),
            Some(json!({"confirmed":true}))
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, plan) = app
        .call("POST", &private_path, Some(private_input.clone()))
        .await;
    assert_eq!(status, StatusCode::OK, "private plan: {}", plan["code"]);
    assert!(plan["access"].is_null());
    let confirmation = |plan: &Value, kept: &Value, ack| {
        json!({
            "handle":plan["handle"], "plan_id":plan["plan_id"],"name":plan["name"],
            "kept_digest":kept,"confirmed":true,"acknowledge_public_access":ack
        })
    };
    if !super_admin {
        lab.stale.store(true, Ordering::SeqCst);
        let (_, stale) = app
            .call("POST", &confirm, Some(confirmation(&plan, &kept, false)))
            .await;
        lab.stale.store(false, Ordering::SeqCst);
        assert_eq!(stale["invalidated"], "source_changed");
        let (status, role_plan) = app
            .call("POST", &private_path, Some(private_input.clone()))
            .await;
        assert_eq!(status, StatusCode::OK, "role plan: {}", role_plan["code"]);
        lab.role_revoked.store(true, Ordering::SeqCst);
        let (_, denied) = app
            .call(
                "POST",
                &confirm,
                Some(confirmation(&role_plan, &kept, false)),
            )
            .await;
        lab.role_revoked.store(false, Ordering::SeqCst);
        assert_ne!(denied["sent"], true);
        assert_eq!(lab.sends.load(Ordering::SeqCst), 0);
    }
    let (_, plan) = app.call("POST", &private_path, Some(private_input)).await;
    let body = confirmation(&plan, &kept, false);
    let (_, verified) = app.call("POST", &confirm, Some(body.clone())).await;
    assert_eq!(
        verified["state"], "verified",
        "private create code: {}",
        verified["code"]
    );
    assert!(verified["record_error"].is_null());
    assert_eq!(verified["clone_source_check"], "current");
    assert_eq!(
        app.call("POST", &confirm, Some(body)).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(lab.sends.load(Ordering::SeqCst), 1);

    let (sharing_path, kept) = stage(&app, &path, &format!("{title} HTTP sharing"), &fixture).await;
    let sharing_input = json!({"kept_digest":kept});
    let (status, plan) = app
        .call("POST", &sharing_path, Some(sharing_input.clone()))
        .await;
    assert_eq!(status, StatusCode::OK, "sharing plan: {}", plan["code"]);
    assert_eq!(plan["access_visible_only"], !super_admin);
    assert_eq!(plan["access"]["private"], false);
    assert_eq!(
        plan["access"]["users"].as_array().map(Vec::len),
        Some(if super_admin { 2 } else { 1 })
    );
    assert_eq!(
        plan["access"]["groups"].as_array().map(Vec::len),
        Some(if super_admin { 2 } else { 1 })
    );
    assert_eq!(
        app.call("POST", &confirm, Some(confirmation(&plan, &kept, false)))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(lab.sends.load(Ordering::SeqCst), 1);
    let (status, plan) = app.call("POST", &sharing_path, Some(sharing_input)).await;
    assert_eq!(status, StatusCode::OK, "sharing replan: {}", plan["code"]);
    lab.drop_response.store(true, Ordering::SeqCst);
    let body = confirmation(&plan, &kept, true);
    let (_, unknown) = app.call("POST", &confirm, Some(body.clone())).await;
    assert_eq!(unknown["state"], "unknown_outcome");
    assert_eq!(
        app.call("POST", &confirm, Some(body)).await.0,
        StatusCode::NOT_FOUND
    );
    lab.hide_copy.store(false, Ordering::SeqCst);
    restart(&mut app, connection).await;
    let operation = plan["plan_id"]
        .as_str()
        .unwrap_or_else(|| panic!("operation ID"));
    let reconcile = format!("/api/publication/journal/{operation}/reconcile");
    for attempt in 0..2 {
        let (status, recovered) = app.call("POST", &reconcile, Some(json!({}))).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "reconciliation: {}",
            recovered["code"]
        );
        assert_eq!(recovered["state"], "reconciled_verified");
        assert!(recovered["record_error"].is_null());
        if attempt == 0 {
            assert_eq!(recovered["clone_source_check"], "current");
        }
    }
    let path = format!("{}/comparison", sharing_path.trim_end_matches("/plan"));
    let (status, comparison) = app.call("GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "comparison: {}", comparison["code"]);
    assert_eq!(comparison["observation"], "verified_read_back");
    assert_eq!(comparison["access"][1], comparison["access"][2]);
    assert_eq!(comparison["access"][1]["private"], false);
    assert_eq!(lab.sends.load(Ordering::SeqCst), 2);
    proxy_task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires dedicated 8.0.0 LLD, dynamic mask and owner-sharing laboratory fixtures"]
async fn lab_z80_extended_clone_http() {
    let (mut app, lab, proxy_task, connection, fixture) = connect_lab(FixtureKind::Extended).await;
    let title = &lab.title;
    let super_admin = fixture["super_admin"] == true;
    let enable = format!("/api/connections/{connection}/enable-writes");
    let admit = async |app: &App| {
        let (status, response) = app
            .call("POST", &enable, Some(json!({"confirmed":true})))
            .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "enable writes: {}",
            response["code"]
        );
    };
    let import = async |app: &App, source: &Value| {
        let source = source
            .as_str()
            .unwrap_or_else(|| panic!("source fixture ID"));
        let (status, imported) = app
            .call(
                "POST",
                &format!("/api/server/{connection}/dashboards/{source}/import"),
                Some(json!({"scope":"dashboard"})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "import: {}", imported["code"]);
        imported["local_id"]
            .as_str()
            .unwrap_or_else(|| panic!("workcopy ID"))
            .to_owned()
    };
    let clone_local = import(&app, &fixture["source"]).await;
    let clone_path = format!("/api/drafts/{clone_local}/clone");
    let clone_confirm = format!("/api/drafts/{clone_local}/publication/confirm");
    let (plan_path, digest) = stage(
        &app,
        &clone_path,
        &format!("{title} HTTP extended"),
        &fixture,
    )
    .await;
    let request = json!({"kept_digest":digest,"copy_source_access":false});
    assert_ne!(
        app.call("POST", &plan_path, Some(request.clone())).await.0,
        StatusCode::OK
    );
    admit(&app).await;
    if fixture["over_cap"] == true {
        let (status, blocked) = app.call("POST", &plan_path, Some(request)).await;
        assert_ne!(
            status,
            StatusCode::OK,
            "51 rows must exceed the renderer bound"
        );
        assert_eq!(lab.sends.load(Ordering::SeqCst), 0);
        assert_eq!(blocked["code"], "write_contract_unverified");
        proxy_task.abort();
        return;
    }
    let plan = async |app: &App, url: &str, input: Value| {
        let (status, response) = app.call("POST", url, Some(input)).await;
        assert_eq!(status, StatusCode::OK, "plan: {}", response["code"]);
        response
    };
    let clone_body = |plan: &Value, digest: Value| {
        json!({
            "handle":plan["handle"],"plan_id":plan["plan_id"],"name":plan["name"],"kept_digest":digest,"confirmed":true
        })
    };
    for variant in [1, 2, 3, 4, 6, 7] {
        if matches!(variant, 2 | 6 | 7) && !super_admin {
            continue;
        }
        let held = plan(&app, &plan_path, request.clone()).await;
        lab.changed_evidence.store(variant, Ordering::SeqCst);
        let (_, refused) = app
            .call(
                "POST",
                &clone_confirm,
                Some(clone_body(&held, digest.clone())),
            )
            .await;
        lab.changed_evidence.store(0, Ordering::SeqCst);
        assert_ne!(refused["sent"], true);
        assert_eq!(
            lab.sends.load(Ordering::SeqCst),
            0,
            "stale evidence variant {variant}"
        );
    }
    let held = plan(&app, &plan_path, request.clone()).await;
    lab.role_revoked.store(true, Ordering::SeqCst);
    let (_, refused) = app
        .call(
            "POST",
            &clone_confirm,
            Some(clone_body(&held, digest.clone())),
        )
        .await;
    lab.role_revoked.store(false, Ordering::SeqCst);
    assert_ne!(refused["sent"], true);
    assert_eq!(lab.sends.load(Ordering::SeqCst), 0);

    let held = plan(&app, &plan_path, request).await;
    let body = clone_body(&held, digest);
    let (_, verified) = app.call("POST", &clone_confirm, Some(body.clone())).await;
    assert_eq!(
        verified["state"], "verified",
        "Clone evidence: {}",
        verified["evidence"]
    );
    assert_eq!(verified["clone_source_check"], "current");
    assert_eq!(
        app.call("POST", &clone_confirm, Some(body)).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(lab.sends.load(Ordering::SeqCst), 1);

    let (lost_path, digest) = stage(
        &app,
        &clone_path,
        &format!("{title} HTTP extended lost"),
        &fixture,
    )
    .await;
    let held = plan(
        &app,
        &lost_path,
        json!({"kept_digest":digest,"copy_source_access":false}),
    )
    .await;
    lab.drop_response.store(true, Ordering::SeqCst);
    let (_, unknown) = app
        .call("POST", &clone_confirm, Some(clone_body(&held, digest)))
        .await;
    assert_eq!(unknown["state"], "unknown_outcome");
    lab.hide_copy.store(false, Ordering::SeqCst);
    restart(&mut app, &connection).await;
    let reconcile = async |app: &App, operation: &Value| {
        let operation = operation.as_str().unwrap_or_else(|| panic!("journal ID"));
        let (status, result) = app
            .call(
                "POST",
                &format!("/api/publication/journal/{operation}/reconcile"),
                Some(json!({})),
            )
            .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "read-only recovery: {}",
            result["code"]
        );
        assert_eq!(
            result["state"], "reconciled_verified",
            "read-only recovery: {}",
            result["evidence"]
        );
        assert!(result["record_error"].is_null());
    };
    reconcile(&app, &held["plan_id"]).await;
    assert_eq!(lab.sends.load(Ordering::SeqCst), 2);

    let copy_local = import(&app, &fixture["simple"]).await;
    let copy_path = format!("/api/drafts/{copy_local}/publication");
    let (status, mut input) = app
        .call("GET", &format!("{copy_path}/copy-source"), None)
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "saved snapshot revision: {}",
        input["code"]
    );
    input["action"] = json!("save_as_copy");
    input["name"] = json!(format!("{title} HTTP restored"));
    input["restore_source_access"] = json!(true);
    admit(&app).await;
    let url = format!("{copy_path}/plan");
    let confirm = format!("{copy_path}/confirm");
    let restore_body = |held: &Value| {
        json!({
            "handle":held["handle"],"plan_id":held["plan"]["plan_id"],"name":input["name"],"confirmed":true
        })
    };
    let held = plan(&app, &url, input.clone()).await;
    assert_eq!(held["plan"]["access_visible_only"], !super_admin);
    assert_eq!(
        &held["plan"]["access"]["owner"],
        if super_admin {
            &fixture["owner"]
        } else {
            &fixture["actor"]
        }
    );
    assert_eq!(
        held["plan"]["access"]["users"].as_array().map(Vec::len),
        Some(if super_admin { 2 } else { 1 })
    );
    lab.stale.store(true, Ordering::SeqCst);
    let (_, refused) = app.call("POST", &confirm, Some(restore_body(&held))).await;
    lab.stale.store(false, Ordering::SeqCst);
    assert_ne!(refused["sent"], true);
    if super_admin {
        let held = plan(&app, &url, input.clone()).await;
        lab.changed_evidence.store(5, Ordering::SeqCst);
        let (_, refused) = app.call("POST", &confirm, Some(restore_body(&held))).await;
        lab.changed_evidence.store(0, Ordering::SeqCst);
        assert_ne!(refused["sent"], true);
    }
    assert_eq!(lab.sends.load(Ordering::SeqCst), 2);
    let held = plan(&app, &url, input.clone()).await;
    lab.drop_response.store(true, Ordering::SeqCst);
    let body = restore_body(&held);
    let (_, unknown) = app.call("POST", &confirm, Some(body.clone())).await;
    assert_eq!(unknown["state"], "unknown_outcome");
    assert_eq!(
        app.call("POST", &confirm, Some(body)).await.0,
        StatusCode::NOT_FOUND
    );
    lab.hide_copy.store(false, Ordering::SeqCst);
    restart(&mut app, &connection).await;
    reconcile(&app, &held["plan"]["plan_id"]).await;
    reconcile(&app, &held["plan"]["plan_id"]).await;
    assert_eq!(lab.sends.load(Ordering::SeqCst), 3);
    proxy_task.abort();
}

async fn composite_clone_stage(
    app: &App,
    path: &str,
    lab: &Lab,
    fixture: &Value,
) -> (String, Value) {
    let (status, setup) = app.call("GET", path, None).await;
    assert_eq!(status, StatusCode::OK, "clone setup: {}", setup["code"]);
    let mut input = json!({
        "revision":setup["revision"],"source_fingerprint":setup["source_fingerprint"],
        "name":format!("{} HTTP composite", lab.title),"hosts":fixture["mapping"]
    });
    let (status, review) = app
        .call(
            "POST",
            &format!("{path}/review-bindings"),
            Some(input.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "LLD review: {}", review["code"]);
    let rows = review["lld_rows"]
        .as_array()
        .unwrap_or_else(|| panic!("LLD rows"));
    assert_eq!(rows.len(), 2);
    let items = fixture["target_items"]
        .as_array()
        .unwrap_or_else(|| panic!("target items"));
    let choices: Vec<_> = rows.iter().zip(items).enumerate().map(|(index, (row, item))| {
        assert!(row["field"]["field_name"].as_str().is_some_and(|name| name.ends_with(&format!(".{index}"))));
        assert!(row["candidates"].as_array().is_some_and(|rows| rows.iter().any(|row| row["itemid"] == *item)));
        json!({"field":row["field"],"slot_id":row["slot_id"],"itemid":item,"fingerprint":row["fingerprint"]})
    }).collect();
    input["lld_choices"] = json!(choices);
    let mut decisions = vec![];
    for mask in setup["patterns"].as_array().into_iter().flatten() {
        let field_name = mask["field"]["field_name"]
            .as_str()
            .unwrap_or_else(|| panic!("mask field"));
        let target_pattern = match field_name {
            "ds.0.hosts.0" | "ds.1.hosts.0" | "problemhosts.0" => {
                fixture["target_names"][0].clone()
            }
            "ds.0.hosts.1" | "ds.1.hosts.1" => fixture["target_names"][1].clone(),
            "ds.0.items.0" | "or.0.items.0" => fixture["masks"][0].clone(),
            "ds.1.items.0" => fixture["masks"][1].clone(),
            "or.0.hosts.0" => json!("*"),
            _ => panic!("unexpected mask field"),
        };
        let (status, preview) = app
            .call(
                "POST",
                &format!("{path}/preview-pattern"),
                Some(json!({"setup":input,"field":mask["field"],"target_pattern":target_pattern})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "mask review: {}", preview["code"]);
        assert_eq!(preview["truncated"], false);
        if field_name == "ds.0.items.0" {
            assert!(
                preview["matches"]
                    .as_array()
                    .is_some_and(|rows| rows.iter().any(|row| row["outside"] == true))
            );
        }
        decisions.push(json!({"field":mask["field"],"decision":{"action":"replace",
            "target_pattern":target_pattern,"fingerprint":preview["fingerprint"],"acknowledge_visible_outside":true}}));
    }
    assert_eq!(
        decisions.len(),
        match fixture["variant"]
            .as_str()
            .unwrap_or_else(|| panic!("variant"))
        {
            "lld" => 0,
            "tagged" => 3,
            "override" | "paired" => 6,
            _ => panic!("unexpected variant"),
        }
    );
    input["pattern_choices"] = json!(decisions);
    let (status, branch) = app.call("POST", path, Some(input)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "composite Clone stage: {}",
        branch["code"]
    );
    assert!(
        branch["blocked"].is_null(),
        "clone blocked: {}",
        branch["blocked"]
    );
    let id = branch["clone_id"]
        .as_str()
        .unwrap_or_else(|| panic!("saved clone ID"));
    (format!("{path}/{id}/plan"), branch["kept_digest"].clone())
}

async fn composite_plan(app: &App, path: &str, input: &Value) -> Value {
    let (status, result) = app.call("POST", path, Some(input.clone())).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "plan: {} {}",
        result["code"],
        result["params"]
    );
    result
}

async fn composite_reconcile(app: &App, id: &Value) {
    let id = id.as_str().unwrap_or_else(|| panic!("journal ID"));
    let (status, result) = app
        .call(
            "POST",
            &format!("/api/publication/journal/{id}/reconcile"),
            Some(json!({})),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "restart reconcile: {}",
        result["code"]
    );
    assert_eq!(
        result["state"], "reconciled_verified",
        "restart reconcile: {}",
        result["evidence"]
    );
    assert!(result["record_error"].is_null());
}

async fn composite_clone_http(
    app: &mut App,
    lab: &Lab,
    connection: &str,
    local: &str,
    fixture: &Value,
) {
    let path = format!("/api/drafts/{local}/clone");
    let confirm = format!("/api/drafts/{local}/publication/confirm");
    let (url, digest) = composite_clone_stage(app, &path, lab, fixture).await;
    let input = json!({"kept_digest":digest,"copy_source_access":false});
    if fixture["mode"] == "paired-cap" {
        let (status, blocked) = app.call("POST", &url, Some(input)).await;
        assert_ne!(status, StatusCode::OK);
        assert_eq!(blocked["code"], "write_contract_unverified");
        assert_eq!(lab.sends.load(Ordering::SeqCst), 0);
        return;
    }
    let body = |held: &Value| {
        json!({
            "handle":held["handle"],"plan_id":held["plan_id"],"name":held["name"],
            "kept_digest":digest,"confirmed":true
        })
    };
    for evidence in [1, 3, 4, 6, 7] {
        if matches!(evidence, 6 | 7) && fixture["variant"] == "lld" {
            continue;
        }
        let held = composite_plan(app, &url, &input).await;
        lab.changed_evidence.store(evidence, Ordering::SeqCst);
        let (_, refused) = app.call("POST", &confirm, Some(body(&held))).await;
        lab.changed_evidence.store(0, Ordering::SeqCst);
        assert_ne!(refused["sent"], true);
        assert_eq!(lab.sends.load(Ordering::SeqCst), 0, "variant {evidence}");
    }
    let held = composite_plan(app, &url, &input).await;
    lab.role_revoked.store(true, Ordering::SeqCst);
    let (_, refused) = app.call("POST", &confirm, Some(body(&held))).await;
    lab.role_revoked.store(false, Ordering::SeqCst);
    assert_ne!(refused["sent"], true);
    assert_eq!(lab.sends.load(Ordering::SeqCst), 0);
    let held = composite_plan(app, &url, &input).await;
    let lose = matches!(fixture["variant"].as_str(), Some("lld" | "paired"));
    lab.drop_response.store(lose, Ordering::SeqCst);
    let (_, result) = app.call("POST", &confirm, Some(body(&held))).await;
    if lose {
        assert_eq!(
            result["state"], "unknown_outcome",
            "lost: {}",
            result["evidence"]
        );
        lab.hide_copy.store(false, Ordering::SeqCst);
        restart(app, connection).await;
        composite_reconcile(app, &held["plan_id"]).await;
    } else {
        assert_eq!(result["state"], "verified", "copy: {}", result["evidence"]);
        assert_eq!(result["clone_source_check"], "current");
    }
    assert_eq!(lab.sends.load(Ordering::SeqCst), 1);
}

async fn edited_workcopy_copy_http(
    app: &mut App,
    lab: &Lab,
    connection: &str,
    local: &str,
    fixture: &Value,
) {
    let path = format!("/api/drafts/{local}");
    if fixture.get("empty_source").is_some() {
        let (status, empty) = app.call("POST", &format!("{path}/replace-host/calculate"), Some(json!({
            "source_hostid":fixture["empty_source"],"target_hostid":fixture["ordinary_target"],
            "scope":{"kind":"dashboard"}
        }))).await;
        assert_eq!(status, StatusCode::OK, "calculate no-op: {}", empty["code"]);
        assert_eq!(empty["base_fingerprint"], empty["local_fingerprint"]);
        let input = json!({
            "revision":empty["revision"],"base_fingerprint":empty["base_fingerprint"],
            "local_fingerprint":empty["local_fingerprint"],"action":"save_as_copy",
            "name":format!("{} HTTP no-op", lab.title)
        });
        let (status, refused) = app
            .call("POST", &format!("{path}/publication/plan"), Some(input))
            .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(refused["params"]["reason"], "bindings_changed");
        assert_eq!(lab.sends.load(Ordering::SeqCst), 0);
        let seq = empty["operation"]["seq"]
            .as_u64()
            .unwrap_or_else(|| panic!("no-op operation ID"));
        assert_eq!(
            app.call(
                "POST",
                &format!("{path}/replace-host/operations/{seq}/remove"),
                Some(json!({}))
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    let (status, calculation) = app.call("POST", &format!("{path}/replace-host/calculate"), Some(json!({
        "source_hostid":fixture["ordinary_source"],"target_hostid":fixture["ordinary_target"],
        "scope":{"kind":"dashboard"}
    }))).await;
    assert_eq!(status, StatusCode::OK, "calculate: {}", calculation["code"]);
    assert_ne!(
        calculation["base_fingerprint"],
        calculation["local_fingerprint"]
    );
    assert_eq!(
        calculation["changes"].as_array().map(Vec::len),
        Some(fixture["expected_changes"].as_u64().unwrap_or(1) as usize)
    );
    let publication = format!("{path}/publication");
    let (status, mut input) = app
        .call("GET", &format!("{publication}/copy-source"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "copy source: {}", input["code"]);
    assert_eq!(input["base_fingerprint"], calculation["base_fingerprint"]);
    assert_eq!(input["local_fingerprint"], calculation["local_fingerprint"]);
    input["action"] = json!("save_as_copy");
    let target_name = format!("{} HTTP restored", lab.title);
    input["name"] = json!(target_name);
    input["restore_source_access"] = json!(true);
    let url = format!("{publication}/plan");
    let confirm = format!("{publication}/confirm");
    let body = |held: &Value| {
        json!({
            "handle":held["handle"],"plan_id":held["plan"]["plan_id"],"name":target_name,"confirmed":true
        })
    };
    let super_admin = fixture["super_admin"] == true;
    let held = composite_plan(app, &url, &input).await;
    assert_eq!(held["plan"]["access_visible_only"], !super_admin);
    assert_eq!(held["plan"]["access"]["owner"], fixture["owner"]);
    assert_eq!(
        held["plan"]["access"]["users"].as_array().map(Vec::len),
        Some(if super_admin { 2 } else { 1 })
    );
    if fixture["mode"] == "save-direct" || fixture["direct"] == true {
        let (_, result) = app.call("POST", &confirm, Some(body(&held))).await;
        assert_eq!(
            result["state"], "verified",
            "read-back: {}",
            result["evidence"]
        );
        assert_eq!(result["clone_source_check"], "current");
        assert!(result["record_error"].is_null());
        assert_eq!(lab.sends.load(Ordering::SeqCst), 1);
        return;
    }
    let (status, renamed) = app
        .call(
            "POST",
            &format!("{path}/rename"),
            Some(json!({"name":"Edited copy revision"})),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "change local revision: {}",
        renamed["code"]
    );
    let (_, refused) = app.call("POST", &confirm, Some(body(&held))).await;
    assert_ne!(refused["sent"], true);
    let (status, refreshed) = app
        .call("GET", &format!("{publication}/copy-source"), None)
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "refresh copy source: {}",
        refreshed["code"]
    );
    assert_ne!(input["revision"], refreshed["revision"]);
    input["revision"] = refreshed["revision"].clone();
    let held = composite_plan(app, &url, &input).await;
    lab.stale.store(true, Ordering::SeqCst);
    let (_, refused) = app.call("POST", &confirm, Some(body(&held))).await;
    lab.stale.store(false, Ordering::SeqCst);
    assert_ne!(refused["sent"], true);
    for evidence in [8, 5] {
        if evidence == 5 && !super_admin {
            continue;
        }
        let held = composite_plan(app, &url, &input).await;
        lab.changed_evidence.store(evidence, Ordering::SeqCst);
        let (_, refused) = app.call("POST", &confirm, Some(body(&held))).await;
        lab.changed_evidence.store(0, Ordering::SeqCst);
        assert_ne!(refused["sent"], true);
        assert_eq!(lab.sends.load(Ordering::SeqCst), 0);
    }
    let held = composite_plan(app, &url, &input).await;
    lab.drop_response.store(true, Ordering::SeqCst);
    let (_, result) = app.call("POST", &confirm, Some(body(&held))).await;
    assert_eq!(
        result["state"], "unknown_outcome",
        "lost: {}",
        result["evidence"]
    );
    assert_eq!(
        app.call("POST", &confirm, Some(body(&held))).await.0,
        StatusCode::NOT_FOUND
    );
    lab.hide_copy.store(false, Ordering::SeqCst);
    restart(app, connection).await;
    composite_reconcile(app, &held["plan"]["plan_id"]).await;
    composite_reconcile(app, &held["plan"]["plan_id"]).await;
    assert_eq!(lab.sends.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the authorized 8.0.0 composite Clone and edited copy laboratory fixtures"]
async fn lab_z80_composite_clone_http() {
    let (mut app, lab, proxy_task, connection, fixture) = connect_lab(FixtureKind::Composite).await;
    let saving = matches!(
        fixture["mode"].as_str(),
        Some("save-admin" | "save-super" | "save-direct")
    );
    let source = if saving {
        &fixture["simple"]
    } else {
        &fixture["source"]
    };
    let source = source.as_str().unwrap_or_else(|| panic!("source ID"));
    let (status, imported) = app
        .call(
            "POST",
            &format!("/api/server/{connection}/dashboards/{source}/import"),
            Some(json!({"scope":"dashboard"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "import: {}", imported["code"]);
    let local = imported["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("workcopy ID"));
    let (status, enabled) = app
        .call(
            "POST",
            &format!("/api/connections/{connection}/enable-writes"),
            Some(json!({"confirmed":true})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable writes: {}", enabled["code"]);
    if saving {
        edited_workcopy_copy_http(&mut app, &lab, &connection, local, &fixture).await;
    } else {
        composite_clone_http(&mut app, &lab, &connection, local, &fixture).await;
    }
    proxy_task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the authorized 8.0.0 multi-field ReplaceHost laboratory fixture"]
async fn lab_z80_multi_workcopy_http() {
    let (mut app, lab, proxy_task, connection, fixture) =
        connect_lab(FixtureKind::MultiWorkcopy).await;
    let source = fixture["source"]
        .as_str()
        .unwrap_or_else(|| panic!("source ID"));
    let (status, imported) = app
        .call(
            "POST",
            &format!("/api/server/{connection}/dashboards/{source}/import"),
            Some(json!({"scope":"dashboard"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "import: {}", imported["code"]);
    let local = imported["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("workcopy ID"));
    let (status, enabled) = app
        .call(
            "POST",
            &format!("/api/connections/{connection}/enable-writes"),
            Some(json!({"confirmed":true})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable writes: {}", enabled["code"]);
    edited_workcopy_copy_http(&mut app, &lab, &connection, local, &fixture).await;
    proxy_task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the authorized 8.0.0 multi-LLD and explicit ordering laboratory fixture"]
async fn lab_z80_multi_clone_http() {
    let (mut app, lab, proxy_task, connection, fixture) =
        connect_lab(FixtureKind::MultiWorkcopy).await;
    let source = fixture["source"]
        .as_str()
        .unwrap_or_else(|| panic!("source ID"));
    let (status, imported) = app
        .call(
            "POST",
            &format!("/api/server/{connection}/dashboards/{source}/import"),
            Some(json!({"scope":"dashboard"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "import: {}", imported["code"]);
    let local = imported["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("workcopy ID"));
    let clone = format!("/api/drafts/{local}/clone");
    let (status, setup) = app.call("GET", &clone, None).await;
    assert_eq!(status, StatusCode::OK, "clone setup: {}", setup["code"]);
    let mut input = json!({
        "revision":setup["revision"],"source_fingerprint":setup["source_fingerprint"],
        "name":format!("{} HTTP explicit order", lab.title),"hosts":fixture["mapping"]
    });
    let (status, review) = app
        .call(
            "POST",
            &format!("{clone}/review-bindings"),
            Some(input.clone()),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "review bindings: {}",
        review["code"]
    );
    let rows = review["lld_rows"]
        .as_array()
        .unwrap_or_else(|| panic!("LLD rows"));
    assert_eq!(rows.len(), 3);
    let expected = fixture["expected_lld"]
        .as_object()
        .unwrap_or_else(|| panic!("expected LLD order"));
    let mut used = BTreeSet::new();
    let choices:Vec<_> = rows.iter().map(|row|{
        let field = row["field"]["field_name"].as_str().unwrap_or_else(|| panic!("LLD field name"));
        assert!(used.insert(field));
        let item = expected.get(field).unwrap_or_else(|| panic!("unexpected dataset slot"));
        assert!(row["candidates"].as_array().is_some_and(|items| items.iter().any(|candidate| candidate["itemid"]==*item)));
        json!({"field":row["field"],"slot_id":row["slot_id"],"itemid":item,"fingerprint":row["fingerprint"]})
    }).collect();
    input["lld_choices"] = json!(choices);
    let (status, branch) = app.call("POST", &clone, Some(input)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "stage Clone: {} {}",
        branch["code"],
        branch["params"]
    );
    assert!(
        branch["blocked"].is_null(),
        "blocked: {}",
        branch["blocked"]
    );
    let clone_id = branch["clone_id"]
        .as_str()
        .unwrap_or_else(|| panic!("saved clone"));
    let (status, enabled) = app
        .call(
            "POST",
            &format!("/api/connections/{connection}/enable-writes"),
            Some(json!({"confirmed":true})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable writes: {}", enabled["code"]);
    let request = json!({"kept_digest":branch["kept_digest"],"copy_source_access":false});
    let plan_path = format!("{clone}/{clone_id}/plan");
    let confirm = format!("/api/drafts/{local}/publication/confirm");
    let body = |held: &Value| {
        json!({"handle":held["handle"],"plan_id":held["plan_id"],
        "name":held["name"],"kept_digest":branch["kept_digest"],"confirmed":true})
    };
    let held = composite_plan(&app, &plan_path, &request).await;
    lab.changed_evidence.store(9, Ordering::SeqCst);
    let (_, refused) = app.call("POST", &confirm, Some(body(&held))).await;
    lab.changed_evidence.store(0, Ordering::SeqCst);
    assert_ne!(refused["sent"], true);
    assert_eq!(lab.sends.load(Ordering::SeqCst), 0);
    let held = composite_plan(&app, &plan_path, &request).await;
    let lose = fixture["variant"] == "reverse";
    lab.drop_response.store(lose, Ordering::SeqCst);
    let (_, result) = app.call("POST", &confirm, Some(body(&held))).await;
    if lose {
        assert_eq!(
            result["state"], "unknown_outcome",
            "lost: {}",
            result["evidence"]
        );
        lab.hide_copy.store(false, Ordering::SeqCst);
        restart(&mut app, &connection).await;
        composite_reconcile(&app, &held["plan_id"]).await;
    } else {
        assert_eq!(
            result["state"], "verified",
            "read-back: {}",
            result["evidence"]
        );
        assert_eq!(result["clone_source_check"], "current");
    }
    assert_eq!(lab.sends.load(Ordering::SeqCst), 1);
    proxy_task.abort();
}
