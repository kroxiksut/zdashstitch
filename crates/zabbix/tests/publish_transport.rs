// SPDX-License-Identifier: MIT OR Apache-2.0

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use zdashstitch_zabbix::{
    Authentication, DashboardPublishOutcome, WidgetFieldReplacement, ZabbixClient, ZabbixEndpoint,
    verify_widget_field_replacements,
};

#[derive(Clone, Copy)]
enum UpdateReply {
    Acknowledge,
    AcknowledgeWithStaleRead,
    LoseJsonRpcResult,
    /// The server commits, then a proxy answers 502 (owner decision
    /// 28.10.2026, item 11).
    CommitThenBadGateway,
    /// Nothing is committed and a proxy answers 502.
    BadGatewayWithoutCommit,
    /// The server refuses with a JSON-RPC error; nothing is committed.
    RejectWithApiError,
}

struct MockData {
    dashboard: Value,
    update_calls: usize,
    dashboard_get_calls: usize,
    reply: UpdateReply,
    stale_dashboard: Option<Value>,
    stale_reads_remaining: usize,
}

#[derive(Clone)]
struct MockState(Arc<Mutex<MockData>>);

async fn rpc(State(state): State<MockState>, Json(body): Json<Value>) -> Response {
    let id = body["id"].clone();
    match body["method"].as_str().unwrap_or_default() {
        "apiinfo.version" => (
            StatusCode::OK,
            Json(json!({"jsonrpc": "2.0", "result": "7.4.16", "id": id})),
        )
            .into_response(),
        "user.checkAuthentication" => (
            StatusCode::OK,
            Json(json!({
                "jsonrpc": "2.0",
                "result": {"userid": "5", "username": "writer"},
                "id": id
            })),
        )
            .into_response(),
        "dashboard.get" => {
            let mut data = state
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            data.dashboard_get_calls += 1;
            let dashboard = if data.stale_reads_remaining > 0 {
                data.stale_reads_remaining -= 1;
                data.stale_dashboard
                    .clone()
                    .unwrap_or_else(|| panic!("stale snapshot must be retained"))
            } else {
                data.dashboard.clone()
            };
            (
                StatusCode::OK,
                Json(json!({"jsonrpc": "2.0", "result": [dashboard], "id": id})),
            )
                .into_response()
        }
        "dashboard.update" => {
            let mut data = state
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            data.update_calls += 1;
            match data.reply {
                UpdateReply::BadGatewayWithoutCommit => {
                    return (StatusCode::BAD_GATEWAY, "bad gateway").into_response();
                }
                UpdateReply::RejectWithApiError => {
                    return (
                        StatusCode::OK,
                        Json(json!({
                            "jsonrpc": "2.0",
                            "error": {
                                "code": -32602,
                                "message": "Invalid params.",
                                "data": "Invalid parameter."
                            },
                            "id": id
                        })),
                    )
                        .into_response();
                }
                _ => {}
            }
            let before = data.dashboard.clone();
            apply_update(&mut data.dashboard, &body["params"]);
            let response_id = match data.reply {
                UpdateReply::CommitThenBadGateway => {
                    return (StatusCode::BAD_GATEWAY, "bad gateway").into_response();
                }
                UpdateReply::BadGatewayWithoutCommit | UpdateReply::RejectWithApiError => {
                    unreachable!("answered before the commit")
                }
                UpdateReply::Acknowledge => id,
                UpdateReply::AcknowledgeWithStaleRead => {
                    data.stale_dashboard = Some(before);
                    data.stale_reads_remaining = 1;
                    id
                }
                UpdateReply::LoseJsonRpcResult => json!(999_999),
            };
            (
                StatusCode::OK,
                Json(json!({
                    "jsonrpc": "2.0",
                    "result": {"dashboardids": ["7"]},
                    "id": response_id
                })),
            )
                .into_response()
        }
        _ => (
            StatusCode::OK,
            Json(json!({"jsonrpc": "2.0", "result": [], "id": id})),
        )
            .into_response(),
    }
}

