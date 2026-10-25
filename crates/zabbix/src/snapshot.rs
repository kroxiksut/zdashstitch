// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{Result, ZabbixError, ZabbixVersion};

pub const ADAPTER_ID_7_4: &str = "zabbix-7.4";

#[derive(Clone, PartialEq)]
pub struct VersionedDashboardSnapshot {
    connection_id: String,
    server_version: ZabbixVersion,
    adapter_id: &'static str,
    raw_api_document: Value,
    fingerprint: String,
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
        let fingerprint = fingerprint(&connection_id, &server_version, &raw_api_document)?;
        Ok(Self {
            connection_id,
            server_version,
            adapter_id: ADAPTER_ID_7_4,
            raw_api_document,
            fingerprint,
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

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
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
/// `ds.<n>.hosts.<m>`, `ds.<n>.items.<m>` and `problemhosts.<m>`.
fn classify_svggraph_pattern(name: &str) -> Option<DependencyKind> {
    let is_index = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    let parts: Vec<&str> = name.split('.').collect();
    match parts.as_slice() {
        ["ds", set, "hosts", index] if is_index(set) && is_index(index) => {
            Some(DependencyKind::HostPattern)
        }
        ["ds", set, "items", index] if is_index(set) && is_index(index) => {
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

fn fingerprint(connection_id: &str, version: &ZabbixVersion, raw: &Value) -> Result<String> {
    let canonical = serde_json::to_vec(raw).map_err(ZabbixError::InvalidResponse)?;
    let mut digest = Sha256::new();
    digest.update(b"zdashstitch-dashboard-snapshot-v1\0");
    digest.update(connection_id.as_bytes());
    digest.update(b"\0");
    digest.update(version.to_string().as_bytes());
    digest.update(b"\0");
    digest.update(&canonical);
    let mut output = String::with_capacity(64);
    for byte in digest.finalize() {
        write!(&mut output, "{byte:02x}").map_err(|_| {
            ZabbixError::SnapshotShape("could not format snapshot fingerprint".to_owned())
        })?;
    }
    Ok(output)
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
    use serde_json::json;

    use super::{DependencyKind, DependencyState, analyze};

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
