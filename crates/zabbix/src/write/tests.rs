// SPDX-License-Identifier: MIT OR Apache-2.0

//! Codec and closed publication path of P2-04B on synthetic dashboards and a
//! loopback mock transport. No laboratory or production server is used.

use std::sync::{Arc, Mutex, PoisonError};

use axum::{Json, Router, extract::State, routing::post};
use secrecy::SecretString;
use serde_json::{Value, json};

use super::*;
use crate::{AdapterRegistry, Authentication, CapabilityState, ZabbixClient, ZabbixEndpoint};

fn snapshot(raw: Value, version: &str) -> VersionedDashboardSnapshot {
    VersionedDashboardSnapshot::from_adapter_raw(
        "test-connection".to_owned(),
        version
            .parse()
            .unwrap_or_else(|error| panic!("valid version: {error}")),
        raw,
    )
    .unwrap_or_else(|error| panic!("valid snapshot: {error}"))
}

fn capabilities(version: &str) -> ZabbixCapabilities {
    AdapterRegistry::resolve(
        version
            .parse()
            .unwrap_or_else(|error| panic!("valid version: {error}")),
    )
    .unwrap_or_else(|error| panic!("adapter: {error}"))
}

/// Every contract proven: only for tests of the gates themselves.
fn all_contracts() -> DashboardWriteContracts {
    let supported = CapabilityState::Supported;
    DashboardWriteContracts {
        update_one_page_with_full_fields: supported,
        preserve_untouched_pages: supported,
        item_widget_itemid: supported,
        preserve_widget_references: supported,
        reconcile_unknown_outcome: supported,
        update_several_pages: supported,
        svggraph_dataset_itemids: supported,
        svggraph_dataset_hosts: supported,
        classic_graph_graphid: supported,
    }
}

fn context(contracts: Option<DashboardWriteContracts>) -> PublicationContext {
    let mut capabilities = capabilities("7.4.16");
    if let Some(contracts) = contracts {
        capabilities.dashboard_write_contracts = contracts;
    }
    PublicationContext::detached_for_testing(capabilities)
}

fn field(kind: Value, name: &str, value: Value) -> Value {
    json!({"type": kind, "name": name, "value": value})
}

fn widget(widgetid: &str, widget_type: &str, x: u32, fields: Vec<Value>) -> Value {
    json!({
        "widgetid": widgetid, "type": widget_type, "name": format!("w{widgetid}"),
        "x": x.to_string(), "y": "0", "width": "12", "height": "4", "view_mode": "0",
        "fields": fields
    })
}

/// Three pages: items and an `svggraph` on the first, one item on the
/// second, an untouched `problems` page last.
fn dashboard() -> Value {
    json!({
        "dashboardid": "7", "name": "contract", "userid": "1", "private": "1",
        "display_period": "30", "auto_start": "1",
        "pages": [
            {"dashboard_pageid": "11", "name": "first", "display_period": "0", "widgets": [
                widget("13", "item", 0, vec![
                    field(json!("4"), "itemid.0", json!("101")),
                    field(json!("1"), "reference", json!("ABCDE")),
                    field(json!("0"), "show.0", json!("2")),
                ]),
                widget("14", "item", 12, vec![
                    field(json!(4), "itemid.0", json!(102)),
                    field(json!("1"), "description", json!("{ITEM.NAME}")),
                ]),
                widget("15", "svggraph", 24, vec![
                    field(json!("1"), "ds.0.hosts.0", json!("old-host")),
                    field(json!("1"), "ds.0.items.0", json!("CPU*")),
                    field(json!("1"), "ds.1.hosts.0", json!("old-*")),
                    field(json!("4"), "ds.2.itemids.0", json!("103")),
                    field(json!("1"), "ds.2.color.0", json!("477BDE")),
                    field(json!("1"), "reference", json!("FGHIJ")),
                ]),
            ]},
            {"dashboard_pageid": "21", "name": "second", "display_period": "60", "widgets": [
                widget("23", "item", 0, vec![field(json!("4"), "itemid.0", json!("104"))]),
            ]},
            {"dashboard_pageid": "31", "name": "third", "display_period": "0", "widgets": [
                widget("33", "problems", 0, vec![field(json!("3"), "hostids.0", json!("11"))]),
            ]}
        ],
        "users": [{"userid": "7", "permission": "2"}], "userGroups": []
    })
}

