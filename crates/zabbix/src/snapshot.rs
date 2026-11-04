// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{Result, ZabbixError, ZabbixVersion};

pub const ADAPTER_ID_7_4: &str = "zabbix-7.4";

/// Domain tag of [`VersionedDashboardSnapshot::storage_fingerprint`]: the
/// exact bytes of the raw document. Unchanged since P0; it names stored
/// snapshot files and the Base reference of workcopies.
const STORAGE_FINGERPRINT_DOMAIN: &[u8] = b"zdashstitch-dashboard-snapshot-v1\0";

/// Domain tag of [`VersionedDashboardSnapshot::fingerprint`]: the canonical
/// form of the document (LAB-02, 28.10.2026). It differs from the storage
/// tag, so a value of one kind never equals a value of the other kind.
const CANONICAL_FINGERPRINT_DOMAIN: &[u8] = b"zdashstitch-dashboard-canonical-v2\0";

/// A dashboard as read through the 7.4 adapter, with two fingerprints of
/// different purpose:
///
/// - [`Self::fingerprint`] compares **server state**: Remote with Base,
///   the source before send, the expected document with the read-back and
///   the reconciliation read. It is computed over [`canonical_document`],
///   because Zabbix 7.4 returns widgets and widget fields in no defined
///   order.
/// - [`Self::storage_fingerprint`] identifies the **stored bytes** of the
///   raw document: the local snapshot store names and verifies files with
///   it, and a workcopy refers to its Base with it. It never decides
///   whether the server changed.
#[derive(Clone, PartialEq)]
pub struct VersionedDashboardSnapshot {
    connection_id: String,
    server_version: ZabbixVersion,
    adapter_id: &'static str,
    raw_api_document: Value,
    fingerprint: String,
    storage_fingerprint: String,
}

impl VersionedDashboardSnapshot {
    /// Construct a snapshot at an adapter boundary. This never grants write
    /// authority: publication re-reads and compares the source before send.
    #[doc(hidden)]
    pub fn from_adapter_raw(
        connection_id: String,
        server_version: ZabbixVersion,
        raw_api_document: Value,
    ) -> Result<Self> {
        require_string(&raw_api_document, "dashboardid", "dashboard")?;
        require_array(&raw_api_document, "pages", "dashboard")?;
        let raw = serde_json::to_vec(&raw_api_document).map_err(ZabbixError::InvalidResponse)?;
        let storage_fingerprint = digest(
            STORAGE_FINGERPRINT_DOMAIN,
            &connection_id,
            &server_version,
            &raw,
        )?;
        let mut canonical = Vec::with_capacity(raw.len());
        write_canonical(&canonical_document(&raw_api_document), &mut canonical)?;
        let fingerprint = digest(
            CANONICAL_FINGERPRINT_DOMAIN,
            &connection_id,
            &server_version,
            &canonical,
        )?;
        Ok(Self {
            connection_id,
            server_version,
            adapter_id: ADAPTER_ID_7_4,
            raw_api_document,
            fingerprint,
            storage_fingerprint,
        })
    }

    pub fn dashboard_id(&self) -> &str {
        self.raw_api_document["dashboardid"]
            .as_str()
            .unwrap_or_default()
    }

    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub fn server_version(&self) -> &ZabbixVersion {
        &self.server_version
    }

