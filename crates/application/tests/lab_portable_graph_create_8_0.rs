// SPDX-License-Identifier: MIT OR Apache-2.0

//! The observer permits only reads and two creates of the dedicated Graph.
use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use reqwest::{Client, Url, redirect::Policy};
use secrecy::SecretString;
use serde_json::{Value, json};
use zdashstitch_application::{
    JournalState, LocalSession, PortableCreateRequest, PublishJournal,
    clone_mapping::{TargetSlot, TargetSlotId},
    offline_dashboards::{OfflineDashboard, OfflineDashboardStore},
    plan_portable_dashboard_with_gateway,
    portable_lld_review::{PortableLldConfirmRequest, PortableLldReviewRequest},
    portable_pattern_review::{PortablePatternConfirmRequest, PortablePatternReviewRequest},
    publish_portable_dashboard_plan, reconcile_create,
    workcopy::WorkcopyService,
};
use zdashstitch_core::formats::{ProjectRoot, TypedPortableBundle, Widget, ZabbixWidgetFieldValue};
use zdashstitch_zabbix::{
    Authentication, ItemDetails, ItemFlags, ReadOnlyGateway, ZabbixClient, ZabbixEndpoint,
    read_only_allowlist,
};

const CONNECTION: &str = "scoped-graph-lab";
const CLEAN: u8 = 0;
const STALE_ITEM: u8 = 1;
const LOST_RESPONSE: u8 = 2;
const VERSION_CHANGED: u8 = 3;
const HOST_DISABLED: u8 = 4;
const MATCH_COUNT_CHANGED: u8 = 5;
const NARROW_ROLE: u8 = 6;
const DISCOVERY_LOST: u8 = 7;
const STALE_EXPLICIT: u8 = 8;
const IDENTITY_CHANGED: u8 = 9;
const STALE_SECOND_MASK: u8 = 10;

enum Scope {
    Explicit(Vec<String>),
    Fields {
        fields: Vec<Value>,
        hosts: Vec<String>,
    },
}

impl Scope {
    fn is_masked(&self) -> bool {
        matches!(self, Self::Fields { fields, .. } if fields.iter().any(dynamic_field))
    }

    fn is_mixed(&self) -> bool {
        self.is_masked()
            && matches!(self, Self::Fields { fields, .. } if fields.iter().any(|field| field["type"] == "4"))
    }

    fn second_item_mask(&self) -> Option<&Value> {
        let Self::Fields { fields, .. } = self else {
            return None;
        };
        fields
            .iter()
            .find(|field| field["name"] == "ds.1.items.0" && dynamic_field(field))
            .map(|field| &field["value"])
    }

    fn accepts(&self, fields: &[Value], lost: bool) -> bool {
        match self {
            Self::Explicit(items) => {
                let selected = if lost { items.as_slice() } else { &items[..2] };
                fields.len() == 1 + selected.len() * 2
                    && selected.iter().enumerate().all(|(i, id)| {
                        fields.iter().any(|field| {
                            field["name"] == format!("ds.0.itemids.{i}")
                                && field["value"] == id.as_str()
                        })
                    })
            }
            Self::Fields {
                fields: expected, ..
            } => fields == expected,
        }
    }
}

struct Observer {
    upstream: Url,
    client: Client,
    name: String,
    scope: Scope,
    mode: AtomicU8,
    sends: AtomicU8,
}