fn replace(
    page_index: usize,
    widgetid: &str,
    field_name: &str,
    expected: &str,
    replacement: &str,
) -> WidgetFieldReplacement {
    WidgetFieldReplacement {
        page_index,
        widgetid: widgetid.to_owned(),
        field_name: field_name.to_owned(),
        expected_value: expected.to_owned(),
        replacement_value: replacement.to_owned(),
    }
}

fn first_page_items() -> Vec<WidgetFieldReplacement> {
    vec![
        replace(0, "13", "itemid.0", "101", "201"),
        replace(0, "14", "itemid.0", "102", "202"),
    ]
}

fn rejection(
    result: std::result::Result<VerifiedDashboardUpdate, WriteRejection>,
) -> WriteRejection {
    match result {
        Ok(_) => panic!("the update must be rejected"),
        Err(rejection) => rejection,
    }
}

fn verified(
    result: std::result::Result<VerifiedDashboardUpdate, WriteRejection>,
) -> VerifiedDashboardUpdate {
    result
        .unwrap_or_else(|rejection| panic!("verified update: {rejection} {:?}", rejection.params()))
}

/// The payload the closed path sends when the read before send is `read`.
fn params(update: &VerifiedDashboardUpdate, read: &VersionedDashboardSnapshot) -> Value {
    update
        .params_for(read)
        .unwrap_or_else(|| panic!("the update must be sendable for this read"))
}

/// The payload widget equals the read widget on every sent key, except the
/// listed field values.
fn assert_only_values_changed(sent: &Value, read: &Value, changed: &[(&str, Value)]) {
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
        assert_eq!(sent[key], read[key], "widget key {key}");
    }
    let sent_fields = sent["fields"]
        .as_array()
        .unwrap_or_else(|| panic!("sent fields"));
    let read_fields = read["fields"]
        .as_array()
        .unwrap_or_else(|| panic!("read fields"));
    assert_eq!(sent_fields.len(), read_fields.len());
    for (sent_field, read_field) in sent_fields.iter().zip(read_fields) {
        assert_eq!(sent_field["type"], read_field["type"]);
        assert_eq!(sent_field["name"], read_field["name"]);
        match changed.iter().find(|(name, _)| sent_field["name"] == *name) {
            Some((_, value)) => assert_eq!(&sent_field["value"], value),
            None => assert_eq!(sent_field, read_field, "untouched field"),
        }
    }
}

