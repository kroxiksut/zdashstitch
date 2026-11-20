// SPDX-License-Identifier: MIT OR Apache-2.0

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;
use zdashstitch_application::offline_dashboards::{OfflineDashboard, OfflineDashboardStore};

struct Observer {
    client: reqwest::Client,
    upstream: reqwest::Url,
    items: Vec<String>,
    replacement_items: Vec<String>,
    masks: Option<Value>,
    selected_count: usize,
    composite: bool,
    composite_pages: usize,
    geometry: bool,
    title: String,
    sends: AtomicUsize,
    hide_reads: AtomicBool,
    stale_target: AtomicBool,
    update_scope: StdMutex<Option<(String, u8)>>,
    updates: AtomicUsize,
    sharing: Option<SharingFixture>,
    role_changed: AtomicBool,
}

struct SharingFixture {
    recipient: String,
    group: String,
    hidden: String,
    hidden_group: String,
    super_admin: bool,
    audit_session: reqwest::header::HeaderValue,
}

fn scoped_graph(widget: &Value, items: &[String], masks: Option<&Value>) -> bool {
    let Some(fields) = widget["fields"].as_array() else {
        return false;
    };
    if let Some(expected) = masks.and_then(|data| data["fields"].as_array()) {
        let canonical = |rows: &[Value]| {
            let mut result: Vec<_> = rows.iter().map(Value::to_string).collect();
            result.sort_unstable();
            result
        };
        return widget["type"] == "svggraph" && canonical(fields) == canonical(expected);
    }
    let expected_field_count = if masks.is_some() {
        5
    } else {
        1 + items.len() * 2
    };
    if widget["type"] != "svggraph" || fields.len() != expected_field_count {
        return false;
    }
    if let Some(masks) = masks {
        return ["ds.0.hosts.0", "ds.0.hosts.1", "ds.0.items.0"]
            .iter()
            .enumerate()
            .all(|(index, key)| {
                fields
                    .iter()
                    .any(|row| row["name"] == *key && row["value"] == masks["patterns"][index])
            });
    }
    items.iter().enumerate().all(|(index, itemid)| {
        fields.iter().any(|row| {
            row["name"] == format!("ds.0.itemids.{index}") && row["value"] == itemid.as_str()
        })
    })
}

async fn forward(
    State(observer): State<Arc<Observer>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let method = body["method"].as_str().unwrap_or_default();
    let current = observer.sends.load(Ordering::SeqCst);
    let mut lost = method == "dashboard.create" && current == 1;
    if method == "dashboard.create" {
        let params = &body["params"];
        let composite = observer.composite && current == 2;
        let suffix = if composite {
            "composite"
        } else if lost {
            "lost"
        } else {
            "verified"
        };
        let widget = &params["pages"][0]["widgets"][0];
        assert!(
            (current < 2 || composite) && params["name"] == format!("{} {suffix}", observer.title)
        );
        if let Some(sharing) = &observer.sharing {
            assert_eq!(params["private"], "0");
            let recipient = if sharing.super_admin {
                &sharing.hidden
            } else {
                &sharing.recipient
            };
            assert_eq!(
                params["users"],
                json!([{"userid":recipient,"permission":"2"}])
            );
            assert_eq!(
                params["userGroups"],
                json!([{"usrgrpid":sharing.group,"permission":"3"}])
            );
            if sharing.super_admin {
                assert_eq!(params["userid"], sharing.recipient);
            } else {
                assert!(params.get("userid").is_none());
            }
        } else {
            assert_eq!(params["private"], "1");
        }
        assert_eq!(
            params["pages"].as_array().map(Vec::len),
            Some(if composite {
                observer.composite_pages
            } else {
                1
            })
        );
        assert_eq!(
            params["pages"][0]["widgets"].as_array().map(Vec::len),
            Some(1)
        );
        assert!(scoped_graph(
            widget,
            if composite {
                &observer.replacement_items
            } else {
                &observer.items
            },
            observer.masks.as_ref()
        ));
        if composite {
            let widgets = params["pages"][1]["widgets"]
                .as_array()
                .unwrap_or_else(|| panic!("composite view"));
            assert_eq!(widgets.len(), observer.selected_count);
            assert!(widgets.iter().all(|widget| scoped_graph(
                widget,
                &observer.replacement_items,
                observer.masks.as_ref()
            )));
        }
        observer.sends.fetch_add(1, Ordering::SeqCst);
    } else if method == "dashboard.update" {
        let (dashboardid, mode) = observer
            .update_scope
            .lock()
            .unwrap_or_else(|_| panic!("update scope"))
            .take()
            .unwrap_or_else(|| panic!("one explicitly authorized laboratory update"));
        let params = &body["params"];
        assert_eq!(params["dashboardid"], dashboardid);
        assert!(
            matches!(mode, 8..=10)
                || params.get("userid").is_none()
                    && params.get("users").is_none()
                    && params.get("userGroups").is_none()
                    && params.get("private").is_none()
        );
        match mode {
            8 | 9 => {
                assert!(observer.sharing.is_some());
                assert_eq!(
                    params,
                    &json!({"dashboardid":dashboardid,"private":if mode == 8 {"0"} else {"1"}})
                );
                lost = mode == 9;
            }
            10 => {
                let sharing = observer
                    .sharing
                    .as_ref()
                    .unwrap_or_else(|| panic!("Super Admin fixture"));
                assert!(sharing.super_admin);
                assert_eq!(
                    params,
                    &json!({
                        "dashboardid":dashboardid,"userid":sharing.hidden,"users":[],
                        "userGroups":[{"usrgrpid":sharing.hidden_group,"permission":"2"}]
                    })
                );
            }
            0 => {
                assert_eq!(params["name"], format!("{} updated", observer.title));
                assert_eq!(params["pages"].as_array().map(Vec::len), Some(1));
                assert!(params["pages"][0]["widgets"][0]["widgetid"].is_string());
                assert!(scoped_graph(
                    &params["pages"][0]["widgets"][0],
                    &observer.replacement_items,
                    observer.masks.as_ref()
                ));
                if observer.geometry {
                    let widget = &params["pages"][0]["widgets"][0];
                    assert_eq!(widget["x"], "4");
                    assert_eq!(widget["y"], "4");
                    assert_eq!(widget["width"], "26");
                    assert_eq!(widget["height"], "7");
                    assert!(
                        widget["fields"].as_array().is_some_and(|fields| fields
                            .iter()
                            .any(|field| field["name"] == "ds.0.color.0"
                                && field["value"] == "654321"))
                    );
                    lost = true;
                }
            }
            1 => {
                assert!(params.get("name").is_none());
                assert_eq!(params["pages"].as_array().map(Vec::len), Some(2));
                assert_eq!(
                    params["pages"][0].as_object().map(serde_json::Map::len),
                    Some(1)
                );
                assert!(scoped_graph(
                    &params["pages"][1]["widgets"][0],
                    &observer.replacement_items,
                    observer.masks.as_ref()
                ));
            }
            2 => {
                assert!(params.get("name").is_none());
                assert_eq!(params["pages"].as_array().map(Vec::len), Some(2));
                assert_eq!(
                    params["pages"][0]["widgets"].as_array().map(Vec::len),
                    Some(2)
                );
                assert_eq!(
                    params["pages"][0]["widgets"][0]
                        .as_object()
                        .map(serde_json::Map::len),
                    Some(1)
                );
                assert!(scoped_graph(
                    &params["pages"][0]["widgets"][1],
                    &observer.replacement_items,
                    observer.masks.as_ref()
                ));
                assert_eq!(params["pages"][0]["widgets"][1]["x"], "18");
                lost = true;
            }
            3 => {
                assert!(params.get("name").is_none());
                assert_eq!(params["pages"].as_array().map(Vec::len), Some(3));
                assert_eq!(
                    params["pages"][0].as_object().map(serde_json::Map::len),
                    Some(1)
                );
                assert!(scoped_graph(
                    &params["pages"][1]["widgets"][0],
                    &observer.replacement_items,
                    observer.masks.as_ref()
                ));
                assert_eq!(
                    params["pages"][2].as_object().map(serde_json::Map::len),
                    Some(1)
                );
            }
            4 => {
                assert!(params.get("name").is_none());
                assert_eq!(params["pages"].as_array().map(Vec::len), Some(4));
                let widgets = params["pages"][3]["widgets"]
                    .as_array()
                    .unwrap_or_else(|| panic!("selected source page"));
                assert_eq!(widgets.len(), observer.selected_count);
                assert!(widgets.iter().all(|widget| scoped_graph(
                    widget,
                    &observer.replacement_items,
                    observer.masks.as_ref()
                )));
            }
            5 => {
                assert!(params.get("name").is_none());
                assert_eq!(params["pages"].as_array().map(Vec::len), Some(4));
                let widgets = params["pages"][0]["widgets"]
                    .as_array()
                    .unwrap_or_else(|| panic!("selected widgets placement"));
                assert_eq!(widgets.len(), 2 + observer.selected_count);
                assert!(widgets[..2].iter().all(|widget| {
                    widget
                        .as_object()
                        .is_some_and(|row| row.len() == 1 && row.contains_key("widgetid"))
                }));
                assert!(widgets[2..].iter().all(|widget| scoped_graph(
                    widget,
                    &observer.replacement_items,
                    observer.masks.as_ref()
                )));
                for (index, widget) in widgets[2..].iter().enumerate() {
                    let (x, y) = proposed_placement(observer.selected_count, index);
                    assert_eq!(widget["x"], x.to_string());
                    assert_eq!(widget["y"], y.to_string());
                }
                lost = true;
            }
            6 => {
                assert_eq!(
                    params["name"],
                    format!("{} composite updated", observer.title)
                );
                let pages = params["pages"]
                    .as_array()
                    .unwrap_or_else(|| panic!("linked pages"));
                assert_eq!(pages.len(), 2);
                for (index, page) in pages.iter().enumerate() {
                    assert_eq!(page["name"], format!("Linked page {}", index + 1));
                    let widgets = page["widgets"]
                        .as_array()
                        .unwrap_or_else(|| panic!("linked mapped widgets"));
                    assert!(widgets.iter().all(|widget| {
                        widget
                            .as_object()
                            .is_some_and(|row| row.len() == 1 && row.contains_key("widgetid"))
                    }));
                }
                lost = true;
            }
            7 => {
                let pages = params["pages"]
                    .as_array()
                    .unwrap_or_else(|| panic!("linked composition"));
                assert_eq!(pages.len(), observer.composite_pages);
                assert_eq!(pages[0].as_object().map(serde_json::Map::len), Some(1));
                let widgets = pages[2]["widgets"]
                    .as_array()
                    .unwrap_or_else(|| panic!("retained and new widgets"));
                assert_eq!(widgets.len(), observer.selected_count);
                assert!(widgets[..widgets.len() - 1].iter().all(|widget| {
                    widget
                        .as_object()
                        .is_some_and(|row| row.len() == 1 && row.contains_key("widgetid"))
                }));
                assert!(scoped_graph(
                    &widgets[widgets.len() - 1],
                    &observer.replacement_items,
                    None
                ));
                assert!(pages[pages.len() - 1].get("dashboard_pageid").is_none());
                assert!(scoped_graph(
                    &pages[pages.len() - 1]["widgets"][0],
                    &observer.replacement_items,
                    None
                ));
                lost = true;
            }
            _ => panic!("unexpected update scope"),
        }
        observer.updates.fetch_add(1, Ordering::SeqCst);
    } else {
        assert!(
            zdashstitch_zabbix::read_only_allowlist().contains(&method),
            "unexpected laboratory method {method}"
        );
        if method == "dashboard.get" && observer.hide_reads.load(Ordering::SeqCst) {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }
    let mut request = observer.client.post(observer.upstream.clone()).json(&body);
    if let Some(authorization) = headers.get(header::AUTHORIZATION) {
        request = request.header(header::AUTHORIZATION, authorization);
    }
    let result = request
        .send()
        .await
        .unwrap_or_else(|_| panic!("lab request"));
    let mut envelope: Value = result
        .json()
        .await
        .unwrap_or_else(|_| panic!("lab envelope"));
    if method == "role.get"
        && observer.role_changed.load(Ordering::SeqCst)
        && let Some(rows) = envelope["result"].as_array_mut()
    {
        for role in rows {
            role["rules"]["api.access"] = json!("0");
        }
    }
    if method == "dashboard.get"
        && observer.stale_target.load(Ordering::SeqCst)
        && body["params"]["selectPages"] == "extend"
        && let Some(dashboard) = envelope["result"]
            .as_array_mut()
            .and_then(|rows| rows.first_mut())
    {
        dashboard["name"] = json!("Independent stale observation");
    }
    if lost {
        assert_eq!(
            envelope["result"]["dashboardids"].as_array().map(Vec::len),
            Some(1)
        );
        observer.hide_reads.store(true, Ordering::SeqCst);
        return StatusCode::BAD_GATEWAY.into_response();
    }
    Json(envelope).into_response()
}

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("missing lab setting {key}"))
}

