// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-02 through the real read-only gateway against a mock Zabbix JSON-RPC
//! transport on loopback. All data is synthetic.

use std::sync::{Arc, Mutex};

use axum::{Json, Router, extract::State, routing::post};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use zdashstitch_application::replace_host::{ChangeState, SessionContext};
use zdashstitch_application::resolver::{
    Applicability, Evidence, ItemOrigin, MatchStatus, ReasonCode, ReplaceHostRequest, ResolveScope,
    SourceValue, TargetValue, collect_source_facts, host_inventory, resolve_replace_host_with,
    source_model,
};
use zdashstitch_application::workcopy::{
    DraftFreshnessState, ImportRequest, ImportScope, WorkcopyService,
};
use zdashstitch_application::{FindingCode, PlanInvalidReason, recheck_replace_host};
use zdashstitch_zabbix::{
    AccessMode, AdapterRegistry, Authentication, ReadOnlyGateway, VersionedDashboardSnapshot,
    WidgetCapabilityRegistry, ZabbixClient, ZabbixEndpoint, dashboard_write_allowlist,
    read_only_allowlist,
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

fn eight_zero_fixture(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|error| panic!("8.0 fixture: {error}"))
}

fn inventory8() -> Value {
    eight_zero_fixture(include_str!(
        "../../../fixtures/zabbix/8.0/item-inventory-synthetic.json"
    ))
}

fn dashboard8() -> Value {
    eight_zero_fixture(include_str!(
        "../../../fixtures/zabbix/8.0/dashboard-get-synthetic.json"
    ))
}

fn eight_zero_read(method: &str, params: &Value) -> Value {
    let inventory = inventory8();
    let filter = |collection: &str, key: &str| {
        let requested = ids(params, &format!("{key}s"));
        Value::Array(
            inventory[collection]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|row| {
                    requested.as_ref().is_none_or(|ids| {
                        row[key]
                            .as_str()
                            .is_some_and(|id| ids.iter().any(|requested| requested == id))
                    })
                })
                .cloned()
                .collect(),
        )
    };
    match method {
        "dashboard.get" if params["dashboardids"] == json!(["8001"]) => json!([dashboard8()]),
        "dashboard.get" if params["dashboardids"] == json!(["8002"]) => json!([{
            "dashboardid":"8002", "pages":[{"widgets":[{"type":"item", "fields":{}}]}]
        }]),
        "host.get" if params["hostids"].is_array() => {
            let requested = ids(params, "hostids").unwrap_or_default();
            Value::Array(
                [
                    json!({"hostid":"8501", "host":"source", "name":"Synthetic source"}),
                    json!({"hostid":"8502", "host":"target", "name":"Synthetic target"}),
                ]
                .into_iter()
                .filter(|host| requested.iter().any(|id| host["hostid"] == id.as_str()))
                .collect(),
            )
        }
        "item.get" if params["itemids"] == json!(["8999"]) => filter("items", "hostid"),
        "item.get" if params["itemids"].is_array() => filter("items", "itemid"),
        "item.get" if params["hostids"] == json!(["8502"]) => json!([
                {"itemid":"8304", "hostid":"8502", "key_":"synthetic.json", "name":"Target binary", "value_type":"5", "flags":"0"},
                {"itemid":"8305", "hostid":"8502", "key_":"synthetic.discovery[alpha]", "name":"Target without visible prototype", "value_type":"6", "flags":"4", "discoveryData":{"parent_itemid":"8602", "key_":"synthetic.discovery[{#KEY}]"}}
        ]),
        "graph.get" if params["graphids"].is_array() => filter("graphs", "graphid"),
        "itemprototype.get" => filter("itemprototypes", "itemid"),
        "graphprototype.get" => filter("graphprototypes", "graphid"),
        _ => json!([]),
    }
}