#[test]
fn several_items_on_one_page_change_only_their_values() {
    let source = snapshot(dashboard(), "7.4.16");
    let update = verified(verify_widget_field_replacements(
        &source,
        &context(None),
        &first_page_items(),
    ));
    assert_eq!(update.replacement_count(), 2);
    assert_eq!(update.changed_pages(), &[0]);
    assert_eq!(
        update
            .replacement_kinds()
            .get(&WriteContract::ItemWidgetItemid),
        Some(&2)
    );
    assert!(
        !update
            .required_contracts()
            .contains(&WriteContract::UpdateSeveralPages)
    );
    assert_ne!(update.source_fingerprint(), update.expected_fingerprint());

    let params = &params(&update, &source);
    let read = source.raw_api_document();
    // Dashboard properties, owner and sharing are never sent.
    let keys = params
        .as_object()
        .map(|object| object.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    assert_eq!(keys, vec!["dashboardid", "pages"]);
    // Every page in read order; untouched pages by ID only.
    assert_eq!(params["pages"].as_array().map(Vec::len), Some(3));
    assert_eq!(params["pages"][1], json!({"dashboard_pageid": "21"}));
    assert_eq!(params["pages"][2], json!({"dashboard_pageid": "31"}));
    let page = &params["pages"][0];
    for key in ["dashboard_pageid", "name", "display_period"] {
        assert_eq!(page[key], read["pages"][0][key]);
    }
    assert_only_values_changed(
        &page["widgets"][0],
        &read["pages"][0]["widgets"][0],
        &[("itemid.0", json!("201"))],
    );
    // A numeric value stays numeric.
    assert_only_values_changed(
        &page["widgets"][1],
        &read["pages"][0]["widgets"][1],
        &[("itemid.0", json!(202))],
    );
    assert_only_values_changed(&page["widgets"][2], &read["pages"][0]["widgets"][2], &[]);
}

/// The 7.4.16 contracts with `withdrawn` set to `Unsupported`.
fn without(withdrawn: &[WriteContract]) -> DashboardWriteContracts {
    let mut contracts = capabilities("7.4.16").dashboard_write_contracts;
    for contract in withdrawn {
        let state = match contract {
            WriteContract::UpdateSeveralPages => &mut contracts.update_several_pages,
            WriteContract::SvggraphDatasetItemids => &mut contracts.svggraph_dataset_itemids,
            WriteContract::SvggraphDatasetHosts => &mut contracts.svggraph_dataset_hosts,
            WriteContract::ClassicGraphGraphid => &mut contracts.classic_graph_graphid,
            other => panic!("not a P2 contract: {other:?}"),
        };
        *state = CapabilityState::Unsupported;
    }
    contracts
}

#[test]
fn lab_02_contracts_are_supported_on_the_proven_patch_only() {
    let p2 = [
        WriteContract::UpdateSeveralPages,
        WriteContract::SvggraphDatasetItemids,
        WriteContract::SvggraphDatasetHosts,
        WriteContract::ClassicGraphGraphid,
    ];
    let proven = capabilities("7.4.16").dashboard_write_contracts;
    let other = capabilities("7.4.17").dashboard_write_contracts;
    for contract in p2 {
        assert!(proven.is_verified(contract), "{contract:?} on 7.4.16");
        assert!(!other.is_verified(contract), "{contract:?} on 7.4.17");
    }
    assert!(RESEND_PROVEN_WIDGET_TYPES.contains(&"graph"));
}

#[test]
fn several_pages_need_their_own_contract() {
    let source = snapshot(dashboard(), "7.4.16");
    let mut replacements = first_page_items();
    replacements.push(replace(1, "23", "itemid.0", "104", "204"));

    let withdrawn = without(&[WriteContract::UpdateSeveralPages]);
    let refused = rejection(verify_widget_field_replacements(
        &source,
        &context(Some(withdrawn.clone())),
        &replacements,
    ));
    assert_eq!(refused.code(), WriteRejectionCode::ContractUnverified);
    assert_eq!(refused.param("contract"), Some("update_several_pages"));

    let requirements = write_requirements(&source, &replacements)
        .unwrap_or_else(|rejection| panic!("requirements: {rejection}"));
    assert_eq!(
        requirements.missing_contracts(&withdrawn),
        vec![WriteContract::UpdateSeveralPages]
    );
    assert!(
        requirements
            .missing_contracts(&capabilities("7.4.16").dashboard_write_contracts)
            .is_empty()
    );

    let update = verified(verify_widget_field_replacements(
        &source,
        &context(None),
        &replacements,
    ));
    assert_eq!(update.changed_pages(), &[0, 1]);
    let params = &params(&update, &source);
    let read = source.raw_api_document();
    assert_eq!(params["pages"][2], json!({"dashboard_pageid": "31"}));
    assert_eq!(params["pages"][1]["display_period"], "60");
    assert_only_values_changed(
        &params["pages"][1]["widgets"][0],
        &read["pages"][1]["widgets"][0],
        &[("itemid.0", json!("204"))],
    );
}

#[test]
fn svggraph_fields_stay_refused_until_their_contracts() {
    let source = snapshot(dashboard(), "7.4.16");
    let itemids = [replace(0, "15", "ds.2.itemids.0", "103", "203")];
    let hosts = [replace(0, "15", "ds.0.hosts.0", "old-host", "new-host")];
    let withdrawn = without(&[
        WriteContract::SvggraphDatasetItemids,
        WriteContract::SvggraphDatasetHosts,
    ]);
    for (replacements, contract) in [
        (&itemids, "svggraph_dataset_itemids"),
        (&hosts, "svggraph_dataset_hosts"),
    ] {
        let refused = rejection(verify_widget_field_replacements(
            &source,
            &context(Some(withdrawn.clone())),
            replacements,
        ));
        assert_eq!(refused.code(), WriteRejectionCode::ContractUnverified);
        assert_eq!(refused.param("contract"), Some(contract));
    }

    let update = verified(verify_widget_field_replacements(
        &source,
        &context(None),
        &[itemids[0].clone(), hosts[0].clone()],
    ));
    assert_only_values_changed(
        &params(&update, &source)["pages"][0]["widgets"][2],
        &source.raw_api_document()["pages"][0]["widgets"][2],
        &[
            ("ds.2.itemids.0", json!("203")),
            ("ds.0.hosts.0", json!("new-host")),
        ],
    );

    // Masks are never replaced, and a replacement never becomes a mask.
    let mask = rejection(verify_widget_field_replacements(
        &source,
        &context(Some(all_contracts())),
        &[replace(0, "15", "ds.1.hosts.0", "old-*", "new-host")],
    ));
    assert_eq!(mask.code(), WriteRejectionCode::FieldKindUnsupported);
    assert_eq!(mask.param("field_kind"), Some("host_mask"));
    let into_mask = rejection(verify_widget_field_replacements(
        &source,
        &context(Some(all_contracts())),
        &[replace(0, "15", "ds.0.hosts.0", "old-host", "new-*")],
    ));
    assert_eq!(
        into_mask.code(),
        WriteRejectionCode::ReplacementValueInvalid
    );
    // Item patterns are not host references.
    let pattern = rejection(verify_widget_field_replacements(
        &source,
        &context(Some(all_contracts())),
        &[replace(0, "15", "ds.0.items.0", "CPU*", "Memory")],
    ));
    assert_eq!(pattern.code(), WriteRejectionCode::FieldKindUnsupported);
}

#[test]
fn a_changed_page_with_an_unproven_widget_type_is_refused() {
    let mut raw = dashboard();
    raw["pages"][1]["widgets"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("widgets"))
        .push(widget(
            "24",
            "problems",
            12,
            vec![field(json!("3"), "hostids.0", json!("11"))],
        ));
    let source = snapshot(raw, "7.4.16");
    let replacements = [replace(1, "23", "itemid.0", "104", "204")];
    let requirements = write_requirements(&source, &replacements)
        .unwrap_or_else(|rejection| panic!("requirements: {rejection}"));
    assert_eq!(
        requirements.unproven_widgets,
        vec![UnprovenWidget {
            page_index: 1,
            widget_index: 1,
            widget_type: "problems".to_owned(),
        }]
    );
    let refused = rejection(verify_widget_field_replacements(
        &source,
        &context(Some(all_contracts())),
        &replacements,
    ));
    assert_eq!(
        refused.code(),
        WriteRejectionCode::ChangedPageWidgetUnproven
    );
    assert_eq!(refused.param("widget_type"), Some("problems"));
    // An untouched page may hold anything: it is sent by ID only.
    assert!(verify_widget_field_replacements(&source, &context(None), &first_page_items()).is_ok());
}

#[test]
fn unproven_versions_unknown_fields_and_stale_values_fail_closed() {
    let replacements = first_page_items();
    let unproven = snapshot(dashboard(), "7.4.17");
    let refused = rejection(verify_widget_field_replacements(
        &unproven,
        &PublicationContext::detached_for_testing(capabilities("7.4.17")),
        &replacements,
    ));
    assert_eq!(refused.code(), WriteRejectionCode::ContractUnverified);

    let source = snapshot(dashboard(), "7.4.16");
    let offline = rejection(verify_widget_field_replacements(
        &source,
        &PublicationContext::offline_for_planning(capabilities("7.4.16")),
        &replacements,
    ));
    assert_eq!(offline.code(), WriteRejectionCode::AuthorityOffline);
    let other_version = rejection(verify_widget_field_replacements(
        &source,
        &PublicationContext::detached_for_testing(capabilities("7.4.17")),
        &replacements,
    ));
    assert_eq!(
        other_version.code(),
        WriteRejectionCode::ServerVersionMismatch
    );

    let cases = [
        (
            replace(0, "13", "itemid.0", "999", "201"),
            WriteRejectionCode::SourceValueStale,
        ),
        (
            // Widget 14 is found by its ID, at whatever position.
            replace(0, "14", "itemid.0", "101", "201"),
            WriteRejectionCode::SourceValueStale,
        ),
        (
            // Widget 23 is on another page.
            replace(0, "23", "itemid.0", "104", "204"),
            WriteRejectionCode::WidgetIdentityMismatch,
        ),
        (
            replace(0, "99", "itemid.0", "101", "201"),
            WriteRejectionCode::WidgetNotFound,
        ),
        (
            replace(9, "13", "itemid.0", "101", "201"),
            WriteRejectionCode::WidgetNotFound,
        ),
        (
            replace(0, "13", "itemid.1", "101", "201"),
            WriteRejectionCode::FieldNotFound,
        ),
        (
            replace(0, "13", "reference", "ABCDE", "ZZZZZ"),
            WriteRejectionCode::FieldKindUnsupported,
        ),
        (
            replace(0, "13", "itemid.0", "101", "1x"),
            WriteRejectionCode::ReplacementValueInvalid,
        ),
        (
            replace(0, "13", "itemid.0", "101", "101"),
            WriteRejectionCode::ReplacementUnchanged,
        ),
        (
            replace(2, "33", "hostids.0", "11", "12"),
            WriteRejectionCode::FieldKindUnsupported,
        ),
    ];
    for (replacement, code) in cases {
        let refused = rejection(verify_widget_field_replacements(
            &source,
            &context(Some(all_contracts())),
            &[replacement],
        ));
        assert_eq!(refused.code(), code);
    }
    let twice = rejection(verify_widget_field_replacements(
        &source,
        &context(None),
        &[
            replace(0, "13", "itemid.0", "101", "201"),
            replace(0, "13", "itemid.0", "101", "202"),
        ],
    ));
    assert_eq!(twice.code(), WriteRejectionCode::FieldReplacedTwice);
    assert_eq!(
        rejection(verify_widget_field_replacements(
            &source,
            &context(None),
            &[]
        ))
        .code(),
        WriteRejectionCode::ReplacementCountInvalid
    );
}

// ---------- closed publication path on a mock transport ----------

#[derive(Default)]
struct MockData {
    dashboard: Value,
    updates: Vec<Value>,
    read_back: ReadBack,
}

/// How the mock stores a page sent by `dashboard.update`.
#[derive(Clone, Copy, Default)]
enum ReadBack {
    /// Exactly as sent.
    #[default]
    AsSent,
    /// As Zabbix 7.4.16 on PostgreSQL may return it (LAB-02): widgets and
    /// each widget's fields in another order, values unchanged.
    Reordered,
    /// Reordered, and one value of another JSON kind (`"201"` → `201`).
    ValueKindChanged,
}

fn store_page(incoming: &Value, read_back: ReadBack) -> Value {
    let mut page = incoming.clone();
    if matches!(read_back, ReadBack::AsSent) {
        return page;
    }
    let widgets = page["widgets"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("sent widgets"));
    widgets.reverse();
    for widget in widgets.iter_mut() {
        let fields = widget["fields"]
            .as_array_mut()
            .unwrap_or_else(|| panic!("sent fields"));
        fields.rotate_left(1);
        if matches!(read_back, ReadBack::ValueKindChanged) {
            for field in fields.iter_mut() {
                if field["value"] == "201" {
                    field["value"] = json!(201);
                }
            }
        }
    }
    page
}

type Shared = Arc<Mutex<MockData>>;

fn lock(data: &Shared) -> std::sync::MutexGuard<'_, MockData> {
    data.lock().unwrap_or_else(PoisonError::into_inner)
}

