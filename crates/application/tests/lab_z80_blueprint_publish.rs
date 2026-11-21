// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::expect_used)]

use std::{env, fs};

use secrecy::SecretString;
use serde_json::{Value, json};
use zdashstitch_application::{
    JournalState, LocalSession, PublishJournal, ReplaceHostPlanRequest,
    catalog_library::{CatalogLibrary, inspect_template_file},
    plan_workcopy_blueprint, publish_replace_host_plan,
    reusable_library::{ReusableDocument, ReusableLibrary},
    workcopy::{
        CopyBlueprintRequest, ImportRequest, ImportScope, PublishedUpdate, WorkcopyService,
    },
};
use zdashstitch_zabbix::{Authentication, ZabbixClient, ZabbixEndpoint, is_zabbix_id};

const CONNECTION: &str = "conn-0123456789abcdef0123456789abcdef";
const BLUEPRINT_ID: &str = "91c0d2e4-6a1b-4f3c-8e5d-7b9a0c1d2e3f";

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("missing {name}"))
}

#[tokio::test]
async fn lost_append_response_never_resends_and_binding_changes_never_write() {
    use axum::{
        Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post,
    };
    use std::sync::{Arc, Mutex};
    use zdashstitch_zabbix::{BlueprintItemBinding, DashboardPublishOutcome};

    struct Server {
        raw: Value,
        units: &'static str,
        sends: u8,
        lose_reads: bool,
    }
    async fn rpc(
        State(server): State<Arc<Mutex<Server>>>,
        Json(body): Json<Value>,
    ) -> axum::response::Response {
        let mut server = server.lock().expect("synthetic state");
        let params = &body["params"];
        let result = match body["method"].as_str().unwrap_or_default() {
            "apiinfo.version" => json!("8.0.0"),
            "user.checkAuthentication" => {
                json!({"userid":"3","roleid":"9","type":2,"username":"synthetic"})
            }
            "role.get" => {
                json!([{"roleid":"9","type":"2","rules":{"api.access":"1","api.mode":"0","api":[],"actions":[{"name":"edit_dashboards","status":"1"}]}}])
            }
            "host.get" => json!([{"hostid":"101","status":"0"}]),
            "item.get" if params["countOutput"] == true => json!("1"),
            "item.get" => {
                json!([{"itemid":"201","hostid":"101","key_":"example.counter","name":"Item",
                "value_type":"3","units":server.units,"templateid":"0","status":"0","flags":"0","discoveryData":[]}])
            }
            "dashboard.get" if server.lose_reads => return StatusCode::BAD_GATEWAY.into_response(),
            "dashboard.get" => json!([server.raw]),
            "dashboard.update" => {
                server.sends += 1;
                assert_eq!(params["pages"][0], json!({"dashboard_pageid":"5"}));
                assert_eq!(params["pages"][1]["dashboard_pageid"], "6");
                assert_eq!(params["pages"][1]["widgets"][0], json!({"widgetid":"20"}));
                assert!(params["pages"][1]["widgets"][1].get("widgetid").is_none());
                assert!(params["pages"][1]["widgets"][1]["fields"][0]["value"] == "201");
                assert!(!params.to_string().contains("example.counter"));
                let mut inserted = params["pages"][1]["widgets"][1].clone();
                inserted["widgetid"] = json!("21");
                server.raw["pages"][1]["widgets"]
                    .as_array_mut()
                    .expect("widgets")
                    .push(inserted);
                server.lose_reads = true;
                return StatusCode::BAD_GATEWAY.into_response();
            }
            method => panic!("unexpected synthetic method {method}"),
        };
        Json(json!({"jsonrpc":"2.0","id":body["id"],"result":result})).into_response()
    }
    let server = Arc::new(Mutex::new(Server {
        raw: json!({"dashboardid":"4","name":"Synthetic target","private":"1","userid":"3","display_period":"30",
            "auto_start":"1","users":[],"userGroups":[],"pages":[
              {"dashboard_pageid":"5","name":"Untouched","display_period":"0","widgets":[]},
              {"dashboard_pageid":"6","name":"Page",
                "display_period":"0","widgets":[{"widgetid":"20","name":"Clock","type":"clock",
                    "x":"0","y":"0","width":"12","height":"3","view_mode":"0","fields":[]}]}]}),
        units: "B",
        sends: 0,
        lose_reads: false,
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let task = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/api_jsonrpc.php", post(rpc))
                .with_state(server.clone()),
        )
        .into_future(),
    );
    let endpoint =
        ZabbixEndpoint::parse(&format!("http://{address}/api_jsonrpc.php")).expect("endpoint");
    let gateway = ZabbixClient::new(endpoint)
        .expect("client")
        .authenticate(Authentication::ApiToken(SecretString::from("synthetic")))
        .await
        .expect("synthetic login")
        .enable_writes();
    let read = || async {
        gateway
            .reader()
            .dashboard_snapshot(CONNECTION, "4")
            .await
            .expect("read")
            .expect("snapshot")
    };
    let before = read().await;
    let candidate = || async {
        gateway
            .verify_blueprint_append(
                &before,
                "6",
                json!({"type":"item","name":"Added","x":"0","y":"3","width":"12","height":"3",
            "view_mode":"0","fields":[{"name":"itemid.0","type":"4","value":"201"}]}),
                vec![BlueprintItemBinding {
                    itemid: "201".into(),
                    hostid: "101".into(),
                    key: "example.counter".into(),
                    value_type: 3,
                    units: "B".into(),
                }],
                "f".repeat(64),
            )
            .await
            .expect("live evidence")
            .expect("verified append")
    };
    let stale = candidate().await;
    server.lock().expect("state").units = "s";
    assert_eq!(
        gateway
            .publish_verified_dashboard_update(&stale, || panic!("stale binding admitted"))
            .await,
        DashboardPublishOutcome::FailedBeforeSend
    );
    assert_eq!(server.lock().expect("state").sends, 0);
    server.lock().expect("state").units = "B";
    let stale = candidate().await;
    server.lock().expect("state").raw["pages"][0]["name"] = json!("Concurrent edit");
    assert_eq!(
        gateway
            .publish_verified_dashboard_update(&stale, || panic!("changed source admitted"))
            .await,
        DashboardPublishOutcome::Conflict
    );
    assert_eq!(server.lock().expect("state").sends, 0);
    server.lock().expect("state").raw = before.raw_api_document().clone();
    let update = candidate().await;
    assert_eq!(
        gateway
            .publish_verified_dashboard_update(&update, || true)
            .await,
        DashboardPublishOutcome::UnknownOutcome
    );
    server.lock().expect("state").lose_reads = false;
    assert_eq!(
        gateway
            .publish_verified_dashboard_update(&update, || panic!("duplicate send admitted"))
            .await,
        DashboardPublishOutcome::Conflict
    );
    assert_eq!(server.lock().expect("state").sends, 1);
    assert!(
        update
            .blueprint_readback()
            .expect("appended")
            .fingerprint(&read().await)
            .as_deref()
            == Some(update.expected_fingerprint())
    );
    server.lock().expect("state").raw = before.raw_api_document().clone();
    assert_eq!(
        gateway
            .publish_verified_dashboard_update(&update, || panic!("a consumed append was admitted"))
            .await,
        DashboardPublishOutcome::FailedBeforeSend
    );
    assert_eq!(server.lock().expect("state").sends, 1);
    task.abort();
}

#[tokio::test]
#[ignore = "requires the authorized private 8.0.0 blueprint fixtures via a loopback tunnel"]
async fn lab_z80_item_blueprint() {
    check("item", "item", 1, false).await;
}

#[tokio::test]
#[ignore = "requires the authorized private 8.0.0 blueprint fixtures via a loopback tunnel"]
async fn lab_z80_svggraph_blueprint() {
    check("svggraph", "svggraph", 2, false).await;
}

#[tokio::test]
#[ignore = "requires a private multi-page fixture on 8.0.0 via a loopback tunnel"]
async fn lab_z80_expanded_styled_item() {
    check("styled_item", "item", 1, true).await;
}

#[tokio::test]
#[ignore = "requires a private multi-page fixture on 8.0.0 via a loopback tunnel"]
async fn lab_z80_expanded_graph_one() {
    check("graph_one", "svggraph", 1, true).await;
}

#[tokio::test]
#[ignore = "requires a private multi-page fixture on 8.0.0 via a loopback tunnel"]
async fn lab_z80_expanded_graph_three() {
    check("graph_three", "svggraph", 3, true).await;
}

#[tokio::test]
#[ignore = "requires a private multi-page fixture on 8.0.0 via a loopback tunnel"]
async fn lab_z80_expanded_graph_four() {
    check("graph_four", "svggraph", 4, true).await;
}

#[tokio::test]
#[ignore = "requires a private multi-page fixture on 8.0.0 via a loopback tunnel"]
async fn lab_z80_expanded_reset_item() {
    check("reset_item", "item", 1, true).await;
}

fn float_item(item: &Value) -> bool {
    item["value_type"] == "0" || item["value_type"] == 0
}

async fn check(key: &str, kind: &str, item_count: usize, expanded: bool) {
    let background = match key {
        "styled_item" => Some("A1B2C3"),
        "reset_item" => Some(""),
        _ => None,
    };
    let fixture: Value = serde_json::from_slice(
        &fs::read(required("ZDASHSTITCH_LAB_BLUEPRINT_FIXTURE")).expect("local fixture file"),
    )
    .expect("local fixture data");
    let url = reqwest::Url::parse(&required("ZDASHSTITCH_LAB_API_URL")).expect("lab URL");
    assert!(url.host_str().is_some_and(|host| {
        host.parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.is_loopback())
    }));
    assert!(url.username().is_empty() && url.password().is_none());
    let id = fixture[key]["dashboardid"]
        .as_str()
        .expect("dashboard fixture id");
    let name = fixture[key]["name"]
        .as_str()
        .expect("dashboard fixture name");
    assert!(name.starts_with("ZDashstitch"));
    let items = fixture["items"].as_array().expect("item fixtures");
    let graph = kind == "svggraph";
    let items = &items[..item_count];
    let writer = ZabbixClient::new(ZabbixEndpoint::parse(url.as_str()).expect("endpoint"))
        .expect("client")
        .authenticate(Authentication::UsernamePassword {
            username: required("ZDASHSTITCH_LAB_USERNAME"),
            password: SecretString::from(required("ZDASHSTITCH_LAB_PASSWORD")),
        })
        .await
        .expect("lab authentication")
        .enable_writes();
    assert_eq!(writer.capabilities().version.to_string(), "8.0.0");
    let reader = writer.reader();
    let read = || async {
        reader
            .dashboard_snapshot(CONNECTION, id)
            .await
            .expect("read")
            .expect("visible fixture")
    };
    let base = read().await;
    let initial = base.raw_api_document();
    assert!(initial["name"] == name);
    assert!(initial["private"] == "1" || initial["private"] == 1);
    let pages = initial["pages"].as_array().expect("fixture pages");
    let page_index = if expanded {
        assert!(pages.len() > 1);
        let pageid = fixture[key]["pageid"].as_str().expect("target page");
        let index = pages
            .iter()
            .position(|page| page["dashboard_pageid"] == pageid)
            .expect("fixture includes target page");
        if key == "graph_four" {
            assert_eq!(index, 0);
            assert!(
                pages
                    .iter()
                    .skip(1)
                    .any(|page| page["widgets"].as_array().is_some_and(Vec::is_empty))
            );
        } else {
            assert!(index > 0);
        }
        index
    } else {
        assert_eq!(pages.len(), 1);
        0
    };
    let page = &pages[page_index];
    let widgets = page["widgets"].as_array().expect("fixture widgets");
    let x = 0;
    let y = if expanded {
        if key == "styled_item" {
            assert!(widgets.is_empty());
        } else {
            assert!(widgets.len() >= 2);
        }
        24
    } else {
        assert_eq!(widgets.len(), 1);
        assert!(widgets[0]["type"] == "clock");
        page["widgets"][0]["y"]
            .as_str()
            .expect("clock y")
            .parse::<u32>()
            .expect("y")
            + page["widgets"][0]["height"]
                .as_str()
                .expect("clock height")
                .parse::<u32>()
                .expect("height")
    };

    let root = tempfile::tempdir().expect("isolated local storage");
    let service = WorkcopyService::new(root.path().to_path_buf());
    let journal = PublishJournal::new(root.path().to_path_buf());
    let draft = service
        .import(
            reader,
            CONNECTION,
            &ImportRequest {
                dashboardid: id.to_owned(),
                scope: ImportScope::Dashboard,
                name: None,
            },
        )
        .await
        .expect("import dashboard");
    let mut export: Value = serde_json::from_str(include_str!(
        "../../../fixtures/zabbix/8.0/template-export-synthetic.json"
    ))
    .expect("export fixture");
    let template = &mut export["zabbix_export"]["templates"][0];
    template
        .as_object_mut()
        .expect("template")
        .remove("templates");
    template
        .as_object_mut()
        .expect("template")
        .remove("discovery_rules");
    template["items"] = json!(
        items
            .iter()
            .enumerate()
            .map(|(index, item)| json!({
                "name": format!("Blueprint metric {}", index + 1),
                "key": item["key_"], "units": item["units"],
                "value_type":if float_item(item) {"FLOAT"} else {"UNSIGNED"},
            }))
            .collect::<Vec<_>>()
    );
    let templates = inspect_template_file(
        "blueprint.json",
        &serde_json::to_vec(&export).expect("serialize export"),
    )
    .expect("known export")
    .expect("template import");
    let catalog = &templates[0].catalog;
    assert_eq!(catalog.metrics.len(), items.len());
    let mut roles = serde_json::Map::new();
    let mut choices = serde_json::Map::new();
    let mut role_specs = Vec::new();
    let mut metric_specs = Vec::new();
    let mut fields = Vec::new();
    if graph {
        fields.push(
            json!({"name":"ds.0.dataset_type","field_type":0,"value":{"kind":"integer","value":0}}),
        );
    } else if let Some(background) = background {
        fields.extend([
            json!({"name":"show.0","field_type":0,"value":{"kind":"integer","value":2}}),
            json!({"name":"bg_color","field_type":1,"value":{"kind":"string","value":background}}),
        ]);
    }
    for (index, item) in items.iter().enumerate() {
        let position = index + 1;
        let role = format!("host_{position}");
        let metric = format!("metric_{position}");
        let selected = catalog.metrics.iter().find(|metric| {
            matches!(&metric.selector, zdashstitch_core::formats::MetricSelector::Key(key) if item["key_"] == key.as_str())
        }).expect("catalog key");
        roles.insert(role.clone(), json!({"host":item["hostid"],"slots":{}}));
        choices.insert(
            metric.clone(),
            json!({"catalog_id":catalog.catalog_id,"version":1,"metric_id":selected.id}),
        );
        role_specs.push(json!({"param":role,"cardinality":"one"}));
        metric_specs.push(json!({
            "param":metric,
            "value_type":if float_item(item) {"numeric_float"} else {"numeric_unsigned"},
            "units":item["units"]
        }));
        let field_name = if graph {
            format!("ds.0.itemids.{index}")
        } else {
            "itemid.0".to_owned()
        };
        fields.push(json!({"name":field_name,"field_type":4,"value":{
            "kind":"metric","metric":format!("${metric}"),"role":format!("${role}"),"selection":"one"
        }}));
        if graph {
            fields.push(
                json!({"name":format!("ds.0.color.{index}"),"field_type":1,"value":{
                    "kind":"string","value":if index == 0 {"A1B2C3"} else {"C3B2A1"}
                }}),
            );
        }
    }
    CatalogLibrary::new(root.path().to_path_buf())
        .save(templates)
        .expect("save catalog");
    let blueprint = ReusableDocument::parse_yaml(
        &serde_json::to_vec(&json!({
            "format":"zdashstitch.widget-blueprint/v1", "blueprint_id":BLUEPRINT_ID,
            "name":"Blueprint lab widget", "version":1,"adapter":"8.0", "widget_type":kind,
            "default_size":{"width":24,"height":6},
            "parameters":{"roles":role_specs,"metrics":metric_specs},"fields":fields
        }))
        .expect("encode blueprint"),
    )
    .expect("validate blueprint");
    let asset = ReusableLibrary::new(root.path().to_path_buf())
        .create(blueprint)
        .expect("save blueprint");
    let request: CopyBlueprintRequest = serde_json::from_value(json!({
        "expected_revision":service.workcopy_blueprint_state(&draft.local_id).expect("draft").revision,
        "blueprint_id":asset.asset.id(),"blueprint_revision":asset.revision,
        "page_id":page["dashboard_pageid"],"roles":roles,"metrics":choices,"x":x,"y":y
    })).expect("insert request");
    let preview = service
        .insert_workcopy_blueprint(&draft.local_id, &request, Some(reader), None, false)
        .await
        .expect("preview exact assignments");
    assert!(preview.unresolved.is_empty());
    service
        .insert_workcopy_blueprint(
            &draft.local_id,
            &request,
            Some(reader),
            Some(&preview.fingerprint),
            false,
        )
        .await
        .expect("insert into workcopy");
    drop(service);
    let service = WorkcopyService::new(root.path().to_path_buf());
    let state = service
        .workcopy_blueprint_state(&draft.local_id)
        .expect("reopen workcopy");
    let publication = state.publication.expect("publication draft");
    let session = LocalSession::new();
    let request = ReplaceHostPlanRequest {
        local_id: draft.local_id.clone(),
        connection_id: CONNECTION.to_owned(),
        revision: state.revision,
        base_fingerprint: publication.base_fingerprint,
        local_fingerprint: publication.local_fingerprint,
        kept_digest: None,
        session: session.current(),
    };
    let mut stale = request.clone();
    stale.local_fingerprint = "0".repeat(64);
    assert!(matches!(
        plan_workcopy_blueprint(&service, &journal, &session, &writer, &stale).await,
        Err(zdashstitch_application::PlanError::Invalidated(
            zdashstitch_application::PlanInvalidReason::DraftChanged
        ))
    ));
    let plan = plan_workcopy_blueprint(&service, &journal, &session, &writer, &request)
        .await
        .unwrap_or_else(|error| match error {
            zdashstitch_application::PlanError::Adapter(ref rejection) => {
                panic!("plan refused: {error}; {:?}", rejection.params());
            }
            _ => panic!("plan refused: {error}"),
        });
    let id = plan.preview().plan_id;
    assert_eq!(plan.preview().changed_pages, [page_index]);
    let confirmed = plan.confirm(&id).expect("confirm exact plan");
    let stage = session
        .begin_write(confirmed.session())
        .expect("write stage");
    let report = publish_replace_host_plan(&service, &writer, &journal, confirmed, &stage).await;
    drop(stage);
    assert_eq!(report.states.last(), Some(&JournalState::Verified));
    drop(journal);
    let journal = PublishJournal::new(root.path().to_path_buf());
    let entry = journal.entry(&id).expect("reopen journal");
    assert_eq!(entry.sends, 1);
    let after = read().await;
    let content = after.raw_api_document();
    let mut unchanged = content.clone();
    let added = unchanged["pages"][page_index]["widgets"]
        .as_array_mut()
        .expect("read page")
        .pop()
        .expect("added widget");
    assert!(added["widgetid"].as_str().is_some_and(is_zabbix_id));
    assert!(
        !pages
            .iter()
            .flat_map(|page| page["widgets"].as_array().into_iter().flatten())
            .any(|widget| widget["widgetid"] == added["widgetid"])
    );
    assert!(added["type"] == kind);
    let fields = added["fields"].as_array().expect("published fields");
    for (index, item) in items.iter().enumerate() {
        let name = if graph {
            format!("ds.0.itemids.{index}")
        } else {
            "itemid.0".into()
        };
        assert!(
            fields
                .iter()
                .any(|field| field["name"] == name && field["value"] == item["itemid"])
        );
    }
    if let Some(background) = background {
        assert!(
            fields
                .iter()
                .any(|field| field["name"] == "show.0" && field["value"] == "2")
        );
        assert!(
            fields
                .iter()
                .any(|field| field["name"] == "bg_color" && field["value"] == background)
        );
    }
    assert!(unchanged == *initial, "original content differed");
    assert!(matches!(
        plan_workcopy_blueprint(&service, &journal, &session, &writer, &request).await,
        Err(zdashstitch_application::PlanError::Invalidated(
            zdashstitch_application::PlanInvalidReason::SourceChanged
        ))
    ));
    assert!(
        entry
            .blueprint_readback()
            .expect("read-back")
            .fingerprint(&after)
            .is_some_and(|fingerprint| fingerprint == entry.expected_fingerprint)
    );
    let advanced = service
        .advance_blueprint_base(
            reader,
            &PublishedUpdate {
                local_id: draft.local_id.clone(),
                connection_id: CONNECTION.to_owned(),
                revision: state.revision,
                source_fingerprint: entry.source_fingerprint.clone(),
                expected_fingerprint: entry.expected_fingerprint.clone(),
            },
            entry.blueprint_readback().expect("retained projection"),
        )
        .await
        .expect("save assigned widget id in the new immutable base");
    assert!(advanced.base_fingerprint == after.fingerprint());
    assert!(
        !service
            .workcopy_blueprint_state(&draft.local_id)
            .expect("advanced workcopy")
            .has_blueprint
    );
}