async fn rpc(
    State((calls, version)): State<(Calls, &'static str)>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let id = body.get("id").cloned().unwrap_or(Value::Null);
    calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(body.clone());
    let params = &body["params"];
    let result = match body["method"].as_str().unwrap_or_default() {
        "apiinfo.version" => json!(version),
        "user.checkAuthentication" => json!({"userid": "3", "username": "synthetic"}),
        "item.get" if version == "8.0.2" && params["itemids"] == json!(["8998"]) => {
            return Json(json!({"jsonrpc": "2.0", "id": id, "error": {
                "code": -32500, "message": "Application error.",
                "data": "No permissions to call item.get."
            }}));
        }
        method if version == "8.0.2" => eight_zero_read(method, params),
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
    start_version("7.4.16").await
}

async fn start_version(version: &'static str) -> (ReadOnlyGateway, Calls, JoinHandle<()>) {
    let calls: Calls = Arc::default();
    let app = Router::new()
        .route("/api_jsonrpc.php", post(rpc))
        .with_state((Arc::clone(&calls), version));
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
async fn eight_zero_replace_host_reopens_and_refetches_without_editing_source_fields() {
    const CONNECTION: &str = "conn-00000000000000000000000000000080";
    let root = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let service = WorkcopyService::new(root.path().to_path_buf());
    let (reader, calls, task) = start_version("8.0.2").await;
    let session = SessionContext::Connected {
        access_mode: AccessMode::ReadOnly,
        capabilities: reader.capabilities(),
    };
    let imported = service
        .import(
            &reader,
            CONNECTION,
            &ImportRequest {
                dashboardid: "8001".to_owned(),
                scope: ImportScope::Dashboard,
                name: None,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("import: {error}"));
    let local_id = &imported.local_id;
    let request = ReplaceHostRequest {
        source_hostid: "8501".to_owned(),
        target_hostid: "8502".to_owned(),
        scope: ResolveScope::Dashboard,
    };
    let preview = service
        .calculate_replace_host(&reader, local_id, &request, session)
        .await
        .unwrap_or_else(|error| panic!("calculate: {error}"));
    assert!(preview.changes.is_empty());
    assert_eq!(preview.base_fingerprint, preview.local_fingerprint);
    assert!(!preview.report.allows_publication());
    assert!(
        preview
            .rows
            .iter()
            .any(|row| row.key.field == "itemid.0" && row.state == ChangeState::KeptOnSource)
    );
    assert!(
        preview
            .report
            .with_code(FindingCode::SourceUnverified)
            .any(|finding| finding
                .subject
                .as_ref()
                .and_then(|subject| subject.field.as_deref())
                == Some("opaque_item"))
    );
    assert!(preview.diff.summary.fields.unresolved > 0);

    let reopened = WorkcopyService::new(root.path().to_path_buf());
    let fresh = recheck_replace_host(&reopened, &reader, CONNECTION, local_id, session)
        .await
        .unwrap_or_else(|error| panic!("recheck: {error}"));
    assert!(fresh.source.is_current());
    assert!(fresh.remote_diff.is_none());
    assert!(!fresh.allows_plan());
    let copy = reopened
        .refetch_replace_host(
            &reader,
            CONNECTION,
            local_id,
            Some("Synthetic refetch".to_owned()),
            session,
        )
        .await
        .unwrap_or_else(|error| panic!("refetch: {error}"));
    assert!(copy.replayed);
    assert_ne!(copy.import.local_id, *local_id);
    for draft in [local_id, &copy.import.local_id] {
        assert_eq!(
            reopened
                .local_snapshot(draft)
                .unwrap_or_else(|error| panic!("local snapshot: {error}"))
                .raw_api_document(),
            &dashboard8()
        );
    }

    // A new patch makes the preview stale before reading any old IDs.
    let (upgraded, upgraded_calls, upgraded_task) = start_version("8.0.3").await;
    let upgraded_session = SessionContext::Connected {
        access_mode: AccessMode::ReadOnly,
        capabilities: upgraded.capabilities(),
    };
    let stale = recheck_replace_host(&reopened, &upgraded, CONNECTION, local_id, upgraded_session)
        .await
        .unwrap_or_else(|error| panic!("stale: {error}"));
    assert_eq!(stale.source.freshness.state, DraftFreshnessState::Stale);
    assert_eq!(
        stale.source.invalidated,
        Some(PlanInvalidReason::ConnectionChanged)
    );
    assert!(stale.remote_diff.is_none());
    assert!(stale.preview.checked_at_ms.is_none());
    assert!(!stale.allows_plan());
    assert_eq!(
        reopened
            .calculate_replace_host(&upgraded, local_id, &request, upgraded_session)
            .await
            .err()
            .map(|error| error.code()),
        Some("unsupported_version")
    );
    assert!(
        upgraded_calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    );

    let calls = calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(calls.iter().all(|call| {
        call["method"].as_str().is_some_and(|method| {
            read_only_allowlist().contains(&method)
                && !dashboard_write_allowlist().contains(&method)
        })
    }));
    task.abort();
    upgraded_task.abort();
}

#[tokio::test]
async fn experimental_eight_zero_resolver_keeps_json_and_restricted_lld_evidence_separate() {
    let (reader, calls, task) = start_version("8.0.2").await;
    let snapshot = reader
        .dashboard_snapshot("synthetic-connection", "8001")
        .await
        .unwrap_or_else(|error| panic!("8.0 read: {error}"))
        .unwrap_or_else(|| panic!("visible dashboard"));
    assert_eq!(snapshot.server_version().to_string(), "8.0.2");
    assert_eq!(snapshot.adapter_id(), "zabbix-8.0");
    assert_eq!(snapshot.raw_api_document(), &dashboard8());
    let registry = AdapterRegistry::widget_capabilities(snapshot.server_version())
        .unwrap_or_else(|error| panic!("8.0 registry: {error}"));
    assert!(source_model(&snapshot, &WidgetCapabilityRegistry::v7_4()).is_err());
    let model = source_model(&snapshot, &registry).unwrap_or_else(|error| panic!("model: {error}"));
    assert_eq!(
        model.itemids().into_iter().collect::<Vec<_>>(),
        ["8301", "8302", "8303", "8399"]
    );
    let facts = collect_source_facts(&reader, &model)
        .await
        .unwrap_or_else(|error| panic!("source inventory: {error}"));
    assert_eq!(facts.items["8302"].value_type, 6);
    assert_eq!(
        facts.items["8303"]
            .discovery
            .as_ref()
            .map(|link| link.parent_itemid.as_str()),
        Some("8601")
    );
    assert_eq!(facts.item_prototypes["8601"].value_type, 6);
    assert_eq!(
        facts.item_prototypes["8601"].rule_itemid.as_deref(),
        Some("8701")
    );
    assert_eq!(
        facts.graph_prototypes["8402"].rule_itemid.as_deref(),
        Some("8701")
    );
    assert_eq!(facts.graphs["8401"].hostids, ["8501"]);
    assert!(!facts.items.contains_key("8399"));
    assert!(
        host_inventory(&model, &facts)
            .unattributed
            .iter()
            .any(|source| { source.field.field_name == "ds.0.itemids.2" })
    );

    let report = resolve_replace_host_with(
        &reader,
        &model,
        &ReplaceHostRequest {
            source_hostid: "8501".to_owned(),
            target_hostid: "8502".to_owned(),
            scope: ResolveScope::Dashboard,
        },
    )
    .await
    .unwrap_or_else(|error| panic!("resolve: {error}"));
    assert!(
        report
            .rows
            .iter()
            .all(|row| row.applicability != Applicability::Editable)
    );
    let json_row = report
        .rows
        .iter()
        .find(|row| row.field.field_name == "ds.0.itemids.0")
        .unwrap_or_else(|| panic!("JSON row"));
    assert!(
        json_row
            .reasons
            .contains(&ReasonCode::ValueTypeIncompatible)
    );
    assert!(!json_row.reasons.contains(&ReasonCode::ValueTypeDiffers));
    let lld_row = report
        .rows
        .iter()
        .find(|row| row.field.field_name == "ds.0.itemids.1")
        .unwrap_or_else(|| panic!("LLD row"));
    assert_eq!(lld_row.match_status, MatchStatus::UnresolvedLld);
    assert!(lld_row.reasons.contains(&ReasonCode::PrototypeOwnerUnknown));
    assert!(lld_row.target.is_none());
    assert!(
        reader
            .dashboard_snapshot("synthetic-connection", "8003")
            .await
            .unwrap_or_else(|error| panic!("hidden read: {error}"))
            .is_none()
    );
    assert!(matches!(
        reader
            .dashboard_snapshot("synthetic-connection", "8002")
            .await,
        Err(zdashstitch_zabbix::ZabbixError::SnapshotShape(_))
    ));
    assert!(matches!(
        reader.items_by_ids(&["8999".to_owned()]).await,
        Err(zdashstitch_zabbix::ZabbixError::SnapshotShape(_))
    ));
    let denied = reader.items_by_ids(&["8998".to_owned()]).await;
    assert!(
        denied
            .err()
            .is_some_and(|error| error.is_permission_denied())
    );
    let calls = calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(
        calls
            .iter()
            .filter(|call| call["method"] == "item.get")
            .all(|call| {
                call["params"]["selectDiscoveryData"] == json!(["parent_itemid", "key_", "status"])
                    && call["params"].get("selectItemDiscovery").is_none()
                    && call["params"]["output"]
                        .as_array()
                        .is_some_and(|fields| fields.contains(&json!("status")))
            })
    );
    assert!(calls.iter().all(|call| {
        call["method"]
            .as_str()
            .is_some_and(|method| !dashboard_write_allowlist().contains(&method))
    }));
    task.abort();
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