async fn rpc(State(data): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    let id = body["id"].clone();
    let mut data = lock(&data);
    let result = match body["method"].as_str().unwrap_or_default() {
        "apiinfo.version" => json!("7.4.16"),
        "user.checkAuthentication" => json!({"userid": "5", "username": "writer"}),
        "dashboard.get" => json!([data.dashboard.clone()]),
        "dashboard.update" => {
            data.updates.push(body["params"].clone());
            let params = body["params"].clone();
            let read_back = data.read_back;
            let pages = data.dashboard["pages"]
                .as_array_mut()
                .unwrap_or_else(|| panic!("pages"));
            for incoming in params["pages"]
                .as_array()
                .unwrap_or_else(|| panic!("sent pages"))
            {
                let current = pages
                    .iter_mut()
                    .find(|page| page["dashboard_pageid"] == incoming["dashboard_pageid"])
                    .unwrap_or_else(|| panic!("update page must exist"));
                if incoming.get("widgets").is_some() {
                    *current = store_page(incoming, read_back);
                }
            }
            json!({"dashboardids": ["7"]})
        }
        _ => json!([]),
    };
    Json(json!({"jsonrpc": "2.0", "result": result, "id": id}))
}

async fn writer(data: &Shared) -> (ReadWriteGateway, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route("/api_jsonrpc.php", post(rpc))
        .with_state(Arc::clone(data));
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
    let gateway = ZabbixClient::new(endpoint)
        .unwrap_or_else(|error| panic!("client: {error}"))
        .authenticate(Authentication::ApiToken(SecretString::from(
            "test-token".to_owned(),
        )))
        .await
        .unwrap_or_else(|error| panic!("authentication: {error}"))
        .enable_writes();
    (gateway, task)
}

