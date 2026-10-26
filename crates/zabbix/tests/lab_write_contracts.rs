// SPDX-License-Identifier: MIT OR Apache-2.0

use std::env;

use reqwest::{Client, redirect::Policy};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use zdashstitch_zabbix::{
    Authentication, VersionedDashboardSnapshot, ZabbixClient, ZabbixEndpoint,
};

const LAB_DASHBOARD_NAME: &str = "ZDashstitch Contract Lab";

struct LabWriteApi {
    endpoint: String,
    client: Client,
    token: SecretString,
    next_id: u64,
}

impl LabWriteApi {
    async fn login(endpoint: String, username: String, password: SecretString) -> Self {
        let client = Client::builder()
            .redirect(Policy::none())
            .build()
            .unwrap_or_else(|error| panic!("cannot build lab HTTP client: {error}"));
        let token = rpc_call(
            &client,
            &endpoint,
            1,
            "user.login",
            json!({ "username": username, "password": password.expose_secret() }),
            None,
        )
        .await
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| panic!("user.login did not return a token"));
        Self {
            endpoint,
            client,
            token: SecretString::from(token),
            next_id: 2,
        }
    }

    async fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        rpc_call(
            &self.client,
            &self.endpoint,
            id,
            method,
            params,
            Some(&self.token),
        )
        .await
    }

    async fn dispatch_without_reading_result(&mut self, method: &str, params: Value) {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(self.token.expose_secret())
            .json(&json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params,
                "id": id
            }))
            .send()
            .await
            .unwrap_or_else(|error| panic!("laboratory {method} dispatch failed: {error}"));
        assert!(
            response.status().is_success(),
            "laboratory {method} returned HTTP {}",
            response.status()
        );
        drop(response);
    }

    async fn logout(mut self) {
        let result = self.call("user.logout", json!([])).await;
        assert_eq!(result, true);
    }
}

async fn rpc_call(
    client: &Client,
    endpoint: &str,
    id: u64,
    method: &str,
    params: Value,
    token: Option<&SecretString>,
) -> Value {
    let mut request = client.post(endpoint).json(&json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": id
    }));
    if let Some(token) = token {
        request = request.bearer_auth(token.expose_secret());
    }
    let response = request
        .send()
        .await
        .unwrap_or_else(|error| panic!("laboratory {method} transport failed: {error}"));
    assert!(
        response.status().is_success(),
        "laboratory {method} returned HTTP {}",
        response.status()
    );
    let body: Value = response
        .json()
        .await
        .unwrap_or_else(|error| panic!("laboratory {method} returned invalid JSON: {error}"));
    if let Some(error) = body.get("error") {
        let code = error
            .get("code")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("API error");
        panic!("laboratory {method} failed with code {code}: {message}");
    }
    body.get("result")
        .cloned()
        .unwrap_or_else(|| panic!("laboratory {method} response has no result"))
}