fn apply_update(dashboard: &mut Value, params: &Value) {
    let current_pages = dashboard["pages"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("fixture pages must be an array"));
    for incoming in params["pages"]
        .as_array()
        .unwrap_or_else(|| panic!("update pages must be an array"))
    {
        let page_id = incoming["dashboard_pageid"]
            .as_str()
            .unwrap_or_else(|| panic!("update page must have an id"));
        let current = current_pages
            .iter_mut()
            .find(|page| page["dashboard_pageid"] == page_id)
            .unwrap_or_else(|| panic!("update page must already exist"));
        if incoming.get("widgets").is_some() {
            *current = incoming.clone();
        }
    }
}

fn dashboard() -> Value {
    json!({
        "dashboardid": "7", "name": "contract", "userid": "1", "private": "1",
        "display_period": "30", "auto_start": "0",
        "pages": [{
            "dashboard_pageid": "11", "name": "page", "display_period": "0",
            "widgets": [{
                "widgetid": "13", "type": "item", "name": "source",
                "x": "0", "y": "0", "width": "12", "height": "3", "view_mode": "0",
                "fields": [
                    {"type": "4", "name": "itemid.0", "value": "101"},
                    {"type": "1", "name": "reference", "value": "ABCDE"}
                ]
            }]
        }],
        "users": [], "userGroups": []
    })
}

async fn mock_server(reply: UpdateReply) -> (ZabbixEndpoint, Arc<Mutex<MockData>>, JoinHandle<()>) {
    let data = Arc::new(Mutex::new(MockData {
        dashboard: dashboard(),
        update_calls: 0,
        dashboard_get_calls: 0,
        reply,
        stale_dashboard: None,
        stale_reads_remaining: 0,
    }));
    let app = Router::new()
        .route("/api_jsonrpc.php", post(rpc))
        .with_state(MockState(Arc::clone(&data)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("mock bind: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("mock address: {error}"));
    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            panic!("mock API failed: {error}");
        }
    });
    let endpoint = ZabbixEndpoint::parse(&format!("http://{address}/api_jsonrpc.php"))
        .unwrap_or_else(|error| panic!("mock endpoint: {error}"));
    (endpoint, data, task)
}

fn replacement() -> WidgetFieldReplacement {
    WidgetFieldReplacement {
        page_index: 0,
        widgetid: "13".to_owned(),
        field_name: "itemid.0".to_owned(),
        expected_value: "101".to_owned(),
        replacement_value: "202".to_owned(),
    }
}

async fn client_and_update(
    reply: UpdateReply,
) -> (
    zdashstitch_zabbix::ReadWriteGateway,
    zdashstitch_zabbix::VerifiedDashboardUpdate,
    Arc<Mutex<MockData>>,
    JoinHandle<()>,
) {
    let (endpoint, data, task) = mock_server(reply).await;
    let client = ZabbixClient::new(endpoint)
        .unwrap_or_else(|error| panic!("client: {error}"))
        .authenticate(Authentication::ApiToken(SecretString::from(
            "test-token".to_owned(),
        )))
        .await
        .unwrap_or_else(|error| panic!("authentication: {error}"))
        .enable_writes();
    let source = client
        .dashboard_snapshots("mock-connection")
        .await
        .unwrap_or_else(|error| panic!("source snapshot: {error}"))
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("source dashboard must exist"));
    let update =
        verify_widget_field_replacements(&source, &client.publication_context(), &[replacement()])
            .unwrap_or_else(|error| panic!("verified update: {error}"));
    (client, update, data, task)
}

#[tokio::test]
async fn acknowledged_write_is_reread_and_verified() {
    let (client, update, data, task) = client_and_update(UpdateReply::Acknowledge).await;
    let outcome = client
        .publish_verified_dashboard_update(&update, || true)
        .await;
    assert_eq!(outcome, DashboardPublishOutcome::Verified);
    assert_eq!(
        data.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .update_calls,
        1
    );
    task.abort();
}

#[tokio::test]
async fn changed_fingerprint_blocks_before_dashboard_update() {
    let (client, update, data, task) = client_and_update(UpdateReply::Acknowledge).await;
    data.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .dashboard["pages"][0]["name"] = json!("concurrent edit");
    let outcome = client
        .publish_verified_dashboard_update(&update, || true)
        .await;
    assert_eq!(outcome, DashboardPublishOutcome::Conflict);
    assert_eq!(
        data.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .update_calls,
        0
    );
    task.abort();
}