async fn read(gateway: &ReadWriteGateway) -> VersionedDashboardSnapshot {
    gateway
        .dashboard_snapshots("mock-connection")
        .await
        .unwrap_or_else(|error| panic!("read: {error}"))
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("dashboard"))
}

#[tokio::test]
async fn a_multi_page_update_is_sent_once_on_the_proven_patch() {
    let data: Shared = Arc::new(Mutex::new(MockData {
        dashboard: dashboard(),
        ..MockData::default()
    }));
    let (gateway, task) = writer(&data).await;

    let source = read(&gateway).await;
    let mut replacements = first_page_items();
    replacements.push(replace(1, "23", "itemid.0", "104", "204"));
    let update = verified(verify_widget_field_replacements(
        &source,
        &gateway.publication_context(),
        &replacements,
    ));
    let outcome = gateway
        .publish_verified_dashboard_update(&update, || true)
        .await;
    assert_eq!(outcome, DashboardPublishOutcome::Verified);
    let data = lock(&data);
    assert_eq!(data.updates.len(), 1);
    assert_eq!(data.updates[0], params(&update, &source));
    // The untouched third page was sent by ID and kept as read.
    assert_eq!(
        data.updates[0]["pages"][2],
        json!({"dashboard_pageid": "31"})
    );
    assert_eq!(
        data.dashboard["pages"][2],
        source.raw_api_document()["pages"][2]
    );
    drop(data);
    task.abort();
}