#[tokio::test]
#[ignore = "performs writes only on the explicitly authorized laboratory dashboard"]
async fn zabbix_7_4_dashboard_write_contracts_preserve_and_reconcile() {
    assert_eq!(
        required("ZDASHSTITCH_LAB_WRITE_ACK"),
        "write-authorized-7.4-contract-dashboard"
    );
    let endpoint_raw = required("ZDASHSTITCH_LAB_API_URL");
    let username = required("ZDASHSTITCH_LAB_USERNAME");
    let password = required("ZDASHSTITCH_LAB_PASSWORD");

    let endpoint = ZabbixEndpoint::parse(&endpoint_raw)
        .unwrap_or_else(|error| panic!("invalid laboratory endpoint: {error}"));
    let reader = ZabbixClient::new(endpoint)
        .unwrap_or_else(|error| panic!("cannot construct read client: {error}"))
        .authenticate(Authentication::UsernamePassword {
            username: username.clone(),
            password: SecretString::from(password.clone()),
        })
        .await
        .unwrap_or_else(|error| panic!("read client authentication failed: {error}"));
    assert!(reader.capabilities().version.is_supported_7_4());
    println!("apiinfo.version: {}", reader.capabilities().version);

    let mut writer = LabWriteApi::login(endpoint_raw, username, SecretString::from(password)).await;
    let itemid = first_item_id(&mut writer).await;
    let existing = find_lab_dashboard(&reader).await;
    let (dashboardid, created) = match existing {
        Some(snapshot) => (dashboard_id(&snapshot), false),
        None => (create_lab_dashboard(&mut writer, &itemid).await, true),
    };
    reset_owned_dashboard(&reader, &mut writer, &dashboardid, &itemid).await;

    let baseline = required_lab_dashboard(&reader).await;
    let baseline_raw = baseline.raw_api_document();
    let pages = baseline_raw["pages"]
        .as_array()
        .unwrap_or_else(|| panic!("baseline pages must be an array"));
    assert_eq!(
        pages.len(),
        2,
        "owned contract dashboard must have exactly two pages"
    );
    let control_page = pages[0].clone();
    let control_pageid = string_property(&control_page, "dashboard_pageid");
    let mutable_page = &pages[1];
    let mutable_pageid = string_property(mutable_page, "dashboard_pageid");
    let widgets = mutable_page["widgets"]
        .as_array()
        .unwrap_or_else(|| panic!("mutable widgets must be an array"));
    assert_eq!(widgets.len(), 2);
    let updated_item = writable_widget(
        &widgets[0],
        Some(json!([
            {"type": 4, "name": "itemid.0", "value": itemid},
            {"type": 1, "name": "reference", "value": "ABCDE"},
            {"type": 0, "name": "show.0", "value": 2}
        ])),
    );
    let copied_graph = writable_widget(&widgets[1], None);

    writer
        .call(
            "dashboard.update",
            json!({
                "dashboardid": dashboardid,
                "pages": [
                    {"dashboard_pageid": control_pageid},
                    {
                        "dashboard_pageid": mutable_pageid,
                        "name": "Mutable verified",
                        "widgets": [updated_item, copied_graph]
                    }
                ]
            }),
        )
        .await;

    let verified = required_lab_dashboard(&reader).await;
    let verified_pages = verified.raw_api_document()["pages"]
        .as_array()
        .unwrap_or_else(|| panic!("verified pages must be an array"));
    assert_eq!(&verified_pages[0], &control_page, "untouched page changed");
    let verified_widgets = verified_pages[1]["widgets"]
        .as_array()
        .unwrap_or_else(|| panic!("verified widgets must be an array"));
    assert_eq!(
        field_names(&verified_widgets[0]),
        vec!["itemid.0", "reference", "show.0"]
    );
    assert_eq!(field_value(&verified_widgets[0], "reference"), "ABCDE");
    assert_eq!(
        field_value(&verified_widgets[1], "ds.0.itemids.0._reference"),
        "ABCDE._itemid"
    );
    assert_eq!(field_value(&verified_widgets[1], "reference"), "FGHIJ");

    let planned_fingerprint = verified.fingerprint().to_owned();
    writer
        .call(
            "dashboard.update",
            json!({
                "dashboardid": dashboardid,
                "pages": [
                    {"dashboard_pageid": control_pageid},
                    {"dashboard_pageid": mutable_pageid, "name": "Concurrent laboratory edit"}
                ]
            }),
        )
        .await;
    let concurrent = required_lab_dashboard(&reader).await;
    assert_ne!(concurrent.fingerprint(), planned_fingerprint);
    let conflict_detected = concurrent.fingerprint() != planned_fingerprint;
    assert!(
        conflict_detected,
        "stale plan must be rejected before another write"
    );

    // Drop the response body after dispatching exactly one write. Without a parsed
    // JSON-RPC result the client must classify the outcome as unknown and reread.
    writer
        .dispatch_without_reading_result(
            "dashboard.update",
            json!({
                "dashboardid": dashboardid,
                "pages": [
                    {"dashboard_pageid": control_pageid},
                    {"dashboard_pageid": mutable_pageid, "name": "Unknown outcome reconciled"}
                ]
            }),
        )
        .await;
    let reconciled = required_lab_dashboard(&reader).await;
    assert_eq!(
        reconciled.raw_api_document()["pages"][1]["name"],
        "Unknown outcome reconciled"
    );
    assert_eq!(reconciled.raw_api_document()["pages"][0], control_page);

    println!(
        "write contracts: dashboard_created={created}, untouched_page_preserved=true, full_fields_verified=true, references_preserved=true, conflict_blocked=true, unknown_outcome_reconciled=true, dashboard_retained=true"
    );
    writer.logout().await;
    reader
        .logout()
        .await
        .unwrap_or_else(|error| panic!("read client logout failed: {error}"));
}

async fn first_item_id(writer: &mut LabWriteApi) -> String {
    let result = writer
        .call(
            "item.get",
            json!({"output": ["itemid"], "sortfield": "itemid", "limit": 1}),
        )
        .await;
    result
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item.get("itemid"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| panic!("laboratory must contain at least one readable item"))
}

async fn create_lab_dashboard(writer: &mut LabWriteApi, itemid: &str) -> String {
    let result = writer
        .call(
            "dashboard.create",
            json!({
                "name": LAB_DASHBOARD_NAME,
                "private": 1,
                "display_period": 30,
                "auto_start": 0,
                "pages": baseline_pages(itemid)
            }),
        )
        .await;
    result["dashboardids"]
        .as_array()
        .and_then(|ids| ids.first())
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| panic!("dashboard.create returned no dashboard ID"))
}

