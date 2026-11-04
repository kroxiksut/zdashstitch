// SPDX-License-Identifier: MIT OR Apache-2.0

//! Laboratory write contracts for P2 (LAB-02): `update_several_pages`,
//! `svggraph` explicit item and host pattern replacement (P2-04A), classic
//! `graph` `graphid` replacement (P2-04H), `dashboard.create` (P2-06A), the
//! reconciliation of a create with an unknown outcome (P2-06D) and the read
//! order of `users` / `userGroups`.
//!
//! Every test is ignored and gated by an acknowledgement; it writes only to
//! dashboards it creates itself, named with the `ZDashstitch Contract Lab —`
//! prefix, and keeps them as evidence. The contract updates themselves go
//! through the adapter's closed publication path
//! (`verify_widget_field_replacements` and
//! `publish_verified_dashboard_update`), so the read-back is decided by the
//! production canonical fingerprint; fixtures are reset with raw API calls. Hosts, items and graphs are only read.
//! The create and sharing tests also maintain two dedicated non-Super-admin
//! users, each in its own user group. Evidence lines contain field names, counts and categories
//! only: no IDs, host names or credentials.

use std::collections::hash_map::RandomState;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::hash::BuildHasher;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::{Client, redirect::Policy};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zdashstitch_zabbix::{
    Authentication, CreateContract, CreateReconciliation, CreateRejectionCode,
    DashboardPublishOutcome, ReadBackVerdict, ReadOnlyGateway, ReadWriteGateway,
    VersionedDashboardSnapshot, WidgetFieldReplacement, WriteContract, WriteRejectionCode,
    ZabbixClient, ZabbixEndpoint, canonical_document, verify_dashboard_create,
    verify_widget_field_replacements, write_requirements,
};

const ACK_VAR: &str = "ZDASHSTITCH_LAB_P2_WRITE_ACK";
const ACK_VALUE: &str = "write-authorized-7.4-p2-contract-dashboards";
const NAME_PREFIX: &str = "ZDashstitch Contract Lab — ";
const SEVERAL_PAGES_NAME: &str = "ZDashstitch Contract Lab — several pages";
const SVGGRAPH_NAME: &str = "ZDashstitch Contract Lab — svggraph host replacement";
const CLASSIC_GRAPH_NAME: &str = "ZDashstitch Contract Lab — classic graph";
const CREATE_DEFAULTS_NAME: &str = "ZDashstitch Contract Lab — create defaults";
const CREATE_COPY_NAME: &str = "ZDashstitch Contract Lab — create copy";
const UNREGISTERED_TYPE_NAME: &str = "ZDashstitch Contract Lab — unregistered widget type";
const HIDDEN_PRIVATE_NAME: &str = "ZDashstitch Contract Lab — hidden private";
const CONTRACT_USER_DASHBOARD_NAME: &str = "ZDashstitch Contract Lab — contract user";
const DUPLICATE_FIELDS_NAME: &str = "ZDashstitch Contract Lab — duplicate field names";
const CREATE_RECONCILE_SOURCE_NAME: &str = "ZDashstitch Contract Lab — create reconcile source";
const CREATE_RECONCILE_NAME: &str = "ZDashstitch Contract Lab — create reconcile";
const SHARING_ORDER_NAME: &str = "ZDashstitch Contract Lab — sharing order";
const CONTRACT_USER: &str = "zds-contract-user";
const CONTRACT_GROUP: &str = "zds-contract-group";
const SECOND_USER: &str = "zds-contract-user-2";
const SECOND_GROUP: &str = "zds-contract-group-2";
const USER_ROLE_TYPE: &str = "1";
const CONNECTION_ID: &str = "lab-write-contracts-p2";

// ---------------------------------------------------------------- transport

#[derive(Debug)]
struct ApiError {
    code: i64,
    message: String,
    data: String,
}

struct LabApi {
    endpoint: String,
    client: Client,
    token: SecretString,
    next_id: u64,
}

impl LabApi {
    async fn login(endpoint: &str, username: &str, password: &SecretString) -> Self {
        let client = Client::builder()
            .redirect(Policy::none())
            .build()
            .unwrap_or_else(|error| panic!("cannot build lab HTTP client: {error}"));
        let token = rpc_call(
            &client,
            endpoint,
            1,
            "user.login",
            json!({ "username": username, "password": password.expose_secret() }),
            None,
        )
        .await
        .unwrap_or_else(|error| panic!("user.login failed with code {}", error.code))
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| panic!("user.login did not return a token"));
        Self {
            endpoint: endpoint.to_owned(),
            client,
            token: SecretString::from(token),
            next_id: 2,
        }
    }

    async fn try_call(&mut self, method: &str, params: Value) -> Result<Value, ApiError> {
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

    async fn call(&mut self, method: &str, params: Value) -> Value {
        self.try_call(method, params).await.unwrap_or_else(|error| {
            panic!(
                "laboratory {method} failed with code {}: {} {}",
                error.code, error.message, error.data
            )
        })
    }

    /// Sends one request and drops the answer without parsing it: the
    /// caller does not learn whether the request succeeded (the unknown
    /// outcome of P2-04D/P2-06D).
    async fn dispatch_without_reading_result(&mut self, method: &str, params: Value) {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(self.token.expose_secret())
            .json(&json!({"jsonrpc": "2.0", "method": method, "params": params, "id": id}))
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
        assert_eq!(self.call("user.logout", json!([])).await, true);
    }
}

async fn rpc_call(
    client: &Client,
    endpoint: &str,
    id: u64,
    method: &str,
    params: Value,
    token: Option<&SecretString>,
) -> Result<Value, ApiError> {
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
        return Err(ApiError {
            code: error
                .get("code")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            message: text(error, "message"),
            data: text(error, "data"),
        });
    }
    Ok(body
        .get("result")
        .cloned()
        .unwrap_or_else(|| panic!("laboratory {method} response has no result")))
}

struct Lab {
    endpoint: String,
    username: String,
    reader: ReadOnlyGateway,
    /// Separate session switched to read-write: the production path.
    writer: ReadWriteGateway,
    api: LabApi,
}

async fn adapter_client(
    endpoint: &str,
    username: &str,
    password: &SecretString,
) -> ReadOnlyGateway {
    let parsed = ZabbixEndpoint::parse(endpoint)
        .unwrap_or_else(|error| panic!("invalid laboratory endpoint: {error}"));
    ZabbixClient::new(parsed)
        .unwrap_or_else(|error| panic!("cannot construct client: {error}"))
        .authenticate(Authentication::UsernamePassword {
            username: username.to_owned(),
            password: password.clone(),
        })
        .await
        .unwrap_or_else(|error| panic!("client authentication failed: {error}"))
}

async fn open_lab() -> Lab {
    assert_eq!(required(ACK_VAR), ACK_VALUE);
    let endpoint = required("ZDASHSTITCH_LAB_API_URL");
    let username = required("ZDASHSTITCH_LAB_USERNAME");
    let password = SecretString::from(required("ZDASHSTITCH_LAB_PASSWORD"));
    let reader = adapter_client(&endpoint, &username, &password).await;
    assert!(reader.capabilities().version.is_supported_7_4());
    println!("apiinfo.version: {}", reader.capabilities().version);
    let writer = adapter_client(&endpoint, &username, &password)
        .await
        .enable_writes();
    let api = LabApi::login(&endpoint, &username, &password).await;
    Lab {
        endpoint,
        username,
        reader,
        writer,
        api,
    }
}

impl Lab {
    async fn close(self) {
        self.api.logout().await;
        self.writer
            .logout()
            .await
            .unwrap_or_else(|error| panic!("write client logout failed: {error}"));
        self.reader
            .logout()
            .await
            .unwrap_or_else(|error| panic!("read client logout failed: {error}"));
    }
}

// ------------------------------------------------------------- lab objects

struct Host {
    hostid: String,
    name: String,
}

struct ItemPair {
    old: String,
    new: String,
    name: String,
}

struct HostPair {
    template_hosts: usize,
    old: Host,
    new: Host,
    items: Vec<ItemPair>,
    graph: Option<(String, String)>,
}

/// Two monitored hosts linked to one template, with numeric template items
/// of equal key and a templated graph of the same name on both.
async fn host_pair(api: &mut LabApi) -> HostPair {
    let templates = api
        .call(
            "template.get",
            json!({
                "output": ["templateid"],
                "selectHosts": ["hostid", "name", "status"]
            }),
        )
        .await;
    let mut templates = array(&templates).to_vec();
    templates.sort_by_key(|template| {
        text(template, "templateid")
            .parse::<u64>()
            .unwrap_or(u64::MAX)
    });
    for template in &templates {
        let mut hosts = array(&template["hosts"])
            .iter()
            .filter(|host| host["status"] == "0")
            .map(|host| Host {
                hostid: text(host, "hostid"),
                name: text(host, "name"),
            })
            .collect::<Vec<_>>();
        if hosts.len() < 2 {
            continue;
        }
        hosts.sort_by_key(|host| host.hostid.parse::<u64>().unwrap_or(u64::MAX));
        let template_hosts = hosts.len();
        let mut hosts = hosts.into_iter();
        let (Some(old), Some(new)) = (hosts.next(), hosts.next()) else {
            continue;
        };
        let items = item_pairs(api, &old, &new).await;
        let graph = graph_pair(api, &old, &new).await;
        if items.len() >= 3 && graph.is_some() {
            return HostPair {
                template_hosts,
                old,
                new,
                items,
                graph,
            };
        }
    }
    panic!("laboratory needs two hosts of one template with common items and a templated graph");
}