#[tokio::test]
async fn a_contract_withdrawn_before_send_blocks_the_write() {
    let data: Shared = Arc::new(Mutex::new(MockData {
        dashboard: dashboard(),
        ..MockData::default()
    }));
    let (mut gateway, task) = writer(&data).await;
    let source = read(&gateway).await;
    let mut replacements = first_page_items();
    replacements.push(replace(1, "23", "itemid.0", "104", "204"));
    let update = verified(verify_widget_field_replacements(
        &source,
        &gateway.publication_context(),
        &replacements,
    ));

    gateway.set_write_contracts_for_test(without(&[WriteContract::UpdateSeveralPages]));
    let outcome = gateway
        .publish_verified_dashboard_update(&update, || true)
        .await;
    assert_eq!(outcome, DashboardPublishOutcome::FailedBeforeSend);
    assert!(lock(&data).updates.is_empty());
    task.abort();
}

#[tokio::test]
async fn a_refused_submit_record_blocks_the_write_and_a_conflict_skips_the_gate() {
    let data: Shared = Arc::new(Mutex::new(MockData {
        dashboard: dashboard(),
        ..MockData::default()
    }));
    let (gateway, task) = writer(&data).await;
    let source = read(&gateway).await;
    let update = verified(verify_widget_field_replacements(
        &source,
        &gateway.publication_context(),
        &first_page_items(),
    ));
    let mut asked = 0;
    let outcome = gateway
        .publish_verified_dashboard_update(&update, || {
            asked += 1;
            false
        })
        .await;
    assert_eq!(outcome, DashboardPublishOutcome::FailedBeforeSend);
    assert_eq!(asked, 1, "the gate runs once, right before the send");
    assert!(lock(&data).updates.is_empty());

    lock(&data).dashboard["pages"][0]["name"] = json!("Concurrent edit");
    let mut asked = 0;
    let outcome = gateway
        .publish_verified_dashboard_update(&update, || {
            asked += 1;
            true
        })
        .await;
    assert_eq!(outcome, DashboardPublishOutcome::Conflict);
    assert_eq!(asked, 0, "no submit record when nothing can be sent");
    assert!(lock(&data).updates.is_empty());
    task.abort();
}