#[tokio::test]
async fn unusable_write_response_is_reconciled_without_retry() {
    let (client, update, data, task) = client_and_update(UpdateReply::LoseJsonRpcResult).await;
    let outcome = client
        .publish_verified_dashboard_update(&update, || true)
        .await;
    assert_eq!(outcome, DashboardPublishOutcome::ReconciledVerified);
    assert_eq!(
        data.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .update_calls,
        1
    );
    task.abort();
}

fn update_calls(data: &Arc<Mutex<MockData>>) -> usize {
    data.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .update_calls
}

/// Owner decision 28.10.2026, item 11: an HTTP status after the send is no
/// known response (a proxy may answer after the server committed); the
/// outcome is decided by read-only reconciliation, never by a retry.
#[tokio::test]
async fn http_status_after_a_committed_update_is_reconciled_as_verified() {
    let (client, update, data, task) = client_and_update(UpdateReply::CommitThenBadGateway).await;
    let outcome = client
        .publish_verified_dashboard_update(&update, || true)
        .await;
    assert_eq!(outcome, DashboardPublishOutcome::ReconciledVerified);
    assert_eq!(update_calls(&data), 1);
    task.abort();
}

#[tokio::test]
async fn http_status_without_a_commit_is_reconciled_as_failed() {
    let (client, update, data, task) =
        client_and_update(UpdateReply::BadGatewayWithoutCommit).await;
    let outcome = client
        .publish_verified_dashboard_update(&update, || true)
        .await;
    assert_eq!(outcome, DashboardPublishOutcome::ReconciledFailed);
    let data = data
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(data.update_calls, 1);
    // The plan read, the read before the send, three reconciliation reads.
    assert_eq!(data.dashboard_get_calls, 5);
    drop(data);
    task.abort();
}

/// A JSON-RPC error is a known answer of the server itself: it stays
/// `FailedAfterResponse` without reconciliation reads.
#[tokio::test]
async fn api_error_after_send_stays_a_known_failure() {
    let (client, update, data, task) = client_and_update(UpdateReply::RejectWithApiError).await;
    let outcome = client
        .publish_verified_dashboard_update(&update, || true)
        .await;
    assert_eq!(outcome, DashboardPublishOutcome::FailedAfterResponse);
    let data = data
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(data.update_calls, 1);
    // The plan read and the read before the send; nothing after it.
    assert_eq!(data.dashboard_get_calls, 2);
    drop(data);
    task.abort();
}

#[tokio::test]
async fn stale_first_read_back_retries_only_the_read() {
    let (client, update, data, task) =
        client_and_update(UpdateReply::AcknowledgeWithStaleRead).await;
    let outcome = client
        .publish_verified_dashboard_update(&update, || true)
        .await;
    assert_eq!(outcome, DashboardPublishOutcome::Verified);
    let data = data
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(data.update_calls, 1);
    assert_eq!(data.dashboard_get_calls, 4);
    task.abort();
}

#[tokio::test]
async fn plan_bound_to_another_credential_is_blocked_before_send() {
    let (endpoint, data, task) = mock_server(UpdateReply::Acknowledge).await;
    let first = ZabbixClient::new(endpoint.clone())
        .unwrap_or_else(|error| panic!("first client: {error}"))
        .authenticate(Authentication::ApiToken(SecretString::from(
            "first-token".to_owned(),
        )))
        .await
        .unwrap_or_else(|error| panic!("first authentication: {error}"))
        .enable_writes();
    let second = ZabbixClient::new(endpoint)
        .unwrap_or_else(|error| panic!("second client: {error}"))
        .authenticate(Authentication::ApiToken(SecretString::from(
            "second-token".to_owned(),
        )))
        .await
        .unwrap_or_else(|error| panic!("second authentication: {error}"))
        .enable_writes();
    let source = first
        .dashboard_snapshots("mock-connection")
        .await
        .unwrap_or_else(|error| panic!("source snapshot: {error}"))
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("source dashboard must exist"));
    let update =
        verify_widget_field_replacements(&source, &first.publication_context(), &[replacement()])
            .unwrap_or_else(|error| panic!("verified update: {error}"));

    let outcome = second
        .publish_verified_dashboard_update(&update, || true)
        .await;
    assert_eq!(outcome, DashboardPublishOutcome::FailedBeforeSend);
    assert_eq!(
        data.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .update_calls,
        0
    );
    task.abort();
}
