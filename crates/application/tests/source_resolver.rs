// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-02 through the real read-only gateway against a mock Zabbix JSON-RPC
//! transport on loopback. All data is synthetic.

use std::sync::{Arc, Mutex};

use axum::{Json, Router, extract::State, routing::post};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use zdashstitch_application::resolver::{
    Evidence, ItemOrigin, MatchStatus, ReasonCode, ReplaceHostRequest, ResolveScope, SourceValue,
    TargetValue, resolve_replace_host_with, source_model,
};
use zdashstitch_zabbix::{
    Authentication, ReadOnlyGateway, VersionedDashboardSnapshot, WidgetCapabilityRegistry,
    ZabbixClient, ZabbixEndpoint, dashboard_write_allowlist, read_only_allowlist,
};

type Calls = Arc<Mutex<Vec<Value>>>;

fn hosts() -> Vec<Value> {
    vec![
        json!({"hostid": "11", "host": "db_old_01", "name": "db-old-01"}),
        json!({"hostid": "12", "host": "db_new_01", "name": "db-new-01"}),
    ]
}

fn items() -> Vec<Value> {
    vec![
        json!({"itemid": "101", "hostid": "11", "key_": "pgsql.ping", "name": "PostgreSQL ping",
               "value_type": "3", "units": "", "flags": "0", "templateid": "901",
               "itemDiscovery": [], "hosts": [{"hostid": "11", "name": "db-old-01"}]}),
        json!({"itemid": "201", "hostid": "12", "key_": "pgsql.ping", "name": "PostgreSQL ping",
               "value_type": "3", "units": "", "flags": "0", "templateid": "901",
               "itemDiscovery": []}),
        json!({"itemid": "901", "hostid": "800", "key_": "pgsql.ping", "name": "PostgreSQL ping",
               "value_type": "3", "units": "", "flags": "0", "templateid": "0",
               "itemDiscovery": [], "hosts": [{"hostid": "800", "name": "Synthetic template"}]}),
    ]
}

fn item_prototypes() -> Vec<Value> {
    vec![
        json!({"itemid": "501", "hostid": "11", "key_": "pgsql.db.size[{#DBNAME}]",
               "name": "DB size", "value_type": "3", "units": "B", "templateid": "510",
               "hosts": [{"hostid": "11", "name": "db-old-01"}],
               "discoveryRule": {"itemid": "400"}}),
        json!({"itemid": "510", "hostid": "800", "key_": "pgsql.db.size[{#DBNAME}]",
               "name": "DB size", "value_type": "3", "units": "B", "templateid": "0",
               "hosts": [{"hostid": "800", "name": "Synthetic template"}],
               "discoveryRule": {"itemid": "399"}}),
        json!({"itemid": "502", "hostid": "12", "key_": "pgsql.db.size[{#DBNAME}]",
               "name": "DB size", "value_type": "3", "units": "B", "templateid": "510",
               "hosts": [{"hostid": "12", "name": "db-new-01"}],
               "discoveryRule": {"itemid": "401"}}),
    ]
}

fn graph_prototypes() -> Vec<Value> {
    vec![
        json!({"graphid": "351", "name": "DB graph", "templateid": "650",
                "hosts": [{"hostid": "11", "name": "db-old-01"}],
                "discoveryRule": {"itemid": "400"}}),
    ]
}

fn ids(params: &Value, key: &str) -> Option<Vec<String>> {
    params.get(key).and_then(Value::as_array).map(|values| {
        values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    })
}