async fn reset_owned_dashboard(
    reader: &zdashstitch_zabbix::ReadOnlyGateway,
    writer: &mut LabWriteApi,
    dashboardid: &str,
    itemid: &str,
) {
    let snapshot = required_lab_dashboard(reader).await;
    let pages = snapshot.raw_api_document()["pages"]
        .as_array()
        .unwrap_or_else(|| panic!("owned dashboard pages must be an array"));
    if pages.len() != 2 {
        panic!("refusing to replace an owned dashboard whose expected two-page shape has drifted");
    }
    writer
        .call(
            "dashboard.update",
            json!({
                "dashboardid": dashboardid,
                "name": LAB_DASHBOARD_NAME,
                "private": 1,
                "pages": [
                    with_page_id(&baseline_pages(itemid)[0], string_property(&pages[0], "dashboard_pageid")),
                    with_page_id(&baseline_pages(itemid)[1], string_property(&pages[1], "dashboard_pageid"))
                ]
            }),
        )
        .await;
}

fn baseline_pages(itemid: &str) -> Value {
    json!([
        {
            "name": "Control — must stay byte-for-byte",
            "display_period": 0,
            "widgets": [{
                "type": "clock",
                "name": "Untouched contract marker",
                "x": 0, "y": 0, "width": 12, "height": 3, "view_mode": 0,
                "fields": [
                    {"type": 0, "name": "clock_type", "value": 1},
                    {"type": 1, "name": "zds.contract_marker", "value": "preserve-me"}
                ]
            }]
        },
        {
            "name": "Mutable baseline",
            "display_period": 0,
            "widgets": [
                {
                    "type": "item",
                    "name": "Contract item source",
                    "x": 0, "y": 0, "width": 12, "height": 3, "view_mode": 0,
                    "fields": [
                        {"type": 4, "name": "itemid.0", "value": itemid},
                        {"type": 1, "name": "reference", "value": "ABCDE"}
                    ]
                },
                {
                    "type": "svggraph",
                    "name": "Contract reference consumer",
                    "x": 12, "y": 0, "width": 36, "height": 5, "view_mode": 0,
                    "fields": [
                        {"type": 0, "name": "ds.0.dataset_type", "value": 0},
                        {"type": 1, "name": "ds.0.itemids.0._reference", "value": "ABCDE._itemid"},
                        {"type": 1, "name": "ds.0.color.0", "value": "477BDE"},
                        {"type": 1, "name": "reference", "value": "FGHIJ"}
                    ]
                }
            ]
        }
    ])
}

fn with_page_id(page: &Value, pageid: String) -> Value {
    let mut page = page.clone();
    page.as_object_mut()
        .unwrap_or_else(|| panic!("baseline page must be an object"))
        .insert("dashboard_pageid".to_owned(), Value::String(pageid));
    page
}

fn writable_widget(widget: &Value, fields: Option<Value>) -> Value {
    let mut output = serde_json::Map::new();
    for key in [
        "widgetid",
        "type",
        "name",
        "x",
        "y",
        "width",
        "height",
        "view_mode",
    ] {
        output.insert(
            key.to_owned(),
            widget
                .get(key)
                .cloned()
                .unwrap_or_else(|| panic!("widget has no {key}")),
        );
    }
    output.insert(
        "fields".to_owned(),
        fields.unwrap_or_else(|| widget["fields"].clone()),
    );
    Value::Object(output)
}

async fn find_lab_dashboard(
    reader: &zdashstitch_zabbix::ReadOnlyGateway,
) -> Option<VersionedDashboardSnapshot> {
    let mut matching = reader
        .dashboard_snapshots("lab-write-contract")
        .await
        .unwrap_or_else(|error| panic!("cannot read laboratory dashboards: {error}"))
        .into_iter()
        .filter(|snapshot| snapshot.raw_api_document()["name"] == LAB_DASHBOARD_NAME);
    let dashboard = matching.next();
    assert!(
        matching.next().is_none(),
        "multiple owned laboratory dashboards share the reserved name"
    );
    dashboard
}

async fn required_lab_dashboard(
    reader: &zdashstitch_zabbix::ReadOnlyGateway,
) -> VersionedDashboardSnapshot {
    find_lab_dashboard(reader)
        .await
        .unwrap_or_else(|| panic!("owned laboratory dashboard is absent"))
}

fn dashboard_id(snapshot: &VersionedDashboardSnapshot) -> String {
    string_property(snapshot.raw_api_document(), "dashboardid")
}

fn string_property(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| panic!("object has no string {key}"))
}

fn field_names(widget: &Value) -> Vec<&str> {
    widget["fields"]
        .as_array()
        .unwrap_or_else(|| panic!("widget fields must be an array"))
        .iter()
        .filter_map(|field| field["name"].as_str())
        .collect()
}

fn field_value<'a>(widget: &'a Value, name: &str) -> &'a str {
    widget["fields"]
        .as_array()
        .unwrap_or_else(|| panic!("widget fields must be an array"))
        .iter()
        .find(|field| field["name"] == name)
        .and_then(|field| field["value"].as_str())
        .unwrap_or_else(|| panic!("widget field {name} is absent or not a string"))
}

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("required environment variable is absent: {name}"))
}