fn proposed_placement(count: usize, index: usize) -> (usize, usize) {
    match count {
        2 => (36 + 18 * index, 0),
        256 => (4 * (index % 16), 4 * (index / 16)),
        _ => ((3 - index % 4) * 18, 8 + 4 * (index / 4)),
    }
}

fn dynamic_graph_field(name: &str) -> bool {
    let parts: Vec<_> = name.split('.').collect();
    matches!(
        parts.as_slice(),
        ["ds" | "or", _, "hosts" | "items", _] | ["problemhosts", _]
    )
}

async fn bind_sources(
    app: &App,
    local_id: &str,
    revision: u64,
    itemids: &[String],
    items: &[zdashstitch_zabbix::ItemDetails],
    masks: Option<&Value>,
) {
    bind_sources_for(app, local_id, revision, itemids, items, masks, (0, 0)).await;
}

async fn bind_sources_for(
    app: &App,
    local_id: &str,
    revision: u64,
    itemids: &[String],
    items: &[zdashstitch_zabbix::ItemDetails],
    masks: Option<&Value>,
    (page_index, widget_index): (usize, usize),
) {
    if let Some(masks) = masks {
        let targets: Vec<_> = masks["hostids"].as_array().unwrap_or_else(|| panic!("mask targets")).iter().enumerate()
            .map(|(index, id)| json!({"slot_id":format!("slot-{index}"), "host":{"hostid":id,"host":"unused","name":"unused"}}))
            .collect();
        assert!((2..=3).contains(&targets.len()));
        let indices = if let Some(fields) = masks["fields"].as_array() {
            let mut indices = Vec::new();
            for (index, field) in fields.iter().enumerate() {
                let name = field["name"].as_str().unwrap_or_default();
                if dynamic_graph_field(name) {
                    indices.push(index);
                } else if field["type"] == "4" {
                    let itemid = field["value"].as_str().unwrap_or_default();
                    let hostid = &items
                        .iter()
                        .find(|item| item.itemid == itemid)
                        .unwrap_or_else(|| panic!("mixed item"))
                        .hostid;
                    let review = json!({"revision":revision,"page_index":page_index,"widget_index":widget_index,"field_index":index,"hostid":hostid});
                    let (status, preview) = app
                        .call(
                            "POST",
                            &format!("/api/drafts/{local_id}/portable-lld-review"),
                            Some(review.clone()),
                        )
                        .await;
                    assert_eq!(
                        status,
                        StatusCode::OK,
                        "mixed row review: {}",
                        preview["code"]
                    );
                    let (status, confirmed) = app
                        .call(
                            "POST",
                            &format!("/api/drafts/{local_id}/portable-lld-confirm"),
                            Some(json!({
                                "review":review,"itemid":itemid,"fingerprint":preview["fingerprint"]
                            })),
                        )
                        .await;
                    assert_eq!(
                        status,
                        StatusCode::OK,
                        "mixed row confirm: {}",
                        confirmed["code"]
                    );
                }
            }
            indices
        } else {
            vec![1, 2, 3]
        };
        for index in indices {
            let review = json!({"revision":revision,"page_index":page_index,"widget_index":widget_index,"field_index":index,"targets":targets});
            let (status, preview) = app
                .call(
                    "POST",
                    &format!("/api/drafts/{local_id}/portable-pattern-review"),
                    Some(review.clone()),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "mask review: {}", preview["code"]);
            let (status, confirmed) = app.call("POST", &format!("/api/drafts/{local_id}/portable-pattern-confirm"), Some(json!({
                "review":review,"fingerprint":preview["fingerprint"],"acknowledge_visible_outside":true
            }))).await;
            assert_eq!(
                status,
                StatusCode::OK,
                "mask confirm: {}",
                confirmed["code"]
            );
        }
    } else {
        for (index, itemid) in itemids.iter().enumerate() {
            let hostid = &items
                .iter()
                .find(|item| item.itemid == *itemid)
                .unwrap_or_else(|| panic!("fixture binding"))
                .hostid;
            let review = json!({"revision":revision,"page_index":page_index,"widget_index":widget_index,"field_index":index,"hostid":hostid});
            let (status, preview) = app
                .call(
                    "POST",
                    &format!("/api/drafts/{local_id}/portable-lld-review"),
                    Some(review.clone()),
                )
                .await;
            assert_eq!(
                status,
                StatusCode::OK,
                "explicit review: {}",
                preview["code"]
            );
            let (status, confirmed) = app
                .call(
                    "POST",
                    &format!("/api/drafts/{local_id}/portable-lld-confirm"),
                    Some(json!({
                        "review":review,"itemid":itemid,"fingerprint":preview["fingerprint"]
                    })),
                )
                .await;
            assert_eq!(
                status,
                StatusCode::OK,
                "explicit confirm: {}",
                confirmed["code"]
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an authorized Zabbix 8.0.0 laboratory and a dedicated account"]
async fn lab_p5_04_portable_create_http_link_and_recovery() {
    assert_eq!(env("ZDS_P504_ACK"), "private-lab-create");
    let upstream = reqwest::Url::parse(&env("ZDS_P504_URL")).unwrap_or_else(|_| panic!("lab URL"));
    assert_eq!(upstream.host_str(), Some("127.0.0.1"));
    assert!(upstream.username().is_empty() && upstream.password().is_none());
    assert!(upstream.path().ends_with("/api_jsonrpc.php"));
    let title = env("ZDS_P504_TITLE");
    assert!(title.starts_with("ZDashstitch Z80-P5-04 private "));
    let sharing = std::env::var("ZDS_P505_ACK").ok().map(|ack| {
        assert_eq!(ack, "owner-sharing-fixture-8.0.0");
        let mut audit_session = reqwest::header::HeaderValue::from_str(&format!(
            "Bearer {}",
            env("ZDS_P505_AUDIT_SESSION")
        ))
        .unwrap_or_else(|_| panic!("lab authorization header"));
        audit_session.set_sensitive(true);
        let fixture = SharingFixture {
            recipient: env("ZDS_P505_RECIPIENT"),
            group: env("ZDS_P505_GROUP"),
            hidden: env("ZDS_P505_HIDDEN"),
            hidden_group: env("ZDS_P505_HIDDEN_GROUP"),
            super_admin: std::env::var("ZDS_P505_SUPER").as_deref() == Ok("1"),
            audit_session,
        };
        assert!(
            [
                &fixture.recipient,
                &fixture.group,
                &fixture.hidden,
                &fixture.hidden_group
            ]
            .into_iter()
            .all(|id| zdashstitch_zabbix::is_zabbix_id(id))
        );
        fixture
    });
    let ids: Vec<_> = env("ZDS_P504_ITEMS")
        .split(',')
        .map(str::to_owned)
        .collect();
    let replacements: Vec<_> = std::env::var("ZDS_P504_REPLACEMENTS").ok().map_or_else(
        || ids.clone(),
        |ids| ids.split(',').map(str::to_owned).collect(),
    );
    let masks: Option<Value> = std::env::var("ZDS_P504_MASKS")
        .ok()
        .map(|raw| serde_json::from_str(&raw).unwrap_or_else(|_| panic!("lab mask fixture")));
    assert_eq!(ids.len(), 2);
    assert!((2..=3).contains(&replacements.len()));
    let selected_count = std::env::var("ZDS_P504_SELECTED_COUNT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(2);
    assert!(matches!(selected_count, 2 | 8 | 12 | 256));
    let geometry = std::env::var("ZDS_P504_GEOMETRY_HTTP").as_deref() == Ok("1");
    let composite = std::env::var("ZDS_P504_COMPOSITE_HTTP").as_deref() == Ok("1");
    let limits = std::env::var("ZDS_P504_LIMITS_HTTP").as_deref() == Ok("1");
    let composite_pages = if limits {
        std::env::var("ZDS_P504_COMPOSITE_PAGES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(50)
    } else {
        2
    };
    assert!(matches!(composite_pages, 2 | 4 | 50));
    assert!(
        ids.iter()
            .chain(&replacements)
            .all(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
    );
    let observer = Arc::new(Observer {
        client: reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| panic!("lab client")),
        upstream,
        items: ids.clone(),
        replacement_items: replacements.clone(),
        masks: masks.clone(),
        selected_count,
        composite,
        composite_pages,
        geometry,
        title: title.clone(),
        sends: AtomicUsize::new(0),
        hide_reads: AtomicBool::new(false),
        stale_target: AtomicBool::new(false),
        update_scope: StdMutex::new(None),
        updates: AtomicUsize::new(0),
        sharing,
        role_changed: AtomicBool::new(false),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|_| panic!("observer listener"));
    let address = format!(
        "http://{}/zabbix",
        listener
            .local_addr()
            .unwrap_or_else(|_| panic!("observer port"))
    );
    let proxy = Router::new()
        .route("/zabbix/api_jsonrpc.php", post(forward))
        .with_state(observer.clone());
    let proxy_task = tokio::spawn(async move { axum::serve(listener, proxy).await });
    let app = App::start(default_script()).await;
    let (status, profile) = app
        .call(
            "POST",
            "/api/connections",
            Some(json!({
                "name": "P5-04 laboratory", "address": address,
                "auth": {"method": "password", "username": env("ZDS_P504_USER")},
                "trust": {"kind": "system"}
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let connection = profile["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("connection ID"));
    let prefix = format!("/api/connections/{connection}");
    let (status, connected) = app
        .call(
            "POST",
            &format!("{prefix}/connect"),
            Some(json!({"secret":env("ZDS_P504_PASSWORD")})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{connected}");
    let service = app
        .connections
        .service()
        .unwrap_or_else(|| panic!("connections"));
    let guard = service.lock().await;
    let reader = guard.reader().unwrap_or_else(|| panic!("lab reader"));
    assert_eq!(reader.capabilities().version.to_string(), "8.0.0");
    let mut all = ids.clone();
    for replacement in &replacements {
        if !all.contains(replacement) {
            all.push(replacement.clone());
        }
    }
    if let Some(masks) = &masks {
        all = masks["fields"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|field| field["type"] == "4")
            .filter_map(|field| field["value"].as_str().map(str::to_owned))
            .collect();
    }
    let items = reader
        .items_by_ids(&all)
        .await
        .unwrap_or_else(|_| panic!("lab items"));
    let mut refs = Vec::new();
    let mut series = Vec::new();
    let mut catalogs = serde_json::Map::new();
    let catalog_for = |id: &str| {
        format!(
            "metrics-{}",
            all.iter()
                .position(|item| item == id)
                .unwrap_or_else(|| panic!("fixture metric"))
        )
    };
    for (index, id) in all.iter().enumerate() {
        let item = items
            .iter()
            .find(|item| &item.itemid == id)
            .unwrap_or_else(|| panic!("fixture item"));
        let catalog = catalog_for(id);
        let slot = format!("slot-{index}");
        let value_type = match item.value_type {
            0 => "numeric_float",
            3 => "numeric_unsigned",
            _ => panic!("numeric fixture"),
        };
        let mut metric = json!({"id":"probe","name":"Probe","selector":{"kind":"key","value":item.key},
            "value_type":value_type,"origin":"host_item","units":item.units});
        let mut source = json!({"kind":"manual"});
        if let Some(discovery) = &item.discovery {
            let (template, origin) = reader
                .portable_prototype_identity(&discovery.parent_itemid)
                .await
                .unwrap_or_else(|_| panic!("prototype lineage request"))
                .unwrap_or_else(|| panic!("visible prototype lineage"));
            source["template_uuid"] = json!(template);
            metric["uuid"] = json!(origin);
            metric["selector"]["value"] = json!(discovery.prototype_key);
            metric["origin"] = json!("lld_prototype");
        } else if item.templateid.is_some() {
            let (template, origin) = reader
                .portable_item_identity(id)
                .await
                .unwrap_or_else(|_| panic!("item lineage request"))
                .unwrap_or_else(|| panic!("visible item lineage"));
            source["template_uuid"] = json!(template);
            metric["uuid"] = json!(origin);
            metric["origin"] = json!("template_item");
        }
        refs.push(json!({"id":catalog,"file":format!("metrics/{catalog}.yaml")}));
        if index < 2 {
            series.push(json!({"metric":{"catalog":catalog,"metric":"probe"},
                "hosts":{"role":"targets","selection":"slot","slot_id":slot}}));
        }
        catalogs.insert(catalog.clone(), json!({"format":"zdashstitch.metrics/v1","catalog_id":catalog,
            "export_profile":"portable_with_public_keys","source":source,"diagnostics":[],"metrics":[metric]}));
    }
    drop(guard);
    let layout = json!({"mode":"fixed","x":0,"y":0,"w":18,"h":4});
    let mut widget = json!({"type":"graph","id":"graph","title":"Graph","expansion":"series_per_host",
                           "layout":layout,"series":series});
    if let Some(masks) = &masks {
        if let Some(raw) = masks["fields"].as_array() {
            let fields: Vec<_> = raw.iter().map(|field| {
                let name = field["name"].as_str().unwrap_or_default();
                let value = field["value"].as_str().unwrap_or_default();
                let kind = field["type"].as_str().and_then(|kind| kind.parse::<u16>().ok())
                    .unwrap_or_else(|| panic!("field type"));
                let semantic = match kind {
                    0 => json!({"kind":"integer","value":value.parse::<i64>().unwrap_or_else(|_| panic!("integer field"))}),
                    1 if dynamic_graph_field(name) => json!({"kind":"dynamic_pattern","value":value,"role":"targets"}),
                    1 => json!({"kind":"string","value":value}),
                    4 => {
                        let item = items.iter().find(|item| item.itemid == value).unwrap_or_else(|| panic!("mixed Graph item"));
                        let index = masks["hostids"].as_array().unwrap_or_else(|| panic!("fixture hosts")).iter()
                            .position(|id| id == item.hostid.as_str()).unwrap_or_else(|| panic!("chosen mixed Graph host"));
                        json!({"kind":"metric","catalog":catalog_for(value),"metric":"probe",
                            "hosts":{"role":"targets","slot_id":format!("slot-{index}"),"selection":"slot"}})
                    }
                    _ => panic!("unsupported fixture field"),
                };
                json!({"name":name,"field_type":kind,"registry_state":"known","value":semantic})
            }).collect();
            widget = json!({"type":"zabbix","id":"graph","name":"Graph","widget_type":"svggraph",
                "registry_state":"known","layout":layout,"fields":fields});
        } else {
            let mut fields = vec![
                json!({"name":"ds.0.dataset_type","field_type":0,"registry_state":"known",
            "value":{"kind":"integer","value":1}}),
            ];
            for (index, field) in ["ds.0.hosts.0", "ds.0.hosts.1", "ds.0.items.0"]
                .iter()
                .enumerate()
            {
                fields.push(json!({"name":field,"field_type":1,"registry_state":"known",
                "value":{"kind":"dynamic_pattern","role":"targets","value":masks["patterns"][index]}}));
            }
            fields.push(
                json!({"name":"ds.0.color","field_type":1,"registry_state":"known",
            "value":{"kind":"string","value":"AA32CC"}}),
            );
            widget = json!({"type":"zabbix","id":"graph","name":"Graph","widget_type":"svggraph","registry_state":"known",
            "layout":layout,"fields":fields});
        }
    }
    let slots_count = masks
        .as_ref()
        .and_then(|masks| masks["hostids"].as_array())
        .map_or(replacements.len(), Vec::len);
    let slots: Vec<_> = (0..slots_count)
        .map(|index| format!("slot-{index}"))
        .collect();
    let bundle = serde_json::from_value(json!({"project":{
        "format":"zdashstitch.project/v2","project_id":"p504-fixture","kind":"dashboard",
        "provenance":{"kind":"local"},"declared_compatibility":{"minimum":"8.0.0"},
        "catalogs":refs,"roles":{"targets":{"cardinality":"many","slots":slots}},
        "root":{"id":"p504-fixture","kind":"global","name":"Graph","pages":[{"id":"page","name":"Graph",
            "widgets":[widget]}]}
    },"catalogs":catalogs})).unwrap_or_else(|_| panic!("portable fixture"));
    let draft = OfflineDashboardStore::new(app.storage.path().to_path_buf())
        .save_imported(bundle)
        .unwrap_or_else(|_| panic!("saved draft"));
    let root = format!("/api/drafts/{}", draft.local_id);
    bind_sources(
        &app,
        &draft.local_id,
        draft.revision,
        &ids,
        &items,
        masks.as_ref(),
    )
    .await;
    let (status, enabled) = app
        .call(
            "POST",
            &format!("{prefix}/enable-writes"),
            Some(json!({"confirmed":true})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{enabled}");
    {
        let service = service.lock().await;
        assert!(service.writer().is_none() && service.portable_writer().is_some());
    }
    if let Some(sharing) = &observer.sharing {
        verify_sharing_http(&app, &observer, sharing, &draft, connection, &root).await;
        proxy_task.abort();
        return;
    }
    let url = format!("{root}/portable-publication/plan");
    let plan = async |name: &str, another| {
        app.call("POST", &url, Some(json!({
            "connection_id":connection,"revision":draft.revision,"name":name,"create_another":another
        }))).await
    };
    let confirm_path = format!("{root}/publication/confirm");
    let confirm = async |preview: &Value, name: &str| {
        app.call("POST", &confirm_path, Some(json!({
            "handle":preview["handle"],"plan_id":preview["plan"]["plan_id"],"name":name,"confirmed":true
        }))).await
    };
    let verified_title = format!("{title} verified");
    let competing_title = format!("{title} competing");
    let lost_title = format!("{title} lost");
    let (status, competing) = plan(&competing_title, false).await;
    assert_eq!(status, StatusCode::OK, "{competing}");
    let (status, first) = plan(&verified_title, false).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(
        confirm(&competing, &competing_title).await.0,
        StatusCode::NOT_FOUND
    );
    let (status, verified) = confirm(&first, &verified_title).await;
    assert_eq!(status, StatusCode::OK, "{verified}");
    assert_eq!(verified["state"], "verified", "{verified}");
    assert!(verified["record_error"].is_null(), "{verified}");
    assert_eq!(
        confirm(&first, &verified_title).await.0,
        StatusCode::NOT_FOUND
    );
    let (_, competing_result) = plan(&competing_title, false).await;
    assert_eq!(
        competing_result["code"], "portable_already_deployed",
        "{competing_result}"
    );
    assert_eq!(observer.sends.load(Ordering::SeqCst), 1);
    let deployments = format!("{root}/deployments");
    let (status, links) = app.call("GET", &deployments, None).await;
    assert_eq!(status, StatusCode::OK, "{links}");
    assert_eq!(links["deployments"].as_array().map(Vec::len), Some(1));
    assert_eq!(links["deployments"][0]["project_id"], draft.local_id);
    assert_eq!(links["deployments"][0]["root_id"], draft.local_id);
    assert_eq!(links["deployments"][0]["source_revision"], draft.revision);
    assert_eq!(
        links["deployments"][0]["dashboard"]["pages"][0]["widgets"][0]["logical_id"],
        "graph"
    );
    assert!(links["deployments"][0]["raw_snapshot"].is_null());
    let (status, lost) = plan(&lost_title, true).await;
    assert_eq!(status, StatusCode::OK, "{lost}");
    let (status, unknown) = confirm(&lost, &lost_title).await;
    assert_eq!(status, StatusCode::OK, "{unknown}");
    assert_eq!(unknown["state"], "unknown_outcome", "{unknown}");
    let (_, links) = app.call("GET", &deployments, None).await;
    assert_eq!(links["deployments"].as_array().map(Vec::len), Some(1));
    assert_eq!(links["recovery"].as_array().map(Vec::len), Some(1));
    let id = lost["plan"]["plan_id"]
        .as_str()
        .unwrap_or_else(|| panic!("operation ID"));
    observer.hide_reads.store(false, Ordering::SeqCst);
    let reconcile = format!("/api/publication/journal/{id}/reconcile");
    for _ in 0..2 {
        let (status, recovered) = app.call("POST", &reconcile, Some(json!({}))).await;
        assert_eq!(status, StatusCode::OK, "{recovered}");
    }
    let (_, links) = app.call("GET", &deployments, None).await;
    assert_eq!(links["deployments"].as_array().map(Vec::len), Some(2));
    assert_eq!(links["recovery"].as_array().map(Vec::len), Some(0));
    assert_eq!(observer.sends.load(Ordering::SeqCst), 2);
    use zdashstitch_application::{
        offline_dashboards::UpdateDashboard, offline_pages::OfflinePageStore,
    };
    use zdashstitch_core::formats::{ProjectKind, ProjectRoot};
    let source = &links["deployments"][0];
    let destination = &links["deployments"][1];
    let source_dashboardid = source["dashboard"]["dashboardid"]
        .as_str()
        .unwrap_or_else(|| panic!("linked source ID"));
    let target_dashboardid = destination["dashboard"]["dashboardid"]
        .as_str()
        .unwrap_or_else(|| panic!("append target ID"));
    let target_pageid = destination["dashboard"]["pages"][0]["dashboard_pageid"]
        .as_str()
        .unwrap_or_else(|| panic!("append page ID"));
    let link_id = source["deployment_id"]
        .as_str()
        .unwrap_or_else(|| panic!("deployment ID"));
    let check_link = format!("{deployments}/{link_id}/check");
    let (status, state) = app.call("GET", &check_link, None).await;
    assert_eq!(status, StatusCode::OK, "{state}");
    assert_eq!(state["state"], "current");
    let ProjectRoot::Dashboard(root_page) = &draft.bundle.project.root else {
        panic!("fixture dashboard");
    };
    let store = OfflineDashboardStore::new(app.storage.path().to_path_buf());
    let mut renamed = store
        .update(
            &draft.local_id,
            UpdateDashboard {
                expected_revision: draft.revision,
                name: format!("{title} updated"),
                page_order: vec![root_page.pages[0].id.clone()],
                append_page_ids: Vec::new(),
            },
        )
        .unwrap_or_else(|error| panic!("draft rename: {error}"));
    if geometry || (ids != replacements && masks.is_none()) {
        assert!(!geometry || (ids == replacements && masks.is_none()));
        let metrics: Vec<_> = replacements
            .iter()
            .enumerate()
            .map(|(index, id)| {
                let mut series = json!({
                    "catalog_id":catalog_for(id),"metric_id":"probe",
                    "hosts":{"role":"targets","selection":"slot","slot_id":format!("slot-{index}")},
                });
                if geometry && index == 0 {
                    series["color"] = json!("#654321");
                }
                series
            })
            .collect();
        let layout = if geometry {
            json!({"mode":"fixed","x":4,"y":4,"w":26,"h":7})
        } else {
            json!({"mode":"fixed","x":0,"y":0,"w":18,"h":4})
        };
        let request = serde_json::from_value(json!({
            "expected_revision":renamed.revision,"page":{
                "name":"Graph","catalogs":renamed.catalog_versions,"roles":renamed.bundle.project.roles,
                "widgets":[{"kind":"graph","widget_id":"graph","title":"Graph","metrics":metrics,
                    "layout":layout}],
            }
        })).unwrap_or_else(|_| panic!("draft Graph edit request"));
        store
            .update_page(&draft.local_id, &root_page.pages[0].id, request)
            .unwrap_or_else(|error| panic!("draft Graph edit: {error}"));
        renamed = store
            .get(&draft.local_id)
            .unwrap_or_else(|_| panic!("updated draft"))
            .unwrap_or_else(|| panic!("draft disappeared"));
    }
    let update_plan = async |id: &str, revision, intent: Value| {
        app.call(
            "POST",
            &format!("/api/drafts/{id}/portable-publication/update-plan"),
            Some(json!({"connection_id":connection,"revision":revision,"intent":intent})),
        )
        .await
    };
    let update_confirm = async |id: &str, preview: &Value| {
        app.call("POST", &format!("/api/drafts/{id}/publication/confirm"),
            Some(json!({"handle":preview["handle"],"plan_id":preview["plan"]["plan_id"],"confirmed":true}))).await
    };
    bind_sources(
        &app,
        &draft.local_id,
        renamed.revision,
        &replacements,
        &items,
        masks.as_ref(),
    )
    .await;
    let update_intent = json!({"kind":"update_created","deployment_id":source["deployment_id"]});
    let (status, stale_plan) =
        update_plan(&draft.local_id, renamed.revision, update_intent.clone()).await;
    assert_eq!(status, StatusCode::OK, "{stale_plan}");
    let preview_changes = stale_plan["plan"]["changes"]
        .as_array()
        .unwrap_or_else(|| panic!("scoped diff"));
    assert!(
        preview_changes
            .iter()
            .any(|change| change["kind"] == "name" && change["object"] == "dashboard")
    );
    if ids != replacements {
        assert!(
            preview_changes
                .iter()
                .any(|change| change["kind"] == "field"),
            "Graph change is in the plan diff"
        );
    }
    if geometry {
        assert!(preview_changes.iter().any(|change| {
            change["kind"] == "geometry"
                && change["widget"] == "Graph"
                && change["before"] == json!([0, 0, 18, 4])
                && change["after"] == json!([4, 4, 26, 7])
        }));
        assert!(preview_changes.iter().any(|change| {
            change["kind"] == "field"
                && change["widget"] == "Graph"
                && change["name"] == "ds.0.color.0"
                && change["after"] == "654321"
        }));
    }
    observer.stale_target.store(true, Ordering::SeqCst);
    let (status, state) = app.call("GET", &check_link, None).await;
    assert_eq!(status, StatusCode::OK, "{state}");
    assert_eq!(state["state"], "changed");
    assert_eq!(
        state["remote"]["changes"][0]["kind"],
        "dashboard_properties"
    );
    let (status, stale) = update_confirm(&draft.local_id, &stale_plan).await;
    assert_eq!(status, StatusCode::OK, "{stale}");
    assert_eq!(stale["state"], "conflict", "{stale}");
    assert_eq!(observer.updates.load(Ordering::SeqCst), 0);
    observer.stale_target.store(false, Ordering::SeqCst);
    let (status, refreshed_plan) =
        update_plan(&draft.local_id, renamed.revision, update_intent).await;
    assert_eq!(status, StatusCode::OK, "{refreshed_plan}");
    *observer
        .update_scope
        .lock()
        .unwrap_or_else(|_| panic!("update scope")) = Some((source_dashboardid.into(), 0));
    let (status, result) = update_confirm(&draft.local_id, &refreshed_plan).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    if geometry {
        assert_eq!(result["state"], "unknown_outcome", "{result}");
        observer.hide_reads.store(false, Ordering::SeqCst);
        let opid = refreshed_plan["plan"]["plan_id"]
            .as_str()
            .unwrap_or_else(|| panic!("Graph update ID"));
        let (status, reconciled) = app
            .call(
                "POST",
                &format!("/api/publication/journal/{opid}/reconcile"),
                Some(json!({})),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(reconciled["state"], "reconciled_verified", "{reconciled}");
        assert_eq!(observer.updates.load(Ordering::SeqCst), 1);
    } else {
        assert_eq!(result["state"], "verified", "{result}");
    }
    assert!(result["record_error"].is_null(), "{result}");
    let (_, links_after) = app.call("GET", &deployments, None).await;
    assert_eq!(
        links_after["deployments"][0]["source_revision"],
        renamed.revision
    );
    assert_eq!(
        links_after["deployments"][0]["dashboard"]["pages"],
        source["dashboard"]["pages"]
    );
    let mut page_bundle = renamed.bundle.clone();
    let ProjectRoot::Dashboard(updated_root) = &page_bundle.project.root else {
        panic!("updated fixture dashboard");
    };
    let mut page = updated_root.pages[0].clone();
    page.id = "appended-source".into();
    page.name = "Appended from draft".into();
    page_bundle.project.kind = ProjectKind::Page;
    page_bundle.project.project_id = page.id.clone();
    page_bundle.project.root = ProjectRoot::Page(page);
    let page = OfflinePageStore::new(app.storage.path().to_path_buf())
        .save_imported(page_bundle)
        .unwrap_or_else(|error| panic!("page fixture: {error}"));
    bind_sources(
        &app,
        &page.local_id,
        page.revision,
        &replacements,
        &items,
        masks.as_ref(),
    )
    .await;
    let (status, add_page) = update_plan(
        &page.local_id,
        page.revision,
        json!({"kind":"append_page","dashboardid":target_dashboardid}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{add_page}");
    *observer
        .update_scope
        .lock()
        .unwrap_or_else(|_| panic!("update scope")) = Some((target_dashboardid.into(), 1));
    let (status, result) = update_confirm(&page.local_id, &add_page).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["state"], "verified", "{result}");
    let append = |x| json!({"kind":"append_widget","dashboardid":target_dashboardid,"pageid":target_pageid,"x":x,"y":0});
    let (status, overlap) = update_plan(&page.local_id, page.revision, append(0)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{overlap}");
    assert_eq!(observer.updates.load(Ordering::SeqCst), 2);
    let (status, add_widget) = update_plan(&page.local_id, page.revision, append(18)).await;
    assert_eq!(status, StatusCode::OK, "{add_widget}");
    *observer
        .update_scope
        .lock()
        .unwrap_or_else(|_| panic!("update scope")) = Some((target_dashboardid.into(), 2));
    let (status, lost_widget) = update_confirm(&page.local_id, &add_widget).await;
    assert_eq!(status, StatusCode::OK, "{lost_widget}");
    assert_eq!(lost_widget["state"], "unknown_outcome", "{lost_widget}");
    observer.hide_reads.store(false, Ordering::SeqCst);
    let operation_id = add_widget["plan"]["plan_id"]
        .as_str()
        .unwrap_or_else(|| panic!("append operation ID"));
    let (status, recovered) = app
        .call(
            "POST",
            &format!("/api/publication/journal/{operation_id}/reconcile"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{recovered}");
    assert_eq!(recovered["state"], "reconciled_verified", "{recovered}");
    assert_eq!(observer.updates.load(Ordering::SeqCst), 3);
    let (status, insert) = update_plan(
        &page.local_id,
        page.revision,
        json!({"kind":"append_page","dashboardid":target_dashboardid,"after_pageid":target_pageid}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{insert}");
    assert_eq!(insert["plan"]["changes"][0]["number"], 2);
    assert_eq!(insert["plan"]["repeats_append"], true);
    assert_eq!(
        update_confirm(&page.local_id, &insert).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(observer.updates.load(Ordering::SeqCst), 3);
    let (status, insert) = update_plan(
        &page.local_id,
        page.revision,
        json!({"kind":"append_page","dashboardid":target_dashboardid,"after_pageid":target_pageid}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{insert}");
    *observer
        .update_scope
        .lock()
        .unwrap_or_else(|_| panic!("update scope")) = Some((target_dashboardid.into(), 3));
    let (status, inserted) = app.call("POST", &format!("/api/drafts/{}/publication/confirm", page.local_id),
        Some(json!({"handle":insert["handle"],"plan_id":insert["plan"]["plan_id"],"confirmed":true,"acknowledge_repeat_append":true}))).await;
    assert_eq!(status, StatusCode::OK, "{inserted}");
    assert_eq!(inserted["state"], "verified", "{inserted}");
    assert_eq!(observer.updates.load(Ordering::SeqCst), 4);
    if std::env::var("ZDS_P504_MULTI_HTTP").as_deref() == Ok("1") {
        let mut multi_bundle = renamed.bundle.clone();
        let ProjectRoot::Dashboard(root) = &mut multi_bundle.project.root else {
            panic!("multi-page fixture");
        };
        let mut chosen = root.pages[0].clone();
        chosen.id = "chosen-page".into();
        chosen.name = "Chosen page".into();
        let original = chosen.widgets[0].clone();
        let mut widgets = Vec::new();
        for index in 0..selected_count {
            let mut widget =
                serde_json::to_value(&original).unwrap_or_else(|_| panic!("widget fixture"));
            widget["id"] = json!(format!("chosen-{index}"));
            if selected_count == 256 {
                let (x, y) = proposed_placement(selected_count, index);
                widget["layout"] = json!({"mode":"fixed","x":x,"y":y,"w":4,"h":4});
            } else {
                widget["layout"] =
                    json!({"mode":"fixed","x":(index % 4) * 18,"y":(index / 4) * 4,"w":18,"h":4});
            }
            widgets
                .push(serde_json::from_value(widget).unwrap_or_else(|_| panic!("chosen widget")));
        }
        chosen.widgets = widgets;
        root.pages.push(chosen);
        if limits {
            for index in 2..composite_pages {
                let mut extra = root.pages[0].clone();
                extra.id = format!("extra-page-{index}");
                extra.name = format!("Extra page {index}");
                root.pages.push(extra);
            }
        }
        let selected = store
            .save_imported(multi_bundle)
            .unwrap_or_else(|error| panic!("two-page fixture: {error}"));
        for index in 0..selected_count {
            bind_sources_for(
                &app,
                &selected.local_id,
                selected.revision,
                &replacements,
                &items,
                masks.as_ref(),
                (1, index),
            )
            .await;
        }
        let (status, select_page) = update_plan(&selected.local_id, selected.revision,
            json!({"kind":"append_page","dashboardid":target_dashboardid,"source_page_id":"chosen-page"})).await;
        assert_eq!(status, StatusCode::OK, "{select_page}");
        assert_eq!(select_page["plan"]["source_name"], "Chosen page");
        *observer
            .update_scope
            .lock()
            .unwrap_or_else(|_| panic!("update scope")) = Some((target_dashboardid.into(), 4));
        let (status, appended) = update_confirm(&selected.local_id, &select_page).await;
        assert_eq!(status, StatusCode::OK, "{appended}");
        assert_eq!(appended["state"], "verified", "{appended}");
        let placements: Vec<_> = (0..selected_count)
            .map(|index| {
                let (x, y) = proposed_placement(selected_count, index);
                json!({"widget_id":format!("chosen-{index}"),"x":x,"y":y})
            })
            .collect();
        let selection = |widgets| {
            json!({"kind":"append_widgets","dashboardid":target_dashboardid,
                "pageid":target_pageid,"source_page_id":"chosen-page","widgets":widgets})
        };
        if selected_count == 256 {
            let mut over_limit = placements.clone();
            over_limit.push(placements[0].clone());
            for placement in [over_limit, placements.clone()] {
                let (status, _) =
                    update_plan(&selected.local_id, selected.revision, selection(placement)).await;
                assert_eq!(status, StatusCode::CONFLICT);
                assert_eq!(observer.updates.load(Ordering::SeqCst), 5);
            }
        } else {
            let mut overlap = placements.clone();
            overlap[1]["x"] = placements[0]["x"].clone();
            overlap[1]["y"] = placements[0]["y"].clone();
            let mut duplicate = placements.clone();
            duplicate[1]["widget_id"] = placements[0]["widget_id"].clone();
            for invalid in [selection(overlap), selection(duplicate)] {
                assert_eq!(
                    update_plan(&selected.local_id, selected.revision, invalid)
                        .await
                        .0,
                    StatusCode::CONFLICT
                );
                assert_eq!(observer.updates.load(Ordering::SeqCst), 5);
            }
            let (status, append) =
                update_plan(&selected.local_id, selected.revision, selection(placements)).await;
            assert_eq!(status, StatusCode::OK, "{append}");
            assert_eq!(
                append["plan"]["changes"].as_array().map(Vec::len),
                Some(selected_count)
            );
            *observer
                .update_scope
                .lock()
                .unwrap_or_else(|_| panic!("update scope")) = Some((target_dashboardid.into(), 5));
            let (status, unknown) = update_confirm(&selected.local_id, &append).await;
            assert_eq!(status, StatusCode::OK, "{unknown}");
            assert_eq!(unknown["state"], "unknown_outcome", "{unknown}");
            observer.hide_reads.store(false, Ordering::SeqCst);
            let id = append["plan"]["plan_id"]
                .as_str()
                .unwrap_or_else(|| panic!("selected operation"));
            let (status, verified) = app
                .call(
                    "POST",
                    &format!("/api/publication/journal/{id}/reconcile"),
                    Some(json!({})),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{verified}");
            assert_eq!(verified["state"], "reconciled_verified", "{verified}");
            assert_eq!(observer.updates.load(Ordering::SeqCst), 6);
        }
        if composite {
            assert!(limits || (masks.is_some() && selected_count == 2));
            for page in (0..composite_pages).filter(|page| *page != 1) {
                bind_sources_for(
                    &app,
                    &selected.local_id,
                    selected.revision,
                    &replacements,
                    &items,
                    masks.as_ref(),
                    (page, 0),
                )
                .await;
            }
            let composite_root = format!("/api/drafts/{}", selected.local_id);
            let composite_name = format!("{title} composite");
            let (status, plan) = app.call("POST", &format!("{composite_root}/portable-publication/plan"), Some(json!({
                "connection_id":connection,"revision":selected.revision,"name":composite_name
            }))).await;
            assert_eq!(status, StatusCode::OK, "{plan}");
            assert_eq!(plan["plan"]["page_count"], composite_pages);
            assert_eq!(
                plan["plan"]["widget_count"],
                composite_pages - 1 + selected_count
            );
            let (status, created) = app.call("POST", &format!("{composite_root}/publication/confirm"), Some(json!({
                "handle":plan["handle"],"plan_id":plan["plan"]["plan_id"],"name":composite_name,"confirmed":true
            }))).await;
            assert_eq!(status, StatusCode::OK, "{created}");
            assert_eq!(created["state"], "verified", "{created}");
            assert!(created["record_error"].is_null(), "{created}");
            let (status, links) = app
                .call("GET", &format!("{composite_root}/deployments"), None)
                .await;
            assert_eq!(status, StatusCode::OK);
            let link = &links["deployments"][0];
            let created_id = link["dashboard"]["dashboardid"]
                .as_str()
                .unwrap_or_else(|| panic!("composite dashboard ID"));
            assert_eq!(
                link["dashboard"]["pages"].as_array().map(Vec::len),
                Some(composite_pages)
            );
            assert_eq!(
                link["dashboard"]["pages"][1]["widgets"][0]["logical_id"],
                "chosen-0"
            );
            assert_eq!(
                link["dashboard"]["pages"][1]["widgets"][1]["logical_id"],
                "chosen-1"
            );
            if limits {
                let last_page = if composite_pages == 2 {
                    "chosen-page".to_owned()
                } else {
                    format!("extra-page-{}", composite_pages - 1)
                };
                assert_eq!(
                    link["dashboard"]["pages"][composite_pages - 1]["logical_id"],
                    last_page
                );
                assert_eq!(
                    link["dashboard"]["pages"][1]["widgets"]
                        .as_array()
                        .map(Vec::len),
                    Some(selected_count)
                );
                assert_eq!(observer.sends.load(Ordering::SeqCst), 3);
                if std::env::var("ZDS_P504_TOPOLOGY_HTTP").as_deref() == Ok("1") {
                    assert!(masks.is_none());
                    verify_linked_topology(
                        &app,
                        &observer,
                        TopologyFixture {
                            store: &store,
                            selected: &selected,
                            new_page_id: &page.local_id,
                            name: &composite_name,
                            connection,
                            link,
                            items: &items,
                        },
                    )
                    .await;
                }
                proxy_task.abort();
                return;
            }
            let ProjectRoot::Dashboard(dashboard) = &selected.bundle.project.root else {
                panic!("composite dashboard draft");
            };
            let page_order: Vec<_> = dashboard.pages.iter().map(|page| page.id.clone()).collect();
            let mut updated = store
                .update(
                    &selected.local_id,
                    UpdateDashboard {
                        expected_revision: selected.revision,
                        name: format!("{title} composite updated"),
                        page_order: page_order.clone(),
                        append_page_ids: Vec::new(),
                    },
                )
                .unwrap_or_else(|error| panic!("composite dashboard name: {error}"));
            for (index, page) in dashboard.pages.iter().enumerate() {
                let widgets: Vec<_> = page
                    .widgets
                    .iter()
                    .map(|widget| json!({"kind":"preserved","widget_id":widget.id()}))
                    .collect();
                let request = serde_json::from_value(json!({"expected_revision":updated.revision,
                    "page":{"name":format!("Linked page {}",index + 1),"catalogs":updated.catalog_versions,
                        "roles":updated.bundle.project.roles,"widgets":widgets}
                })).unwrap_or_else(|_| panic!("page edit input"));
                store
                    .update_page(&selected.local_id, &page.id, request)
                    .unwrap_or_else(|error| panic!("composite page name: {error}"));
                updated = store
                    .get(&selected.local_id)
                    .unwrap_or_else(|_| panic!("composite saved state"))
                    .unwrap_or_else(|| panic!("composite draft disappeared"));
            }
            for (page, widget) in [(0, 0), (1, 0), (1, 1)] {
                bind_sources_for(
                    &app,
                    &selected.local_id,
                    updated.revision,
                    &replacements,
                    &items,
                    masks.as_ref(),
                    (page, widget),
                )
                .await;
            }
            let (status, plan) = update_plan(
                &selected.local_id,
                updated.revision,
                json!({"kind":"update_created","deployment_id":link["deployment_id"]}),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{plan}");
            assert_eq!(plan["plan"]["changes"].as_array().map(Vec::len), Some(3));
            *observer
                .update_scope
                .lock()
                .unwrap_or_else(|_| panic!("update scope")) = Some((created_id.into(), 6));
            let (status, result) = update_confirm(&selected.local_id, &plan).await;
            assert_eq!(status, StatusCode::OK, "{result}");
            assert_eq!(result["state"], "unknown_outcome", "{result}");
            observer.hide_reads.store(false, Ordering::SeqCst);
            let opid = plan["plan"]["plan_id"]
                .as_str()
                .unwrap_or_else(|| panic!("composite update journal"));
            let (status, result) = app
                .call(
                    "POST",
                    &format!("/api/publication/journal/{opid}/reconcile"),
                    Some(json!({})),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{result}");
            assert_eq!(result["state"], "reconciled_verified", "{result}");
            assert!(result["record_error"].is_null(), "{result}");
            let (_, updated_links) = app
                .call("GET", &format!("{composite_root}/deployments"), None)
                .await;
            assert_eq!(
                updated_links["deployments"][0]["source_revision"],
                updated.revision
            );
            assert_eq!(
                updated_links["deployments"][0]["dashboard"]["pages"],
                link["dashboard"]["pages"]
            );
            assert_eq!(observer.sends.load(Ordering::SeqCst), 3);
            assert_eq!(observer.updates.load(Ordering::SeqCst), 7);
        }
    }
    proxy_task.abort();
}

struct TopologyFixture<'a> {
    store: &'a OfflineDashboardStore,
    selected: &'a OfflineDashboard,
    new_page_id: &'a str,
    name: &'a str,
    connection: &'a str,
    link: &'a Value,
    items: &'a [zdashstitch_zabbix::ItemDetails],
}

async fn verify_linked_topology(app: &App, observer: &Observer, fixture: TopologyFixture<'_>) {
    use zdashstitch_application::offline_dashboards::UpdateDashboard;
    use zdashstitch_core::formats::ProjectRoot;

    let TopologyFixture {
        store,
        selected,
        new_page_id,
        name,
        connection,
        link,
        items,
    } = fixture;
    let ProjectRoot::Dashboard(draft) = &selected.bundle.project.root else {
        panic!("composition draft");
    };
    let mut order: Vec<_> = draft.pages.iter().map(|page| page.id.clone()).collect();
    order.remove(2);
    let last = order.pop().unwrap_or_else(|| panic!("last linked page"));
    order.insert(0, last);
    let renamed = store
        .update(
            &selected.local_id,
            UpdateDashboard {
                expected_revision: selected.revision,
                name: name.to_owned(),
                page_order: order,
                append_page_ids: vec![new_page_id.to_owned()],
            },
        )
        .unwrap_or_else(|error| panic!("reordered draft and new page: {error}"));
    let graph_choice = |widget: &zdashstitch_core::formats::Widget, replacement: bool| {
        let raw = serde_json::to_value(widget).unwrap_or_else(|_| panic!("Graph choice"));
        let choices = raw["series"].as_array().unwrap_or_else(|| panic!("explicit source choices"))
            .iter().map(|source| json!({"catalog_id":source["metric"]["catalog"],"metric_id":source["metric"]["metric"],
            "hosts":source["hosts"],"color":source.get("color")})).collect::<Vec<_>>();
        json!({"kind":"graph","widget_id":if replacement { "replacement-topology" } else { widget.id() },
            "title":if replacement {"New Graph in linked page"} else {raw["title"].as_str().unwrap_or_default()},
            "layout":raw["layout"],"metrics":choices})
    };
    let mut new_widgets: Vec<_> = draft.pages[1].widgets[1..]
        .iter()
        .map(|widget| graph_choice(widget, false))
        .collect();
    new_widgets.push(graph_choice(&draft.pages[1].widgets[0], true));
    let request = serde_json::from_value(json!({"expected_revision":renamed.revision,
        "page":{"name":"Chosen page","catalogs":renamed.catalog_versions,"roles":renamed.bundle.project.roles,"widgets":new_widgets}
    })).unwrap_or_else(|_| panic!("topology edit input"));
    store
        .update_page(&selected.local_id, "chosen-page", request)
        .unwrap_or_else(|error| panic!("add and remove Graph: {error}"));
    let updated = store
        .get(&selected.local_id)
        .unwrap_or_else(|_| panic!("topology saved state"))
        .unwrap_or_else(|| panic!("topology draft disappeared"));
    let ProjectRoot::Dashboard(after) = &updated.bundle.project.root else {
        panic!("updated draft")
    };
    for (page_idx, page) in after.pages.iter().enumerate() {
        for widget_idx in 0..page.widgets.len() {
            bind_sources_for(
                app,
                &selected.local_id,
                updated.revision,
                &observer.replacement_items,
                items,
                None,
                (page_idx, widget_idx),
            )
            .await;
        }
    }
    verify_topology_confirmation(
        app,
        observer,
        &selected.local_id,
        updated.revision,
        connection,
        link,
    )
    .await;
}

async fn verify_topology_confirmation(
    app: &App,
    observer: &Observer,
    local_id: &str,
    revision: u64,
    connection: &str,
    link: &Value,
) {
    let root = format!("/api/drafts/{local_id}");
    let expected_dashboard = link["dashboard"]["dashboardid"]
        .as_str()
        .unwrap_or_default();
    let prepare = async || {
        let (status, response) = app
            .call(
                "POST",
                &format!("{root}/portable-publication/update-plan"),
                Some(json!({
                    "connection_id":connection, "revision":revision,
                    "intent":{"kind":"update_created","deployment_id":link["deployment_id"]}
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{response}");
        response
    };
    let confirm = async |preview: &Value, acknowledge: bool| {
        app.call("POST", &format!("{root}/publication/confirm"), Some(json!({
            "handle":preview["handle"],"plan_id":preview["plan"]["plan_id"],"confirmed":true,"acknowledge_deletions":acknowledge
        }))).await
    };
    let refused_plan = prepare().await;
    assert_eq!(
        refused_plan["plan"]["removes_content"], true,
        "{refused_plan}"
    );
    let changes = refused_plan["plan"]["changes"]
        .as_array()
        .unwrap_or_else(|| panic!("topology preview"));
    for kind in [
        "add_page",
        "remove_page",
        "move_page",
        "add_widget",
        "remove_widget",
    ] {
        assert!(
            changes.iter().any(|row| row["kind"] == kind),
            "missing topology diff {kind}"
        );
    }
    let (status, response) = confirm(&refused_plan, false).await;
    assert_eq!(status, StatusCode::CONFLICT, "{response}");
    assert_eq!(observer.updates.load(Ordering::SeqCst), 6);
    let stale_plan = prepare().await;
    observer.stale_target.store(true, Ordering::SeqCst);
    let (_, stale) = confirm(&stale_plan, true).await;
    assert_eq!(stale["state"], "conflict", "{stale}");
    assert_eq!(observer.updates.load(Ordering::SeqCst), 6);
    observer.stale_target.store(false, Ordering::SeqCst);
    let topology = prepare().await;
    *observer
        .update_scope
        .lock()
        .unwrap_or_else(|_| panic!("topology update scope")) = Some((expected_dashboard.into(), 7));
    let (status, unknown) = confirm(&topology, true).await;
    assert_eq!(status, StatusCode::OK, "{unknown}");
    assert_eq!(unknown["state"], "unknown_outcome", "{unknown}");
    assert_eq!(confirm(&topology, true).await.0, StatusCode::NOT_FOUND);
    observer.hide_reads.store(false, Ordering::SeqCst);
    let opid = topology["plan"]["plan_id"]
        .as_str()
        .unwrap_or_else(|| panic!("topology journal"));
    let (status, reconciled) = app
        .call(
            "POST",
            &format!("/api/publication/journal/{opid}/reconcile"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reconciled}");
    assert_eq!(reconciled["state"], "reconciled_verified", "{reconciled}");
    assert!(reconciled["record_error"].is_null(), "{reconciled}");
    let (_, links) = app.call("GET", &format!("{root}/deployments"), None).await;
    let result = &links["deployments"][0];
    assert_eq!(result["source_revision"], revision);
    let pages = &result["dashboard"]["pages"];
    assert_eq!(
        pages[0],
        link["dashboard"]["pages"][observer.composite_pages - 1]
    );
    assert_eq!(
        pages[2]["dashboard_pageid"],
        link["dashboard"]["pages"][1]["dashboard_pageid"]
    );
    assert_eq!(
        pages[2]["widgets"][0],
        link["dashboard"]["pages"][1]["widgets"][1]
    );
    assert_eq!(
        pages[2]["widgets"][observer.selected_count - 1]["logical_id"],
        "replacement-topology"
    );
    assert_ne!(
        pages[2]["widgets"][observer.selected_count - 1]["widgetid"],
        link["dashboard"]["pages"][1]["widgets"][0]["widgetid"]
    );
    assert_eq!(observer.updates.load(Ordering::SeqCst), 7);
}

#[allow(clippy::expect_used)]
async fn verify_sharing_http(
    app: &App,
    observer: &Observer,
    sharing: &SharingFixture,
    draft: &OfflineDashboard,
    connection: &str,
    root: &str,
) {
    let (status, choices) = app
        .call("GET", "/api/publication/access-choices", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(choices["can_choose_owner"], sharing.super_admin);
    let has_id = |key: &str, id: &str| {
        choices[key]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["id"] == id))
    };
    assert!(has_id("users", &sharing.recipient) && has_id("groups", &sharing.group));
    assert_eq!(has_id("users", &sharing.hidden), sharing.super_admin);

    let create = format!("{root}/portable-publication/plan");
    let confirm = format!("{root}/publication/confirm");
    let first = format!("{} verified", observer.title);
    let second = format!("{} lost", observer.title);
    let recipient = if sharing.super_admin {
        &sharing.hidden
    } else {
        &sharing.recipient
    };
    let users = json!([{"id":recipient,"permission":"read_only"}]);
    let groups = json!([{"id":sharing.group,"permission":"read_write"}]);
    let mut selection = json!({"private":false,"users":users,"groups":groups});
    if sharing.super_admin {
        selection["owner"] = json!(sharing.recipient);
    }
    let prepare = async |name: &str, another: bool, access: Value| {
        app.call("POST", &create, Some(json!({
            "connection_id":connection,"revision":draft.revision,"name":name,"create_another":another,"access":access
        }))).await
    };
    let confirm_create = async |plan: &Value, name: &str, public: bool| {
        app.call(
            "POST",
            &confirm,
            Some(json!({
                "handle":plan["handle"],"plan_id":plan["plan"]["plan_id"],"name":name,
                "confirmed":true,"acknowledge_public_access":public
            })),
        )
        .await
    };
    if !sharing.super_admin {
        for forbidden in [
            json!({"owner":sharing.recipient}),
            json!({"users":[{"id":sharing.hidden,"permission":"read_only"}]}),
        ] {
            assert_eq!(
                prepare(&first, false, forbidden).await.0,
                StatusCode::CONFLICT
            );
        }
    }
    let (status, preview) = prepare(&first, false, selection.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["plan"]["private"], false);
    assert_eq!(preview["plan"]["access"]["users"], users);
    assert_eq!(preview["plan"]["access"]["groups"], groups);
    assert_eq!(
        confirm_create(&preview, &first, false).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(observer.sends.load(Ordering::SeqCst), 0);
    let (_, preview) = prepare(&first, false, selection.clone()).await;
    let (status, result) = confirm_create(&preview, &first, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["state"], "verified");
    assert!(result["record_error"].is_null());

    let links_path = format!("{root}/deployments");
    let (status, links) = app.call("GET", &links_path, None).await;
    assert_eq!(status, StatusCode::OK);
    let link = &links["deployments"][0];
    let deployment = link["deployment_id"].as_str().expect("deployment ID");
    let dashboardid = link["dashboard"]["dashboardid"]
        .as_str()
        .expect("dashboard ID");
    let read_access = format!("{links_path}/{deployment}/access");
    let (status, current) = app.call("GET", &read_access, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(current["private"], false);
    assert_eq!(current["users"], users);
    let update_path = format!("{root}/portable-publication/update-plan");
    let update = async |access: Value| {
        app.call(
            "POST",
            &update_path,
            Some(json!({
                "connection_id":connection,"revision":draft.revision,
                "intent":{"kind":"change_access","deployment_id":deployment,"access":access}
            })),
        )
        .await
    };
    let confirm_update = async |plan: &Value| {
        app.call(
            "POST",
            &confirm,
            Some(json!({
                "handle":plan["handle"],"plan_id":plan["plan"]["plan_id"],"confirmed":true
            })),
        )
        .await
    };
    if sharing.super_admin {
        assert_eq!(current["owner"], sharing.recipient);
        let proposed = json!({"owner":sharing.hidden,"users":[],
            "groups":[{"id":sharing.hidden_group,"permission":"read_only"}]});
        let (status, change) = update(proposed).await;
        assert_eq!(status, StatusCode::OK, "Super Admin owner and list plan");
        assert_eq!(change["plan"]["access_change"]["before"], current);
        let after = &change["plan"]["access_change"]["after"];
        assert_eq!(after["owner"], sharing.hidden);
        assert_eq!(after["users"], json!([]));
        assert_eq!(
            after["groups"],
            json!([{"id":sharing.hidden_group,"permission":"read_only"}])
        );
        *observer
            .update_scope
            .lock()
            .expect("owned dashboard update") = Some((dashboardid.to_owned(), 10));
        let (status, result) = confirm_update(&change).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result["state"], "verified");
        assert!(result["record_error"].is_null());
        let (status, observed) = app.call("GET", &read_access, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(&observed, after);
        assert_eq!(observer.updates.load(Ordering::SeqCst), 1);
        println!("P5-05 HTTP: Super Admin owner transfer and explicit lists verified");
        return;
    }

    let audit = async |method, params| {
        let envelope: Value = observer
            .client
            .post(observer.upstream.clone())
            .header(
                reqwest::header::AUTHORIZATION,
                sharing.audit_session.clone(),
            )
            .json(&json!({"jsonrpc":"2.0","method":method,"params":params,"id":1}))
            .send()
            .await
            .expect("fixture audit connection")
            .json()
            .await
            .expect("fixture audit response");
        assert!(
            envelope.get("result").is_some(),
            "isolated fixture audit read/write"
        );
        envelope["result"].clone()
    };
    let read_full =
        async || {
            let observed = audit("dashboard.get", json!({
            "dashboardids":[dashboardid],"output":["dashboardid","name","userid","private"],
            "selectUsers":["userid","permission"],"selectUserGroups":["usrgrpid","permission"],
            "selectPages":"extend"
        })).await;
            observed[0].clone()
        };
    let initial = read_full().await;
    assert_eq!(initial["dashboardid"], dashboardid);
    assert_eq!(initial["name"], first);
    let hidden_users = json!([{"userid":sharing.recipient,"permission":"2"},{"userid":sharing.hidden,"permission":"2"}]);
    let hidden_groups = json!([{"usrgrpid":sharing.group,"permission":"3"},{"usrgrpid":sharing.hidden_group,"permission":"2"}]);
    let injected = audit(
        "dashboard.update",
        json!({
            "dashboardid":dashboardid,"users":hidden_users,"userGroups":hidden_groups
        }),
    )
    .await;
    assert_eq!(injected["dashboardids"], json!([dashboardid]));
    let independent_base = read_full().await;
    assert_eq!(independent_base["users"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        independent_base["userGroups"].as_array().map(Vec::len),
        Some(2)
    );
    for forbidden in [
        json!({"owner":current["owner"]}),
        json!({"groups":[]}),
        json!({"users":[]}),
    ] {
        assert_eq!(update(forbidden).await.0, StatusCode::CONFLICT);
    }

    let (status, change) = update(json!({"private":true})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(change["plan"]["access_change"]["after"]["private"], true);
    assert_eq!(change["plan"]["access_change"]["after"]["groups"], groups);
    observer.stale_target.store(true, Ordering::SeqCst);
    let (status, stale) = confirm_update(&change).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stale["state"], "conflict");
    observer.stale_target.store(false, Ordering::SeqCst);
    assert_eq!(observer.updates.load(Ordering::SeqCst), 0);

    let (status, change) = update(json!({"private":true})).await;
    assert_eq!(status, StatusCode::OK);
    observer.role_changed.store(true, Ordering::SeqCst);
    let (status, role) = confirm_update(&change).await;
    observer.role_changed.store(false, Ordering::SeqCst);
    assert!(status == StatusCode::CONFLICT || role["state"] == "failed_before_send");
    assert_eq!(observer.updates.load(Ordering::SeqCst), 0);

    let (status, change) = update(json!({"private":true})).await;
    assert_eq!(status, StatusCode::OK);
    *observer.update_scope.lock().expect("scoped update") = Some((dashboardid.to_owned(), 9));
    let (status, unknown) = confirm_update(&change).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unknown["state"], "unknown_outcome");
    observer.hide_reads.store(false, Ordering::SeqCst);
    let operation = change["plan"]["plan_id"]
        .as_str()
        .expect("update operation ID");
    let (status, recovered) = app
        .call(
            "POST",
            &format!("/api/publication/journal/{operation}/reconcile"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recovered["state"], "reconciled_verified");
    assert_eq!(observer.updates.load(Ordering::SeqCst), 1);
    let (status, observed) = app.call("GET", &read_access, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(observed["private"], true);
    assert_eq!(observed["users"], users);
    assert_eq!(observed["groups"], groups);
    let mut independent_after = read_full().await;
    assert_eq!(independent_after["private"], "1");
    independent_after["private"] = json!("0");
    assert_eq!(independent_after, independent_base);

    let (status, preview) = prepare(&second, true, selection).await;
    assert_eq!(status, StatusCode::OK);
    let (status, unknown) = confirm_create(&preview, &second, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unknown["state"], "unknown_outcome");
    observer.hide_reads.store(false, Ordering::SeqCst);
    let operation = preview["plan"]["plan_id"]
        .as_str()
        .expect("create operation ID");
    let (status, recovered) = app
        .call(
            "POST",
            &format!("/api/publication/journal/{operation}/reconcile"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recovered["state"], "reconciled_verified");
    assert_eq!(observer.sends.load(Ordering::SeqCst), 2);
    let (status, links) = app.call("GET", &links_path, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(links["deployments"].as_array().map(Vec::len), Some(2));
    assert!(
        links["deployments"]
            .as_array()
            .expect("links")
            .iter()
            .all(|link| link["raw_snapshot"].is_null())
    );
    let name = format!("{} updated", observer.title);
    let renamed = OfflineDashboardStore::new(app.storage.path().to_path_buf())
        .update(
            &draft.local_id,
            zdashstitch_application::offline_dashboards::UpdateDashboard {
                expected_revision: draft.revision,
                name: name.clone(),
                page_order: vec!["page".to_owned()],
                append_page_ids: Vec::new(),
            },
        )
        .expect("rename linked draft");
    let connections = app.connections.service().expect("app connection");
    let client = connections.lock().await;
    let items = client
        .reader()
        .expect("app reader")
        .items_by_ids(&observer.items)
        .await
        .expect("fixture items");
    drop(client);
    bind_sources(
        app,
        &draft.local_id,
        renamed.revision,
        &observer.items,
        &items,
        None,
    )
    .await;
    let (status, content) = app
        .call(
            "POST",
            &update_path,
            Some(json!({
                "connection_id":connection,"revision":renamed.revision,
                "intent":{"kind":"update_created","deployment_id":deployment}
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "linked content update plan");
    *observer.update_scope.lock().expect("scoped content update") =
        Some((dashboardid.to_owned(), 0));
    let (status, result) = confirm_update(&content).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["state"], "verified");
    assert!(result["record_error"].is_null());
    assert_eq!(observer.updates.load(Ordering::SeqCst), 2);
    let mut final_read = read_full().await;
    assert_eq!(final_read["name"], name);
    final_read["name"] = independent_base["name"].clone();
    final_read["private"] = json!("0");
    assert_eq!(final_read, independent_base);
    println!(
        "P5-05 HTTP: Admin sharing, hidden recipients preserved, stale and role change blocked, lost responses recovered by reads"
    );
}