async fn observe(
    State(state): State<Arc<Observer>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let method = body["method"].as_str().unwrap_or_default();
    let params = &body["params"];
    let mode = state.mode.load(Ordering::Relaxed);
    let is_create = method == "dashboard.create";
    if is_create {
        let expected_name = format!(
            "{} {}",
            state.name,
            if mode == LOST_RESPONSE {
                "lost"
            } else {
                "verified"
            }
        );
        let fields = params["pages"][0]["widgets"][0]["fields"].as_array();
        let in_scope =
            fields.is_some_and(|fields| state.scope.accepts(fields, mode == LOST_RESPONSE));
        assert!(
            in_scope
                && params["name"] == expected_name
                && params["private"] == "1"
                && params["pages"]
                    .as_array()
                    .is_some_and(|pages| pages.len() == 1)
                && params["pages"][0]["widgets"]
                    .as_array()
                    .is_some_and(|widgets| widgets.len() == 1)
        );
        assert!(matches!(mode, CLEAN | LOST_RESPONSE));
        assert!(state.sends.fetch_add(1, Ordering::Relaxed) < 2);
    } else if !read_only_allowlist().contains(&method) {
        panic!("the scoped observer received a non-dashboard write");
    }
    if method == "dashboard.get"
        && mode == LOST_RESPONSE
        && state.sends.load(Ordering::Relaxed) == 2
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let mut request = state.client.post(state.upstream.clone()).json(&body);
    if let Some(auth) = headers.get(header::AUTHORIZATION) {
        request = request.header(header::AUTHORIZATION, auth);
    }
    let Ok(response) = request.send().await else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    let Ok(mut envelope) = response.json::<Value>().await else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    if is_create && mode == LOST_RESPONSE {
        assert!(
            envelope["result"]["dashboardids"]
                .as_array()
                .is_some_and(|ids| ids.len() == 1)
        );
        return StatusCode::BAD_GATEWAY.into_response();
    }
    if matches!(mode, STALE_ITEM | STALE_EXPLICIT)
        && method == "item.get"
        && params["hostids"].is_array()
        && let Some(rows) = envelope["result"].as_array_mut()
    {
        match &state.scope {
            Scope::Explicit(items) => {
                for row in rows
                    .iter_mut()
                    .filter(|item| item["itemid"].as_str() == Some(&items[0]))
                {
                    row["name"] = json!("Observed a different name");
                }
            }
            Scope::Fields { fields, hosts } => {
                if mode == STALE_ITEM
                    && state.scope.is_masked()
                    && let Some(row) = rows.iter_mut().find(|row| {
                        row["hostid"]
                            .as_str()
                            .is_some_and(|id| !hosts.iter().any(|host| host == id))
                    })
                {
                    row["name_resolved"] = json!("Changed external match");
                }
                if mode == STALE_EXPLICIT || !state.scope.is_masked() {
                    let first_id = fields
                        .iter()
                        .find(|field| field["type"] == "4")
                        .map(|field| &field["value"]);
                    if let Some(row) = rows.iter_mut().find(|row| Some(&row["itemid"]) == first_id)
                    {
                        row["name"] = json!("Changed selected item");
                    }
                }
            }
        }
    }
    if mode == VERSION_CHANGED && method == "apiinfo.version" {
        envelope["result"] = json!("8.0.1");
    }
    if mode == STALE_SECOND_MASK
        && method == "item.get"
        && state.scope.second_item_mask().is_some_and(|mask| {
            params["search"]["name_resolved"]
                .as_array()
                .is_some_and(|patterns| patterns.contains(mask))
        })
        && let Some(row) = envelope["result"]
            .as_array_mut()
            .and_then(|rows| rows.first_mut())
    {
        row["name_resolved"] = json!("Second data set match changed");
    }
    if mode == HOST_DISABLED
        && method == "host.get"
        && params["output"] == json!(["hostid", "status"])
    {
        envelope["result"][0]["status"] = json!("1");
    }
    if mode == DISCOVERY_LOST
        && method == "item.get"
        && params["selectDiscoveryData"].is_array()
        && let Some(item) = envelope["result"]
            .as_array_mut()
            .and_then(|rows| rows.iter_mut().find(|row| row["flags"] == "4"))
    {
        item["discoveryData"]["status"] = json!("1");
    }
    if mode == MATCH_COUNT_CHANGED && method == "item.get" && params["countOutput"] == true {
        let count = envelope["result"]
            .as_str()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);
        envelope["result"] = json!((count + 1).to_string());
    }
    if mode == NARROW_ROLE && method == "user.checkAuthentication" {
        envelope["result"]["type"] = json!(2);
    }
    if mode == IDENTITY_CHANGED && method == "user.checkAuthentication" {
        let other = if envelope["result"]["userid"] == "1" {
            "2"
        } else {
            "1"
        };
        envelope["result"]["userid"] = json!(other);
    }
    Json(envelope).into_response()
}