async fn item_pairs(api: &mut LabApi, old: &Host, new: &Host) -> Vec<ItemPair> {
    let items = api
        .call(
            "item.get",
            json!({
                "output": ["itemid", "hostid", "key_", "name", "templateid"],
                "hostids": [old.hostid, new.hostid],
                "filter": {"flags": 0, "status": 0, "value_type": [0, 3]}
            }),
        )
        .await;
    let mut by_key: BTreeMap<String, (Option<String>, Option<String>, String)> = BTreeMap::new();
    for item in array(&items) {
        if item["templateid"] == "0" {
            continue;
        }
        let entry = by_key
            .entry(text(item, "key_"))
            .or_insert_with(|| (None, None, text(item, "name")));
        if item["hostid"] == old.hostid.as_str() {
            entry.0 = Some(text(item, "itemid"));
        } else if item["hostid"] == new.hostid.as_str() {
            entry.1 = Some(text(item, "itemid"));
        }
    }
    by_key
        .into_values()
        .filter_map(|(old, new, name)| {
            Some(ItemPair {
                old: old?,
                new: new?,
                name,
            })
        })
        .take(3)
        .collect()
}

/// Old-host graph, new-host graph and graph names of one parent template graph.
type GraphCandidates = (Option<String>, Option<String>, BTreeSet<String>);

/// `graphid` on the old and new host of one templated graph: same name and
/// same parent template graph.
async fn graph_pair(api: &mut LabApi, old: &Host, new: &Host) -> Option<(String, String)> {
    let graphs = api
        .call(
            "graph.get",
            json!({
                "output": ["graphid", "name", "templateid", "flags"],
                "hostids": [old.hostid, new.hostid],
                "selectHosts": ["hostid"],
                "filter": {"flags": 0}
            }),
        )
        .await;
    let mut by_template: BTreeMap<String, GraphCandidates> = BTreeMap::new();
    for graph in array(&graphs) {
        let hosts = array(&graph["hosts"]);
        if graph["templateid"] == "0" || hosts.len() != 1 {
            continue;
        }
        let entry = by_template
            .entry(text(graph, "templateid"))
            .or_insert_with(|| (None, None, BTreeSet::new()));
        entry.2.insert(text(graph, "name"));
        if hosts[0]["hostid"] == old.hostid.as_str() {
            entry.0 = Some(text(graph, "graphid"));
        } else if hosts[0]["hostid"] == new.hostid.as_str() {
            entry.1 = Some(text(graph, "graphid"));
        }
    }
    by_template
        .into_values()
        .find_map(|(old, new, names)| (names.len() == 1).then_some((old?, new?)))
}

async fn caller_userid(api: &mut LabApi, username: &str) -> String {
    let users = api
        .call(
            "user.get",
            json!({"output": ["userid", "username"], "filter": {"username": username}}),
        )
        .await;
    let users = array(&users);
    assert_eq!(users.len(), 1, "the calling user must see itself");
    text(&users[0], "userid")
}

// ------------------------------------------------------------- dashboards

fn find_dashboard(
    snapshots: &[VersionedDashboardSnapshot],
    name: &str,
) -> Option<VersionedDashboardSnapshot> {
    let mut matching = snapshots
        .iter()
        .filter(|snapshot| snapshot.raw_api_document()["name"] == name);
    let found = matching.next().cloned();
    assert!(
        matching.next().is_none(),
        "several laboratory dashboards share a reserved name"
    );
    found
}

async fn read_dashboard(reader: &ReadOnlyGateway, name: &str) -> VersionedDashboardSnapshot {
    let snapshots = reader
        .dashboard_snapshots(CONNECTION_ID)
        .await
        .unwrap_or_else(|error| panic!("cannot read laboratory dashboards: {error}"));
    find_dashboard(&snapshots, name)
        .unwrap_or_else(|| panic!("owned laboratory dashboard is absent"))
}

/// Brings the test-owned dashboard `name` to `pages` (created on the first
/// run, all pages and widgets replaced afterwards) and returns the read
/// baseline together with the server normalisation of the sent pages.
async fn reset_dashboard(
    lab: &mut Lab,
    name: &str,
    pages: &Value,
) -> (VersionedDashboardSnapshot, BTreeMap<String, usize>) {
    assert!(name.starts_with(NAME_PREFIX));
    let snapshots = lab
        .reader
        .dashboard_snapshots(CONNECTION_ID)
        .await
        .unwrap_or_else(|error| panic!("cannot read laboratory dashboards: {error}"));
    match find_dashboard(&snapshots, name) {
        Some(existing) => {
            lab.api
                .call(
                    "dashboard.update",
                    json!({"dashboardid": existing.dashboard_id(), "pages": pages}),
                )
                .await;
        }
        None => {
            lab.api
                .call(
                    "dashboard.create",
                    json!({"name": name, "private": 1, "pages": pages}),
                )
                .await;
        }
    }
    let baseline = read_dashboard(&lab.reader, name).await;
    let normalisation = normalisation(pages, &baseline.raw_api_document()["pages"]);
    (baseline, normalisation)
}

/// Deletes a dashboard the caller created on an earlier run, so that the
/// create contract starts from an absent name. Refuses anything not owned
/// by the caller or outside the reserved prefix.
async fn delete_owned(api: &mut LabApi, name: &str, owner: &str) {
    assert!(name.starts_with(NAME_PREFIX));
    let found = api
        .call(
            "dashboard.get",
            json!({"output": ["dashboardid", "name", "userid"], "filter": {"name": name}}),
        )
        .await;
    for dashboard in array(&found) {
        assert_eq!(
            dashboard["userid"], owner,
            "refusing to delete a reserved-name dashboard of another owner"
        );
        api.call("dashboard.delete", json!([text(dashboard, "dashboardid")]))
            .await;
    }
}

// --------------------------------------------------------- update helpers

struct Change {
    page: usize,
    widget: usize,
    field: &'static str,
    value: String,
}

impl Change {
    fn new(page: usize, widget: usize, field: &'static str, value: &str) -> Self {
        Self {
            page,
            widget,
            field,
            value: value.to_owned(),
        }
    }
}

/// The read document with `changes` applied, and the adapter replacements
/// describing them.
fn apply_changes(
    snapshot: &VersionedDashboardSnapshot,
    changes: &[Change],
) -> (Value, Vec<WidgetFieldReplacement>) {
    let mut expected = snapshot.raw_api_document().clone();
    let mut replacements = Vec::new();
    for change in changes {
        let widget = &mut expected["pages"][change.page]["widgets"][change.widget];
        let widgetid = text(widget, "widgetid");
        let fields = widget["fields"]
            .as_array_mut()
            .unwrap_or_else(|| panic!("widget fields must be an array"));
        let mut matching = fields
            .iter_mut()
            .filter(|field| field["name"] == change.field);
        let field = matching
            .next()
            .unwrap_or_else(|| panic!("baseline field {} is absent", change.field));
        assert!(matching.next().is_none(), "baseline field is ambiguous");
        let previous = text(field, "value");
        field["value"] = Value::String(change.value.clone());
        replacements.push(WidgetFieldReplacement {
            page_index: change.page,
            widgetid,
            field_name: change.field.to_owned(),
            expected_value: previous,
            replacement_value: change.value.clone(),
        });
    }
    (expected, replacements)
}

/// What the read-back showed about `fields` order.
struct OrderEvidence {
    /// The whole document equals `expected` including every array order.
    byte_identical: bool,
    /// Widgets whose read `fields` order differs from the expected order.
    reordered_widgets: usize,
    /// In every reordered widget the unchanged fields keep their relative
    /// order and exactly the changed fields moved, to the end.
    only_changed_fields_moved_to_end: bool,
}

/// Publishes `replacements` through the adapter's closed path (fresh read,
/// canonical source check, one `dashboard.update`, canonical read-back) and
/// requires `Verified`. Then independently requires the read document to
/// equal `expected` with every widget's `fields` compared as a set keyed by
/// (`name`, `type`) and everything else, including page and widget order,
/// exactly; and the adapter's canonical form and fingerprint of `expected`
/// to equal those of the read-back. Field order is reported separately:
/// Zabbix 7.4 reads `widget_field` rows without `ORDER BY`.
async fn update_and_verify(
    lab: &mut Lab,
    name: &str,
    baseline: &VersionedDashboardSnapshot,
    expected: &Value,
    changes: &[Change],
    replacements: &[WidgetFieldReplacement],
) -> (VersionedDashboardSnapshot, OrderEvidence) {
    let changed: BTreeSet<usize> = changes.iter().map(|change| change.page).collect();
    let update =
        verify_widget_field_replacements(baseline, &lab.writer.publication_context(), replacements)
            .unwrap_or_else(|rejection| {
                panic!(
                    "adapter refused the update: {rejection} {:?}",
                    rejection.params()
                )
            });
    assert_eq!(
        update
            .changed_pages()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        changed
    );
    let mut sends = 0;
    let outcome = lab
        .writer
        .publish_verified_dashboard_update(&update, || {
            sends += 1;
            true
        })
        .await;
    assert_eq!(sends, 1, "exactly one send");
    assert_eq!(
        outcome,
        DashboardPublishOutcome::Verified,
        "production read-back must verify the write"
    );
    let after = read_dashboard(&lab.reader, name).await;
    assert_eq!(after.fingerprint(), update.expected_fingerprint());
    let expected_snapshot = VersionedDashboardSnapshot::from_adapter_raw(
        after.connection_id().to_owned(),
        after.server_version().clone(),
        expected.clone(),
    )
    .unwrap_or_else(|error| panic!("expected document is not a snapshot: {error}"));
    assert_eq!(expected_snapshot.fingerprint(), after.fingerprint());
    assert_eq!(
        canonical_document(expected),
        canonical_document(after.raw_api_document())
    );
    let raw = after.raw_api_document();
    let mut differences = Vec::new();
    diff(
        "dashboard",
        &sorted_fields(expected),
        &sorted_fields(raw),
        &mut differences,
    );
    assert!(
        differences.is_empty(),
        "read-back differs from the expected document at: {differences:?}"
    );
    let mut evidence = OrderEvidence {
        byte_identical: raw == expected,
        reordered_widgets: 0,
        only_changed_fields_moved_to_end: true,
    };
    for (page_index, page) in array(&expected["pages"]).iter().enumerate() {
        for (widget_index, widget) in array(&page["widgets"]).iter().enumerate() {
            let before = field_names(widget);
            let read = field_names(&raw["pages"][page_index]["widgets"][widget_index]);
            if before == read {
                continue;
            }
            evidence.reordered_widgets += 1;
            let moved = changes
                .iter()
                .filter(|change| change.page == page_index && change.widget == widget_index)
                .map(|change| change.field.to_owned())
                .collect::<BTreeSet<_>>();
            let kept = before
                .iter()
                .filter(|name| !moved.contains(*name))
                .cloned()
                .collect::<Vec<_>>();
            let tail = read[kept.len()..].iter().cloned().collect::<BTreeSet<_>>();
            if read[..kept.len()] != kept[..] || tail != moved {
                evidence.only_changed_fields_moved_to_end = false;
            }
        }
    }
    (after, evidence)
}