async fn rpc(State(calls): State<Calls>, Json(body): Json<Value>) -> Json<Value> {
    let id = body.get("id").cloned().unwrap_or(Value::Null);
    calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(body.clone());
    let params = &body["params"];
    let result = match body["method"].as_str().unwrap_or_default() {
        "apiinfo.version" => json!("7.4.16"),
        "user.checkAuthentication" => json!({"userid": "3", "username": "synthetic"}),
        "host.get" => {
            let by_id = ids(params, "hostids");
            let by_name = params["search"]["name"].as_array().cloned();
            Value::Array(
                hosts()
                    .into_iter()
                    .filter(|host| {
                        by_id.as_ref().is_none_or(|ids| {
                            ids.iter()
                                .any(|id| Some(id.as_str()) == host["hostid"].as_str())
                        }) && by_name.as_ref().is_none_or(|names| {
                            names.iter().any(|name| {
                                name.as_str().map(str::to_lowercase)
                                    == host["name"].as_str().map(str::to_lowercase)
                            })
                        })
                    })
                    .collect(),
            )
        }
        "item.get" => {
            let by_id = ids(params, "itemids");
            let by_host = ids(params, "hostids");
            Value::Array(
                items()
                    .into_iter()
                    .filter(|item| {
                        by_id.as_ref().is_none_or(|ids| {
                            ids.iter()
                                .any(|id| Some(id.as_str()) == item["itemid"].as_str())
                        }) && by_host.as_ref().is_none_or(|ids| {
                            ids.iter()
                                .any(|id| Some(id.as_str()) == item["hostid"].as_str())
                        })
                    })
                    .collect(),
            )
        }
        "graph.get" => json!([]),
        "itemprototype.get" => {
            let by_id = ids(params, "itemids").unwrap_or_default();
            Value::Array(
                item_prototypes()
                    .into_iter()
                    .filter(|item| {
                        by_id
                            .iter()
                            .any(|id| Some(id.as_str()) == item["itemid"].as_str())
                    })
                    .collect(),
            )
        }
        "graphprototype.get" => {
            let by_id = ids(params, "graphids").unwrap_or_default();
            Value::Array(
                graph_prototypes()
                    .into_iter()
                    .filter(|graph| {
                        by_id
                            .iter()
                            .any(|id| Some(id.as_str()) == graph["graphid"].as_str())
                    })
                    .collect(),
            )
        }
        _ => {
            return Json(json!({"jsonrpc": "2.0", "id": id, "error": {
                "code": -32602, "message": "Invalid params.", "data": "Not expected by the mock"
            }}));
        }
    };
    Json(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

async fn start() -> (ReadOnlyGateway, Calls, JoinHandle<()>) {
    let calls: Calls = Arc::default();
    let app = Router::new()
        .route("/api_jsonrpc.php", post(rpc))
        .with_state(Arc::clone(&calls));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("address: {error}"));
    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            panic!("mock API failed: {error}");
        }
    });
    let endpoint = ZabbixEndpoint::parse(&format!("http://{address}/api_jsonrpc.php"))
        .unwrap_or_else(|error| panic!("endpoint: {error}"));
    let reader = ZabbixClient::new(endpoint)
        .unwrap_or_else(|error| panic!("client: {error}"))
        .authenticate(Authentication::ApiToken(SecretString::from(
            "synthetic-token".to_owned(),
        )))
        .await
        .unwrap_or_else(|error| panic!("sign-in: {error}"));
    calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    (reader, calls, task)
}

#[tokio::test]
async fn resolver_reads_through_the_read_only_gateway_only() {
    let (reader, calls, _task) = start().await;
    let version = "7.4.16"
        .parse()
        .unwrap_or_else(|error| panic!("version: {error}"));
    let snapshot = VersionedDashboardSnapshot::from_adapter_raw(
        "conn-test".to_owned(),
        version,
        json!({"dashboardid": "900", "name": "Synthetic", "pages": [
            {"dashboard_pageid": "1000", "name": "", "widgets": [
                {"widgetid": "2000", "type": "item", "name": "", "fields": [
                    {"type": "4", "name": "itemid.0", "value": "101"}
                ]},
                {"widgetid": "2001", "type": "svggraph", "name": "", "fields": [
                    {"type": "1", "name": "ds.0.hosts.0", "value": "db-old-01"},
                    {"type": "1", "name": "ds.0.items.0", "value": "PostgreSQL*"}
                ]}
            ]}
        ]}),
    )
    .unwrap_or_else(|error| panic!("snapshot: {error}"));
    let model = source_model(&snapshot, &WidgetCapabilityRegistry::v7_4())
        .unwrap_or_else(|error| panic!("model: {error}"));
    let request = ReplaceHostRequest {
        source_hostid: "11".to_owned(),
        target_hostid: "12".to_owned(),
        scope: ResolveScope::Dashboard,
    };
    let resolution = resolve_replace_host_with(&reader, &model, &request)
        .await
        .unwrap_or_else(|error| panic!("resolve: {error}"));

    assert_eq!(resolution.rows.len(), 2);
    let item = &resolution.rows[0];
    assert_eq!(item.match_status, MatchStatus::Exact);
    assert_eq!(
        item.target,
        Some(TargetValue::Item {
            itemid: "201".to_owned()
        })
    );
    let pattern = &resolution.rows[1];
    assert_eq!(pattern.match_status, MatchStatus::Exact);
    assert_eq!(
        pattern.target,
        Some(TargetValue::HostPattern {
            pattern: "db-new-01".to_owned()
        })
    );
    assert_eq!(pattern.pattern_coverage[0].target_matches, 1);
    assert!(resolution.kept_on_source.is_empty());

    let calls = calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let allowed = read_only_allowlist();
    let writes = dashboard_write_allowlist();
    for call in &calls {
        let method = call["method"].as_str().unwrap_or_default();
        assert!(allowed.contains(&method), "{method} is not allowlisted");
        assert!(!writes.contains(&method), "{method} writes");
    }
    // Host patterns are looked up as the frontend evaluates them.
    let by_name = calls
        .iter()
        .find(|call| call["method"] == "host.get" && call["params"].get("search").is_some())
        .unwrap_or_else(|| panic!("host lookup by visible name"));
    assert_eq!(by_name["params"]["searchWildcardsEnabled"], json!(true));
    assert_eq!(by_name["params"]["search"]["name"], json!(["db-old-01"]));
    // LLD evidence is requested with every item read.
    assert!(
        calls
            .iter()
            .filter(|call| call["method"] == "item.get")
            .all(|call| call["params"]["selectItemDiscovery"].is_array())
    );
}