struct SourceFixture {
    item: ItemDetails,
    identity: Option<(String, String)>,
}

fn dynamic_field(field: &Value) -> bool {
    let name = field["name"].as_str().unwrap_or_default();
    let parts: Vec<_> = name.split('.').collect();
    matches!(
        parts.as_slice(),
        ["ds" | "or", _, "hosts" | "items", _] | ["problemhosts", _]
    )
}

fn metric_catalog(fixture: &SourceFixture, catalog: &str) -> Value {
    let item = &fixture.item;
    let key = item
        .discovery
        .as_ref()
        .map_or(item.key.as_str(), |link| link.prototype_key.as_str());
    let origin = if item.flags == ItemFlags::Discovered {
        "lld_prototype"
    } else if fixture.identity.is_some() {
        "template_item"
    } else {
        "host_item"
    };
    let value_type = match item.value_type {
        0 => "numeric_float",
        3 => "numeric_unsigned",
        _ => panic!("numeric fixture required"),
    };
    let mut metric = json!({"id":"item","name":"Probe","selector":{"kind":"key","value":key},
        "value_type":value_type,"origin":origin,"units":item.units});
    let mut source = json!({"kind":"manual"});
    if let Some((template, item)) = &fixture.identity {
        source["template_uuid"] = json!(template);
        metric["uuid"] = json!(item);
    }
    json!({"format":"zdashstitch.metrics/v1","catalog_id":catalog,"export_profile":"portable_with_public_keys",
        "source":source,"diagnostics":[],"metrics":[metric]})
}

fn project_bundle(
    widget: Value,
    slots: Vec<String>,
    catalogs: serde_json::Map<String, Value>,
) -> TypedPortableBundle {
    let references: Vec<_> = catalogs
        .keys()
        .map(|id| json!({"id":id,"file":format!("metrics/{id}.yaml")}))
        .collect();
    serde_json::from_value(json!({
        "project":{"format":"zdashstitch.project/v2","project_id":"lab","kind":"dashboard",
            "declared_compatibility":{"minimum":"8.0.0"},"provenance":{"kind":"local"},
            "roles":{"targets":{"cardinality":"many","slots":slots}}, "catalogs":references,
            "root":{"kind":"global","id":"lab","name":"Graph lab","pages":[{"id":"page","name":"Graph","widgets":[widget]}]}},
        "catalogs":catalogs,
    })).unwrap_or_else(|_| panic!("Graph lab fixture"))
}

fn fixture(inputs: &[SourceFixture]) -> TypedPortableBundle {
    let mut catalogs = serde_json::Map::new();
    let mut series = Vec::new();
    let mut hosts = BTreeMap::new();
    for (index, fixture) in inputs.iter().enumerate() {
        let next_slot = format!("slot-{}", hosts.len() + 1);
        let slot = hosts.entry(&fixture.item.hostid).or_insert(next_slot);
        let catalog = format!("metric-{}", index + 1);
        catalogs.insert(catalog.clone(), metric_catalog(fixture, &catalog));
        series.push(json!({"metric":{"catalog":catalog,"metric":"item"},"hosts":{"role":"targets","selection":"slot","slot_id":slot}}));
    }
    project_bundle(
        json!({
                    "type":"graph","id":"graph","title":"Monitored hosts","expansion":"series_per_host",
                    "layout":{"mode":"fixed","x":0,"y":0,"w":18,"h":4},
                    "series":series
        }),
        hosts.into_values().collect(),
        catalogs,
    )
}

