// SPDX-License-Identifier: MIT OR Apache-2.0

//! Laboratory observation of the read-only gateway with a privileged account.
//! The loopback observer records only method names and refuses every non-read
//! method before forwarding it to the explicitly configured laboratory.

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use secrecy::SecretString;
use serde_json::Value;
use std::{
    env,
    sync::{Arc, Mutex},
};
use zdashstitch_zabbix::{
    Authentication, ZabbixClient, ZabbixEndpoint, dashboard_write_allowlist, read_only_allowlist,
};

#[derive(Clone)]
struct Observer {
    endpoint: String,
    client: reqwest::Client,
    methods: Arc<Mutex<Vec<String>>>,
}

async fn observe(
    State(observer): State<Observer>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let method = body
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    observer
        .methods
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(method.to_owned());
    if !read_only_allowlist().contains(&method) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut request = observer.client.post(&observer.endpoint).json(&body);
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        request = request.header(header::AUTHORIZATION, value);
    }
    match request.send().await {
        Ok(response) => match response.json::<Value>().await {
            Ok(value) => Json(value).into_response(),
            Err(_) => StatusCode::BAD_GATEWAY.into_response(),
        },
        Err(_) => StatusCode::BAD_GATEWAY.into_response(),
    }
}

#[tokio::test]
#[ignore = "requires the explicitly authorized laboratory and a privileged account"]
async fn privileged_lab_read_gateway_never_dispatches_writes() {
    assert_eq!(
        env::var("ZDASHSTITCH_LAB_ACK").ok().as_deref(),
        Some("read-only-authorized-7.4")
    );
    let endpoint =
        env::var("ZDASHSTITCH_LAB_API_URL").unwrap_or_else(|_| panic!("lab endpoint required"));
    let username =
        env::var("ZDASHSTITCH_LAB_USERNAME").unwrap_or_else(|_| panic!("lab username required"));
    let password =
        env::var("ZDASHSTITCH_LAB_PASSWORD").unwrap_or_else(|_| panic!("lab password required"));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observer = Observer {
        endpoint,
        client: reqwest::Client::new(),
        methods: calls.clone(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|_| panic!("loopback listener"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|_| panic!("loopback address"));
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/api_jsonrpc.php", post(observe))
                .with_state(observer),
        )
        .await
    });
    let endpoint = ZabbixEndpoint::parse(&format!("http://{address}"))
        .unwrap_or_else(|_| panic!("observer endpoint"));
    let reader = ZabbixClient::new(endpoint)
        .unwrap_or_else(|_| panic!("client"))
        .authenticate(Authentication::UsernamePassword {
            username,
            password: SecretString::from(password),
        })
        .await
        .unwrap_or_else(|_| panic!("lab login"));
    assert_eq!(reader.capabilities().version.to_string(), "7.4.16");
    assert!(
        reader
            .dashboard_create_permission()
            .await
            .unwrap_or_else(|_| panic!("role check")),
        "account must have dashboard create permission"
    );
    reader
        .inventory()
        .await
        .unwrap_or_else(|_| panic!("inventory"));
    reader
        .readable_item_ids(10)
        .await
        .unwrap_or_else(|_| panic!("item inventory"));
    reader
        .dashboard_snapshots("p5-07-lab")
        .await
        .unwrap_or_else(|_| panic!("dashboard inventory"));
    let writer = reader.enable_writes();
    let reader = writer.into_read_only();
    reader.logout().await.unwrap_or_else(|_| panic!("logout"));
    let observed = calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(!observed.is_empty());
    assert_eq!(
        dashboard_write_allowlist(),
        vec!["dashboard.update", "dashboard.create"]
    );
    assert!(
        observed
            .iter()
            .all(|method| read_only_allowlist().contains(&method.as_str()))
    );
    assert!(observed.iter().all(|method| !method.ends_with(".create")
        && !method.ends_with(".update")
        && !method.ends_with(".delete")));
    server.abort();
}