#[tokio::test]
async fn editable_check_is_a_read_with_the_editable_filter() {
    let data: Shared = Arc::new(Mutex::new(MockData {
        dashboard: dashboard(),
        ..MockData::default()
    }));
    let (gateway, task) = writer(&data).await;
    assert!(
        gateway
            .dashboard_is_editable("7")
            .await
            .unwrap_or_else(|error| panic!("editable: {error}"))
    );
    assert!(
        !gateway
            .dashboard_is_editable("8")
            .await
            .unwrap_or_else(|error| panic!("editable: {error}"))
    );
    assert!(gateway.dashboard_is_editable("x").await.is_err());
    assert!(lock(&data).updates.is_empty());
    task.abort();
}

/// The first-page items plus the second page: two changed pages.
fn two_page_items() -> Vec<WidgetFieldReplacement> {
    let mut replacements = first_page_items();
    replacements.push(replace(1, "23", "itemid.0", "104", "204"));
    replacements
}

#[tokio::test]
async fn a_read_back_in_another_widget_and_field_order_is_verified() {
    let data: Shared = Arc::new(Mutex::new(MockData {
        dashboard: dashboard(),
        read_back: ReadBack::Reordered,
        ..MockData::default()
    }));
    let (gateway, task) = writer(&data).await;
    let source = read(&gateway).await;
    let update = verified(verify_widget_field_replacements(
        &source,
        &gateway.publication_context(),
        &two_page_items(),
    ));
    let outcome = gateway
        .publish_verified_dashboard_update(&update, || true)
        .await;
    assert_eq!(outcome, DashboardPublishOutcome::Verified);
    let after = read(&gateway).await;
    assert_ne!(
        after.raw_api_document()["pages"][0],
        source.raw_api_document()["pages"][0],
        "the mock must return another order"
    );
    assert_eq!(after.fingerprint(), update.expected_fingerprint());
    assert_eq!(lock(&data).updates.len(), 1);
    task.abort();
}

#[tokio::test]
async fn a_read_back_with_another_value_kind_is_not_verified() {
    let data: Shared = Arc::new(Mutex::new(MockData {
        dashboard: dashboard(),
        read_back: ReadBack::ValueKindChanged,
        ..MockData::default()
    }));
    let (gateway, task) = writer(&data).await;
    let source = read(&gateway).await;
    let update = verified(verify_widget_field_replacements(
        &source,
        &gateway.publication_context(),
        &two_page_items(),
    ));
    let outcome = gateway
        .publish_verified_dashboard_update(&update, || true)
        .await;
    assert_eq!(outcome, DashboardPublishOutcome::FailedAfterResponse);
    assert_eq!(lock(&data).updates.len(), 1, "never a second send");
    task.abort();
}

#[tokio::test]
async fn a_source_read_in_another_order_is_not_a_conflict() {
    let data: Shared = Arc::new(Mutex::new(MockData {
        dashboard: dashboard(),
        ..MockData::default()
    }));
    let (gateway, task) = writer(&data).await;
    let source = read(&gateway).await;
    let update = verified(verify_widget_field_replacements(
        &source,
        &gateway.publication_context(),
        &first_page_items(),
    ));
    // Between plan and send the server returns the same dashboard with the
    // fields of every widget in another order.
    {
        let mut data = lock(&data);
        for page in data.dashboard["pages"]
            .as_array_mut()
            .unwrap_or_else(|| panic!("pages"))
        {
            for widget in page["widgets"]
                .as_array_mut()
                .unwrap_or_else(|| panic!("widgets"))
            {
                widget["fields"]
                    .as_array_mut()
                    .unwrap_or_else(|| panic!("fields"))
                    .reverse();
            }
        }
    }
    let outcome = gateway
        .publish_verified_dashboard_update(&update, || true)
        .await;
    assert_eq!(outcome, DashboardPublishOutcome::Verified);
    assert_eq!(lock(&data).updates.len(), 1);
    task.abort();
}