async fn synthetic_rpc(
    State(dashboards): State<Arc<Mutex<Vec<Value>>>>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let params = &body["params"];
    let method = body["method"].as_str().unwrap_or_default();
    let includes = |key: &str, id: &str| {
        params[key]
            .as_array()
            .is_none_or(|values| values.iter().any(|value| value == id))
    };
    let result = match method {
        "apiinfo.version" => json!("8.0.0"),
        "user.checkAuthentication" => json!({"userid":"3","roleid":"9","type":2,"username":"synthetic"}),
        "role.get" => json!([{"roleid":"9","type":"2","rules":{"api.access":"1","api.mode":"0","api":[],"actions":[{"name":"edit_dashboards","status":"1"}]}}]),
        "host.get" => json!(["101", "102", "103"].into_iter().filter(|id| includes("hostids", id)).map(|id| json!({
            "hostid":id,"host":format!("synthetic-{id}"),"name":format!("Synthetic {id}"),"status":"0",
        })).collect::<Vec<_>>()),
        "item.get" => {
            let items: Vec<_> = [("201","101","one","3"),("202","102","two","0"),("203","103","three","3")]
                .into_iter().filter(|(id, host, _, _)| includes("itemids", id) && includes("hostids", host))
                .map(|(id, host, key, value_type)| json!({"itemid":id,"hostid":host,"key_":key,"name":format!("Item {key}"),
                    "value_type":value_type,"units":"","templateid":"0","status":"0","flags":"0","discoveryData":[]})).collect();
            if params["countOutput"] == true { json!(items.len().to_string()) } else { json!(items) }
        }
        "dashboard.get" => {
            let dashboards = dashboards.lock().unwrap_or_else(|_| panic!("dashboard fixture"));
            json!(dashboards.iter().filter(|dashboard| includes("dashboardids", dashboard["dashboardid"].as_str().unwrap_or_default())
                && params["filter"]["name"].as_array().is_none_or(|names| names.iter().any(|name| *name == dashboard["name"])))
                .collect::<Vec<_>>())
        }
        "dashboard.create" => {
            let mut dashboards = dashboards.lock().unwrap_or_else(|_| panic!("dashboard fixture"));
            let id = (400 + dashboards.len()).to_string();
            let mut dashboard = params.clone();
            dashboard["dashboardid"] = json!(id);
            dashboard["userid"] = json!("3");
            dashboard["users"] = json!([]);
            dashboard["userGroups"] = json!([]);
            dashboard["pages"][0]["dashboard_pageid"] = json!(format!("{id}0"));
            dashboard["pages"][0]["widgets"][0]["widgetid"] = json!(format!("{id}00"));
            dashboards.push(dashboard);
            json!({"dashboardids":[id]})
        }
        _ => panic!("unexpected synthetic method {method}"),
    };
    Json(json!({"jsonrpc":"2.0","id":body["id"],"result":result}))
}

#[tokio::test]
async fn synthetic_graph_pipeline_grows_from_two_to_three_monitored_hosts() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|_| panic!("synthetic listener"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|_| panic!("synthetic address"));
    let dashboards = Arc::new(Mutex::new(Vec::new()));
    tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/api_jsonrpc.php", post(synthetic_rpc))
                .with_state(dashboards.clone()),
        )
        .into_future(),
    );
    let upstream = Url::parse(&format!("http://{address}/api_jsonrpc.php"))
        .unwrap_or_else(|_| panic!("synthetic endpoint"));
    graph_pipeline(
        upstream,
        "Synthetic Graph growth".into(),
        Scope::Explicit(["201", "202", "203"].map(str::to_owned).to_vec()),
        Authentication::ApiToken(SecretString::from("synthetic")),
    )
    .await;
    let dashboards = dashboards
        .lock()
        .unwrap_or_else(|_| panic!("dashboard fixture"));
    assert_eq!(dashboards.len(), 2);
    assert_eq!(
        dashboards[0]["pages"][0]["widgets"][0]["fields"]
            .as_array()
            .map(Vec::len),
        Some(5)
    );
    assert_eq!(
        dashboards[1]["pages"][0]["widgets"][0]["fields"]
            .as_array()
            .map(Vec::len),
        Some(7)
    );
}