fn field_names(widget: &Value) -> Vec<String> {
    array(&widget["fields"])
        .iter()
        .map(|field| text(field, "name"))
        .collect()
}

/// The document with every widget's `fields` sorted by (`name`, `type`).
fn sorted_fields(raw: &Value) -> Value {
    let mut raw = raw.clone();
    if let Some(pages) = raw["pages"].as_array_mut() {
        for page in pages {
            if let Some(widgets) = page["widgets"].as_array_mut() {
                for widget in widgets {
                    if let Some(fields) = widget["fields"].as_array_mut() {
                        fields.sort_by_key(|field| (text(field, "name"), text(field, "type")));
                    }
                }
            }
        }
    }
    raw
}

/// The canonical and the raw-bytes fingerprint of the expected document
/// against the read-back (`update_and_verify` already requires the
/// canonical one to match).
fn print_fingerprint(scope: &str, after: &VersionedDashboardSnapshot, expected: &Value) {
    let expected = VersionedDashboardSnapshot::from_adapter_raw(
        after.connection_id().to_owned(),
        after.server_version().clone(),
        expected.clone(),
    )
    .unwrap_or_else(|error| panic!("expected document is not a snapshot: {error}"));
    println!(
        "evidence {scope}: publish_outcome=verified, canonical fingerprint equal={}, \
         raw-bytes fingerprint equal={}",
        expected.fingerprint() == after.fingerprint(),
        expected.storage_fingerprint() == after.storage_fingerprint()
    );
}

fn print_order(scope: &str, evidence: &OrderEvidence) {
    println!(
        "evidence {scope}: read-back byte_identical_including_field_order={}, \
         equal_with_fields_as_set=true, widgets_with_reordered_fields={}, \
         only_changed_fields_moved_to_end={}",
        evidence.byte_identical,
        evidence.reordered_widgets,
        evidence.only_changed_fields_moved_to_end
    );
}

fn ids(raw: &Value) -> (Vec<String>, Vec<Vec<String>>) {
    let pages = array(&raw["pages"]);
    (
        pages
            .iter()
            .map(|page| text(page, "dashboard_pageid"))
            .collect(),
        pages
            .iter()
            .map(|page| {
                array(&page["widgets"])
                    .iter()
                    .map(|widget| text(widget, "widgetid"))
                    .collect()
            })
            .collect(),
    )
}

// ------------------------------------------------------------ comparison

/// Sanitized differences between two documents: paths and field names
/// only, never values.
fn diff(path: &str, expected: &Value, actual: &Value, out: &mut Vec<String>) {
    match (expected, actual) {
        (Value::Object(left), Value::Object(right)) => {
            for key in left.keys().filter(|key| !right.contains_key(*key)) {
                out.push(format!("{path}.{key}: removed"));
            }
            for key in right.keys().filter(|key| !left.contains_key(*key)) {
                out.push(format!("{path}.{key}: added"));
            }
            for (key, value) in left {
                if let Some(other) = right.get(key) {
                    diff(&format!("{path}.{key}"), value, other, out);
                }
            }
        }
        (Value::Array(left), Value::Array(right)) => {
            if left.len() != right.len() {
                out.push(format!("{path}: {} -> {} entries", left.len(), right.len()));
                return;
            }
            for (index, (value, other)) in left.iter().zip(right).enumerate() {
                let label = value["name"]
                    .as_str()
                    .filter(|_| path.ends_with(".fields"))
                    .map_or_else(
                        || format!("{path}[{index}]"),
                        |name| format!("{path}({name})"),
                    );
                diff(&label, value, other, out);
            }
        }
        (left, right) if left == right => {}
        (left, right) => out.push(format!(
            "{path}: {} -> {} value differs",
            kind(left),
            kind(right)
        )),
    }
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// How the server rewrote sent pages on read-back, by category: keys
/// added or removed, scalars turned into strings, values changed, fields
/// reordered. Pages and widgets are matched by position.
fn normalisation(sent: &Value, read: &Value) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    let mut note = |category: String| *out.entry(category).or_insert(0) += 1;
    let (sent_pages, read_pages) = (array(sent), array(read));
    if sent_pages.len() != read_pages.len() {
        note(format!(
            "page count {} -> {}",
            sent_pages.len(),
            read_pages.len()
        ));
    }
    for (sent_page, read_page) in sent_pages.iter().zip(read_pages) {
        keys_and_scalars("page", sent_page, read_page, &["widgets"], &mut note);
        let (sent_widgets, read_widgets) = (
            optional_array(&sent_page["widgets"]),
            optional_array(&read_page["widgets"]),
        );
        if sent_widgets.len() != read_widgets.len() {
            note(format!(
                "widget count {} -> {}",
                sent_widgets.len(),
                read_widgets.len()
            ));
            continue;
        }
        for (sent_widget, read_widget) in sent_widgets.iter().zip(read_widgets) {
            let widget_type = sent_widget["type"].as_str().unwrap_or("?");
            if read_widget["type"] != sent_widget["type"] {
                note(format!("widget {widget_type}: type rewritten"));
            }
            let label = format!("widget {widget_type}");
            keys_and_scalars(&label, sent_widget, read_widget, &["fields"], &mut note);
            fields(&label, sent_widget, read_widget, &mut note);
        }
    }
    out
}

fn keys_and_scalars(
    label: &str,
    sent: &Value,
    read: &Value,
    skip: &[&str],
    note: &mut impl FnMut(String),
) {
    let (Some(sent), Some(read)) = (sent.as_object(), read.as_object()) else {
        note(format!("{label}: not an object"));
        return;
    };
    for key in read.keys().filter(|key| !sent.contains_key(*key)) {
        let value = &read[key];
        let shown = match value {
            Value::Array(items) if items.is_empty() => "[]".to_owned(),
            Value::String(text) if key != "dashboard_pageid" && key != "widgetid" => {
                format!("{text:?}")
            }
            _ => kind(value).to_owned(),
        };
        note(format!("{label}: key {key} added ({shown})"));
    }
    for key in sent.keys().filter(|key| !read.contains_key(*key)) {
        note(format!("{label}: key {key} removed"));
    }
    for (key, value) in sent {
        if skip.contains(&key.as_str()) {
            continue;
        }
        if let Some(other) = read.get(key) {
            scalar_change(&format!("{label}: {key}"), value, other, note);
        }
    }
}

fn scalar_change(label: &str, sent: &Value, read: &Value, note: &mut impl FnMut(String)) {
    if sent == read {
        return;
    }
    if kind(sent) != kind(read) && scalar_text(sent) == scalar_text(read) {
        note(format!(
            "{label}: {} -> {} (same text)",
            kind(sent),
            kind(read)
        ));
    } else {
        note(format!("{label}: value changed"));
    }
}

fn fields(label: &str, sent: &Value, read: &Value, note: &mut impl FnMut(String)) {
    let (sent, read) = (
        optional_array(&sent["fields"]),
        optional_array(&read["fields"]),
    );
    let names = |fields: &[Value]| {
        fields
            .iter()
            .map(|field| field["name"].as_str().unwrap_or("?").to_owned())
            .collect::<Vec<_>>()
    };
    let (sent_names, read_names) = (names(sent), names(read));
    if sent_names != read_names {
        let sent_set = sent_names.iter().collect::<BTreeSet<_>>();
        let read_set = read_names.iter().collect::<BTreeSet<_>>();
        if sent_set == read_set {
            note(format!("{label}: fields reordered"));
        }
        for name in read_set.difference(&sent_set) {
            note(format!("{label}: field {name} added"));
        }
        for name in sent_set.difference(&read_set) {
            note(format!("{label}: field {name} removed"));
        }
    }
    for sent_field in sent {
        let Some(read_field) = read
            .iter()
            .find(|field| field["name"] == sent_field["name"])
        else {
            continue;
        };
        let name = sent_field["name"].as_str().unwrap_or("?");
        scalar_change(
            &format!("{label}: field type"),
            &sent_field["type"],
            &read_field["type"],
            note,
        );
        let (sent_value, read_value) = (&sent_field["value"], &read_field["value"]);
        if sent_value != read_value {
            if scalar_text(sent_value) == scalar_text(read_value) {
                note(format!(
                    "{label}: field value {} -> {} (same text)",
                    kind(sent_value),
                    kind(read_value)
                ));
            } else {
                note(format!("{label}: field {name} value changed"));
            }
        }
    }
}

