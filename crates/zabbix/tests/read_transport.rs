// SPDX-License-Identifier: MIT OR Apache-2.0

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use zdashstitch_zabbix::{
    AdapterRegistry, Authentication, CapabilityState, DependencyKind, DependencyState,
    MethodOutcome, SnapshotSummary, ZabbixClient, ZabbixEndpoint, ZabbixError, ZabbixVersion,
};

const SNAPSHOT_FIXTURE: &str =
    include_str!("../../../fixtures/zabbix/7.4/dashboard-get-mixed.json");

#[derive(Clone, Debug)]
struct Captured {
    authorization: Option<String>,
    body: Value,
}

#[derive(Clone)]
struct MockState {
    calls: Arc<Mutex<Vec<Captured>>>,
}

async fn rpc(
    State(state): State<MockState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let authorization = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    state
        .calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(Captured {
            authorization: authorization.clone(),
            body: body.clone(),
        });

    let id = body.get("id").cloned().unwrap_or(Value::Null);
    let method = body
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let response = match method {
        "apiinfo.version" => json!({ "jsonrpc": "2.0", "result": "7.4.3", "id": id }),
        "user.login" => json!({ "jsonrpc": "2.0", "result": "session-secret", "id": id }),
        "user.logout" => json!({ "jsonrpc": "2.0", "result": true, "id": id }),
        "graph.get" => json!({
            "jsonrpc": "2.0",
            "error": { "code": -32500, "message": "Application error.", "data": "No permissions to referred object" },
            "id": id
        }),
        "dashboard.get"
            if authorization.as_deref() == Some("Bearer api-secret")
                || authorization.as_deref() == Some("Bearer session-secret") =>
        {
            let fixture: Value = serde_json::from_str(SNAPSHOT_FIXTURE)
                .unwrap_or_else(|error| panic!("snapshot fixture must parse: {error}"));
            json!({ "jsonrpc": "2.0", "result": fixture, "id": id })
        }
        _ if authorization.as_deref() == Some("Bearer api-secret")
            || authorization.as_deref() == Some("Bearer session-secret") =>
        {
            json!({ "jsonrpc": "2.0", "result": [], "id": id })
        }
        _ => json!({
            "jsonrpc": "2.0",
            "error": { "code": -32602, "message": "Invalid params.", "data": "Not authorized" },
            "id": id
        }),
    };
    (StatusCode::OK, Json(response))
}

async fn mock_server() -> (ZabbixEndpoint, Arc<Mutex<Vec<Captured>>>, JoinHandle<()>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/api_jsonrpc.php", post(rpc))
        .with_state(MockState {
            calls: Arc::clone(&calls),
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("cannot bind mock API: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("cannot read mock address: {error}"));
    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            panic!("mock API failed: {error}");
        }
    });
    let endpoint = ZabbixEndpoint::parse(&format!("http://{address}/api_jsonrpc.php"))
        .unwrap_or_else(|error| panic!("mock endpoint must be valid: {error}"));
    (endpoint, calls, task)
}

#[tokio::test]
async fn api_token_is_only_sent_as_bearer_and_inventory_diagnoses_permissions() {
    let (endpoint, calls, task) = mock_server().await;
    let client = ZabbixClient::new(endpoint)
        .unwrap_or_else(|error| panic!("client must be constructed: {error}"))
        .authenticate(Authentication::ApiToken(SecretString::from(
            "api-secret".to_owned(),
        )))
        .await
        .unwrap_or_else(|error| panic!("token authentication must succeed: {error}"));
    assert_eq!(client.capabilities().version.to_string(), "7.4.3");
    assert_eq!(
        client.capabilities().read_inventory,
        CapabilityState::Supported
    );
    assert_eq!(
        client.capabilities().dashboard_write,
        CapabilityState::Unsupported
    );

    let inventory = client
        .inventory()
        .await
        .unwrap_or_else(|error| panic!("inventory must complete: {error}"));
    assert!(matches!(inventory.template, MethodOutcome::Available(_)));
    assert!(matches!(
        inventory.graph,
        MethodOutcome::PermissionDenied {
            method: "graph.get",
            ..
        }
    ));
    client
        .logout()
        .await
        .unwrap_or_else(|error| panic!("API token logout must be a no-op: {error}"));

    let captured = calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(
        captured
            .first()
            .and_then(|call| call.authorization.as_deref()),
        None
    );
    assert!(
        captured
            .iter()
            .skip(1)
            .all(|call| call.authorization.as_deref() == Some("Bearer api-secret"))
    );
    assert!(
        captured
            .iter()
            .all(|call| !call.body.to_string().contains("api-secret"))
    );
    task.abort();
}

#[tokio::test]
async fn username_password_creates_and_explicitly_closes_a_session() {
    let (endpoint, calls, task) = mock_server().await;
    let client = ZabbixClient::new(endpoint)
        .unwrap_or_else(|error| panic!("client must be constructed: {error}"))
        .authenticate(Authentication::UsernamePassword {
            username: "lab-user".to_owned(),
            password: SecretString::from("login-secret".to_owned()),
        })
        .await
        .unwrap_or_else(|error| panic!("user.login must succeed: {error}"));
    client
        .logout()
        .await
        .unwrap_or_else(|error| panic!("user.logout must succeed: {error}"));

    let captured = calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(captured.len(), 3);
    assert_eq!(captured[0].body["method"], "apiinfo.version");
    assert_eq!(captured[1].body["method"], "user.login");
    assert_eq!(captured[1].body["params"]["username"], "lab-user");
    assert_eq!(captured[1].body["params"]["password"], "login-secret");
    assert_eq!(captured[2].body["method"], "user.logout");
    assert_eq!(
        captured[2].authorization.as_deref(),
        Some("Bearer session-secret")
    );
    assert!(
        captured
            .iter()
            .all(|call| !call.body.to_string().contains("session-secret"))
    );
    task.abort();
}