/// `widgetid`s of a page as read or sent, in array order.
fn widget_ids(page: &Value) -> Vec<String> {
    page["widgets"]
        .as_array()
        .unwrap_or_else(|| panic!("widgets"))
        .iter()
        .map(|widget| widget["widgetid"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn reverse_widgets(raw: &mut Value, page: usize) {
    raw["pages"][page]["widgets"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("widgets"))
        .reverse();
}

#[test]
fn replacements_are_addressed_by_widgetid_whatever_the_widget_order() {
    let base = snapshot(dashboard(), "7.4.16");
    let planned = verified(verify_widget_field_replacements(
        &base,
        &context(None),
        &first_page_items(),
    ));
    let mut raw = dashboard();
    reverse_widgets(&mut raw, 0);
    let remote = snapshot(raw, "7.4.16");
    assert_eq!(remote.fingerprint(), base.fingerprint());
    // The same replacements on a read with another widget order: no
    // identity mismatch, the same expected state.
    let update = verified(verify_widget_field_replacements(
        &remote,
        &context(None),
        &first_page_items(),
    ));
    assert_eq!(
        update.expected_fingerprint(),
        planned.expected_fingerprint()
    );
    // The payload keeps the order of the read it was built from.
    let sent = params(&update, &remote);
    assert_eq!(widget_ids(&sent["pages"][0]), vec!["15", "14", "13"]);
    assert_only_values_changed(
        &sent["pages"][0]["widgets"][2],
        &remote.raw_api_document()["pages"][0]["widgets"][2],
        &[("itemid.0", json!("201"))],
    );
    // A refusal reports the position of this read.
    let refused = rejection(verify_widget_field_replacements(
        &remote,
        &context(None),
        &[replace(0, "13", "itemid.1", "101", "201")],
    ));
    assert_eq!(refused.code(), WriteRejectionCode::FieldNotFound);
    assert_eq!(refused.param("widget_index"), Some("2"));
}

#[test]
fn a_widgetid_twice_on_the_page_is_an_identity_mismatch() {
    let mut raw = dashboard();
    let duplicate = raw["pages"][0]["widgets"][0].clone();
    raw["pages"][0]["widgets"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("widgets"))
        .push(duplicate);
    let source = snapshot(raw, "7.4.16");
    let refused = rejection(verify_widget_field_replacements(
        &source,
        &context(None),
        &[replace(0, "13", "itemid.0", "101", "201")],
    ));
    assert_eq!(refused.code(), WriteRejectionCode::WidgetIdentityMismatch);
}

#[tokio::test]
async fn a_source_read_with_widgets_in_another_order_is_sent_in_that_order() {
    let data: Shared = Arc::new(Mutex::new(MockData {
        dashboard: dashboard(),
        ..MockData::default()
    }));
    let (gateway, task) = writer(&data).await;
    let source = read(&gateway).await;
    let update = verified(verify_widget_field_replacements(
        &source,
        &gateway.publication_context(),
        &two_page_items(),
    ));
    // Between plan and send the server returns the widgets of the first
    // page in another order.
    reverse_widgets(&mut lock(&data).dashboard, 0);
    let outcome = gateway
        .publish_verified_dashboard_update(&update, || true)
        .await;
    assert_eq!(outcome, DashboardPublishOutcome::Verified);
    let data = lock(&data);
    assert_eq!(data.updates.len(), 1);
    assert_eq!(
        widget_ids(&data.updates[0]["pages"][0]),
        vec!["15", "14", "13"],
        "widgets are sent in the order of the read before send"
    );
    assert_ne!(
        widget_ids(&data.updates[0]["pages"][0]),
        widget_ids(&params(&update, &source)["pages"][0])
    );
    drop(data);
    task.abort();
}

#[test]
fn a_changed_page_widget_with_a_duplicate_field_name_is_refused() {
    let mut raw = dashboard();
    raw["pages"][0]["widgets"][2]["fields"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("fields"))
        .push(field(json!("1"), "reference", json!("KLMNO")));
    let source = snapshot(raw, "7.4.16");
    let refused = rejection(verify_widget_field_replacements(
        &source,
        &context(None),
        &first_page_items(),
    ));
    assert_eq!(refused.code(), WriteRejectionCode::FieldAmbiguous);
    assert_eq!(refused.param("widget_index"), Some("2"));
    assert_eq!(refused.param("field"), Some("reference"));
    // The same widget on an untouched page is sent by ID only.
    assert!(
        verify_widget_field_replacements(
            &source,
            &context(None),
            &[replace(1, "23", "itemid.0", "104", "204")],
        )
        .is_ok()
    );
}
