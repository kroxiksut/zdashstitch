// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-05A/B/C against a mock Zabbix JSON-RPC transport on loopback. All data
//! is synthetic: no real hostnames, addresses or dashboards.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::{Json, Router, extract::State, routing::post};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use zdashstitch_application::workcopy::{
    AccessCounts, AccessFilter, DashboardAccess, DashboardOwner, DraftKind, ImportRequest,
    ImportScope, ItemMetadataState, MAX_IMPORT_ATTEMPTS, ServerDashboardQuery, WorkcopyService,
};
use zdashstitch_core::formats::{
    build_typed_package_bytes, load_typed_project_file, read_typed_package_bytes,
};
use zdashstitch_zabbix::{
    Authentication, ReadOnlyGateway, ZabbixClient, ZabbixEndpoint, dashboard_write_allowlist,
    read_only_allowlist,
};

const CONNECTION: &str = "conn-00000000000000000000000000000001";
const LOCAL_MARKER: &str = "zds-local-only-unit-marker";

#[derive(Default)]
struct Script {
    /// Answers of `dashboard.get` with `dashboardids`, consumed in order;
    /// the last one repeats.
    snapshots: VecDeque<Value>,
    /// Answer of `dashboard.get` without `dashboardids` (the list).
    list: Value,
    /// Answer of `item.get`; `None` means «No permissions to call».
    items: Option<Value>,
    calls: Vec<Value>,
}

type Shared = Arc<Mutex<Script>>;