    pub const fn adapter_id(&self) -> &'static str {
        self.adapter_id
    }

    pub fn raw_api_document(&self) -> &Value {
        &self.raw_api_document
    }

    /// Canonical fingerprint (64 lowercase hex): equal for two reads of
    /// the same server state whatever order Zabbix returned widgets and
    /// fields in. Use it for every comparison of server state.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// Fingerprint of the exact raw bytes (64 lowercase hex, the P0
    /// algorithm): names and verifies stored snapshot files and the Base
    /// reference of a workcopy. Never use it to decide whether the server
    /// state changed.
    pub fn storage_fingerprint(&self) -> &str {
        &self.storage_fingerprint
    }

    pub fn analyze(&self) -> Result<DashboardSnapshotAnalysis> {
        analyze(&self.raw_api_document)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DependencyKind {
    Item,
    Graph,
    Host,
    /// Host name or mask stored as a string, e.g. `svggraph` `ds.N.hosts.N`.
    HostPattern,
    /// Item name or mask stored as a string, e.g. `svggraph` `ds.N.items.N`.
    ItemPattern,
    WidgetReferenceDefinition,
    WidgetReference,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DependencyState {
    /// Understood well enough to report, but not approved for writes.
    DiscoveredReadOnly,
    /// Reserved for codecs proven by the later write-contract tasks.
    SupportedForModification,
    /// Preserved in raw form but not interpreted as safe to modify.
    Unknown,
}

#[derive(Clone, Eq, PartialEq)]
pub struct SnapshotDependency {
    pub page_index: usize,
    pub widget_index: usize,
    pub widget_type: String,
    pub field_name: String,
    pub field_type: Option<i64>,
    pub kind: DependencyKind,
    pub state: DependencyState,
    pub value: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WidgetClassification {
    pub page_index: usize,
    pub widget_index: usize,
    pub widget_type: String,
    pub state: DependencyState,
    /// Number of `fields` entries, including ones without a usable type.
    pub field_count: usize,
    /// Count of fields per numeric Zabbix field type; untyped fields are skipped.
    pub field_types: BTreeMap<i64, usize>,
}

#[derive(Clone, Eq, PartialEq)]
pub struct DashboardSnapshotAnalysis {
    pub dashboardid: String,
    pub page_count: usize,
    pub widget_count: usize,
    pub widgets: Vec<WidgetClassification>,
    pub dependencies: Vec<SnapshotDependency>,
    /// Widget count of every page, in page order.
    pub widgets_per_page: Vec<usize>,
    /// Name lengths in Unicode scalar values; the names themselves are not kept.
    pub dashboard_name_chars: usize,
    pub max_page_name_chars: usize,
    pub max_widget_name_chars: usize,
}

/// Aggregated, content-free view of many dashboards. It holds counts, widget
/// type identifiers, numeric field types and name lengths only, so it can be
/// reported without disclosing names, hosts, items or other object contents.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SnapshotSummary {
    pub dashboard_count: usize,
    pub page_count: usize,
    pub widget_count: usize,
    pub dependency_count: usize,
    pub dependencies_by_kind: BTreeMap<DependencyKind, usize>,
    pub discovered_read_only: usize,
    pub supported_for_modification: usize,
    pub unknown_dependencies: usize,
    pub unknown_widgets: usize,
    pub unknown_widget_types: BTreeSet<String>,
    pub widgets_by_type: BTreeMap<String, usize>,
    pub field_types_by_widget_type: BTreeMap<String, BTreeMap<i64, usize>>,
    /// Sorted ascending, one entry per dashboard.
    pub pages_per_dashboard: Vec<usize>,
    /// Sorted ascending, one entry per page.
    pub widgets_per_page: Vec<usize>,
    pub max_fields_per_widget: usize,
    pub max_dashboard_name_chars: usize,
    pub max_page_name_chars: usize,
    pub max_widget_name_chars: usize,
}

impl SnapshotSummary {
    pub fn from_analyses(analyses: &[DashboardSnapshotAnalysis]) -> Self {
        let mut summary = Self {
            dashboard_count: analyses.len(),
            ..Self::default()
        };
        for analysis in analyses {
            summary.page_count += analysis.page_count;
            summary.widget_count += analysis.widget_count;
            summary.pages_per_dashboard.push(analysis.page_count);
            summary
                .widgets_per_page
                .extend(analysis.widgets_per_page.iter().copied());
            summary.max_dashboard_name_chars = summary
                .max_dashboard_name_chars
                .max(analysis.dashboard_name_chars);
            summary.max_page_name_chars = summary
                .max_page_name_chars
                .max(analysis.max_page_name_chars);
            summary.max_widget_name_chars = summary
                .max_widget_name_chars
                .max(analysis.max_widget_name_chars);
            for widget in &analysis.widgets {
                *summary
                    .widgets_by_type
                    .entry(widget.widget_type.clone())
                    .or_default() += 1;
                summary.max_fields_per_widget =
                    summary.max_fields_per_widget.max(widget.field_count);
                let field_types = summary
                    .field_types_by_widget_type
                    .entry(widget.widget_type.clone())
                    .or_default();
                for (field_type, count) in &widget.field_types {
                    *field_types.entry(*field_type).or_default() += count;
                }
                if widget.state == DependencyState::Unknown {
                    summary.unknown_widgets += 1;
                    summary
                        .unknown_widget_types
                        .insert(widget.widget_type.clone());
                }
            }
            for dependency in &analysis.dependencies {
                summary.dependency_count += 1;
                *summary
                    .dependencies_by_kind
                    .entry(dependency.kind)
                    .or_default() += 1;
                match dependency.state {
                    DependencyState::DiscoveredReadOnly => summary.discovered_read_only += 1,
                    DependencyState::SupportedForModification => {
                        summary.supported_for_modification += 1;
                    }
                    DependencyState::Unknown => summary.unknown_dependencies += 1,
                }
            }
        }
        summary.pages_per_dashboard.sort_unstable();
        summary.widgets_per_page.sort_unstable();
        summary
    }
}

fn name_chars(value: &Value) -> usize {
    value
        .get("name")
        .and_then(Value::as_str)
        .map_or(0, |name| name.chars().count())
}

fn analyze(dashboard: &Value) -> Result<DashboardSnapshotAnalysis> {
    let dashboardid = require_string(dashboard, "dashboardid", "dashboard")?.to_owned();
    let pages = require_array(dashboard, "pages", "dashboard")?;
    let mut widgets_out = Vec::new();
    let mut dependencies = Vec::new();
    let mut widget_count = 0usize;
    let mut widgets_per_page = Vec::with_capacity(pages.len());
    let mut max_page_name_chars = 0usize;
    let mut max_widget_name_chars = 0usize;

    for (page_index, page) in pages.iter().enumerate() {
        let widgets = require_array(page, "widgets", "dashboard page")?;
        widget_count = widget_count.saturating_add(widgets.len());
        widgets_per_page.push(widgets.len());
        max_page_name_chars = max_page_name_chars.max(name_chars(page));
        for (widget_index, widget) in widgets.iter().enumerate() {
            let widget_type = require_string(widget, "type", "dashboard widget")?.to_owned();
            let known_widget = matches!(widget_type.as_str(), "item" | "svggraph" | "graph");
            let widget_state = if known_widget {
                DependencyState::DiscoveredReadOnly
            } else {
                DependencyState::Unknown
            };
            max_widget_name_chars = max_widget_name_chars.max(name_chars(widget));

            let fields = require_array(widget, "fields", "dashboard widget")?;
            let mut field_types = BTreeMap::new();
            for field in fields {
                if let Some(field_type) = field.get("type").and_then(value_as_i64) {
                    *field_types.entry(field_type).or_default() += 1;
                }
            }
            widgets_out.push(WidgetClassification {
                page_index,
                widget_index,
                widget_type: widget_type.clone(),
                state: widget_state,
                field_count: fields.len(),
                field_types,
            });

            for field in fields {
                let Some(field_name) = field.get("name").and_then(Value::as_str) else {
                    dependencies.push(unknown_dependency(
                        page_index,
                        widget_index,
                        &widget_type,
                        "<missing-name>",
                        field.get("type").and_then(value_as_i64),
                        field.get("value").cloned().unwrap_or(Value::Null),
                    ));
                    continue;
                };
                let field_type = field.get("type").and_then(value_as_i64);
                let value = field.get("value").cloned().unwrap_or(Value::Null);
                let kind = classify_field(&widget_type, field_name, field_type, &value);
                let Some(kind) = kind else { continue };
                let state = if known_widget && kind != DependencyKind::Unknown {
                    DependencyState::DiscoveredReadOnly
                } else {
                    DependencyState::Unknown
                };
                dependencies.push(SnapshotDependency {
                    page_index,
                    widget_index,
                    widget_type: widget_type.clone(),
                    field_name: field_name.to_owned(),
                    field_type,
                    kind,
                    state,
                    value,
                });
            }
        }
    }

    Ok(DashboardSnapshotAnalysis {
        dashboardid,
        page_count: pages.len(),
        widget_count,
        widgets: widgets_out,
        dependencies,
        widgets_per_page,
        dashboard_name_chars: name_chars(dashboard),
        max_page_name_chars,
        max_widget_name_chars,
    })
}

fn classify_field(
    widget_type: &str,
    name: &str,
    field_type: Option<i64>,
    value: &Value,
) -> Option<DependencyKind> {
    if name == "reference" {
        return Some(DependencyKind::WidgetReferenceDefinition);
    }
    if widget_type == "svggraph"
        && field_type == Some(1)
        && let Some(kind) = classify_svggraph_pattern(name)
    {
        return Some(kind);
    }
    if name.ends_with("._reference") || value.as_str().is_some_and(is_widget_reference_value) {
        return Some(DependencyKind::WidgetReference);
    }
    match field_type {
        Some(3) => Some(DependencyKind::Host),
        Some(4) => Some(DependencyKind::Item),
        Some(6) => Some(DependencyKind::Graph),
        Some(5 | 7) => Some(DependencyKind::Unknown),
        _ => None,
    }
}

/// Recognises the string source fields of `svggraph` in Zabbix 7.4:
/// data sets `ds.<n>.hosts.<m>` / `ds.<n>.items.<m>`, overrides
/// `or.<n>.hosts.<m>` / `or.<n>.items.<m>` and `problemhosts.<m>`.
fn classify_svggraph_pattern(name: &str) -> Option<DependencyKind> {
    let is_index = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    let parts: Vec<&str> = name.split('.').collect();
    match parts.as_slice() {
        ["ds" | "or", set, "hosts", index] if is_index(set) && is_index(index) => {
            Some(DependencyKind::HostPattern)
        }
        ["ds" | "or", set, "items", index] if is_index(set) && is_index(index) => {
            Some(DependencyKind::ItemPattern)
        }
        ["problemhosts", index] if is_index(index) => Some(DependencyKind::HostPattern),
        _ => None,
    }
}

fn is_widget_reference_value(value: &str) -> bool {
    value == "DASHBOARD._hostid"
        || value.split_once("._").is_some_and(|(reference, target)| {
            reference.len() == 5
                && reference
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
                && matches!(target, "hostid" | "itemid" | "timeperiod")
        })
}

fn value_as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn unknown_dependency(
    page_index: usize,
    widget_index: usize,
    widget_type: &str,
    field_name: &str,
    field_type: Option<i64>,
    value: Value,
) -> SnapshotDependency {
    SnapshotDependency {
        page_index,
        widget_index,
        widget_type: widget_type.to_owned(),
        field_name: field_name.to_owned(),
        field_type,
        kind: DependencyKind::Unknown,
        state: DependencyState::Unknown,
        value,
    }
}

fn digest(
    domain: &[u8],
    connection_id: &str,
    version: &ZabbixVersion,
    document: &[u8],
) -> Result<String> {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(connection_id.as_bytes());
    digest.update(b"\0");
    digest.update(version.to_string().as_bytes());
    digest.update(b"\0");
    digest.update(document);
    let mut output = String::with_capacity(64);
    for byte in digest.finalize() {
        write!(&mut output, "{byte:02x}").map_err(|_| {
            ZabbixError::SnapshotShape("could not format snapshot fingerprint".to_owned())
        })?;
    }
    Ok(output)
}

/// Canonical form of a Zabbix 7.4 `dashboard.get` document, the input of
/// [`VersionedDashboardSnapshot::fingerprint`] (LAB-02, 28.10.2026).
///
/// Zabbix 7.4.16 sorts pages by `sortorder` but selects `widget` and
/// `widget_field` rows without `ORDER BY`; after `dashboard.update` a
/// changed field keeps its `widget_fieldid` but is returned at another
/// position. The canonical form differs from the raw document only in the
/// order of these two arrays:
///
/// - `pages` keep their order (it is `sortorder`, a property of the
///   dashboard);
/// - `widgets` of each page are ordered by `widgetid` (numerically); ties
///   and widgets without a `widgetid` by their canonical encoding;
/// - `fields` of each widget are ordered by (`name`, `type`); ties by their
///   canonical encoding. Nothing is merged or dropped: duplicate fields
///   stay, so equal fingerprints mean equal multisets;
/// - the sharing lists are sets: `users` ordered by `userid` and
///   `userGroups` by `usrgrpid` (numerically), then by `permission`; ties
///   by their canonical encoding. Zabbix grants by these entries whatever
///   their order, returns them in insertion order and moves an entry whose
///   permission changed to the end (LAB-02 `sharing_lists_are_read_as_sets`);
///   duplicates stay;
/// - every other value is kept exactly: scalars with their JSON kind (`"4"`
///   and `4` differ), every other array in its order. Object keys are
///   encoded in sorted order.
///
/// Arrays that are not where this shape expects them are left as they are.
#[must_use]
pub fn canonical_document(raw: &Value) -> Value {
    let mut document = raw.clone();
    for (list, id) in [("users", "userid"), ("userGroups", "usrgrpid")] {
        if let Some(entries) = document.get_mut(list).and_then(Value::as_array_mut) {
            entries.sort_by_cached_key(|entry| {
                (
                    numeric_id(entry.get(id)),
                    entry.get("permission").map(canonical_text),
                    canonical_text(entry),
                )
            });
        }
    }
    let Some(pages) = document.get_mut("pages").and_then(Value::as_array_mut) else {
        return document;
    };
    for page in pages {
        let Some(widgets) = page.get_mut("widgets").and_then(Value::as_array_mut) else {
            continue;
        };
        for widget in widgets.iter_mut() {
            if let Some(fields) = widget.get_mut("fields").and_then(Value::as_array_mut) {
                fields.sort_by_cached_key(|field| {
                    (
                        field.get("name").and_then(Value::as_str).map(str::to_owned),
                        field.get("type").map(canonical_text),
                        canonical_text(field),
                    )
                });
            }
        }
        widgets.sort_by_cached_key(|widget| {
            (numeric_id(widget.get("widgetid")), canonical_text(widget))
        });
    }
    document
}

/// Sort key of a string ID: numeric order for digit strings of any length;
/// `None` (first) when absent or not a string.
fn numeric_id(value: Option<&Value>) -> Option<(usize, String)> {
    value
        .and_then(Value::as_str)
        .map(|id| (id.len(), id.to_owned()))
}

/// Canonical encoding of a value as text; used as a sort key only.
fn canonical_text(value: &Value) -> String {
    let mut output = Vec::new();
    if write_canonical(value, &mut output).is_err() {
        output.clear();
    }
    String::from_utf8(output).unwrap_or_default()
}

/// JSON with object keys in sorted order, independent of how
/// `serde_json::Map` orders them; scalars as `serde_json` writes them.
fn write_canonical(value: &Value, output: &mut Vec<u8>) -> Result<()> {
    match value {
        Value::Array(items) => {
            output.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                write_canonical(item, output)?;
            }
            output.push(b']');
        }
        Value::Object(map) => {
            let mut entries = map.iter().collect::<Vec<_>>();
            entries.sort_unstable_by(|left, right| left.0.cmp(right.0));
            output.push(b'{');
            for (index, (key, item)) in entries.into_iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                serde_json::to_writer(&mut *output, key).map_err(ZabbixError::InvalidResponse)?;
                output.push(b':');
                write_canonical(item, output)?;
            }
            output.push(b'}');
        }
        scalar => {
            serde_json::to_writer(&mut *output, scalar).map_err(ZabbixError::InvalidResponse)?;
        }
    }
    Ok(())
}