#[tokio::test]
#[ignore = "requires dedicated account and test names on explicitly authorized Zabbix 8.0.0"]
async fn product_creates_one_private_graph_at_a_time_and_recovers_without_resend() {
    let var = |key| std::env::var(key).unwrap_or_else(|_| panic!("laboratory setting missing"));
    assert_eq!(
        var("ZDASHSTITCH_P5_02_LAB_ACK"),
        "dedicated-private-graph-8.0.0"
    );
    let upstream =
        Url::parse(&var("ZDASHSTITCH_P5_02_LAB_URL")).unwrap_or_else(|_| panic!("laboratory URL"));
    assert_eq!(upstream.host_str(), Some("127.0.0.1"));
    let name = var("ZDASHSTITCH_P5_02_LAB_NAME");
    assert!(name.starts_with("ZDashstitch Z80-P5-02 Rust "));
    let items: Vec<_> = std::env::var("ZDASHSTITCH_P5_02_LAB_FIXTURES").map_or_else(
        |_| {
            vec![
                var("ZDASHSTITCH_P5_02_LAB_ITEM_A"),
                var("ZDASHSTITCH_P5_02_LAB_ITEM_B"),
            ]
        },
        |value| value.split(',').map(str::to_owned).collect(),
    );
    let auth = Authentication::UsernamePassword {
        username: var("ZDASHSTITCH_P5_02_LAB_USERNAME"),
        password: SecretString::from(var("ZDASHSTITCH_P5_02_LAB_PASSWORD")),
    };
    let scope = match std::env::var("ZDASHSTITCH_P5_02_LAB_PATTERN_FIELDS") {
        Ok(fields) if !fields.is_empty() => Scope::Fields {
            fields: serde_json::from_str(&fields).unwrap_or_else(|_| panic!("lab mask contract")),
            hosts: var("ZDASHSTITCH_P5_02_LAB_PATTERN_HOSTS")
                .split(',')
                .map(str::to_owned)
                .collect(),
        },
        _ => Scope::Explicit(items),
    };
    graph_pipeline(upstream, name, scope, auth).await;
}