fn print_normalisation(scope: &str, categories: &BTreeMap<String, usize>) {
    if categories.is_empty() {
        println!("evidence {scope}: no normalisation");
    }
    for (category, count) in categories {
        println!("evidence {scope}: {category} x{count}");
    }
}

// ------------------------------------------------------------ fixtures

fn item_widget(name: &str, x: u32, y: u32, itemid: &str, extra: &[Value]) -> Value {
    let mut fields = vec![
        json!({"type": 4, "name": "itemid.0", "value": itemid}),
        json!({"type": 0, "name": "show.0", "value": 2}),
    ];
    fields.extend(extra.iter().cloned());
    json!({
        "type": "item", "name": name, "x": x, "y": y, "width": 12, "height": 3,
        "view_mode": 0, "fields": fields
    })
}

fn clock_widget(marker: &str) -> Value {
    json!({
        "type": "clock", "name": "Control clock", "x": 0, "y": 0, "width": 12, "height": 3,
        "view_mode": 0,
        "fields": [
            {"type": 0, "name": "clock_type", "value": 1},
            {"type": 1, "name": "zds.contract_marker", "value": marker}
        ]
    })
}

fn several_pages_fixture(pair: &HostPair) -> Value {
    let (a0, a1) = (&pair.items[0].old, &pair.items[1].old);
    json!([
        {
            "name": "Changed A",
            "display_period": 0,
            "widgets": [
                item_widget("Item A0", 0, 0, a0, &[]),
                {
                    "type": "svggraph", "name": "Resent graph A", "x": 12, "y": 0,
                    "width": 36, "height": 5, "view_mode": 0,
                    "fields": [
                        {"type": 0, "name": "ds.0.dataset_type", "value": 0},
                        {"type": 4, "name": "ds.0.itemids.0", "value": a1},
                        {"type": 1, "name": "ds.0.color.0", "value": "FF465C"},
                        {"type": 1, "name": "reference", "value": "SVGPA"}
                    ]
                }
            ]
        },
        {
            "name": "Control",
            "display_period": 60,
            "widgets": [
                clock_widget("several-pages-control"),
                {
                    "type": "problems", "name": "Control problems", "x": 12, "y": 0,
                    "width": 36, "height": 5, "view_mode": 0,
                    "fields": [{"type": 0, "name": "show_lines", "value": 12}]
                },
                {
                    "type": "gauge", "name": "Control gauge", "x": 48, "y": 0,
                    "width": 12, "height": 5, "view_mode": 0,
                    "fields": [
                        {"type": 4, "name": "itemid.0", "value": a0},
                        {"type": 1, "name": "max", "value": "100"}
                    ]
                }
            ]
        },
        {
            "name": "Changed B",
            "display_period": 0,
            "widgets": [
                item_widget("Item B changed", 0, 0, a1, &[]),
                item_widget("Item B untouched", 12, 0, a0, &[
                    json!({"type": 1, "name": "reference", "value": "ITMBU"})
                ]),
                {
                    "type": "svggraph", "name": "Resent pattern graph B", "x": 24, "y": 0,
                    "width": 36, "height": 5, "view_mode": 0,
                    "fields": [
                        {"type": 1, "name": "ds.0.hosts.0", "value": "*"},
                        {"type": 1, "name": "ds.0.items.0", "value": "*"},
                        {"type": 1, "name": "ds.0.color", "value": "00BFFF"}
                    ]
                }
            ]
        },
        {
            "name": "Control tail",
            "display_period": 0,
            "widgets": [item_widget("Tail item", 0, 0, a1, &[])]
        }
    ])
}

fn svggraph_fixture(pair: &HostPair) -> Value {
    let old = &pair.old.name;
    let mask = format!("{}*", old.chars().take(2).collect::<String>());
    let (a0, a1) = (&pair.items[0].old, &pair.items[1].old);
    json!([
        {
            "name": "Replace host in svggraph",
            "display_period": 0,
            "widgets": [
                {
                    "type": "svggraph", "name": "Explicit and pattern data sets", "x": 0, "y": 0,
                    "width": 48, "height": 6, "view_mode": 0,
                    "fields": [
                        {"type": 0, "name": "ds.0.dataset_type", "value": 0},
                        {"type": 4, "name": "ds.0.itemids.0", "value": a0},
                        {"type": 4, "name": "ds.0.itemids.1", "value": a1},
                        {"type": 1, "name": "ds.0.color.0", "value": "FF465C"},
                        {"type": 1, "name": "ds.0.color.1", "value": "FFD54F"},
                        {"type": 1, "name": "ds.1.hosts.0", "value": old},
                        {"type": 1, "name": "ds.1.hosts.1", "value": mask},
                        {"type": 1, "name": "ds.1.items.0", "value": pair.items[0].name},
                        {"type": 1, "name": "ds.1.items.1", "value": "CPU*"},
                        {"type": 1, "name": "ds.1.color", "value": "00BFFF"},
                        {"type": 1, "name": "ds.2.hosts.0", "value": old.to_uppercase()},
                        {"type": 1, "name": "ds.2.items.0", "value": "*"},
                        {"type": 1, "name": "ds.2.color", "value": "0EC9AC"},
                        {"type": 0, "name": "ds.2.width", "value": 3},
                        {"type": 0, "name": "ds.3.dataset_type", "value": 0},
                        {"type": 1, "name": "ds.3.itemids.0._reference", "value": "KLMNO._itemid"},
                        {"type": 1, "name": "ds.3.color.0", "value": "B0AF07"},
                        {"type": 1, "name": "or.0.hosts.0", "value": old},
                        {"type": 1, "name": "or.0.items.0", "value": "*"},
                        {"type": 1, "name": "or.0.color", "value": "E91E63"},
                        {"type": 1, "name": "problemhosts.0", "value": old},
                        {"type": 0, "name": "show_problems", "value": 1},
                        {"type": 1, "name": "time_period.from", "value": "now-3h"},
                        {"type": 1, "name": "time_period.to", "value": "now"},
                        {"type": 0, "name": "legend", "value": 0},
                        {"type": 1, "name": "reference", "value": "SVGPB"}
                    ]
                },
                {
                    "type": "item", "name": "Referenced item", "x": 48, "y": 0,
                    "width": 12, "height": 3, "view_mode": 0,
                    "fields": [
                        {"type": 4, "name": "itemid.0", "value": a0},
                        {"type": 1, "name": "reference", "value": "KLMNO"}
                    ]
                },
                {
                    "type": "svggraph", "name": "Mask only, stays on source", "x": 0, "y": 6,
                    "width": 48, "height": 5, "view_mode": 0,
                    "fields": [
                        {"type": 1, "name": "ds.0.hosts.0", "value": mask},
                        {"type": 1, "name": "ds.0.items.0", "value": "*"},
                        {"type": 1, "name": "ds.0.color", "value": "FF465C"},
                        {"type": 1, "name": "reference", "value": "SVGPC"}
                    ]
                }
            ]
        },
        {
            "name": "Control",
            "display_period": 0,
            "widgets": [clock_widget("svggraph-control")]
        }
    ])
}

fn classic_graph_fixture(pair: &HostPair, graphid: &str) -> Value {
    let (a0, a1) = (&pair.items[0].old, &pair.items[1].old);
    json!([
        {
            "name": "Replace graph",
            "display_period": 0,
            "widgets": [
                {
                    "type": "graph", "name": "Templated graph", "x": 0, "y": 0,
                    "width": 36, "height": 5, "view_mode": 0,
                    "fields": [
                        {"type": 0, "name": "source_type", "value": 0},
                        {"type": 6, "name": "graphid.0", "value": graphid},
                        {"type": 0, "name": "show_legend", "value": 0},
                        {"type": 1, "name": "time_period.from", "value": "now-6h"},
                        {"type": 1, "name": "time_period.to", "value": "now"},
                        {"type": 1, "name": "zds.contract_marker", "value": "graph-preserve"}
                    ]
                },
                {
                    "type": "graph", "name": "Simple graph", "x": 36, "y": 0,
                    "width": 24, "height": 5, "view_mode": 0,
                    "fields": [
                        {"type": 0, "name": "source_type", "value": 1},
                        {"type": 4, "name": "itemid.0", "value": a0}
                    ]
                },
                item_widget("Neighbour item", 60, 0, a1, &[])
            ]
        },
        {
            "name": "Control",
            "display_period": 0,
            "widgets": [clock_widget("classic-graph-control")]
        }
    ])
}

// ------------------------------------------------------------- contracts