#[tokio::test]
async fn prototype_owners_are_read_with_read_methods_only() {
    let (reader, calls, _task) = start().await;
    let version = "7.4.16"
        .parse()
        .unwrap_or_else(|error| panic!("version: {error}"));
    let snapshot = VersionedDashboardSnapshot::from_adapter_raw(
        "conn-test".to_owned(),
        version,
        json!({"dashboardid": "901", "name": "Synthetic", "pages": [
            {"dashboard_pageid": "1000", "name": "", "widgets": [
                {"widgetid": "2000", "type": "vendor.widget", "name": "", "fields": [
                    {"type": "5", "name": "itemid.0", "value": "501"}
                ]},
                {"widgetid": "2001", "type": "graphprototype", "name": "", "fields": [
                    {"type": "7", "name": "graphid.0", "value": "351"}
                ]},
                {"widgetid": "2002", "type": "vendor.widget", "name": "", "fields": [
                    {"type": "5", "name": "itemid.0", "value": "502"}
                ]},
                {"widgetid": "2003", "type": "graphprototype", "name": "", "fields": [
                    {"type": "7", "name": "graphid.0", "value": "999"}
                ]}
            ]}
        ]}),
    )
    .unwrap_or_else(|error| panic!("snapshot: {error}"));
    let model = source_model(&snapshot, &WidgetCapabilityRegistry::v7_4())
        .unwrap_or_else(|error| panic!("model: {error}"));
    let request = ReplaceHostRequest {
        source_hostid: "11".to_owned(),
        target_hostid: "12".to_owned(),
        scope: ResolveScope::Dashboard,
    };
    let resolution = resolve_replace_host_with(&reader, &model, &request)
        .await
        .unwrap_or_else(|error| panic!("resolve: {error}"));

    // 501 and 351 belong to the source host: listed, never changed. 502
    // belongs to the target host: not a source reference. 999 is invisible.
    assert_eq!(resolution.rows.len(), 2);
    assert!(resolution.rows.iter().all(|row| !row.will_change()));
    assert_eq!(
        resolution.rows[0].source,
        SourceValue::ItemPrototype {
            itemid: "501".to_owned()
        }
    );
    let Some(Evidence::Item(evidence)) = &resolution.rows[0].source_evidence else {
        panic!("item prototype evidence");
    };
    assert_eq!(
        evidence.origin,
        ItemOrigin::Template {
            template_itemid: "510".to_owned(),
            template_name: Some("Synthetic template".to_owned()),
        }
    );
    assert_eq!(
        resolution.rows[1].source,
        SourceValue::GraphPrototype {
            graphid: "351".to_owned()
        }
    );
    assert_eq!(resolution.kept_on_source.len(), 2);
    assert_eq!(resolution.unverified.len(), 1);
    assert_eq!(
        resolution.unverified[0].reason,
        ReasonCode::PrototypeOwnerUnknown
    );

    let calls = calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let allowed = read_only_allowlist();
    let writes = dashboard_write_allowlist();
    let methods: Vec<&str> = calls
        .iter()
        .map(|call| call["method"].as_str().unwrap_or_default())
        .collect();
    for method in &methods {
        assert!(allowed.contains(method), "{method} is not allowlisted");
        assert!(!writes.contains(method), "{method} writes");
        assert!(method.ends_with(".get"), "{method} is not a read");
    }
    assert!(methods.contains(&"itemprototype.get"));
    assert!(methods.contains(&"graphprototype.get"));
    // Prototype IDs are never sent to `item.get` / `graph.get`.
    assert!(calls.iter().all(|call| {
        call["method"] != "item.get" || call["params"]["itemids"] != json!(["501", "502"])
    }));
    for call in calls.iter().filter(|call| {
        call["method"] == "itemprototype.get" || call["method"] == "graphprototype.get"
    }) {
        assert!(call["params"]["selectHosts"].is_array());
    }
}
