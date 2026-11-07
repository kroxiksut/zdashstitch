// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-02A: the source model of a Base snapshot — widget → sources → host,
//! item, graph or pattern, each with its address in Base.
//!
//! Field classification is the P0-05 dependency map of the adapter
//! ([`VersionedDashboardSnapshot::analyze`]); this module only groups it by
//! widget, adds addresses and decides applicability from the adapter
//! capability registry, O-003 and, for classic `graph`, the adapter's
//! `classic_graph_graphid` contract for the exact server version of Base.
//! It never parses widget fields a second time.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zdashstitch_zabbix::{
    AdapterRegistry, DependencyKind, SnapshotDependency, VersionedDashboardSnapshot,
    WidgetCapability, WidgetCapabilityRegistry, WidgetFieldType, WriteContract, is_zabbix_id,
};

use super::ResolverError;
use super::links::WidgetLinks;
use super::pattern::is_mask;
use super::status::{Applicability, ReasonCode};

/// Address of a widget in Base: positions plus server IDs when present.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WidgetAddress {
    /// 0-based page position in Base.
    pub page_index: usize,
    pub dashboard_pageid: Option<String>,
    /// 0-based widget position on its page.
    pub widget_index: usize,
    pub widgetid: Option<String>,
}

/// Address of one widget field in Base.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FieldAddress {
    pub widget: WidgetAddress,
    /// Zabbix field name, e.g. `itemid.0`, `ds.1.hosts.0`.
    pub field_name: String,
}

/// Where a string pattern of `svggraph` lives.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PatternPlace {
    /// `ds.<n>.hosts.<m>` / `ds.<n>.items.<m>`.
    DataSet(u32),
    /// `or.<n>.hosts.<m>` / `or.<n>.items.<m>` (series overrides).
    Override(u32),
    /// `problemhosts.<m>` (problems drawn on the graph).
    ProblemHosts,
}

/// What a widget field refers to.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SourceKind {
    /// `ZBX_WIDGET_FIELD_TYPE_ITEM`: an `itemid` (plain item or LLD instance).
    Item { itemid: String },
    /// `ZBX_WIDGET_FIELD_TYPE_ITEM_PROTOTYPE`.
    ItemPrototype { itemid: String },
    /// `ZBX_WIDGET_FIELD_TYPE_GRAPH`: a classic graph.
    Graph { graphid: String },
    /// `ZBX_WIDGET_FIELD_TYPE_GRAPH_PROTOTYPE`.
    GraphPrototype { graphid: String },
    /// `ZBX_WIDGET_FIELD_TYPE_HOST`: a `hostid`.
    Host { hostid: String },
    /// Host visible-name pattern of `svggraph`; may be a mask.
    HostPattern {
        pattern: String,
        place: PatternPlace,
    },
    /// Item name pattern of `svggraph`; may be a mask.
    ItemPattern {
        pattern: String,
        place: PatternPlace,
    },
    /// Consumer of another widget's data: `<reference>._<channel>`.
    WidgetReference { reference: String, channel: String },
    /// The widget's own `reference` that other widgets can listen to.
    ReferenceDefinition { reference: String },
    /// A reference-like field the adapter does not understand.
    Unknown,
}

impl SourceKind {
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Item { .. } => "item",
            Self::ItemPrototype { .. } => "item_prototype",
            Self::Graph { .. } => "graph",
            Self::GraphPrototype { .. } => "graph_prototype",
            Self::Host { .. } => "host",
            Self::HostPattern { .. } => "host_pattern",
            Self::ItemPattern { .. } => "item_pattern",
            Self::WidgetReference { .. } => "widget_reference",
            Self::ReferenceDefinition { .. } => "reference_definition",
            Self::Unknown => "unknown",
        }
    }
}

/// One referencing field of a widget.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Source {
    pub field: FieldAddress,
    /// `ZBX_WIDGET_FIELD_TYPE_*` as sent by the server.
    pub field_type: Option<i64>,
    pub kind: SourceKind,
    /// Whether ReplaceHost may change this field.
    pub applicability: Applicability,
    /// Why the field is not editable; `None` when editable.
    pub applicability_reason: Option<ReasonCode>,
}