#[tokio::test]
#[ignore = "writes only to laboratory dashboards created by this test"]
async fn update_several_pages_keeps_untouched_page_and_order() {
    let mut lab = open_lab().await;
    let pair = host_pair(&mut lab.api).await;
    let fixture = several_pages_fixture(&pair);
    let (baseline, created) = reset_dashboard(&mut lab, SEVERAL_PAGES_NAME, &fixture).await;
    print_normalisation("several-pages create/reset", &created);
    let (page_ids, widget_ids) = ids(baseline.raw_api_document());
    assert_eq!(page_ids.len(), 4);

    let changes = [
        Change::new(0, 0, "itemid.0", &pair.items[0].new),
        Change::new(2, 0, "itemid.0", &pair.items[1].new),
    ];
    let (expected, replacements) = apply_changes(&baseline, &changes);
    let requirements = write_requirements(&baseline, &replacements)
        .unwrap_or_else(|error| panic!("adapter rejected the replacements: {error}"));
    assert!(
        requirements
            .contracts
            .contains(&WriteContract::UpdateSeveralPages)
    );
    assert!(
        requirements
            .contracts
            .contains(&WriteContract::ItemWidgetItemid)
    );
    assert!(requirements.unproven_widgets.is_empty());
    assert_eq!(
        requirements.changed_pages,
        BTreeSet::from([0, 2]),
        "adapter must send pages 0 and 2 in full"
    );

    let control_before = baseline.raw_api_document()["pages"][1].clone();
    let tail_before = baseline.raw_api_document()["pages"][3].clone();
    let (after, order) = update_and_verify(
        &mut lab,
        SEVERAL_PAGES_NAME,
        &baseline,
        &expected,
        &changes,
        &replacements,
    )
    .await;
    print_order("update_several_pages", &order);
    print_fingerprint("update_several_pages", &after, &expected);
    let raw = after.raw_api_document();
    assert_eq!(raw["pages"][1], control_before, "untouched page changed");
    assert_eq!(raw["pages"][3], tail_before, "untouched tail page changed");
    assert_eq!(
        ids(raw),
        (page_ids, widget_ids),
        "page or widget order changed"
    );
    assert_ne!(after.fingerprint(), baseline.fingerprint());

    println!(
        "evidence update_several_pages: pages=4, changed_pages=2, changed_fields=2, \
         untouched_pages_byte_identical=2, page_and_widget_ids_and_order_preserved=true, resent_types=item,svggraph, \
         untouched_page_types=clock,problems,gauge,item"
    );
    lab.close().await;
}

#[tokio::test]
#[ignore = "writes only to laboratory dashboards created by this test"]
async fn svggraph_replaces_itemids_and_exact_host_patterns_only() {
    let mut lab = open_lab().await;
    let pair = host_pair(&mut lab.api).await;
    let fixture = svggraph_fixture(&pair);
    let (baseline, created) = reset_dashboard(&mut lab, SVGGRAPH_NAME, &fixture).await;
    print_normalisation("svggraph create/reset", &created);
    let (page_ids, widget_ids) = ids(baseline.raw_api_document());

    let changes = [
        Change::new(0, 0, "ds.0.itemids.0", &pair.items[0].new),
        Change::new(0, 0, "ds.0.itemids.1", &pair.items[1].new),
        Change::new(0, 0, "ds.1.hosts.0", &pair.new.name),
        Change::new(0, 0, "ds.2.hosts.0", &pair.new.name),
    ];
    let (expected, replacements) = apply_changes(&baseline, &changes);
    let requirements = write_requirements(&baseline, &replacements)
        .unwrap_or_else(|error| panic!("adapter rejected the replacements: {error}"));
    assert_eq!(
        requirements
            .kinds
            .get(&WriteContract::SvggraphDatasetItemids),
        Some(&2)
    );
    assert_eq!(
        requirements.kinds.get(&WriteContract::SvggraphDatasetHosts),
        Some(&2)
    );
    assert!(
        !requirements
            .contracts
            .contains(&WriteContract::UpdateSeveralPages)
    );
    assert!(requirements.unproven_widgets.is_empty());

    // A host mask is never a replaceable host pattern for the adapter.
    let (_, mask_replacement) = apply_changes(
        &baseline,
        &[Change::new(0, 0, "ds.1.hosts.1", &pair.new.name)],
    );
    let mask = write_requirements(&baseline, &mask_replacement)
        .err()
        .unwrap_or_else(|| panic!("adapter accepted a host mask replacement"));
    assert_eq!(mask.code(), WriteRejectionCode::FieldKindUnsupported);

    let (after, order) = update_and_verify(
        &mut lab,
        SVGGRAPH_NAME,
        &baseline,
        &expected,
        &changes,
        &replacements,
    )
    .await;
    print_order("svggraph", &order);
    print_fingerprint("svggraph", &after, &expected);
    assert_eq!(ids(after.raw_api_document()), (page_ids, widget_ids));
    let fields = array(&after.raw_api_document()["pages"][0]["widgets"][0]["fields"]);
    let types = fields
        .iter()
        .map(|field| kind(&field["type"]))
        .collect::<BTreeSet<_>>();
    println!(
        "evidence svggraph: template_hosts={}, fields={}, replaced=itemids x2 + exact host \
         patterns x2 (one upper-case), kept=host mask, item patterns, or.*, problemhosts, \
         _reference, time_period, legend, reference; field type JSON kinds after read={types:?}",
        pair.template_hosts,
        fields.len()
    );
    lab.close().await;
}

#[tokio::test]
#[ignore = "writes only to laboratory dashboards created by this test"]
async fn classic_graph_replaces_graphid_with_same_template_graph() {
    let mut lab = open_lab().await;
    let pair = host_pair(&mut lab.api).await;
    let (old_graph, new_graph) = pair
        .graph
        .clone()
        .unwrap_or_else(|| panic!("no templated graph pair"));
    let fixture = classic_graph_fixture(&pair, &old_graph);
    let (baseline, created) = reset_dashboard(&mut lab, CLASSIC_GRAPH_NAME, &fixture).await;
    print_normalisation("classic graph create/reset", &created);
    let (page_ids, widget_ids) = ids(baseline.raw_api_document());

    let changes = [Change::new(0, 0, "graphid.0", &new_graph)];
    let (expected, replacements) = apply_changes(&baseline, &changes);
    let requirements = write_requirements(&baseline, &replacements)
        .unwrap_or_else(|error| panic!("adapter rejected the replacements: {error}"));
    assert_eq!(
        requirements.kinds.get(&WriteContract::ClassicGraphGraphid),
        Some(&1)
    );
    // `graph` (both source types) and `item` are resend-proven (LAB-02).
    assert!(
        requirements.unproven_widgets.is_empty(),
        "unproven resend: {:?}",
        requirements.unproven_widgets
    );

    let (after, order) = update_and_verify(
        &mut lab,
        CLASSIC_GRAPH_NAME,
        &baseline,
        &expected,
        &changes,
        &replacements,
    )
    .await;
    print_order("classic graph", &order);
    print_fingerprint("classic graph", &after, &expected);
    assert_eq!(ids(after.raw_api_document()), (page_ids, widget_ids));
    println!(
        "evidence classic graph: graphid replaced by the same-name graph of the same parent \
         template graph, simple graph (source_type=1) and item neighbours resent unchanged"
    );
    lab.close().await;
}