fn require_string<'a>(value: &'a Value, key: &str, context: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ZabbixError::SnapshotShape(format!("{context} has no string {key}")))
}

fn require_array<'a>(value: &'a Value, key: &str, context: &str) -> Result<&'a Vec<Value>> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| ZabbixError::SnapshotShape(format!("{context} has no array {key}")))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};

    use super::{DependencyKind, DependencyState, VersionedDashboardSnapshot, analyze};

    fn snapshot(raw: Value) -> VersionedDashboardSnapshot {
        VersionedDashboardSnapshot::from_adapter_raw(
            "c".to_owned(),
            "7.4.16"
                .parse()
                .unwrap_or_else(|error| panic!("version: {error}")),
            raw,
        )
        .unwrap_or_else(|error| panic!("snapshot: {error}"))
    }

    /// Two pages; the first with two widgets of several fields.
    fn dashboard() -> Value {
        json!({
            "dashboardid": "7", "name": "d", "userid": "1", "private": "1",
            "users": [{"userid": "2", "permission": "2"}, {"userid": "3", "permission": "3"}],
            "userGroups": [],
            "pages": [
                {"dashboard_pageid": "11", "name": "a", "widgets": [
                    {"widgetid": "9", "type": "item", "x": "0", "fields": [
                        {"type": "4", "name": "itemid.0", "value": "101"},
                        {"type": "1", "name": "reference", "value": "ABCDE"},
                        {"type": "0", "name": "show.0", "value": "2"}
                    ]},
                    {"widgetid": "10", "type": "svggraph", "x": "12", "fields": [
                        {"type": "1", "name": "ds.0.hosts.0", "value": "h"},
                        {"type": "4", "name": "ds.0.itemids.0", "value": "102"}
                    ]}
                ]},
                {"dashboard_pageid": "12", "name": "b", "widgets": []}
            ]
        })
    }

    fn fields(raw: &mut Value, page: usize, widget: usize) -> &mut Vec<Value> {
        raw["pages"][page]["widgets"][widget]["fields"]
            .as_array_mut()
            .unwrap_or_else(|| panic!("fields"))
    }

    #[test]
    fn field_and_widget_order_do_not_change_the_canonical_fingerprint() {
        let base = snapshot(dashboard());
        let mut moved = dashboard();
        // The changed field read back last (LAB-02), the widgets swapped.
        fields(&mut moved, 0, 0).rotate_left(1);
        fields(&mut moved, 0, 1).reverse();
        moved["pages"][0]["widgets"]
            .as_array_mut()
            .unwrap_or_else(|| panic!("widgets"))
            .reverse();
        let moved = snapshot(moved);
        assert_eq!(base.fingerprint(), moved.fingerprint());
        assert_ne!(base.storage_fingerprint(), moved.storage_fingerprint());
        assert_ne!(base.fingerprint(), base.storage_fingerprint());
    }

    #[test]
    fn values_kinds_pages_and_other_arrays_still_count() {
        let base = snapshot(dashboard()).fingerprint().to_owned();
        let mut changes: Vec<Value> = Vec::new();
        let mut value = dashboard();
        fields(&mut value, 0, 0)[0]["value"] = json!("201");
        changes.push(value);
        let mut kind = dashboard();
        fields(&mut kind, 0, 0)[0]["value"] = json!(101);
        changes.push(kind);
        let mut field_type = dashboard();
        fields(&mut field_type, 0, 0)[0]["type"] = json!(4);
        changes.push(field_type);
        let mut pages = dashboard();
        pages["pages"]
            .as_array_mut()
            .unwrap_or_else(|| panic!("pages"))
            .reverse();
        changes.push(pages);
        let mut permission = dashboard();
        permission["users"][0]["permission"] = json!("3");
        changes.push(permission);
        let mut kind_of_id = dashboard();
        kind_of_id["users"][0]["userid"] = json!(2);
        changes.push(kind_of_id);
        let mut group = dashboard();
        group["userGroups"] = json!([{"usrgrpid": "7", "permission": "2"}]);
        changes.push(group);
        let mut moved_widget = dashboard();
        moved_widget["pages"][0]["widgets"][0]["x"] = json!("1");
        changes.push(moved_widget);
        let mut dropped = dashboard();
        fields(&mut dropped, 0, 0).pop();
        changes.push(dropped);
        for (index, raw) in changes.into_iter().enumerate() {
            assert_ne!(snapshot(raw).fingerprint(), base, "change {index}");
        }
    }

    #[test]
    fn sharing_lists_are_compared_as_sets() {
        let mut raw = dashboard();
        raw["userGroups"] = json!([
            {"usrgrpid": "13", "permission": "3"},
            {"usrgrpid": "8", "permission": "2"}
        ]);
        let base = snapshot(raw.clone());
        let mut moved = raw.clone();
        for list in ["users", "userGroups"] {
            moved[list]
                .as_array_mut()
                .unwrap_or_else(|| panic!("{list}"))
                .reverse();
        }
        let moved = snapshot(moved);
        assert_eq!(base.fingerprint(), moved.fingerprint());
        assert_ne!(base.storage_fingerprint(), moved.storage_fingerprint());
        // Numeric order: group 8 before group 13.
        let canonical = super::canonical_document(&raw);
        assert_eq!(canonical["userGroups"][0]["usrgrpid"], "8");
        // A duplicate entry is not merged away.
        let mut twice = raw;
        twice["users"]
            .as_array_mut()
            .unwrap_or_else(|| panic!("users"))
            .push(json!({"userid": "2", "permission": "2"}));
        assert_ne!(snapshot(twice).fingerprint(), base.fingerprint());
    }

    #[test]
    fn duplicate_fields_are_kept_not_merged() {
        let mut once = dashboard();
        fields(&mut once, 0, 0).push(json!({"type": "1", "name": "reference", "value": "X"}));
        let mut twice = once.clone();
        fields(&mut twice, 0, 0).push(json!({"type": "1", "name": "reference", "value": "X"}));
        assert_ne!(snapshot(once).fingerprint(), snapshot(twice).fingerprint());
    }

    #[test]
    fn widgets_without_an_id_are_ordered_by_content() {
        let mut raw = dashboard();
        for widget in raw["pages"][0]["widgets"]
            .as_array_mut()
            .unwrap_or_else(|| panic!("widgets"))
        {
            widget
                .as_object_mut()
                .unwrap_or_else(|| panic!("widget"))
                .remove("widgetid");
        }
        let mut swapped = raw.clone();
        swapped["pages"][0]["widgets"]
            .as_array_mut()
            .unwrap_or_else(|| panic!("widgets"))
            .reverse();
        assert_eq!(snapshot(raw).fingerprint(), snapshot(swapped).fingerprint());
    }

    /// Stored snapshot files are named by this value: it must stay the P0
    /// algorithm over the exact bytes.
    #[test]
    fn storage_fingerprint_is_the_p0_digest_of_the_raw_bytes() {
        let raw = dashboard();
        let mut digest = Sha256::new();
        digest.update(b"zdashstitch-dashboard-snapshot-v1\0");
        digest.update(b"c\0");
        digest.update(b"7.4.16\0");
        digest.update(serde_json::to_vec(&raw).unwrap_or_else(|error| panic!("{error}")));
        let expected = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(snapshot(raw).storage_fingerprint(), expected);
    }

    #[test]
    fn svggraph_string_patterns_are_reported_as_read_only_dependencies() {
        let dashboard = json!({
            "dashboardid": "1",
            "name": "Synthetic",
            "pages": [{"name": "", "widgets": [{
                "type": "svggraph",
                "name": "",
                "fields": [
                    {"type": "1", "name": "ds.0.hosts.0", "value": "synthetic-host-*"},
                    {"type": "1", "name": "ds.0.items.0", "value": "CPU *"},
                    {"type": "1", "name": "problemhosts.0", "value": "synthetic-host-1"},
                    {"type": "1", "name": "or.0.hosts.0", "value": "synthetic-host-2"},
                    {"type": "1", "name": "or.0.items.0", "value": "Memory *"},
                    {"type": "0", "name": "or.0.width", "value": "2"},
                    {"type": "1", "name": "ds.0.color", "value": "FF465C"},
                    {"type": "1", "name": "ds.x.hosts.0", "value": "not an index"}
                ]
            }]}]
        });
        let analysis =
            analyze(&dashboard).unwrap_or_else(|error| panic!("analysis must succeed: {error}"));
        let kinds: Vec<_> = analysis
            .dependencies
            .iter()
            .map(|dependency| (dependency.field_name.as_str(), dependency.kind))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("ds.0.hosts.0", DependencyKind::HostPattern),
                ("ds.0.items.0", DependencyKind::ItemPattern),
                ("problemhosts.0", DependencyKind::HostPattern),
                ("or.0.hosts.0", DependencyKind::HostPattern),
                ("or.0.items.0", DependencyKind::ItemPattern),
            ]
        );
        assert!(
            analysis
                .dependencies
                .iter()
                .all(|dependency| dependency.state == DependencyState::DiscoveredReadOnly)
        );
    }

    #[test]
    fn string_fields_of_other_widgets_are_not_treated_as_patterns() {
        let dashboard = json!({
            "dashboardid": "1",
            "pages": [{"widgets": [{
                "type": "tophosts",
                "fields": [{"type": "1", "name": "ds.0.hosts.0", "value": "x"}]
            }]}]
        });
        let analysis =
            analyze(&dashboard).unwrap_or_else(|error| panic!("analysis must succeed: {error}"));
        assert!(analysis.dependencies.is_empty());
    }
}