async fn graph_pipeline(upstream: Url, name: String, scope: Scope, auth: Authentication) {
    let observer = Arc::new(Observer {
        upstream,
        name: name.clone(),
        scope,
        client: Client::builder()
            .redirect(Policy::none())
            .build()
            .unwrap_or_else(|_| panic!("observer client")),
        mode: AtomicU8::new(CLEAN),
        sends: AtomicU8::new(0),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|_| panic!("observer listener"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|_| panic!("observer address"));
    tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/api_jsonrpc.php", post(observe))
                .with_state(observer.clone()),
        )
        .into_future(),
    );
    let endpoint = ZabbixEndpoint::parse(&format!("http://{address}/api_jsonrpc.php"))
        .unwrap_or_else(|_| panic!("endpoint"));
    let gateway = ZabbixClient::new(endpoint)
        .unwrap_or_else(|_| panic!("client"))
        .authenticate(auth)
        .await
        .unwrap_or_else(|_| panic!("lab authentication"))
        .enable_writes();
    let reader = gateway.reader();
    assert_eq!(reader.capabilities().version.to_string(), "8.0.0");
    let temp = tempfile::tempdir().unwrap_or_else(|_| panic!("storage"));
    let store = OfflineDashboardStore::new(temp.path().into());
    let service = WorkcopyService::new(temp.path().into());
    let journal = PublishJournal::new(temp.path().into());
    let session = LocalSession::new();
    let drafts = match &observer.scope {
        Scope::Explicit(items) => bind_explicit(reader, &service, &store, items).await,
        Scope::Fields { fields, hosts } => {
            vec![bind_fields(reader, &service, &store, fields, hosts).await]
        }
    };
    let request = |suffix| {
        let draft = if suffix == "verified" {
            &drafts[0]
        } else {
            &drafts[drafts.len() - 1]
        };
        PortableCreateRequest {
            local_id: draft.local_id.clone(),
            connection_id: CONNECTION.into(),
            revision: draft.revision,
            target_name: format!("{name} {suffix}"),
            create_another: suffix == "lost",
            session: session.current(),
        }
    };
    let mut cases = vec![
        ("stale", STALE_ITEM, JournalState::FailedBeforeSend, 0),
        ("disabled", HOST_DISABLED, JournalState::FailedBeforeSend, 0),
    ];
    if drafts.iter().any(|draft| {
        let ProjectRoot::Dashboard(dashboard) = &draft.bundle.project.root else {
            return false;
        };
        matches!(&dashboard.pages[0].widgets[0], Widget::Zabbix { fields, .. }
            if fields.iter().any(|field| matches!(field.value, ZabbixWidgetFieldValue::LldInstance { .. })))
    }) {
        cases.push(("discovery", DISCOVERY_LOST, JournalState::FailedBeforeSend, 0));
    }
    let contract = if observer.scope.is_masked() {
        cases.extend([
            (
                "partial",
                MATCH_COUNT_CHANGED,
                JournalState::FailedBeforeSend,
                0,
            ),
            ("hidden", NARROW_ROLE, JournalState::FailedBeforeSend, 0),
        ]);
        if observer.scope.is_mixed() {
            cases.extend([
                (
                    "explicit-stale",
                    STALE_EXPLICIT,
                    JournalState::FailedBeforeSend,
                    0,
                ),
                (
                    "identity",
                    IDENTITY_CHANGED,
                    JournalState::FailedBeforeSend,
                    0,
                ),
            ]);
            "create_portable_graph_mixed_8_0_0"
        } else if observer.scope.second_item_mask().is_some() {
            cases.extend([
                (
                    "second-mask",
                    STALE_SECOND_MASK,
                    JournalState::FailedBeforeSend,
                    0,
                ),
                (
                    "identity",
                    IDENTITY_CHANGED,
                    JournalState::FailedBeforeSend,
                    0,
                ),
            ]);
            "create_portable_graph_paired_patterns_8_0_0"
        } else {
            "create_portable_graph_patterns_8_0_0"
        }
    } else {
        "create_portable_graph_8_0_0"
    };
    cases.extend([
        ("verified", CLEAN, JournalState::Verified, 1),
        ("lost", LOST_RESPONSE, JournalState::UnknownOutcome, 2),
    ]);
    let mut last_id = String::new();
    for (suffix, mode, state, writes) in cases {
        observer.mode.store(CLEAN, Ordering::Relaxed);
        let plan = plan_portable_dashboard_with_gateway(
            &service,
            &journal,
            &session,
            &gateway,
            &request(suffix),
        )
        .await
        .unwrap_or_else(|error| panic!("Graph plan {suffix}: {error}"));
        let preview = plan.preview();
        assert_eq!(preview.contracts, [contract]);
        assert_eq!(preview.write_requests, 1);
        last_id = preview.plan_id;
        let plan = plan
            .confirm(&last_id)
            .unwrap_or_else(|_| panic!("confirm plan"));
        observer.mode.store(mode, Ordering::Relaxed);
        let stage = session
            .begin_write(plan.session())
            .unwrap_or_else(|_| panic!("write stage"));
        let report =
            publish_portable_dashboard_plan(&service, &gateway, &journal, plan, &stage).await;
        assert!(report.journal_error.is_none());
        assert_eq!(
            report.journal_state,
            Some(state),
            "scoped Graph case {suffix}, outcome {}",
            report
                .outcome
                .as_ref()
                .map_or("refused", |outcome| outcome.as_str())
        );
        assert_eq!(observer.sends.load(Ordering::Relaxed), writes);
        let entry = journal
            .entry(&last_id)
            .unwrap_or_else(|_| panic!("journal entry"));
        assert_eq!(entry.sends, u8::from(matches!(mode, CLEAN | LOST_RESPONSE)));
    }
    let journal = PublishJournal::new(temp.path().into());
    observer.mode.store(CLEAN, Ordering::Relaxed);
    assert!(
        plan_portable_dashboard_with_gateway(
            &service,
            &journal,
            &session,
            &gateway,
            &request("lost")
        )
        .await
        .is_err()
    );
    observer.mode.store(VERSION_CHANGED, Ordering::Relaxed);
    assert!(
        reconcile_create(&journal, &service, reader, &last_id)
            .await
            .is_err()
    );
    assert_eq!(
        journal
            .entry(&last_id)
            .unwrap_or_else(|_| panic!("unknown journal"))
            .state,
        JournalState::UnknownOutcome
    );
    observer.mode.store(CLEAN, Ordering::Relaxed);
    let result = reconcile_create(&journal, &service, reader, &last_id)
        .await
        .unwrap_or_else(|_| panic!("read-only recovery"));
    assert_eq!(result.entry.state, JournalState::ReconciledVerified);
    assert_eq!(result.entry.sends, 1);
    assert_eq!(observer.sends.load(Ordering::Relaxed), 2);
}