async fn rpc(State(script): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    let id = body.get("id").cloned().unwrap_or(Value::Null);
    let method = body["method"].as_str().unwrap_or_default().to_owned();
    let mut script = script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    script.calls.push(body.clone());
    let result = match method.as_str() {
        "apiinfo.version" => json!("7.4.16"),
        "user.checkAuthentication" => json!({"userid": "3", "username": "synthetic"}),
        "dashboard.get" if body["params"].get("dashboardids").is_some() => {
            if script.snapshots.len() > 1 {
                script.snapshots.pop_front().unwrap_or(Value::Null)
            } else {
                script
                    .snapshots
                    .front()
                    .cloned()
                    .unwrap_or_else(|| json!([]))
            }
        }
        "dashboard.get" => script.list.clone(),
        "item.get" => match &script.items {
            Some(items) => items.clone(),
            None => {
                return Json(json!({"jsonrpc": "2.0", "id": id, "error": {
                    "code": -32500, "message": "Application error.",
                    "data": "No permissions to call \"item.get\"."
                }}));
            }
        },
        _ => {
            return Json(json!({"jsonrpc": "2.0", "id": id, "error": {
                "code": -32602, "message": "Invalid params.", "data": "Not expected by the mock"
            }}));
        }
    };
    Json(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

struct Mock {
    script: Shared,
    reader: ReadOnlyGateway,
    _task: JoinHandle<()>,
}

impl Mock {
    async fn start(script: Script) -> Self {
        let script = Arc::new(Mutex::new(script));
        let app = Router::new()
            .route("/api_jsonrpc.php", post(rpc))
            .with_state(Arc::clone(&script));
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
        script
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .calls
            .clear();
        Self {
            script,
            reader,
            _task: task,
        }
    }

    fn methods(&self) -> Vec<String> {
        self.script
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .calls
            .iter()
            .map(|call| call["method"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    fn calls(&self) -> Vec<Value> {
        self.script
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .calls
            .clone()
    }
}

/// Two pages: known widgets with item references on the first, an unknown
/// widget type with unknown fields and properties on the second.
fn dashboard(name: &str) -> Value {
    json!({
        "dashboardid": "501",
        "name": name,
        "userid": "3",
        "private": "1",
        "display_period": "60",
        "auto_start": "0",
        "x_dashboard_extension": {"future": [1, 2, 3]},
        "users": [{"userid": "7", "permission": "2"}],
        "userGroups": [{"usrgrpid": "9", "permission": "3"}],
        "pages": [
            {
                "dashboard_pageid": "5010",
                "name": "Overview",
                "display_period": "0",
                "widgets": [
                    {
                        "widgetid": "50100", "type": "item", "name": "Synthetic value",
                        "x": "0", "y": "0", "width": "12", "height": "3", "view_mode": "0",
                        "fields": [
                            {"type": "4", "name": "itemid.0", "value": "30001"},
                            {"type": "1", "name": "reference", "value": "AAAAA"}
                        ]
                    },
                    {
                        "widgetid": "50101", "type": "svggraph", "name": "Synthetic graph",
                        "x": "12", "y": "0", "width": "36", "height": "5", "view_mode": "0",
                        "fields": [
                            {"type": "4", "name": "ds.0.itemids.0", "value": "30002"},
                            {"type": "1", "name": "ds.0.hosts.0", "value": "synthetic-host-*"},
                            {"type": "4", "name": "ds.0.itemids.1", "value": "30003"}
                        ]
                    }
                ]
            },
            {
                "dashboard_pageid": "5011",
                "name": "Vendor",
                "display_period": "120",
                "widgets": [
                    {
                        "widgetid": "50110", "type": "vendor.heatmap", "name": "",
                        "x": "0", "y": "0", "width": "72", "height": "8", "view_mode": "1",
                        "x_future": {"layers": ["a", "b"]},
                        "fields": [
                            {"type": "1", "name": "vendor.palette", "value": "synthetic"},
                            {"type": "99", "name": "vendor.unknown_type", "value": "kept"}
                        ]
                    }
                ]
            }
        ]
    })
}

fn items() -> Value {
    json!([
        {"itemid": "30001", "value_type": "3", "units": LOCAL_MARKER, "valuemapid": "11",
         "valuemap": {"valuemapid": "11", "name": "Synthetic state",
                      "mappings": [{"type": "0", "value": "1", "newvalue": "Up"},
                                   {"type": "0", "value": "0", "newvalue": "Down"}]}},
        {"itemid": "30002", "value_type": "0", "units": "%", "valuemapid": "0", "valuemap": []}
    ])
}

fn listing(id: &str, name: &str, userid: &str, private: &str) -> Value {
    json!({
        "dashboardid": id, "name": name, "userid": userid, "private": private,
        "pages": [{"dashboard_pageid": format!("{id}0"), "name": "", "widgets": [
            {"type": "item", "fields": [{"type": "4", "name": "itemid.0", "value": "1"}]},
            {"type": "vendor.x", "fields": []}
        ]}]
    })
}

fn storage() -> tempfile::TempDir {
    tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"))
}

fn files_under(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_under(&path));
        } else {
            found.push(path.display().to_string());
        }
    }
    found
}

fn assert_read_only(methods: &[String]) {
    let allowed = read_only_allowlist();
    let writes = dashboard_write_allowlist();
    for method in methods {
        assert!(allowed.contains(&method.as_str()), "{method} is not a read");
        assert!(!writes.contains(&method.as_str()), "{method} is a write");
        assert!(
            !method.ends_with(".create")
                && !method.ends_with(".update")
                && !method.ends_with(".delete"),
            "{method} changes the server"
        );
    }
}

fn request(scope: ImportScope) -> ImportRequest {
    ImportRequest {
        dashboardid: "501".to_owned(),
        scope,
        name: None,
    }
}

#[tokio::test]
async fn list_filters_sorts_pages_and_counts_editable_widgets() {
    let mock = Mock::start(Script {
        list: json!([
            listing("12", "beta shared", "8", "0"),
            listing("10", "Alpha mine", "3", "1"),
            listing("11", "gamma other private", "8", "1")
        ]),
        ..Script::default()
    })
    .await;
    let directory = storage();
    let service = WorkcopyService::new(directory.path().to_path_buf());

    let all = service
        .list_server_dashboards(&mock.reader, CONNECTION, &ServerDashboardQuery::default())
        .await
        .unwrap_or_else(|error| panic!("list: {error}"));
    let names: Vec<_> = all.rows.iter().map(|row| row.name.as_str()).collect();
    assert_eq!(names, ["Alpha mine", "beta shared", "gamma other private"]);
    assert_eq!(all.total, 3);
    assert_eq!(
        all.counts,
        AccessCounts {
            all: 3,
            mine: 1,
            shared: 1
        }
    );
    assert!(!all.truncated);
    assert_eq!(all.empty_reason, None);
    assert!(all.existing_copies_known);
    let alpha = &all.rows[0];
    assert_eq!(alpha.owner, DashboardOwner::Me);
    assert_eq!(alpha.access, DashboardAccess::Private);
    assert_eq!(
        (
            alpha.page_count,
            alpha.widget_count,
            alpha.editable_widget_count
        ),
        (1, 2, 1)
    );
    assert_eq!(all.rows[1].owner, DashboardOwner::Other);
    assert_eq!(all.rows[1].access, DashboardAccess::Shared);

    let mine = service
        .list_server_dashboards(
            &mock.reader,
            CONNECTION,
            &ServerDashboardQuery {
                access: AccessFilter::Mine,
                ..ServerDashboardQuery::default()
            },
        )
        .await
        .unwrap_or_else(|error| panic!("mine: {error}"));
    assert_eq!(mine.rows.len(), 1);
    assert_eq!(mine.rows[0].dashboardid, "10");

    let shared_page = service
        .list_server_dashboards(
            &mock.reader,
            CONNECTION,
            &ServerDashboardQuery {
                search: Some("  a ".to_owned()),
                access: AccessFilter::All,
                offset: 1,
                limit: 1,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("paged: {error}"));
    assert_eq!(shared_page.total, 3);
    assert_eq!(shared_page.rows.len(), 1);
    assert_eq!(shared_page.rows[0].name, "beta shared");
    let last = mock.calls().pop().unwrap_or(Value::Null);
    assert_eq!(last["params"]["search"], json!({"name": "a"}));
    assert_eq!(last["params"]["limit"], json!(1001));
    assert_read_only(&mock.methods());
}

#[tokio::test]
async fn empty_list_reports_missing_or_hidden_objects() {
    let mock = Mock::start(Script {
        list: json!([]),
        ..Script::default()
    })
    .await;
    let directory = storage();
    let service = WorkcopyService::new(directory.path().to_path_buf());
    let list = service
        .list_server_dashboards(&mock.reader, CONNECTION, &ServerDashboardQuery::default())
        .await
        .unwrap_or_else(|error| panic!("list: {error}"));
    assert!(list.rows.is_empty());
    assert_eq!(list.empty_reason, Some("not_visible_or_denied"));
}

#[tokio::test]
async fn invalid_queries_are_rejected_before_any_read() {
    let mock = Mock::start(Script::default()).await;
    let directory = storage();
    let service = WorkcopyService::new(directory.path().to_path_buf());
    let cases = [
        (
            CONNECTION,
            ServerDashboardQuery {
                limit: 0,
                ..ServerDashboardQuery::default()
            },
            "list_limit_invalid",
        ),
        (
            CONNECTION,
            ServerDashboardQuery {
                search: Some("x".repeat(256)),
                ..ServerDashboardQuery::default()
            },
            "search_invalid",
        ),
        (
            CONNECTION,
            ServerDashboardQuery {
                search: Some("a\u{7}b".to_owned()),
                ..ServerDashboardQuery::default()
            },
            "search_invalid",
        ),
        (
            "../conn",
            ServerDashboardQuery::default(),
            "connection_id_invalid",
        ),
    ];
    for (connection, query, code) in cases {
        let error = service
            .list_server_dashboards(&mock.reader, connection, &query)
            .await
            .err()
            .map(|error| error.code());
        assert_eq!(error, Some(code));
    }
    for (dashboardid, code) in [
        ("../501", "dashboard_id_invalid"),
        ("", "dashboard_id_invalid"),
    ] {
        let error = service
            .import(
                &mock.reader,
                CONNECTION,
                &ImportRequest {
                    dashboardid: dashboardid.to_owned(),
                    scope: ImportScope::Dashboard,
                    name: None,
                },
            )
            .await
            .err()
            .map(|error| error.code());
        assert_eq!(error, Some(code));
    }
    assert!(
        mock.methods().is_empty(),
        "no server call for invalid input"
    );
}

#[tokio::test]
async fn import_stores_base_workcopy_and_local_item_metadata() {
    let mock = Mock::start(Script {
        snapshots: VecDeque::from([json!([dashboard("Synthetic DB")])]),
        items: Some(items()),
        ..Script::default()
    })
    .await;
    let directory = storage();
    let service = WorkcopyService::new(directory.path().to_path_buf());

    let outcome = service
        .import(&mock.reader, CONNECTION, &request(ImportScope::Dashboard))
        .await
        .unwrap_or_else(|error| panic!("import: {error}"));
    assert_eq!(outcome.kind, DraftKind::Dashboard);
    assert_eq!(outcome.attempts, 1);
    assert!(!outcome.snapshot_reused);
    assert_eq!(outcome.item_metadata, ItemMetadataState::Partial);
    assert_eq!(
        mock.methods(),
        ["dashboard.get", "item.get", "dashboard.get"],
        "first read, item metadata, final read"
    );
    assert_read_only(&mock.methods());
    let item_call = &mock.calls()[1];
    assert_eq!(
        item_call["params"]["itemids"],
        json!(["30001", "30002", "30003"])
    );

    // Draft list and detail (read model of `ui/src/lib/shell/drafts.ts`).
    let list = service
        .list_drafts()
        .unwrap_or_else(|error| panic!("drafts: {error}"));
    assert_eq!(list.drafts.len(), 1);
    let summary = &list.drafts[0];
    assert_eq!(summary.local_id, outcome.local_id);
    assert_eq!(summary.name, "Synthetic DB");
    assert_eq!(summary.origin.as_str(), "server_copy");
    assert_eq!(summary.freshness.state.as_str(), "current");
    assert_eq!((summary.counts.pages, summary.counts.widgets), (2, 3));
    assert_eq!(summary.source.server_version, "7.4.16");
    assert_eq!(summary.source.connection_id, CONNECTION);

    let detail = service
        .draft(&outcome.local_id)
        .unwrap_or_else(|error| panic!("detail: {error}"));
    assert_eq!(detail.pages.len(), 2);
    assert_eq!(detail.pages[0].display_period, None);
    assert_eq!(detail.pages[1].display_period, Some(120));
    let editable: Vec<_> = detail
        .pages
        .iter()
        .flat_map(|page| page.widgets.iter())
        .map(|widget| (widget.widget_type.as_str(), widget.editable))
        .collect();
    assert_eq!(
        editable,
        [
            ("item", true),
            ("svggraph", true),
            ("vendor.heatmap", false)
        ]
    );
    assert_eq!(
        detail
            .properties
            .map(|properties| (properties.display_period, properties.auto_start)),
        Some((60, false))
    );

    // Unknown widget types, fields and properties are preserved in Base,
    // together with owner and sharing (O-031), which stay local.
    let base = service
        .base_snapshot(&outcome.local_id)
        .unwrap_or_else(|error| panic!("base: {error}"));
    assert_eq!(base.storage_fingerprint(), outcome.snapshot_fingerprint);
    let raw = base.raw_api_document();
    assert_eq!(raw, &dashboard("Synthetic DB"));
    assert_eq!(
        raw["pages"][1]["widgets"][0]["x_future"],
        json!({"layers": ["a", "b"]})
    );
    assert_eq!(raw["x_dashboard_extension"], json!({"future": [1, 2, 3]}));
    assert_eq!(raw["userGroups"][0]["usrgrpid"], "9");

    // I-4: value type, units and value map per referenced item.
    let metadata = service
        .item_metadata(&outcome.local_id)
        .unwrap_or_else(|error| panic!("metadata: {error}"));
    assert_eq!(metadata.items_state, ItemMetadataState::Partial);
    assert_eq!(metadata.missing_items, 1, "30003 is not visible");
    let first = &metadata.items["30001"];
    assert_eq!((first.value_type, first.units.as_str()), (3, LOCAL_MARKER));
    let map = &metadata.value_maps[first.valuemapid.as_deref().unwrap_or_default()];
    assert_eq!(map.name, "Synthetic state");
    assert_eq!(map.mappings.len(), 2);
    assert_eq!(metadata.items["30002"].valuemapid, None);

    // Layout and internal formats on disk.
    let files = files_under(directory.path());
    assert_eq!(files.len(), 2, "{files:?}");
    let snapshot_path = directory
        .path()
        .join("snapshots")
        .join(CONNECTION)
        .join("501")
        .join(format!("{}.json", outcome.snapshot_fingerprint));
    let workcopy_path = directory
        .path()
        .join("workcopies")
        .join(&outcome.local_id)
        .join("workcopy.json");
    let snapshot: Value = serde_json::from_slice(
        &std::fs::read(&snapshot_path).unwrap_or_else(|error| panic!("snapshot file: {error}")),
    )
    .unwrap_or_else(|error| panic!("snapshot json: {error}"));
    assert_eq!(snapshot["format"], "zdashstitch.raw-snapshot/v0");
    assert_eq!(snapshot["storage_scope"], "local_only");
    assert_eq!(
        snapshot["first_read_fingerprint"],
        snapshot["last_read_fingerprint"]
    );
    let workcopy: Value = serde_json::from_slice(
        &std::fs::read(&workcopy_path).unwrap_or_else(|error| panic!("workcopy file: {error}")),
    )
    .unwrap_or_else(|error| panic!("workcopy json: {error}"));
    assert_eq!(workcopy["format"], "zdashstitch.workcopy/v0");
    assert_eq!(workcopy["storage_scope"], "local_only");
    assert_eq!(workcopy["operations"], json!([]));
    assert_eq!(workcopy["created_dashboards"], json!([]));

    // A second copy reuses the identical immutable snapshot.
    let again = service
        .import(&mock.reader, CONNECTION, &request(ImportScope::Dashboard))
        .await
        .unwrap_or_else(|error| panic!("second import: {error}"));
    assert!(again.snapshot_reused);
    assert_ne!(again.local_id, outcome.local_id);
}

#[tokio::test]
async fn page_copy_keeps_the_whole_dashboard_as_base() {
    let mock = Mock::start(Script {
        snapshots: VecDeque::from([json!([dashboard("Synthetic DB")])]),
        items: Some(items()),
        ..Script::default()
    })
    .await;
    let directory = storage();
    let service = WorkcopyService::new(directory.path().to_path_buf());
    let outcome = service
        .import(
            &mock.reader,
            CONNECTION,
            &request(ImportScope::Page {
                dashboard_pageid: "5011".to_owned(),
            }),
        )
        .await
        .unwrap_or_else(|error| panic!("import: {error}"));
    assert_eq!(outcome.kind, DraftKind::Page);
    let detail = service
        .draft(&outcome.local_id)
        .unwrap_or_else(|error| panic!("detail: {error}"));
    assert_eq!(detail.summary.name, "Vendor");
    assert_eq!(detail.summary.source.page_number, Some(2));
    assert_eq!(detail.summary.source.page_name.as_deref(), Some("Vendor"));
    assert_eq!(
        (detail.summary.counts.pages, detail.summary.counts.widgets),
        (1, 1)
    );
    assert_eq!(detail.pages.len(), 1);
    assert_eq!(detail.pages[0].id, "5011");
    assert!(detail.properties.is_none());

    let error = service
        .import(
            &mock.reader,
            CONNECTION,
            &request(ImportScope::Page {
                dashboard_pageid: "9999".to_owned(),
            }),
        )
        .await
        .err()
        .map(|error| error.code());
    assert_eq!(error, Some("import_page_not_found"));
}

#[tokio::test]
async fn change_during_import_is_read_again_once() {
    let mock = Mock::start(Script {
        snapshots: VecDeque::from([
            json!([dashboard("Before")]),
            json!([dashboard("After")]),
            json!([dashboard("After")]),
        ]),
        items: Some(items()),
        ..Script::default()
    })
    .await;
    let directory = storage();
    let service = WorkcopyService::new(directory.path().to_path_buf());
    let outcome = service
        .import(&mock.reader, CONNECTION, &request(ImportScope::Dashboard))
        .await
        .unwrap_or_else(|error| panic!("import: {error}"));
    assert_eq!(outcome.attempts, 2);
    let base = service
        .base_snapshot(&outcome.local_id)
        .unwrap_or_else(|error| panic!("base: {error}"));
    assert_eq!(base.raw_api_document()["name"], "After");
    assert_eq!(
        mock.methods(),
        [
            "dashboard.get",
            "item.get",
            "dashboard.get",
            "dashboard.get",
            "item.get",
            "dashboard.get"
        ]
    );
}

#[tokio::test]
async fn second_change_fails_with_import_inconsistent_and_stores_nothing() {
    let mock = Mock::start(Script {
        snapshots: VecDeque::from([
            json!([dashboard("One")]),
            json!([dashboard("Two")]),
            json!([dashboard("Two")]),
            json!([dashboard("Three")]),
        ]),
        items: Some(items()),
        ..Script::default()
    })
    .await;
    let directory = storage();
    let service = WorkcopyService::new(directory.path().to_path_buf());
    let error = service
        .import(&mock.reader, CONNECTION, &request(ImportScope::Dashboard))
        .await
        .err()
        .unwrap_or_else(|| panic!("import must fail"));
    assert_eq!(error.code(), "import_inconsistent");
    assert_eq!(
        error.param("attempts"),
        Some(MAX_IMPORT_ATTEMPTS.to_string().as_str())
    );
    let dashboard_reads = mock
        .methods()
        .iter()
        .filter(|method| *method == "dashboard.get")
        .count();
    assert_eq!(dashboard_reads, 4, "bounded: two attempts of two reads");
    assert!(
        files_under(directory.path()).is_empty(),
        "no Base, no Local"
    );
    let list = service
        .list_drafts()
        .unwrap_or_else(|error| panic!("drafts: {error}"));
    assert!(list.drafts.is_empty());
}

#[tokio::test]
async fn dashboard_deleted_during_import_is_reported_as_not_visible() {
    let mock = Mock::start(Script {
        snapshots: VecDeque::from([json!([dashboard("Going")]), json!([]), json!([])]),
        items: Some(items()),
        ..Script::default()
    })
    .await;
    let directory = storage();
    let service = WorkcopyService::new(directory.path().to_path_buf());
    let error = service
        .import(&mock.reader, CONNECTION, &request(ImportScope::Dashboard))
        .await
        .err()
        .map(|error| error.code());
    assert_eq!(error, Some("not_visible_or_denied"));
    assert!(files_under(directory.path()).is_empty());
}

#[tokio::test]
async fn denied_item_get_keeps_the_copy_without_metadata() {
    let mock = Mock::start(Script {
        snapshots: VecDeque::from([json!([dashboard("Synthetic DB")])]),
        items: None,
        ..Script::default()
    })
    .await;
    let directory = storage();
    let service = WorkcopyService::new(directory.path().to_path_buf());
    let outcome = service
        .import(&mock.reader, CONNECTION, &request(ImportScope::Dashboard))
        .await
        .unwrap_or_else(|error| panic!("import: {error}"));
    assert_eq!(outcome.item_metadata, ItemMetadataState::Unavailable);
    let metadata = service
        .item_metadata(&outcome.local_id)
        .unwrap_or_else(|error| panic!("metadata: {error}"));
    assert!(metadata.items.is_empty());
    assert_eq!(metadata.missing_items, 3);
}

#[tokio::test]
async fn preview_and_list_show_existing_copies() {
    let mut list_entry = dashboard("Synthetic DB");
    list_entry
        .as_object_mut()
        .map(|object| object.remove("users"));
    let mock = Mock::start(Script {
        snapshots: VecDeque::from([json!([dashboard("Synthetic DB")])]),
        list: json!([list_entry]),
        items: Some(items()),
        ..Script::default()
    })
    .await;
    let directory = storage();
    let service = WorkcopyService::new(directory.path().to_path_buf());

    let preview = service
        .preview_import(&mock.reader, CONNECTION, "501")
        .await
        .unwrap_or_else(|error| panic!("preview: {error}"));
    assert!(preview.existing_copies.is_empty());
    assert_eq!(preview.owner, DashboardOwner::Me);
    assert_eq!(preview.access, DashboardAccess::Private);
    assert_eq!(preview.pages.len(), 2);
    assert_eq!(preview.pages[0].editable_widget_count, 2);
    assert_eq!(preview.pages[1].editable_widget_count, 0);
    let first_page: Vec<_> = preview.pages[0]
        .widgets
        .iter()
        .map(|widget| {
            (
                widget.name.as_str(),
                widget.widget_type.as_str(),
                widget.editable,
            )
        })
        .collect();
    assert_eq!(
        first_page,
        [
            ("Synthetic value", "item", true),
            ("Synthetic graph", "svggraph", true)
        ]
    );
    assert_eq!(preview.pages[1].widgets.len(), 1);
    assert!(!preview.pages[1].widgets[0].editable);
    assert_eq!(preview.unknown_widget_types, ["vendor.heatmap"]);
    assert_eq!(preview.dependencies.items, 3);
    assert_eq!(preview.dependencies.host_patterns, 1);
    assert!(preview.unknown_field_count >= 2);
    assert!(
        files_under(directory.path()).is_empty(),
        "preview stores nothing"
    );

    let outcome = service
        .import(&mock.reader, CONNECTION, &request(ImportScope::Dashboard))
        .await
        .unwrap_or_else(|error| panic!("import: {error}"));
    let list = service
        .list_server_dashboards(&mock.reader, CONNECTION, &ServerDashboardQuery::default())
        .await
        .unwrap_or_else(|error| panic!("list: {error}"));
    assert_eq!(list.rows[0].existing_copies.len(), 1);
    assert_eq!(list.rows[0].existing_copies[0].local_id, outcome.local_id);
    let other_connection = service
        .list_server_dashboards(
            &mock.reader,
            "conn-00000000000000000000000000000002",
            &ServerDashboardQuery::default(),
        )
        .await
        .unwrap_or_else(|error| panic!("list: {error}"));
    assert!(other_connection.rows[0].existing_copies.is_empty());

    let renamed = service
        .rename_draft(&outcome.local_id, "  My copy ")
        .unwrap_or_else(|error| panic!("rename: {error}"));
    assert_eq!(renamed.name, "My copy");
    assert_eq!(
        service
            .rename_draft(&outcome.local_id, "\u{1b}[31m")
            .err()
            .map(|error| error.code()),
        Some("draft_name_invalid")
    );
    assert_read_only(&mock.methods());
}

#[tokio::test]
async fn portable_export_never_contains_local_only_storage() {
    let mock = Mock::start(Script {
        snapshots: VecDeque::from([json!([dashboard("Synthetic DB")])]),
        items: Some(items()),
        ..Script::default()
    })
    .await;
    // The storage root deliberately lives inside the project directory.
    let project_directory = storage();
    let project = project_directory.path().join("project.yaml");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/formats/v2/positive/unknown-widget.yaml");
    std::fs::copy(&fixture, &project).unwrap_or_else(|error| panic!("copy fixture: {error}"));
    let service = WorkcopyService::new(project_directory.path().join("zdashstitch-data"));
    service
        .import(&mock.reader, CONNECTION, &request(ImportScope::Dashboard))
        .await
        .unwrap_or_else(|error| panic!("import: {error}"));

    let bundle =
        load_typed_project_file(&project).unwrap_or_else(|error| panic!("project: {error}"));
    let package =
        build_typed_package_bytes(&bundle).unwrap_or_else(|error| panic!("package: {error}"));
    let contains = |needle: &[u8]| package.windows(needle.len()).any(|window| window == needle);
    assert!(contains(b"project.yaml"));
    for forbidden in [
        b"workcopies".as_slice(),
        b"snapshots",
        b"workcopy.json",
        b"zdashstitch-data",
        LOCAL_MARKER.as_bytes(),
    ] {
        assert!(!contains(forbidden), "package names local-only storage");
    }
    let reread =
        read_typed_package_bytes(&package).unwrap_or_else(|error| panic!("re-read: {error}"));
    let rendered = format!("{reread:?}");
    assert!(!rendered.contains(LOCAL_MARKER));
    assert!(!rendered.contains("30001"));
}

/// The P0 snapshot digest, computed here independently of the library: the
/// name an earlier build gave the stored Base file.
fn p0_snapshot_digest(connection_id: &str, version: &str, raw: &Value) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(b"zdashstitch-dashboard-snapshot-v1\0");
    digest.update(connection_id.as_bytes());
    digest.update(b"\0");
    digest.update(version.as_bytes());
    digest.update(b"\0");
    digest.update(serde_json::to_vec(raw).unwrap_or_else(|error| panic!("raw: {error}")));
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Writes a Base snapshot and a dashboard workcopy exactly as a build from
/// before the canonical fingerprint (LAB-02) stored them.
fn store_pre_lab_02_workcopy(root: &Path, local_id: &str, raw: &Value) -> String {
    let fingerprint = p0_snapshot_digest(CONNECTION, "7.4.16", raw);
    let snapshots = root.join("snapshots").join(CONNECTION).join("501");
    std::fs::create_dir_all(&snapshots).unwrap_or_else(|error| panic!("dir: {error}"));
    let snapshot = json!({
        "format": "zdashstitch.raw-snapshot/v0",
        "storage_scope": "local_only",
        "connection_id": CONNECTION,
        "adapter_id": "zabbix-7.4",
        "server_version": "7.4.16",
        "dashboardid": "501",
        "scope": "dashboard",
        "first_read_at_ms": 1,
        "last_read_at_ms": 2,
        "first_read_fingerprint": fingerprint,
        "last_read_fingerprint": fingerprint,
        "fingerprint": fingerprint,
        "raw_api_document": raw,
    });
    std::fs::write(
        snapshots.join(format!("{fingerprint}.json")),
        serde_json::to_vec(&snapshot).unwrap_or_else(|error| panic!("json: {error}")),
    )
    .unwrap_or_else(|error| panic!("snapshot: {error}"));
    let workcopy_dir = root.join("workcopies").join(local_id);
    std::fs::create_dir_all(&workcopy_dir).unwrap_or_else(|error| panic!("dir: {error}"));
    let workcopy = json!({
        "format": "zdashstitch.workcopy/v0",
        "storage_scope": "local_only",
        "workcopy_id": local_id,
        "kind": "dashboard",
        "name": "Stored before LAB-02",
        "created_at_ms": 10,
        "updated_at_ms": 10,
        "revision": 0,
        "source": {
            "connection_id": CONNECTION,
            "dashboardid": "501",
            "dashboard_name": "Synthetic DB",
            "page": null,
            "adapter_id": "zabbix-7.4",
            "server_version": "7.4.16"
        },
        "base": {
            "fingerprint": fingerprint,
            "read_at_ms": 2,
            "source_checked_at_ms": 2,
            "page_count": 2,
            "widget_count": 3
        },
        "operations": [],
        "created_dashboards": [],
        "local_metadata": {
            "items_state": "not_needed", "items": {}, "value_maps": {}, "missing_items": 0
        }
    });
    std::fs::write(
        workcopy_dir.join("workcopy.json"),
        serde_json::to_vec(&workcopy).unwrap_or_else(|error| panic!("json: {error}")),
    )
    .unwrap_or_else(|error| panic!("workcopy: {error}"));
    fingerprint
}

/// LAB-02 migration: a workcopy stored by an earlier build keeps loading
/// (its Base file name is the unchanged storage fingerprint) and checks as
/// `current` against an unchanged Remote that Zabbix returns with widgets
/// and fields in another order; a real change is still a conflict. Nothing
/// stored is rewritten.
#[tokio::test]
async fn a_workcopy_stored_before_lab_02_checks_current_against_a_reordered_remote() {
    use zdashstitch_application::workcopy::DraftFreshnessState;

    let stored = dashboard("Synthetic DB");
    let mut reordered = stored.clone();
    for page in reordered["pages"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("pages"))
    {
        let widgets = page["widgets"]
            .as_array_mut()
            .unwrap_or_else(|| panic!("widgets"));
        widgets.reverse();
        for widget in widgets {
            widget["fields"]
                .as_array_mut()
                .unwrap_or_else(|| panic!("fields"))
                .reverse();
        }
    }
    let mut changed = reordered.clone();
    changed["pages"][0]["widgets"][1]["fields"][1]["value"] = json!("30009");
    assert_ne!(reordered, stored);

    let directory = storage();
    let local_id = "wc-0123456789abcdef0123456789abcdef";
    let fingerprint = store_pre_lab_02_workcopy(directory.path(), local_id, &stored);
    let before = files_under(directory.path());
    let service = WorkcopyService::new(directory.path().to_path_buf());
    let base = service
        .base_snapshot(local_id)
        .unwrap_or_else(|error| panic!("an earlier workcopy must load: {error}"));
    assert_eq!(base.storage_fingerprint(), fingerprint);

    let mock = Mock::start(Script {
        snapshots: VecDeque::from([json!([reordered]), json!([changed])]),
        ..Script::default()
    })
    .await;
    let current = service
        .check_source(&mock.reader, CONNECTION, local_id)
        .await
        .unwrap_or_else(|error| panic!("check: {error}"));
    assert_eq!(current.freshness.state, DraftFreshnessState::Current);
    assert!(current.is_current());
    assert_eq!(
        current.remote_fingerprint.as_deref(),
        Some(base.fingerprint())
    );

    let conflict = service
        .check_source(&mock.reader, CONNECTION, local_id)
        .await
        .unwrap_or_else(|error| panic!("check: {error}"));
    assert_eq!(conflict.freshness.state, DraftFreshnessState::Conflict);
    assert_read_only(&mock.methods());
    assert_eq!(
        files_under(directory.path()),
        before,
        "nothing is rewritten"
    );
}