#[tokio::test]
async fn snapshot_preserves_raw_unknowns_and_reports_dependencies_without_write_support() {
    let (endpoint, calls, task) = mock_server().await;
    let client = ZabbixClient::new(endpoint)
        .unwrap_or_else(|error| panic!("client must be constructed: {error}"))
        .authenticate(Authentication::ApiToken(SecretString::from(
            "api-secret".to_owned(),
        )))
        .await
        .unwrap_or_else(|error| panic!("token authentication must succeed: {error}"));
    let snapshots = client
        .dashboard_snapshots("synthetic-connection")
        .await
        .unwrap_or_else(|error| panic!("snapshot read must succeed: {error}"));
    assert_eq!(snapshots.len(), 1);
    let snapshot = &snapshots[0];
    assert_eq!(snapshot.server_version().to_string(), "7.4.3");
    assert_eq!(snapshot.adapter_id(), "zabbix-7.4");
    assert_eq!(snapshot.fingerprint().len(), 64);
    assert_eq!(
        snapshot.raw_api_document()["unknown_dashboard_property"]["must_survive"],
        true
    );
    assert_eq!(
        snapshot.raw_api_document()["pages"][0]["widgets"][3]["vendor_extension"]["must_survive"],
        true
    );

    let analysis = snapshot
        .analyze()
        .unwrap_or_else(|error| panic!("snapshot analysis must succeed: {error}"));
    assert_eq!(analysis.page_count, 1);
    assert_eq!(analysis.widget_count, 4);
    assert!(analysis.dependencies.iter().any(|dependency| {
        dependency.kind == DependencyKind::Item
            && dependency.state == DependencyState::DiscoveredReadOnly
    }));
    assert!(analysis.dependencies.iter().any(|dependency| {
        dependency.kind == DependencyKind::Graph
            && dependency.state == DependencyState::DiscoveredReadOnly
    }));
    assert!(analysis.dependencies.iter().any(|dependency| {
        dependency.kind == DependencyKind::WidgetReference
            && dependency.state == DependencyState::DiscoveredReadOnly
    }));
    assert!(analysis.dependencies.iter().any(|dependency| {
        dependency.widget_type == "vendor.private" && dependency.state == DependencyState::Unknown
    }));
    assert!(
        !analysis
            .dependencies
            .iter()
            .any(|dependency| { dependency.state == DependencyState::SupportedForModification })
    );
    let summary = SnapshotSummary::from_analyses(&[analysis]);
    assert_eq!(summary.dashboard_count, 1);
    assert_eq!(summary.unknown_widgets, 1);
    assert_eq!(summary.supported_for_modification, 0);
    assert!(summary.unknown_widget_types.contains("vendor.private"));
    assert_eq!(summary.widgets_by_type.get("svggraph"), Some(&1));
    assert_eq!(summary.widgets_by_type.len(), 4);
    let svggraph_fields = summary
        .field_types_by_widget_type
        .get("svggraph")
        .unwrap_or_else(|| panic!("svggraph field types must be counted"));
    assert_eq!(svggraph_fields.get(&4), Some(&1));
    assert_eq!(svggraph_fields.get(&1), Some(&2));
    let unknown_fields = summary
        .field_types_by_widget_type
        .get("vendor.private")
        .unwrap_or_else(|| panic!("unknown widget field types must be counted"));
    assert_eq!(unknown_fields.get(&99), Some(&1));
    assert_eq!(summary.pages_per_dashboard, vec![1]);
    assert_eq!(summary.widgets_per_page, vec![4]);
    assert_eq!(summary.max_fields_per_widget, 3);
    assert_eq!(summary.max_dashboard_name_chars, 30);
    assert_eq!(summary.max_page_name_chars, 17);
    assert_eq!(summary.max_widget_name_chars, 24);

    {
        let captured = calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let request = captured
            .iter()
            .find(|call| call.body["method"] == "dashboard.get")
            .unwrap_or_else(|| panic!("dashboard.get request must be captured"));
        assert_eq!(request.body["params"]["output"], "extend");
        assert_eq!(request.body["params"]["selectPages"], "extend");
        assert_eq!(request.body["params"]["selectUsers"], "extend");
        assert_eq!(request.body["params"]["selectUserGroups"], "extend");
    }
    client
        .logout()
        .await
        .unwrap_or_else(|error| panic!("token logout must be a no-op: {error}"));
    task.abort();
}

#[test]
fn endpoint_and_registry_fail_closed() {
    assert!(ZabbixEndpoint::parse("file:///tmp/api").is_err());
    assert!(ZabbixEndpoint::parse("https://user:secret@example.test/api_jsonrpc.php").is_err());
    assert!(ZabbixEndpoint::parse("https://example.test/api_jsonrpc.php#fragment").is_err());
    let unsupported: ZabbixVersion = "7.0.19"
        .parse()
        .unwrap_or_else(|error| panic!("version must parse: {error}"));
    assert!(matches!(
        AdapterRegistry::resolve(unsupported),
        Err(ZabbixError::UnsupportedVersion(_))
    ));
    assert!("7.4.3rc1".parse::<ZabbixVersion>().is_err());
}