/// Registry view of a whole widget type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WidgetSupport {
    /// The registry allows editing the widget type.
    Editable,
    /// Known type without edit capability.
    ReadOnly,
    /// Type unknown to the adapter registry.
    Unknown,
}

/// One `svggraph` data set: the series it draws.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SvgDataSet {
    /// `<n>` of `ds.<n>.*`.
    pub index: u32,
    /// `ds.<n>.dataset_type` when the server stored it (only non-default
    /// values are stored).
    pub dataset_type: Option<i64>,
    pub host_patterns: Vec<String>,
    pub item_patterns: Vec<String>,
    /// Explicit items `ds.<n>.itemids.<m>`.
    pub itemids: Vec<String>,
    /// `ds.<n>.itemids.<m>._reference` consumers.
    pub item_references: u32,
}

/// A widget and its sources.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WidgetSources {
    pub widget: WidgetAddress,
    pub widget_type: String,
    /// Server data: the widget name (may be empty).
    pub name: String,
    pub support: WidgetSupport,
    pub sources: Vec<Source>,
    /// `svggraph` data sets in index order; empty for other types.
    pub datasets: Vec<SvgDataSet>,
}

/// P2-02A result: every widget of Base with its sources and the
/// widget-to-widget links.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceModel {
    pub dashboardid: String,
    /// Storage fingerprint of the Base snapshot the model was built from
    /// (`VersionedDashboardSnapshot::storage_fingerprint`): it is frozen
    /// into the operation log and checked against the workcopy's Base
    /// reference, never compared with server state.
    pub snapshot_fingerprint: String,
    pub page_count: usize,
    pub widgets: Vec<WidgetSources>,
    pub links: WidgetLinks,
}

impl SourceModel {
    /// Every source with its widget, in Base order.
    pub fn sources(&self) -> impl Iterator<Item = (&WidgetSources, &Source)> {
        self.widgets
            .iter()
            .flat_map(|widget| widget.sources.iter().map(move |source| (widget, source)))
    }

    /// Distinct item IDs of item fields (plain items and LLD instances).
    #[must_use]
    pub fn itemids(&self) -> BTreeSet<String> {
        self.sources()
            .filter_map(|(_, source)| match &source.kind {
                SourceKind::Item { itemid } => Some(itemid.clone()),
                _ => None,
            })
            .collect()
    }

    /// Distinct item prototype IDs (`ZBX_WIDGET_FIELD_TYPE_ITEM_PROTOTYPE`).
    #[must_use]
    pub fn item_prototype_ids(&self) -> BTreeSet<String> {
        self.sources()
            .filter_map(|(_, source)| match &source.kind {
                SourceKind::ItemPrototype { itemid } => Some(itemid.clone()),
                _ => None,
            })
            .collect()
    }

    /// Distinct graph prototype IDs (`ZBX_WIDGET_FIELD_TYPE_GRAPH_PROTOTYPE`).
    #[must_use]
    pub fn graph_prototype_ids(&self) -> BTreeSet<String> {
        self.sources()
            .filter_map(|(_, source)| match &source.kind {
                SourceKind::GraphPrototype { graphid } => Some(graphid.clone()),
                _ => None,
            })
            .collect()
    }

    /// Distinct classic graph IDs.
    #[must_use]
    pub fn graphids(&self) -> BTreeSet<String> {
        self.sources()
            .filter_map(|(_, source)| match &source.kind {
                SourceKind::Graph { graphid } => Some(graphid.clone()),
                _ => None,
            })
            .collect()
    }

    /// Distinct host IDs of host fields.
    #[must_use]
    pub fn host_field_ids(&self) -> BTreeSet<String> {
        self.sources()
            .filter_map(|(_, source)| match &source.kind {
                SourceKind::Host { hostid } => Some(hostid.clone()),
                _ => None,
            })
            .collect()
    }