#[tokio::test]
#[ignore = "creates laboratory dashboards, one contract user and its group"]
async fn dashboard_create_defaults_owner_uniqueness_and_copy() {
    let mut lab = open_lab().await;
    let pair = host_pair(&mut lab.api).await;
    let admin_userid = caller_userid(&mut lab.api, &lab.username).await;

    // 1. Defaults: only a name and one page with one minimal widget.
    delete_owned(&mut lab.api, CREATE_DEFAULTS_NAME, &admin_userid).await;
    let minimal_pages = json!([{
        "widgets": [{"type": "clock", "x": 0, "y": 0, "width": 12, "height": 3}]
    }]);
    let created = lab
        .api
        .call(
            "dashboard.create",
            json!({"name": CREATE_DEFAULTS_NAME, "pages": minimal_pages}),
        )
        .await;
    assert_eq!(array(&created["dashboardids"]).len(), 1);
    let defaults = read_dashboard(&lab.reader, CREATE_DEFAULTS_NAME).await;
    let raw = defaults.raw_api_document();
    assert_eq!(raw["private"], "1", "a new dashboard must be private");
    assert_eq!(
        raw["userid"],
        admin_userid.as_str(),
        "owner must be the caller"
    );
    assert_eq!(raw["users"], json!([]));
    assert_eq!(raw["userGroups"], json!([]));
    let dashboard_keys = raw
        .as_object()
        .map(|object| object.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    println!(
        "evidence create defaults: dashboard keys={dashboard_keys:?}, display_period={}, \
         auto_start={}, private={}, owner_is_caller=true, users=[], userGroups=[]",
        raw["display_period"], raw["auto_start"], raw["private"]
    );
    print_normalisation(
        "create defaults",
        &normalisation(&minimal_pages, &raw["pages"]),
    );

    // 2. Name uniqueness: exact repeat, then a case variant.
    let repeat = lab
        .api
        .try_call(
            "dashboard.create",
            json!({"name": CREATE_DEFAULTS_NAME, "pages": [{"widgets": []}]}),
        )
        .await;
    let repeat = repeat
        .err()
        .unwrap_or_else(|| panic!("a repeated dashboard name was accepted"));
    assert!(repeat.data.contains("already exists"));
    println!(
        "evidence create uniqueness: repeat rejected code={} message={:?} data={:?}",
        repeat.code, repeat.message, repeat.data
    );
    let upper = CREATE_DEFAULTS_NAME.to_uppercase();
    let upper_name = format!("{NAME_PREFIX}{}", &upper[NAME_PREFIX.len()..]);
    delete_owned(&mut lab.api, &upper_name, &admin_userid).await;
    match lab
        .api
        .try_call(
            "dashboard.create",
            json!({"name": upper_name, "pages": [{"widgets": []}]}),
        )
        .await
    {
        Ok(_) => println!("evidence create uniqueness: case variant accepted (case-sensitive)"),
        Err(error) => println!(
            "evidence create uniqueness: case variant rejected code={} data={:?}",
            error.code, error.data
        ),
    }
    // The adapter's name check (a hint) compares names exactly, with case.
    for exact in [CREATE_DEFAULTS_NAME, upper_name.as_str()] {
        let named = lab
            .reader
            .dashboards_named(CONNECTION_ID, exact)
            .await
            .unwrap_or_else(|error| panic!("adapter name check failed: {error}"));
        assert!(!named.truncated);
        assert_eq!(named.dashboards.len(), 1, "exactly the same-case name");
        assert_eq!(named.dashboards[0].raw_api_document()["name"], exact);
    }
    println!("evidence create uniqueness: adapter name check is case-sensitive");

    // 3. Copy of read widgets (a visible built-in dashboard) and of widget
    //    types the adapter does not resend, into a new dashboard.
    let snapshots = lab
        .reader
        .dashboard_snapshots(CONNECTION_ID)
        .await
        .unwrap_or_else(|error| panic!("cannot read laboratory dashboards: {error}"));
    let source = snapshots
        .iter()
        .filter(|snapshot| {
            !snapshot.raw_api_document()["name"]
                .as_str()
                .unwrap_or_default()
                .starts_with("ZDashstitch")
        })
        .max_by_key(|snapshot| widget_types(snapshot.raw_api_document()).len())
        .unwrap_or_else(|| panic!("laboratory needs a readable non-test dashboard"));
    let copied_pages = array(&source.raw_api_document()["pages"])
        .iter()
        .map(copy_page)
        .collect::<Vec<_>>();
    let (a0, a1) = (&pair.items[0].old, &pair.items[1].old);
    let synthetic = json!({
        "name": "Not resend-proven types",
        "display_period": 0,
        "widgets": [
            {"type": "gauge", "name": "Gauge", "x": 0, "y": 0, "width": 12, "height": 5,
             "view_mode": 0, "fields": [
                {"type": 4, "name": "itemid.0", "value": a0},
                {"type": 1, "name": "min", "value": "0"},
                {"type": 1, "name": "max", "value": "100"}]},
            {"type": "piechart", "name": "Pie", "x": 12, "y": 0, "width": 24, "height": 5,
             "view_mode": 0, "fields": [
                {"type": 0, "name": "ds.0.dataset_type", "value": 0},
                {"type": 4, "name": "ds.0.itemids.0", "value": a0},
                {"type": 4, "name": "ds.0.itemids.1", "value": a1},
                {"type": 1, "name": "ds.0.color.0", "value": "FF465C"},
                {"type": 1, "name": "ds.0.color.1", "value": "FFD54F"}]},
            {"type": "itemhistory", "name": "History", "x": 36, "y": 0, "width": 24,
             "height": 5, "view_mode": 0, "fields": [
                {"type": 1, "name": "columns.0.name", "value": "Value"},
                {"type": 4, "name": "columns.0.itemid", "value": a1}]},
            {"type": "tophosts", "name": "Top hosts", "x": 0, "y": 5, "width": 36,
             "height": 5, "view_mode": 0, "fields": [
                {"type": 1, "name": "columns.0.name", "value": "Name"},
                {"type": 0, "name": "columns.0.data", "value": 2},
                {"type": 1, "name": "columns.1.name", "value": "Load"},
                {"type": 0, "name": "columns.1.data", "value": 1},
                {"type": 1, "name": "columns.1.item", "value": pair.items[0].name},
                {"type": 0, "name": "column", "value": 1}]},
            {"type": "url", "name": "URL", "x": 36, "y": 5, "width": 24, "height": 5,
             "view_mode": 0, "fields": [
                {"type": 1, "name": "url", "value": "https://example.invalid/"}]},
            {"type": "honeycomb", "name": "Honeycomb", "x": 60, "y": 0, "width": 12,
             "height": 5, "view_mode": 0, "fields": [
                {"type": 1, "name": "items.0", "value": "CPU*"}]}
        ]
    });
    let mut copy_pages = copied_pages.clone();
    copy_pages.push(synthetic.clone());
    let copy_pages = Value::Array(copy_pages);
    delete_owned(&mut lab.api, CREATE_COPY_NAME, &admin_userid).await;
    lab.api
        .call(
            "dashboard.create",
            json!({"name": CREATE_COPY_NAME, "pages": copy_pages}),
        )
        .await;
    let copy = read_dashboard(&lab.reader, CREATE_COPY_NAME).await;
    let copy_raw = copy.raw_api_document();
    // The production gate on this exact version: private create, read
    // widget copy and the reconciliation of an unknown outcome are proven
    // (the last by `create_unknown_outcome_is_reconciled_by_reads_only`), so
    // a copy of registered widget types verifies; it is never sent here.
    // Widgets the registry does not know stay refused (step 4).
    let capabilities = lab.writer.capabilities().dashboard_create_contracts.clone();
    assert!(capabilities.is_verified(CreateContract::CreatePrivateDashboard));
    assert!(capabilities.is_verified(CreateContract::CopyReadWidgets));
    assert!(capabilities.is_verified(CreateContract::ReconcileCreateOutcome));
    assert!(!capabilities.is_verified(CreateContract::CopyUnknownWidgets));
    let gate = verify_dashboard_create(
        source,
        &lab.writer.publication_context(),
        "ZDashstitch Contract Lab — never created",
    );
    println!(
        "evidence create gate: copy of registered widget types verified={} (not sent)",
        gate.is_ok()
    );
    assert!(
        gate.is_ok(),
        "the adapter refused a copy of registered types"
    );
    let copied_count = copied_pages.len();
    let copied_read = Value::Array(array(&copy_raw["pages"])[..copied_count].to_vec());
    let synthetic_read = Value::Array(array(&copy_raw["pages"])[copied_count..].to_vec());
    let mut copy_differences = Vec::new();
    diff(
        "copy",
        &strip_ids(&Value::Array(copied_pages.clone())),
        &strip_ids(&copied_read),
        &mut copy_differences,
    );
    println!(
        "evidence create copy: source widget types={:?}, pages={copied_count}, \
         read widgets identical except IDs={}, differences={copy_differences:?}",
        widget_types(source.raw_api_document()),
        copy_differences.is_empty()
    );
    print_normalisation(
        "create copy (not resend-proven types)",
        &normalisation(&Value::Array(vec![synthetic]), &synthetic_read),
    );

    // 4. A widget type no 7.4 module registers.
    delete_owned(&mut lab.api, UNREGISTERED_TYPE_NAME, &admin_userid).await;
    match lab
        .api
        .try_call(
            "dashboard.create",
            json!({"name": UNREGISTERED_TYPE_NAME, "pages": [{"widgets": [{
                "type": "zdsunregistered", "x": 0, "y": 0, "width": 12, "height": 3,
                "fields": [{"type": 1, "name": "zds.contract_marker", "value": "unknown"}]
            }]}]}),
        )
        .await
    {
        Ok(_) => {
            let unknown = read_dashboard(&lab.reader, UNREGISTERED_TYPE_NAME).await;
            println!(
                "evidence create unregistered type: accepted, read widget={:?}",
                strip_ids(&unknown.raw_api_document()["pages"][0]["widgets"][0])
            );
            let refused = verify_dashboard_create(
                &unknown,
                &lab.writer.publication_context(),
                "ZDashstitch Contract Lab — never created",
            )
            .err()
            .unwrap_or_else(|| panic!("the adapter verified a copy of an unregistered type"));
            assert_eq!(refused.code(), CreateRejectionCode::WidgetCopyUnproven);
            println!(
                "evidence create gate: unregistered type refused with {}",
                refused.code().as_str()
            );
        }
        Err(error) => println!(
            "evidence create unregistered type: rejected code={} message={:?} data={:?}",
            error.code, error.message, error.data
        ),
    }

    // 5. The name of another user's private dashboard, invisible to the caller.
    let contract_userid = ensure_contract_user(&mut lab).await;
    let hidden = lab
        .reader
        .dashboard_snapshots(CONNECTION_ID)
        .await
        .ok()
        .and_then(|snapshots| find_dashboard(&snapshots, HIDDEN_PRIVATE_NAME));
    if hidden.is_none() {
        lab.api
            .call(
                "dashboard.create",
                json!({"name": HIDDEN_PRIVATE_NAME, "private": 1,
                       "pages": [{"widgets": [clock_widget("hidden-private")]}]}),
            )
            .await;
    }
    let hidden = read_dashboard(&lab.reader, HIDDEN_PRIVATE_NAME).await;
    let hidden_raw = hidden.raw_api_document();
    assert_eq!(hidden_raw["private"], "1");
    assert_eq!(hidden_raw["userid"], admin_userid.as_str());
    assert_eq!(hidden_raw["users"], json!([]));
    assert_eq!(hidden_raw["userGroups"], json!([]));

    let contract_password = generated_password();
    lab.api
        .call(
            "user.update",
            json!({"userid": contract_userid, "passwd": contract_password.expose_secret()}),
        )
        .await;
    let mut user = LabApi::login(&lab.endpoint, CONTRACT_USER, &contract_password).await;
    let visible = user
        .call(
            "dashboard.get",
            json!({"output": ["dashboardid"], "filter": {"name": HIDDEN_PRIVATE_NAME}}),
        )
        .await;
    let searched = user
        .call(
            "dashboard.get",
            json!({"output": ["dashboardid"], "search": {"name": "hidden private"}}),
        )
        .await;
    assert_eq!(visible, json!([]));
    assert_eq!(searched, json!([]));
    let invisible = user
        .try_call(
            "dashboard.create",
            json!({"name": HIDDEN_PRIVATE_NAME, "pages": [{"widgets": []}]}),
        )
        .await
        .err()
        .unwrap_or_else(|| panic!("the name of an invisible dashboard was accepted"));
    assert!(invisible.data.contains("already exists"));
    println!(
        "evidence create invisible name: dashboard.get filter=0 search=0, create rejected \
         code={} message={:?} data={:?}",
        invisible.code, invisible.message, invisible.data
    );

    delete_owned(&mut user, CONTRACT_USER_DASHBOARD_NAME, &contract_userid).await;
    user.call(
        "dashboard.create",
        json!({"name": CONTRACT_USER_DASHBOARD_NAME,
               "pages": [{"widgets": [clock_widget("contract-user")]}]}),
    )
    .await;
    let own = user
        .call(
            "dashboard.get",
            json!({"output": "extend", "selectUsers": "extend", "selectUserGroups": "extend",
                   "filter": {"name": CONTRACT_USER_DASHBOARD_NAME}}),
        )
        .await;
    let own = array(&own);
    assert_eq!(own.len(), 1);
    assert_eq!(own[0]["userid"], contract_userid.as_str());
    assert_eq!(own[0]["private"], "1");
    println!(
        "evidence create by User role: owner_is_caller=true, private={}, display_period={}, \
         auto_start={}, users={}, userGroups={}",
        own[0]["private"],
        own[0]["display_period"],
        own[0]["auto_start"],
        array(&own[0]["users"]).len(),
        array(&own[0]["userGroups"]).len()
    );
    user.logout().await;
    lab.close().await;
}

#[tokio::test]
#[ignore = "creates one laboratory dashboard to probe duplicate widget field names"]
async fn duplicate_widget_field_names_are_probed() {
    let mut lab = open_lab().await;
    let admin_userid = caller_userid(&mut lab.api, &lab.username).await;
    for (case, fields) in [
        (
            "same name and type",
            json!([
                {"type": 1, "name": "zds.contract_marker", "value": "a"},
                {"type": 1, "name": "zds.contract_marker", "value": "b"}
            ]),
        ),
        (
            "same name, other type",
            json!([
                {"type": 1, "name": "zds.contract_marker", "value": "a"},
                {"type": 0, "name": "zds.contract_marker", "value": 1}
            ]),
        ),
    ] {
        delete_owned(&mut lab.api, DUPLICATE_FIELDS_NAME, &admin_userid).await;
        let result = lab
            .api
            .try_call(
                "dashboard.create",
                json!({"name": DUPLICATE_FIELDS_NAME, "pages": [{"widgets": [{
                    "type": "clock", "x": 0, "y": 0, "width": 12, "height": 3,
                    "fields": fields
                }]}]}),
            )
            .await;
        match result {
            Ok(_) => {
                let read = read_dashboard(&lab.reader, DUPLICATE_FIELDS_NAME).await;
                let names = field_names(&read.raw_api_document()["pages"][0]["widgets"][0]);
                println!(
                    "evidence duplicate field names ({case}): accepted, read field count={}",
                    names.len()
                );
            }
            Err(error) => println!(
                "evidence duplicate field names ({case}): rejected code={} message={:?} data={:?}",
                error.code, error.message, error.data
            ),
        }
    }
    lab.close().await;
}

/// P2-06D: a `dashboard.create` whose answer is lost is reconciled by the
/// adapter's read-only path (`reconcile_dashboard_create`, which the
/// application's `reconcile_save_as_copy` calls), by exact name and content
/// fingerprint: before the request it finds nothing, after it exactly one
/// confirmed candidate, also from a new session and by the reported ID; a
/// changed candidate is a mismatch, never confirmed.
#[tokio::test]
#[ignore = "creates two laboratory dashboards and drops one create answer"]
async fn create_unknown_outcome_is_reconciled_by_reads_only() {
    let mut lab = open_lab().await;
    let pair = host_pair(&mut lab.api).await;
    let admin_userid = caller_userid(&mut lab.api, &lab.username).await;
    // Local: a test-owned dashboard of several pages of registered types.
    let (source, _) = reset_dashboard(
        &mut lab,
        CREATE_RECONCILE_SOURCE_NAME,
        &several_pages_fixture(&pair),
    )
    .await;
    let capabilities = lab.writer.capabilities().dashboard_create_contracts.clone();
    assert!(capabilities.is_verified(CreateContract::ReconcileCreateOutcome));
    let create = verify_dashboard_create(
        &source,
        &lab.writer.publication_context(),
        CREATE_RECONCILE_NAME,
    )
    .unwrap_or_else(|rejection| {
        panic!(
            "adapter refused the create: {} {:?}",
            rejection.code().as_str(),
            rejection.params()
        )
    });
    let expectation = lab.writer.create_expectation(&create);
    assert_eq!(expectation.owner_userid, admin_userid);
    assert_eq!(expectation.name, CREATE_RECONCILE_NAME);

    // 1. The request was never sent: nothing is found.
    delete_owned(&mut lab.api, CREATE_RECONCILE_NAME, &admin_userid).await;
    let before = lab
        .reader
        .reconcile_dashboard_create(&expectation, None)
        .await;
    assert_eq!(before, CreateReconciliation::NotFound);

    // 2. Exactly one create in the adapter's payload form (no IDs, private,
    //    empty sharing, every page in order), its answer dropped.
    let raw = source.raw_api_document();
    let payload = json!({
        "name": CREATE_RECONCILE_NAME,
        "display_period": raw["display_period"],
        "auto_start": raw["auto_start"],
        "private": 1,
        "users": [],
        "userGroups": [],
        "pages": array(&raw["pages"]).iter().map(copy_page).collect::<Vec<_>>()
    });
    lab.api
        .dispatch_without_reading_result("dashboard.create", payload)
        .await;

    // 3. Read-only reconciliation finds exactly one confirmed candidate.
    let reconciled = lab
        .reader
        .reconcile_dashboard_create(&expectation, None)
        .await;
    let CreateReconciliation::Confirmed {
        dashboardid,
        fingerprint,
    } = reconciled.clone()
    else {
        panic!("reconciliation did not confirm: {}", reconciled.code());
    };
    let named = lab
        .api
        .call(
            "dashboard.get",
            json!({"output": ["dashboardid"], "filter": {"name": CREATE_RECONCILE_NAME}}),
        )
        .await;
    assert_eq!(array(&named).len(), 1, "exactly one dashboard was created");
    assert_eq!(text(&array(&named)[0], "dashboardid"), dashboardid);
    let created = read_dashboard(&lab.reader, CREATE_RECONCILE_NAME).await;
    assert_eq!(created.fingerprint(), fingerprint);

    // 4. After a restart (new session) and by the reported ID: the same.
    let fresh = adapter_client(
        &lab.endpoint,
        &lab.username,
        &SecretString::from(required("ZDASHSTITCH_LAB_PASSWORD")),
    )
    .await;
    assert_eq!(
        fresh.reconcile_dashboard_create(&expectation, None).await,
        reconciled
    );
    assert_eq!(
        fresh
            .reconcile_dashboard_create(&expectation, Some(&dashboardid))
            .await,
        reconciled
    );

    // 5. A candidate with other content is never confirmed.
    let pageid = text(&created.raw_api_document()["pages"][0], "dashboard_pageid");
    let page_name = text(&created.raw_api_document()["pages"][0], "name");
    let pages = &created.raw_api_document()["pages"];
    lab.api
        .call(
            "dashboard.update",
            json!({"dashboardid": dashboardid,
                   "pages": rename_page(pages, &pageid, "Changed after create")}),
        )
        .await;
    let changed = fresh.reconcile_dashboard_create(&expectation, None).await;
    assert_eq!(
        changed,
        CreateReconciliation::Mismatch {
            verdict: ReadBackVerdict::ContentDiffers
        }
    );
    lab.api
        .call(
            "dashboard.update",
            json!({"dashboardid": dashboardid, "pages": rename_page(pages, &pageid, &page_name)}),
        )
        .await;
    let restored = fresh.reconcile_dashboard_create(&expectation, None).await;
    assert!(restored.is_confirmed(), "{}", restored.code());
    println!(
        "evidence create unknown outcome: not sent → {}, sent with the answer dropped → {}, \
         visible dashboards with the name=1, new session → {}, by reported id → {}, \
         content changed → {}, restored → {}",
        before.code(),
        reconciled.code(),
        reconciled.code(),
        reconciled.code(),
        changed.code(),
        restored.code()
    );
    fresh
        .logout()
        .await
        .unwrap_or_else(|error| panic!("fresh read client logout failed: {error}"));
    lab.close().await;
}

/// `dashboard.update` pages that rename the page `pageid` and leave every
/// other page as stored (sent by ID only).
fn rename_page(pages: &Value, pageid: &str, name: &str) -> Vec<Value> {
    array(pages)
        .iter()
        .map(|page| {
            let id = text(page, "dashboard_pageid");
            if id == pageid {
                json!({"dashboard_pageid": id, "name": name})
            } else {
                json!({"dashboard_pageid": id})
            }
        })
        .collect()
}

/// `users`/`userGroups` read order: Zabbix 7.4.16 returns both lists in no
/// defined order (like widget fields), so the canonical fingerprint compares
/// them as sets. This test shares one
/// dashboard with two test users and two test groups, changes permissions
/// and the sent order, and records the read order; it requires the
/// canonical fingerprint to follow the set, never the order.
#[tokio::test]
#[ignore = "shares one laboratory dashboard with two test users and two test groups"]
async fn sharing_lists_are_read_as_sets() {
    let mut lab = open_lab().await;
    let admin_userid = caller_userid(&mut lab.api, &lab.username).await;
    let (first_user, first_group) = ensure_test_user(&mut lab, CONTRACT_USER, CONTRACT_GROUP).await;
    let (second_user, second_group) = ensure_test_user(&mut lab, SECOND_USER, SECOND_GROUP).await;
    let by_id = |left: String, right: String| {
        let key = |id: &String| (id.len(), id.clone());
        if key(&left) < key(&right) {
            (left, right)
        } else {
            (right, left)
        }
    };
    // `low` has the lower ID, `high` the higher one; evidence uses only
    // these labels.
    let (low_user, high_user) = by_id(first_user, second_user);
    let (low_group, high_group) = by_id(first_group, second_group);
    let label = |id: &str, low: &str| if id == low { "low" } else { "high" };
    let users_order = |raw: &Value| {
        array(&raw["users"])
            .iter()
            .map(|user| {
                format!(
                    "{}:{}",
                    label(&text(user, "userid"), &low_user),
                    text(user, "permission")
                )
            })
            .collect::<Vec<_>>()
    };
    let groups_order = |raw: &Value| {
        array(&raw["userGroups"])
            .iter()
            .map(|group| {
                format!(
                    "{}:{}",
                    label(&text(group, "usrgrpid"), &low_group),
                    text(group, "permission")
                )
            })
            .collect::<Vec<_>>()
    };
    let entries = |key: &str, ids: [(&String, u32); 2]| {
        Value::Array(
            ids.iter()
                .map(|(id, permission)| {
                    let mut entry = serde_json::Map::new();
                    entry.insert(key.to_owned(), json!(id));
                    entry.insert("permission".to_owned(), json!(permission));
                    Value::Object(entry)
                })
                .collect(),
        )
    };

    // 1. Created with the higher IDs first.
    delete_owned(&mut lab.api, SHARING_ORDER_NAME, &admin_userid).await;
    lab.api
        .call(
            "dashboard.create",
            json!({
                "name": SHARING_ORDER_NAME, "private": 1,
                "users": entries("userid", [(&high_user, 2), (&low_user, 3)]),
                "userGroups": entries("usrgrpid", [(&high_group, 2), (&low_group, 3)]),
                "pages": [{"widgets": [clock_widget("sharing-order")]}]
            }),
        )
        .await;
    let created = read_dashboard(&lab.reader, SHARING_ORDER_NAME).await;
    let dashboardid = created.dashboard_id().to_owned();
    let created_again = read_dashboard(&lab.reader, SHARING_ORDER_NAME).await;
    assert_eq!(created.raw_api_document(), created_again.raw_api_document());
    println!(
        "evidence sharing order: sent users=[high:2, low:3] groups=[high:2, low:3]; read users={:?} \
         groups={:?}; second read identical",
        users_order(created.raw_api_document()),
        groups_order(created.raw_api_document())
    );

    // 2. Permission of the first-sent entries changed, same order sent.
    lab.api
        .call(
            "dashboard.update",
            json!({
                "dashboardid": dashboardid,
                "users": entries("userid", [(&high_user, 3), (&low_user, 3)]),
                "userGroups": entries("usrgrpid", [(&high_group, 3), (&low_group, 3)])
            }),
        )
        .await;
    let permission = read_dashboard(&lab.reader, SHARING_ORDER_NAME).await;
    println!(
        "evidence sharing order: after permission change read users={:?} groups={:?}",
        users_order(permission.raw_api_document()),
        groups_order(permission.raw_api_document())
    );

    // 3. The same set sent in the other order: the server state does not
    //    change, and the canonical fingerprint must not either.
    lab.api
        .call(
            "dashboard.update",
            json!({
                "dashboardid": dashboardid,
                "users": entries("userid", [(&low_user, 3), (&high_user, 3)]),
                "userGroups": entries("usrgrpid", [(&low_group, 3), (&high_group, 3)])
            }),
        )
        .await;
    let reordered = read_dashboard(&lab.reader, SHARING_ORDER_NAME).await;
    println!(
        "evidence sharing order: after the same set in the other order read users={:?} \
         groups={:?}; raw identical to previous read={}",
        users_order(reordered.raw_api_document()),
        groups_order(reordered.raw_api_document()),
        reordered.raw_api_document() == permission.raw_api_document()
    );
    assert_eq!(reordered.fingerprint(), permission.fingerprint());

    // The canonical form compares the lists as sets: the expected document
    // with the lists in the order sent equals what was read.
    let mut expected = permission.raw_api_document().clone();
    expected["users"] = json!([
        {"userid": low_user, "permission": "3"},
        {"userid": high_user, "permission": "3"}
    ]);
    expected["userGroups"] = json!([
        {"usrgrpid": low_group, "permission": "3"},
        {"usrgrpid": high_group, "permission": "3"}
    ]);
    let mut expected_reversed = expected.clone();
    for list in ["users", "userGroups"] {
        if let Some(items) = expected_reversed[list].as_array_mut() {
            items.reverse();
        }
    }
    for document in [expected, expected_reversed] {
        let snapshot = VersionedDashboardSnapshot::from_adapter_raw(
            reordered.connection_id().to_owned(),
            reordered.server_version().clone(),
            document,
        )
        .unwrap_or_else(|error| panic!("expected document is not a snapshot: {error}"));
        assert_eq!(snapshot.fingerprint(), reordered.fingerprint());
    }
    // A changed permission is a change.
    assert_ne!(created.fingerprint(), permission.fingerprint());
    lab.close().await;
}

fn widget_types(raw: &Value) -> BTreeSet<String> {
    array(&raw["pages"])
        .iter()
        .flat_map(|page| array(&page["widgets"]))
        .filter_map(|widget| widget["type"].as_str().map(str::to_owned))
        .collect()
}

/// A read page as `dashboard.create` input: everything as read, without
/// page and widget IDs.
fn copy_page(page: &Value) -> Value {
    let widgets = array(&page["widgets"])
        .iter()
        .map(|widget| {
            let mut widget = widget.clone();
            if let Some(object) = widget.as_object_mut() {
                object.remove("widgetid");
            }
            widget
        })
        .collect::<Vec<_>>();
    json!({
        "name": page["name"],
        "display_period": page["display_period"],
        "widgets": widgets
    })
}

fn strip_ids(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| *key != "widgetid" && *key != "dashboard_pageid")
                .map(|(key, value)| (key.clone(), strip_ids(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(strip_ids).collect()),
        other => other.clone(),
    }
}

/// The dedicated non-Super-admin user (User role) in its own group without
/// host permissions; created on the first run, reused afterwards.
async fn ensure_contract_user(lab: &mut Lab) -> String {
    ensure_test_user(lab, CONTRACT_USER, CONTRACT_GROUP).await.0
}

/// A test user `username` (User role) in its own group `group` without host
/// permissions, created on the first run and reused afterwards; returns
/// (`userid`, `usrgrpid`). Only names reserved for these tests are used.
async fn ensure_test_user(lab: &mut Lab, username: &str, group: &str) -> (String, String) {
    assert!(username.starts_with("zds-contract-user") && group.starts_with("zds-contract-group"));
    let groups = lab
        .api
        .call(
            "usergroup.get",
            json!({"output": ["usrgrpid"], "filter": {"name": group}}),
        )
        .await;
    let groupid = match array(&groups).first() {
        Some(found) => text(found, "usrgrpid"),
        None => {
            let created = lab
                .api
                .call("usergroup.create", json!({"name": group}))
                .await;
            text(&created["usrgrpids"], 0)
        }
    };
    let roles = lab
        .api
        .call(
            "role.get",
            json!({"output": ["roleid", "type"], "filter": {"type": USER_ROLE_TYPE},
                   "sortfield": "roleid"}),
        )
        .await;
    let roleid = array(&roles)
        .first()
        .map(|role| text(role, "roleid"))
        .unwrap_or_else(|| panic!("laboratory has no User-type role"));
    let users = lab
        .api
        .call(
            "user.get",
            json!({"output": ["userid", "roleid"], "filter": {"username": username}}),
        )
        .await;
    if let Some(user) = array(&users).first() {
        assert_eq!(
            user["roleid"],
            roleid.as_str(),
            "contract user role drifted"
        );
        return (text(user, "userid"), groupid);
    }
    let created = lab
        .api
        .call(
            "user.create",
            json!({
                "username": username,
                "passwd": generated_password().expose_secret(),
                "roleid": roleid,
                "usrgrps": [{"usrgrpid": groupid}]
            }),
        )
        .await;
    (text(&created["userids"], 0), groupid)
}

/// A fresh random password for the contract user; never printed or stored.
fn generated_password() -> SecretString {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let mut digest = Sha256::new();
    digest.update(nanos.to_be_bytes());
    digest.update(std::process::id().to_be_bytes());
    digest.update(RandomState::new().hash_one(nanos).to_be_bytes());
    let hex = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    SecretString::from(format!("Zds-{}-Q7", &hex[..28]))
}

// ---------------------------------------------------------------- values

fn array(value: &Value) -> &[Value] {
    value
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_else(|| panic!("expected a JSON array"))
}

fn text<I: serde_json::value::Index>(value: &Value, key: I) -> String {
    value.get(key).and_then(scalar_text).unwrap_or_default()
}

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("required environment variable is absent: {name}"))
}

/// An absent key reads as an empty array (a widget sent without `fields`).
fn optional_array(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}