async fn bind_explicit(
    reader: &ReadOnlyGateway,
    service: &WorkcopyService,
    store: &OfflineDashboardStore,
    items: &[String],
) -> Vec<OfflineDashboard> {
    assert!((2..=6).contains(&items.len()));
    let fixtures = read_fixtures(reader, items).await;
    let mut drafts = Vec::new();
    for len in [2, items.len()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
    {
        let draft = store
            .save_imported(fixture(&fixtures[..len]))
            .unwrap_or_else(|error| panic!("Graph draft: {error}"));
        for (field_index, source) in fixtures[..len].iter().enumerate() {
            bind_item(reader, service, &draft, field_index, source).await;
        }
        drafts.push(draft);
    }
    drafts
}

async fn read_fixtures(reader: &ReadOnlyGateway, items: &[String]) -> Vec<SourceFixture> {
    let details = reader
        .items_by_ids(items)
        .await
        .unwrap_or_else(|_| panic!("lab item details"));
    let mut fixtures = Vec::new();
    for id in items {
        let item = details
            .iter()
            .find(|item| item.itemid == *id)
            .unwrap_or_else(|| panic!("fixture item"))
            .clone();
        let identity = if item.flags == ItemFlags::Discovered {
            let link = item
                .discovery
                .as_ref()
                .unwrap_or_else(|| panic!("fixture LLD link"));
            let identity = reader
                .portable_prototype_identity(&link.parent_itemid)
                .await
                .unwrap_or_else(|_| panic!("fixture prototype identity"));
            assert!(identity.is_some());
            identity
        } else if item.templateid.is_some() {
            let identity = reader
                .portable_item_identity(id)
                .await
                .unwrap_or_else(|_| panic!("fixture item identity"));
            assert!(identity.is_some());
            identity
        } else {
            None
        };
        fixtures.push(SourceFixture { item, identity });
    }
    fixtures
}

async fn bind_item(
    reader: &ReadOnlyGateway,
    service: &WorkcopyService,
    draft: &OfflineDashboard,
    field_index: usize,
    source: &SourceFixture,
) {
    let request = PortableLldReviewRequest {
        revision: draft.revision,
        page_index: 0,
        widget_index: 0,
        field_index,
        hostid: source.item.hostid.clone(),
    };
    let preview = service
        .review_portable_lld(reader, &draft.local_id, &request)
        .await
        .unwrap_or_else(|error| panic!("lab source review: {error}"));
    service
        .confirm_portable_lld(
            reader,
            &draft.local_id,
            &PortableLldConfirmRequest {
                review: request,
                itemid: Some(source.item.itemid.clone()),
                fingerprint: preview.fingerprint,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("lab bind: {error}"));
}

async fn bind_fields(
    reader: &ReadOnlyGateway,
    service: &WorkcopyService,
    store: &OfflineDashboardStore,
    fields: &[Value],
    hosts: &[String],
) -> OfflineDashboard {
    assert!((2..=3).contains(&hosts.len()));
    let slots: BTreeMap<_, _> = hosts
        .iter()
        .enumerate()
        .map(|(i, host)| (host.clone(), format!("slot-{}", i + 1)))
        .collect();
    let ids: Vec<_> = fields
        .iter()
        .filter(|field| field["type"] == "4")
        .map(|field| field["value"].as_str().unwrap_or_default().to_owned())
        .collect();
    let fixtures = read_fixtures(reader, &ids).await;
    let mut catalogs = serde_json::Map::new();
    let mut item_fields = Vec::new();
    let portable_fields: Vec<_> = fields.iter().enumerate().map(|(index,field)| {
        let kind = field["type"].as_str().and_then(|s| s.parse::<u16>().ok()).unwrap_or_else(|| panic!("field type"));
        let value = match (dynamic_field(field), kind) {
            (true, 1) => json!({"kind":"dynamic_pattern","role":"targets","value":field["value"]}),
            (false, 0) => json!({"kind":"integer","value":field["value"].as_str().and_then(|s| s.parse::<u32>().ok()).unwrap_or_else(|| panic!("numeric field"))}),
            (false, 1) => json!({"kind":"string","value":field["value"]}),
            (false, 4) => {
                let source = fixtures.iter().find(|source| field["value"] == source.item.itemid.as_str()).unwrap_or_else(|| panic!("row fixture"));
                let catalog = format!("series-{index}");
                catalogs.insert(catalog.clone(),metric_catalog(source,&catalog));
                item_fields.push((index,source));
                let slot = slots.get(&source.item.hostid).unwrap_or_else(|| panic!("row slot"));
                let mut value = json!({"kind":"metric","catalog":catalog,"metric":"item","hosts":{"role":"targets","selection":"slot","slot_id":slot}});
                if source.item.flags == ItemFlags::Discovered {
                    value["kind"] = json!("lld_instance"); value["source_key"] = json!(source.item.key);
                }
                value
            }
            _ => panic!("unproved field type"),
        };
        json!({"name":field["name"],"field_type":kind,"registry_state":"known","value":value})
    }).collect();
    let bundle = project_bundle(
        json!({"type":"zabbix","id":"graph","name":"Graph","widget_type":"svggraph","registry_state":"known",
        "layout":{"mode":"fixed","x":0,"y":0,"w":18,"h":4},"fields":portable_fields}),
        slots.values().cloned().collect(),
        catalogs,
    );
    let draft = store
        .save_imported(bundle)
        .unwrap_or_else(|error| panic!("masked fixture: {error}"));
    for (field, source) in item_fields {
        bind_item(reader, service, &draft, field, source).await;
    }
    if !fields.iter().any(dynamic_field) {
        return draft;
    }
    let targets: Vec<_> = reader
        .hosts_by_ids(hosts)
        .await
        .unwrap_or_else(|_| panic!("source hosts"))
        .into_iter()
        .map(|host| TargetSlot {
            slot_id: TargetSlotId(slots[&host.hostid].clone()),
            host,
        })
        .collect();
    assert_eq!(targets.len(), hosts.len());
    let mut exterior = false;
    for (field_index, _) in fields
        .iter()
        .enumerate()
        .filter(|(_, field)| dynamic_field(field))
    {
        let review = PortablePatternReviewRequest {
            revision: draft.revision,
            page_index: 0,
            widget_index: 0,
            field_index,
            targets: targets.clone(),
        };
        let preview = service
            .preview_saved_portable_pattern(reader, &draft.local_id, &review)
            .await
            .unwrap_or_else(|error| panic!("pattern preview: {error}"));
        let mut request = PortablePatternConfirmRequest {
            review,
            fingerprint: preview.fingerprint,
            acknowledge_visible_outside: false,
        };
        if preview.visible_matches.iter().any(|row| !row.selected) {
            exterior = true;
            assert!(
                service
                    .confirm_saved_portable_pattern(reader, &draft.local_id, &request)
                    .await
                    .is_err()
            );
            request.acknowledge_visible_outside = true;
        }
        service
            .confirm_saved_portable_pattern(reader, &draft.local_id, &request)
            .await
            .unwrap_or_else(|error| panic!("pattern proof confirmation: {error}"));
    }
    assert!(exterior);
    draft
}