    /// Distinct host patterns without `*`, which name exactly one host.
    #[must_use]
    pub fn exact_host_patterns(&self) -> BTreeSet<String> {
        self.sources()
            .filter_map(|(_, source)| match &source.kind {
                SourceKind::HostPattern { pattern, .. } if !is_mask(pattern) => {
                    Some(pattern.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// Whether any `svggraph` data set selects items by name pattern.
    #[must_use]
    pub fn has_item_patterns(&self) -> bool {
        self.widgets
            .iter()
            .flat_map(|widget| &widget.datasets)
            .any(|dataset| !dataset.item_patterns.is_empty())
    }
}

/// Builds the source model of a Base snapshot. Pure: no server access.
pub fn source_model(
    snapshot: &VersionedDashboardSnapshot,
    registry: &WidgetCapabilityRegistry,
) -> Result<SourceModel, ResolverError> {
    let analysis = snapshot
        .analyze()
        .map_err(|error| ResolverError::from_zabbix(&error))?;
    let contracts = Contracts::of(snapshot);
    let raw = snapshot.raw_api_document();
    let pages = raw
        .get("pages")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();

    let mut widgets = Vec::with_capacity(analysis.widgets.len());
    let mut positions = BTreeMap::new();
    for classification in &analysis.widgets {
        let page = pages.get(classification.page_index);
        let widget = page
            .and_then(|page| page.get("widgets"))
            .and_then(Value::as_array)
            .and_then(|widgets| widgets.get(classification.widget_index));
        let address = WidgetAddress {
            page_index: classification.page_index,
            dashboard_pageid: page.and_then(|page| id_property(page, "dashboard_pageid")),
            widget_index: classification.widget_index,
            widgetid: widget.and_then(|widget| id_property(widget, "widgetid")),
        };
        let support = match registry.find(&classification.widget_type) {
            Some(capability) if capability.flags.edit => WidgetSupport::Editable,
            Some(_) => WidgetSupport::ReadOnly,
            None => WidgetSupport::Unknown,
        };
        positions.insert(
            (classification.page_index, classification.widget_index),
            widgets.len(),
        );
        widgets.push(WidgetSources {
            widget: address,
            widget_type: classification.widget_type.clone(),
            name: widget
                .and_then(|widget| widget.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            support,
            sources: Vec::new(),
            datasets: Vec::new(),
        });
    }

    for dependency in &analysis.dependencies {
        let Some(&position) = positions.get(&(dependency.page_index, dependency.widget_index))
        else {
            continue;
        };
        let widget = &mut widgets[position];
        let Some(kind) = source_kind(dependency) else {
            continue;
        };
        let capability = registry.find(&widget.widget_type);
        let (applicability, applicability_reason) = applicability(
            &widget.widget_type,
            capability,
            dependency,
            &kind,
            contracts,
        );
        widget.sources.push(Source {
            field: FieldAddress {
                widget: widget.widget.clone(),
                field_name: dependency.field_name.clone(),
            },
            field_type: dependency.field_type,
            kind,
            applicability,
            applicability_reason,
        });
    }

    for widget in &mut widgets {
        if widget.widget_type == "svggraph" {
            let raw_widget = pages
                .get(widget.widget.page_index)
                .and_then(|page| page.get("widgets"))
                .and_then(Value::as_array)
                .and_then(|widgets| widgets.get(widget.widget.widget_index));
            widget.datasets = datasets(&widget.sources, raw_widget);
        }
    }

    let links = WidgetLinks::from_widgets(&widgets);
    Ok(SourceModel {
        dashboardid: analysis.dashboardid,
        snapshot_fingerprint: snapshot.storage_fingerprint().to_owned(),
        page_count: analysis.page_count,
        widgets,
        links,
    })
}

fn id_property(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|id| is_zabbix_id(id))
        .map(str::to_owned)
}

/// A usable object ID: digits, not the «not set» value `0`.
fn object_id(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|id| is_zabbix_id(id) && *id != "0")
        .map(str::to_owned)
}

fn index_part(part: &str) -> Option<u32> {
    if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

fn pattern_place(field_name: &str) -> Option<PatternPlace> {
    let parts: Vec<&str> = field_name.split('.').collect();
    match parts.as_slice() {
        ["ds", set, _, _] => index_part(set).map(PatternPlace::DataSet),
        ["or", set, _, _] => index_part(set).map(PatternPlace::Override),
        ["problemhosts", _] => Some(PatternPlace::ProblemHosts),
        _ => None,
    }
}

/// Maps a P0-05 dependency to a source; `None` for an unset value.
fn source_kind(dependency: &SnapshotDependency) -> Option<SourceKind> {
    let value = &dependency.value;
    let text = value.as_str().unwrap_or_default();
    let kind = match dependency.kind {
        DependencyKind::Item => SourceKind::Item {
            itemid: object_id(value)?,
        },
        DependencyKind::Graph => SourceKind::Graph {
            graphid: object_id(value)?,
        },
        DependencyKind::Host => SourceKind::Host {
            hostid: object_id(value)?,
        },
        DependencyKind::HostPattern | DependencyKind::ItemPattern => {
            if text.is_empty() {
                return None;
            }
            let Some(place) = pattern_place(&dependency.field_name) else {
                return Some(SourceKind::Unknown);
            };
            let pattern = text.to_owned();
            if dependency.kind == DependencyKind::HostPattern {
                SourceKind::HostPattern { pattern, place }
            } else {
                SourceKind::ItemPattern { pattern, place }
            }
        }
        DependencyKind::WidgetReferenceDefinition => {
            if text.is_empty() {
                return None;
            }
            SourceKind::ReferenceDefinition {
                reference: text.to_owned(),
            }
        }
        DependencyKind::WidgetReference => {
            if text.is_empty() {
                return None;
            }
            let (reference, channel) = text.split_once("._").unwrap_or((text, ""));
            SourceKind::WidgetReference {
                reference: reference.to_owned(),
                channel: channel.to_owned(),
            }
        }
        DependencyKind::Unknown => match dependency.field_type {
            Some(5) => SourceKind::ItemPrototype {
                itemid: object_id(value)?,
            },
            Some(7) => SourceKind::GraphPrototype {
                graphid: object_id(value)?,
            },
            _ => SourceKind::Unknown,
        },
    };
    Some(kind)
}

/// `itemid.<m>` of the `item` widget.
fn is_item_widget_item_field(field_name: &str) -> bool {
    field_name
        .strip_prefix("itemid.")
        .is_some_and(|index| index_part(index).is_some())
}

/// `graphid.<m>` of the classic `graph` widget.
fn is_graph_widget_graph_field(field_name: &str) -> bool {
    field_name
        .strip_prefix("graphid.")
        .is_some_and(|index| index_part(index).is_some())
}

/// Laboratory contracts the applicability depends on, for the exact server
/// version of Base. Publication checks the connected server again (same
/// version, every contract `Supported`); this only decides what a
/// calculation offers to change.
#[derive(Clone, Copy, Debug)]
struct Contracts {
    /// `classic_graph_graphid` (P2-04H): `Supported` on 7.4.16 (LAB-02).
    classic_graph: bool,
}

impl Contracts {
    fn of(snapshot: &VersionedDashboardSnapshot) -> Self {
        let classic_graph =
            AdapterRegistry::resolve(snapshot.server_version().clone()).is_ok_and(|capabilities| {
                capabilities
                    .dashboard_write_contracts
                    .is_verified(WriteContract::ClassicGraphGraphid)
            });
        Self { classic_graph }
    }
}

/// `ds.<n>.itemids.<m>` of `svggraph`.
fn is_svggraph_item_field(field_name: &str) -> bool {
    let parts: Vec<&str> = field_name.split('.').collect();
    matches!(parts.as_slice(), ["ds", set, "itemids", index]
        if index_part(set).is_some() && index_part(index).is_some())
}

/// Applicability for ReplaceHost: the registry must know the widget type
/// and field and grant `edit`, and O-003 must name the field. A classic
/// graph `graphid.<m>` is editable only when the adapter's
/// `classic_graph_graphid` contract is `Supported` for the Base version
/// (P2-04H); otherwise it stays `unsupported` with
/// `classic_graph_contract_pending`. The item of a simple graph
/// (`source_type=1`, `itemid.<m>`) is not changed: LAB-02 proved only its
/// unchanged resend, not a replacement.
fn applicability(
    widget_type: &str,
    capability: Option<&WidgetCapability>,
    dependency: &SnapshotDependency,
    kind: &SourceKind,
    contracts: Contracts,
) -> (Applicability, Option<ReasonCode>) {
    let Some(capability) = capability else {
        return (Applicability::Unsupported, Some(ReasonCode::UnknownWidget));
    };
    let field_type = dependency
        .field_type
        .and_then(|number| u16::try_from(number).ok())
        .and_then(|number| WidgetFieldType::try_from(number).ok());
    let known = field_type
        .is_some_and(|field_type| capability.knows_field(&dependency.field_name, field_type));
    if !known {
        return (Applicability::Unsupported, Some(ReasonCode::UnknownField));
    }
    if widget_type == "graph"
        && matches!(kind, SourceKind::Graph { .. })
        && !contracts.classic_graph
    {
        return (
            Applicability::Unsupported,
            Some(ReasonCode::ClassicGraphContractPending),
        );
    }
    let replaceable = match (widget_type, kind) {
        ("item", SourceKind::Item { .. }) => is_item_widget_item_field(&dependency.field_name),
        ("graph", SourceKind::Graph { .. }) => is_graph_widget_graph_field(&dependency.field_name),
        ("svggraph", SourceKind::Item { .. }) => is_svggraph_item_field(&dependency.field_name),
        (
            "svggraph",
            SourceKind::HostPattern {
                place: PatternPlace::DataSet(_),
                ..
            },
        ) => true,
        _ => false,
    };
    match (replaceable, capability.flags.edit) {
        (true, true) => (Applicability::Editable, None),
        (true, false) => (Applicability::ReadOnly, Some(ReasonCode::WidgetNotEditable)),
        (false, _) => match kind {
            // The owner is read with `itemprototype.get` / `graphprototype.get`
            // (ALLOWLIST-01); O-003 does not change prototype fields.
            SourceKind::ItemPrototype { .. } | SourceKind::GraphPrototype { .. } => (
                Applicability::ReadOnly,
                Some(ReasonCode::FieldNotReplaceable),
            ),
            SourceKind::Host { .. } => (
                Applicability::ReadOnly,
                Some(ReasonCode::HostFieldNotSupported),
            ),
            SourceKind::HostPattern { .. } => (
                Applicability::ReadOnly,
                Some(ReasonCode::PatternFieldNotSupported),
            ),
            _ if !capability.flags.edit => {
                (Applicability::ReadOnly, Some(ReasonCode::WidgetNotEditable))
            }
            _ => (
                Applicability::ReadOnly,
                Some(ReasonCode::FieldNotReplaceable),
            ),
        },
    }
}

/// Groups `svggraph` sources by data set.
fn datasets(sources: &[Source], raw_widget: Option<&Value>) -> Vec<SvgDataSet> {
    let mut sets: BTreeMap<u32, SvgDataSet> = BTreeMap::new();
    for source in sources {
        let parts: Vec<&str> = source.field.field_name.split('.').collect();
        let Some(index) = (match parts.as_slice() {
            ["ds", set, ..] => index_part(set),
            _ => None,
        }) else {
            continue;
        };
        let set = sets.entry(index).or_insert_with(|| SvgDataSet {
            index,
            ..SvgDataSet::default()
        });
        match &source.kind {
            SourceKind::HostPattern { pattern, .. } => set.host_patterns.push(pattern.clone()),
            SourceKind::ItemPattern { pattern, .. } => set.item_patterns.push(pattern.clone()),
            SourceKind::Item { itemid } => set.itemids.push(itemid.clone()),
            SourceKind::WidgetReference { .. } => set.item_references += 1,
            _ => {}
        }
    }
    if let Some(fields) = raw_widget
        .and_then(|widget| widget.get("fields"))
        .and_then(Value::as_array)
    {
        for field in fields {
            let Some(name) = field.get("name").and_then(Value::as_str) else {
                continue;
            };
            let parts: Vec<&str> = name.split('.').collect();
            if let ["ds", set, "dataset_type"] = parts.as_slice()
                && let Some(index) = index_part(set)
                && let Some(dataset) = sets.get_mut(&index)
            {
                dataset.dataset_type = field.get("value").and_then(|value| {
                    value
                        .as_i64()
                        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
                });
            }
        }
    }
    sets.into_values().collect()
}
