// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-02 resolver tests on synthetic fixtures: no real hostnames, addresses
//! or dashboards. The lookup fixture answers like `host.get`, `item.get` and
//! `graph.get` would: hidden objects are simply absent.

use std::future::{Future, ready};
use std::sync::Mutex;

use serde_json::{Value, json};
use zdashstitch_zabbix::{
    GraphDetails, GraphPrototypeDetails, HostDetails, HostObjects, ItemDetails, ItemDiscoveryLink,
    ItemFlags, ItemPrototypeDetails, VersionedDashboardSnapshot, WidgetCapabilityRegistry,
    ZabbixError,
};

use super::*;

pub(crate) const SOURCE: &str = "11";
pub(crate) const TARGET: &str = "12";

pub(crate) fn host(hostid: &str, name: &str) -> HostDetails {
    HostDetails {
        hostid: hostid.to_owned(),
        host: name.replace(['-', ' '], "_"),
        name: name.to_owned(),
    }
}

pub(crate) fn item(itemid: &str, hostid: &str, key: &str, name: &str) -> ItemDetails {
    ItemDetails {
        itemid: itemid.to_owned(),
        hostid: hostid.to_owned(),
        key: key.to_owned(),
        name: name.to_owned(),
        value_type: 3,
        units: String::new(),
        flags: ItemFlags::Plain,
        templateid: None,
        discovery: None,
        owner_name: None,
    }
}

pub(crate) fn typed(mut item: ItemDetails, value_type: u8, units: &str) -> ItemDetails {
    item.value_type = value_type;
    item.units = units.to_owned();
    item
}

pub(crate) fn templated(mut item: ItemDetails, templateid: &str) -> ItemDetails {
    item.templateid = Some(templateid.to_owned());
    item
}

pub(crate) fn discovered(mut item: ItemDetails, parent: &str, prototype_key: &str) -> ItemDetails {
    item.flags = ItemFlags::Discovered;
    item.discovery = Some(ItemDiscoveryLink {
        parent_itemid: parent.to_owned(),
        prototype_key: prototype_key.to_owned(),
    });
    item
}

pub(crate) fn graph(
    graphid: &str,
    name: &str,
    templateid: Option<&str>,
    hostid: &str,
) -> GraphDetails {
    GraphDetails {
        graphid: graphid.to_owned(),
        name: name.to_owned(),
        templateid: templateid.map(str::to_owned),
        discovered: false,
        hostids: vec![hostid.to_owned()],
    }
}

#[derive(Default)]
pub(crate) struct Fixture {
    pub(crate) hosts: Vec<HostDetails>,
    pub(crate) items: Vec<ItemDetails>,
    pub(crate) graphs: Vec<GraphDetails>,
    pub(crate) item_prototypes: Vec<ItemPrototypeDetails>,
    pub(crate) graph_prototypes: Vec<GraphPrototypeDetails>,
    pub(crate) calls: Mutex<Vec<&'static str>>,
}

impl Fixture {
    fn record(&self, call: &'static str) {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(call);
    }

    pub(crate) fn calls(&self) -> Vec<&'static str> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

type Answer<T> = Result<T, ZabbixError>;

impl ResolverLookup for Fixture {
    fn hosts_by_ids(
        &self,
        hostids: &[String],
    ) -> impl Future<Output = Answer<Vec<HostDetails>>> + Send {
        self.record("host.get:ids");
        ready(Ok(self
            .hosts
            .iter()
            .filter(|host| hostids.contains(&host.hostid))
            .cloned()
            .collect()))
    }

    fn hosts_by_visible_names(
        &self,
        names: &[String],
    ) -> impl Future<Output = Answer<Vec<HostDetails>>> + Send {
        self.record("host.get:names");
        assert!(
            names.iter().all(|name| !name.contains('*')),
            "no mask lookups"
        );
        ready(Ok(self
            .hosts
            .iter()
            .filter(|host| names.iter().any(|name| same_visible_name(name, &host.name)))
            .cloned()
            .collect()))
    }

    fn items_by_ids(
        &self,
        itemids: &[String],
    ) -> impl Future<Output = Answer<Vec<ItemDetails>>> + Send {
        self.record("item.get:ids");
        ready(Ok(self
            .items
            .iter()
            .filter(|item| itemids.contains(&item.itemid))
            .cloned()
            .collect()))
    }

    fn host_items(
        &self,
        hostid: &str,
    ) -> impl Future<Output = Answer<HostObjects<ItemDetails>>> + Send {
        self.record("item.get:host");
        ready(Ok(HostObjects {
            objects: self
                .items
                .iter()
                .filter(|item| item.hostid == hostid)
                .cloned()
                .collect(),
            truncated: false,
        }))
    }

    fn graphs_by_ids(
        &self,
        graphids: &[String],
    ) -> impl Future<Output = Answer<Vec<GraphDetails>>> + Send {
        self.record("graph.get:ids");
        ready(Ok(self
            .graphs
            .iter()
            .filter(|graph| graphids.contains(&graph.graphid))
            .cloned()
            .collect()))
    }

    fn host_graphs(
        &self,
        hostid: &str,
    ) -> impl Future<Output = Answer<HostObjects<GraphDetails>>> + Send {
        self.record("graph.get:host");
        ready(Ok(HostObjects {
            objects: self
                .graphs
                .iter()
                .filter(|graph| graph.hostids.iter().any(|id| id == hostid))
                .cloned()
                .collect(),
            truncated: false,
        }))
    }

    fn item_prototypes_by_ids(
        &self,
        itemids: &[String],
    ) -> impl Future<Output = Answer<Vec<ItemPrototypeDetails>>> + Send {
        self.record("itemprototype.get:ids");
        ready(Ok(self
            .item_prototypes
            .iter()
            .filter(|prototype| itemids.contains(&prototype.itemid))
            .cloned()
            .collect()))
    }

    fn graph_prototypes_by_ids(
        &self,
        graphids: &[String],
    ) -> impl Future<Output = Answer<Vec<GraphPrototypeDetails>>> + Send {
        self.record("graphprototype.get:ids");
        ready(Ok(self
            .graph_prototypes
            .iter()
            .filter(|prototype| graphids.contains(&prototype.graphid))
            .cloned()
            .collect()))
    }
}

/// Synthetic server: source host 11, target host 12, an unrelated host 13
/// and a hidden item 140 that no lookup returns.
pub(crate) fn fixture() -> Fixture {
    let mut template_item = item("901", "800", "pgsql.ping", "Ping");
    template_item.owner_name = Some("Synthetic PostgreSQL template".to_owned());
    Fixture {
        hosts: vec![
            host(SOURCE, "db-old-01"),
            host(TARGET, "db-new-01"),
            host("13", "shared-srv"),
            host("15", "odd*name"),
        ],
        items: vec![
            template_item,
            // Source host.
            templated(item("101", SOURCE, "pgsql.ping", "PostgreSQL ping"), "901"),
            templated(
                typed(
                    item("102", SOURCE, "system.cpu.util", "CPU utilization"),
                    0,
                    "%",
                ),
                "902",
            ),
            discovered(
                typed(
                    item("103", SOURCE, "pgsql.db.size[proddb]", "DB proddb size"),
                    3,
                    "B",
                ),
                "501",
                "pgsql.db.size[{#DBNAME}]",
            ),
            typed(
                item("104", SOURCE, "custom.metric", "Custom metric"),
                0,
                "ms",
            ),
            templated(
                item("105", SOURCE, "pgsql.connections", "Connections"),
                "905",
            ),
            templated(
                typed(item("106", SOURCE, "pgsql.uptime", "Uptime"), 3, "s"),
                "906",
            ),
            templated(
                typed(item("107", SOURCE, "pgsql.cache.hit", "Cache hit"), 0, "%"),
                "907",
            ),
            typed(
                item("108", SOURCE, "system.cpu.util[,user]", "CPU user time"),
                0,
                "%",
            ),
            // Target host.
            templated(item("201", TARGET, "pgsql.ping", "PostgreSQL ping"), "901"),
            templated(
                typed(
                    item("202", TARGET, "system.cpu.util", "CPU utilization"),
                    0,
                    "%",
                ),
                "902",
            ),
            discovered(
                typed(
                    item("203", TARGET, "pgsql.db.size[stagedb]", "DB stagedb size"),
                    3,
                    "B",
                ),
                "601",
                "pgsql.db.size[{#DBNAME}]",
            ),
            discovered(
                typed(
                    item("204", TARGET, "pgsql.db.size[testdb]", "DB testdb size"),
                    3,
                    "B",
                ),
                "601",
                "pgsql.db.size[{#DBNAME}]",
            ),
            templated(
                typed(
                    item("205", TARGET, "pgsql.connections", "Connections"),
                    4,
                    "",
                ),
                "905",
            ),
            templated(
                typed(item("206", TARGET, "pgsql.uptime", "Uptime"), 3, "uptime"),
                "906",
            ),
            templated(
                typed(item("207", TARGET, "pgsql.cache.hit", "Cache hit"), 0, "%"),
                "999",
            ),
            typed(
                item("208", TARGET, "system.cpu.util[,user]", "CPU user time"),
                0,
                "%",
            ),
            typed(
                item("209", TARGET, "system.cpu.util[,system]", "CPU system time"),
                0,
                "%",
            ),
            // Unrelated host.
            typed(
                item("130", "13", "system.cpu.util", "CPU utilization"),
                0,
                "%",
            ),
        ],
        graphs: vec![
            graph("301", "PostgreSQL load", Some("701"), SOURCE),
            graph("302", "Custom graph", None, SOURCE),
            graph("401", "PostgreSQL load", Some("701"), TARGET),
        ],
        item_prototypes: vec![
            item_prototype("501", SOURCE, Some("510"), None),
            item_prototype("601", TARGET, Some("510"), None),
            item_prototype("510", "800", None, Some("Synthetic PostgreSQL template")),
        ],
        graph_prototypes: Vec::new(),
        calls: Mutex::new(Vec::new()),
    }
}

pub(crate) fn field(kind: &str, name: &str, value: &str) -> Value {
    json!({"type": kind, "name": name, "value": value})
}

pub(crate) fn widget(widgetid: &str, widget_type: &str, fields: Vec<Value>) -> Value {
    json!({
        "widgetid": widgetid, "type": widget_type, "name": "", "view_mode": "0",
        "x": "0", "y": "0", "width": "12", "height": "4", "fields": fields
    })
}

pub(crate) fn dashboard() -> Value {
    json!({
        "dashboardid": "900",
        "name": "Synthetic resolver dashboard",
        "pages": [
            {"dashboard_pageid": "1000", "name": "Main", "widgets": [
                widget("2000", "item", vec![
                    field("4", "itemid.0", "101"),
                    field("1", "reference", "AAAAA"),
                ]),
                widget("2001", "svggraph", vec![
                    field("1", "ds.0.hosts.0", "DB-OLD-01"),
                    field("1", "ds.0.items.0", "CPU*"),
                    field("0", "ds.1.dataset_type", "0"),
                    field("4", "ds.1.itemids.0", "102"),
                    field("4", "ds.1.itemids.1", "103"),
                    field("1", "ds.2.hosts.0", "db-old-*"),
                    field("1", "ds.2.items.0", "PostgreSQL ping"),
                    field("1", "ds.3.hosts.0", "*"),
                    field("1", "ds.3.items.0", "Uptime"),
                    field("1", "ds.4.hosts.0", "shared-srv"),
                    field("1", "ds.4.items.0", "CPU utilization"),
                    field("1", "problemhosts.0", "db-old-01"),
                    field("1", "or.0.hosts.0", "db-old-01"),
                    field("1", "or.0.items.0", "CPU*"),
                ]),
                widget("2002", "item", vec![field("4", "itemid.0", "104")]),
                widget("2003", "item", vec![field("4", "itemid.0", "105")]),
                widget("2004", "item", vec![field("4", "itemid.0", "106")]),
                widget("2005", "item", vec![field("4", "itemid.0", "107")]),
                widget("2006", "item", vec![field("4", "itemid.0", "130")]),
                widget("2007", "item", vec![field("4", "itemid.0", "140")]),
                widget("2008", "item", vec![field("4", "itemid.0", "0")]),
            ]},
            {"dashboard_pageid": "1001", "name": "Other", "widgets": [
                widget("2100", "problems", vec![
                    field("3", "hostids.0", SOURCE),
                    field("1", "override_hostid._reference", "BBBBB._hostid"),
                ]),
                widget("2101", "tophosts", vec![field("3", "hostids.0", SOURCE)]),
                widget("2102", "graph", vec![field("6", "graphid.0", "301")]),
                widget("2103", "graph", vec![field("6", "graphid.0", "302")]),
                widget("2104", "clock", vec![
                    field("4", "itemid.0", "101"),
                    field("1", "reference", "DDDDD"),
                ]),
                widget("2105", "vendor.widget", vec![
                    field("4", "itemid.0", "102"),
                    field("1", "reference", "DDDDD"),
                ]),
                widget("2106", "graphprototype", vec![field("7", "graphid.0", "350")]),
                widget("2107", "item", vec![
                    field("4", "itemid.0", "201"),
                    field("1", "override_hostid._reference", "AAAAA._hostid"),
                    field("1", "time_period._reference", "DASHBOARD._timeperiod"),
                ]),
            ]}
        ]
    })
}

fn snapshot(raw: Value) -> VersionedDashboardSnapshot {
    let version = "7.4.16"
        .parse()
        .unwrap_or_else(|error| panic!("version: {error}"));
    VersionedDashboardSnapshot::from_adapter_raw("conn-test".to_owned(), version, raw)
        .unwrap_or_else(|error| panic!("snapshot: {error}"))
}

fn model() -> SourceModel {
    source_model(&snapshot(dashboard()), &WidgetCapabilityRegistry::v7_4())
        .unwrap_or_else(|error| panic!("model: {error}"))
}

fn request(scope: ResolveScope) -> ReplaceHostRequest {
    ReplaceHostRequest {
        source_hostid: SOURCE.to_owned(),
        target_hostid: TARGET.to_owned(),
        scope,
    }
}

async fn resolve_with(fixture: &Fixture, request: &ReplaceHostRequest) -> ReplaceHostResolution {
    resolve_replace_host_with(fixture, &model(), request)
        .await
        .unwrap_or_else(|error| panic!("resolve: {error}"))
}

fn widget_sources<'a>(model: &'a SourceModel, widgetid: &str) -> &'a WidgetSources {
    model
        .widgets
        .iter()
        .find(|widget| widget.widget.widgetid.as_deref() == Some(widgetid))
        .unwrap_or_else(|| panic!("widget {widgetid}"))
}

fn row<'a>(
    resolution: &'a ReplaceHostResolution,
    widgetid: &str,
    field: &str,
) -> &'a ResolutionRow {
    resolution
        .rows
        .iter()
        .find(|row| {
            row.field.widget.widgetid.as_deref() == Some(widgetid) && row.field.field_name == field
        })
        .unwrap_or_else(|| panic!("row {widgetid}/{field}"))
}

fn kept<'a>(
    resolution: &'a ReplaceHostResolution,
    widgetid: &str,
    field: &str,
) -> &'a KeptOnSource {
    resolution
        .kept_on_source
        .iter()
        .find(|kept| {
            kept.field.widget.widgetid.as_deref() == Some(widgetid)
                && kept.field.field_name == field
        })
        .unwrap_or_else(|| panic!("kept {widgetid}/{field}"))
}

fn has_row(resolution: &ReplaceHostResolution, widgetid: &str, field: &str) -> bool {
    resolution.rows.iter().any(|row| {
        row.field.widget.widgetid.as_deref() == Some(widgetid) && row.field.field_name == field
    })
}

// ---------- P2-02A: source model ----------

#[test]
fn a_model_addresses_every_source_in_base() {
    let model = model();
    assert_eq!(model.dashboardid, "900");
    assert_eq!(model.page_count, 2);
    assert_eq!(model.widgets.len(), 17);
    let item = widget_sources(&model, "2000");
    assert_eq!(item.support, WidgetSupport::Editable);
    assert_eq!(item.widget.page_index, 0);
    assert_eq!(item.widget.dashboard_pageid.as_deref(), Some("1000"));
    let source = &item.sources[0];
    assert_eq!(source.field.field_name, "itemid.0");
    assert_eq!(
        source.kind,
        SourceKind::Item {
            itemid: "101".to_owned()
        }
    );
    assert_eq!(source.applicability, Applicability::Editable);

    let graph = widget_sources(&model, "2102");
    assert_eq!(graph.widget.page_index, 1);
    assert_eq!(graph.widget.widget_index, 2);
    assert_eq!(
        graph.sources[0].kind,
        SourceKind::Graph {
            graphid: "301".to_owned()
        }
    );
    assert_eq!(
        widget_sources(&model, "2105").support,
        WidgetSupport::Unknown
    );
    assert_eq!(
        widget_sources(&model, "2106").sources[0].kind,
        SourceKind::GraphPrototype {
            graphid: "350".to_owned()
        }
    );
    // An unset item field (`0`) is not a source.
    assert!(widget_sources(&model, "2008").sources.is_empty());
    assert_eq!(model.itemids().len(), 10);
    assert_eq!(model.graphids().len(), 2);
}

#[test]
fn a_applicability_follows_registry_and_o003() {
    let model = model();
    let applicability = |widgetid: &str, field: &str| {
        widget_sources(&model, widgetid)
            .sources
            .iter()
            .find(|source| source.field.field_name == field)
            .map(|source| (source.applicability, source.applicability_reason))
            .unwrap_or_else(|| panic!("{widgetid}/{field}"))
    };
    assert_eq!(
        applicability("2001", "ds.1.itemids.0"),
        (Applicability::Editable, None)
    );
    assert_eq!(
        applicability("2001", "ds.0.hosts.0"),
        (Applicability::Editable, None)
    );
    assert_eq!(
        applicability("2001", "problemhosts.0"),
        (
            Applicability::ReadOnly,
            Some(ReasonCode::PatternFieldNotSupported)
        )
    );
    assert_eq!(
        applicability("2001", "or.0.hosts.0"),
        (
            Applicability::ReadOnly,
            Some(ReasonCode::PatternFieldNotSupported)
        )
    );
    assert_eq!(
        applicability("2100", "hostids.0"),
        (
            Applicability::ReadOnly,
            Some(ReasonCode::HostFieldNotSupported)
        )
    );
    assert_eq!(
        applicability("2104", "itemid.0"),
        (Applicability::ReadOnly, Some(ReasonCode::WidgetNotEditable))
    );
    // LAB-02: `classic_graph_graphid` is `Supported` on 7.4.16.
    assert_eq!(
        applicability("2102", "graphid.0"),
        (Applicability::Editable, None)
    );
    assert_eq!(
        applicability("2105", "itemid.0"),
        (Applicability::Unsupported, Some(ReasonCode::UnknownWidget))
    );
}

// ---------- P2-02B: direct item IDs ----------

#[tokio::test]
async fn b_direct_items_in_item_and_svggraph_resolve_by_exact_key() {
    let fixture = fixture();
    let resolution = resolve_with(&fixture, &request(ResolveScope::Dashboard)).await;
    let item_row = row(&resolution, "2000", "itemid.0");
    assert_eq!(item_row.match_status, MatchStatus::Exact);
    assert_eq!(item_row.applicability, Applicability::Editable);
    assert_eq!(
        item_row.target,
        Some(TargetValue::Item {
            itemid: "201".to_owned()
        })
    );
    assert!(item_row.will_change());
    match &item_row.target_evidence {
        Some(Evidence::Item(evidence)) => {
            assert_eq!(evidence.key, "pgsql.ping");
            assert_eq!(
                evidence.origin,
                ItemOrigin::Template {
                    template_itemid: "901".to_owned(),
                    template_name: Some("Synthetic PostgreSQL template".to_owned()),
                }
            );
        }
        other => panic!("item evidence expected: {other:?}"),
    }
    let series = row(&resolution, "2001", "ds.1.itemids.0");
    assert_eq!(series.match_status, MatchStatus::Exact);
    assert!(series.will_change());
    // An item of another host is not part of the replacement.
    assert!(!has_row(&resolution, "2006", "itemid.0"));
}

#[tokio::test]
async fn b_missing_item_is_missing_not_matched_by_name() {
    let mut fixture = fixture();
    // Same visible name on the target, different key: not evidence.
    fixture.items.push(typed(
        item("210", TARGET, "custom.metric.v2", "Custom metric"),
        0,
        "ms",
    ));
    let resolution = resolve_with(&fixture, &request(ResolveScope::Dashboard)).await;
    let missing = row(&resolution, "2002", "itemid.0");
    assert_eq!(missing.match_status, MatchStatus::Missing);
    assert_eq!(missing.target, None);
    assert_eq!(missing.reasons, vec![ReasonCode::KeyNotFound]);
    assert!(missing.needs_decision());
    assert!(!missing.will_change());
}

#[tokio::test]
async fn b_incompatible_value_type_and_units_are_reported() {
    let fixture = fixture();
    let resolution = resolve_with(&fixture, &request(ResolveScope::Dashboard)).await;

    let incompatible = row(&resolution, "2003", "itemid.0");
    assert_eq!(incompatible.match_status, MatchStatus::Compatible);
    assert_eq!(incompatible.differences, vec![Difference::ValueType]);
    assert!(
        incompatible
            .reasons
            .contains(&ReasonCode::ValueTypeIncompatible)
    );
    assert!(
        !incompatible.will_change(),
        "numeric → text needs a decision"
    );
    assert!(incompatible.needs_decision());

    let units = row(&resolution, "2004", "itemid.0");
    assert_eq!(units.match_status, MatchStatus::Compatible);
    assert_eq!(units.differences, vec![Difference::Units]);
    assert_eq!(units.reasons, vec![ReasonCode::UnitsDiffer]);
    assert!(units.will_change(), "units only warn");

    let origin = row(&resolution, "2005", "itemid.0");
    assert_eq!(origin.match_status, MatchStatus::Compatible);
    assert_eq!(origin.differences, vec![Difference::Origin]);
}

#[tokio::test]
async fn b_svggraph_series_needs_a_numeric_target() {
    let mut fixture = fixture();
    for item in &mut fixture.items {
        if item.itemid == "202" {
            item.value_type = 1;
        }
    }
    let resolution = resolve_with(&fixture, &request(ResolveScope::Dashboard)).await;
    let series = row(&resolution, "2001", "ds.1.itemids.0");
    assert!(series.reasons.contains(&ReasonCode::ValueTypeIncompatible));
    assert!(!series.will_change());
}

// ---------- P2-02C: svggraph series ----------

#[tokio::test]
async fn c_exact_host_pattern_becomes_target_visible_name_with_coverage() {
    let fixture = fixture();
    let resolution = resolve_with(&fixture, &request(ResolveScope::Dashboard)).await;
    // `DB-OLD-01` equals the visible name `db-old-01` case-insensitively.
    let pattern = row(&resolution, "2001", "ds.0.hosts.0");
    assert_eq!(
        pattern.target,
        Some(TargetValue::HostPattern {
            pattern: "db-new-01".to_owned()
        })
    );
    assert_eq!(pattern.applicability, Applicability::Editable);
    // `CPU*` matches 2 items on the source and 3 on the target.
    assert_eq!(pattern.match_status, MatchStatus::Compatible);
    assert_eq!(pattern.differences, vec![Difference::PatternCoverage]);
    assert_eq!(pattern.pattern_coverage.len(), 1);
    let coverage = &pattern.pattern_coverage[0];
    assert_eq!((coverage.source_matches, coverage.target_matches), (2, 3));
    assert_eq!(coverage.target_candidates.len(), 3);
    assert!(pattern.will_change());
    // Patterns never become item IDs: no row targets an item for a pattern
    // field, and item patterns themselves produce no rows.
    assert!(!has_row(&resolution, "2001", "ds.0.items.0"));
    assert!(resolution.rows.iter().all(|row| {
        !matches!(row.source, SourceValue::HostPattern { .. })
            || !matches!(row.target, Some(TargetValue::Item { .. }))
    }));
    // A pattern of another host is untouched.
    assert!(!has_row(&resolution, "2001", "ds.4.hosts.0"));
}

#[tokio::test]
async fn c_item_patterns_without_target_items_are_missing() {
    let mut fixture = fixture();
    fixture
        .items
        .retain(|item| !(item.hostid == TARGET && item.name.starts_with("CPU")));
    let resolution = resolve_with(&fixture, &request(ResolveScope::Dashboard)).await;
    let pattern = row(&resolution, "2001", "ds.0.hosts.0");
    assert_eq!(pattern.match_status, MatchStatus::Missing);
    assert!(
        pattern
            .reasons
            .contains(&ReasonCode::ItemPatternsWithoutTargetItems)
    );
    assert!(pattern.needs_decision());
}

#[tokio::test]
async fn c_ambiguous_mask_and_wildcard_stay_on_source() {
    let fixture = fixture();
    let resolution = resolve_with(&fixture, &request(ResolveScope::Dashboard)).await;
    let mask = row(&resolution, "2001", "ds.2.hosts.0");
    assert_eq!(mask.match_status, MatchStatus::Ambiguous);
    assert_eq!(mask.applicability, Applicability::ReadOnly);
    assert_eq!(mask.target, None);
    assert!(!mask.will_change() && !mask.needs_decision());
    assert_eq!(
        kept(&resolution, "2001", "ds.2.hosts.0").reason,
        ReasonCode::HostMask
    );
    assert_eq!(
        kept(&resolution, "2001", "ds.3.hosts.0").reason,
        ReasonCode::HostWildcardAll
    );
}

#[tokio::test]
async fn c_target_name_with_wildcard_is_not_written_as_pattern() {
    let fixture = fixture();
    let request = ReplaceHostRequest {
        source_hostid: SOURCE.to_owned(),
        target_hostid: "15".to_owned(),
        scope: ResolveScope::Dashboard,
    };
    let resolution = resolve_with(&fixture, &request).await;
    let pattern = row(&resolution, "2001", "ds.0.hosts.0");
    assert_eq!(pattern.match_status, MatchStatus::Ambiguous);
    assert!(pattern.reasons.contains(&ReasonCode::TargetNameIsMask));
    assert!(!pattern.will_change());
}

// ---------- P2-02D: LLD ----------

#[tokio::test]
async fn d_lld_instance_without_target_instance_is_unresolved_lld() {
    let fixture = fixture();
    let resolution = resolve_with(&fixture, &request(ResolveScope::Dashboard)).await;
    let lld = row(&resolution, "2001", "ds.1.itemids.1");
    assert_eq!(lld.match_status, MatchStatus::UnresolvedLld);
    assert_eq!(lld.target, None, "no invented itemid");
    assert_eq!(lld.reasons, vec![ReasonCode::LldInstanceMissing]);
    let keys: Vec<&str> = lld.candidates.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(
        keys,
        vec!["pgsql.db.size[stagedb]", "pgsql.db.size[testdb]"]
    );
    match &lld.source_evidence {
        Some(Evidence::Item(evidence)) => assert_eq!(
            evidence.origin,
            ItemOrigin::Discovered {
                prototype_key: "pgsql.db.size[{#DBNAME}]".to_owned()
            }
        ),
        other => panic!("item evidence expected: {other:?}"),
    }
    assert!(lld.needs_decision());
}

#[tokio::test]
async fn d_lld_instance_with_same_key_on_target_resolves() {
    let mut fixture = fixture();
    fixture.items.push(discovered(
        typed(
            item("211", TARGET, "pgsql.db.size[proddb]", "DB proddb size"),
            3,
            "B",
        ),
        "601",
        "pgsql.db.size[{#DBNAME}]",
    ));
    let resolution = resolve_with(&fixture, &request(ResolveScope::Dashboard)).await;
    let lld = row(&resolution, "2001", "ds.1.itemids.1");
    assert_eq!(lld.match_status, MatchStatus::Exact);
    assert_eq!(
        lld.target,
        Some(TargetValue::Item {
            itemid: "211".to_owned()
        })
    );
}

#[tokio::test]
async fn d_lld_without_any_instance_has_no_candidates() {
    let mut fixture = fixture();
    fixture
        .items
        .retain(|item| item.flags != ItemFlags::Discovered || item.hostid == SOURCE);
    let resolution = resolve_with(&fixture, &request(ResolveScope::Dashboard)).await;
    let lld = row(&resolution, "2001", "ds.1.itemids.1");
    assert_eq!(lld.match_status, MatchStatus::UnresolvedLld);
    assert!(lld.candidates.is_empty());
    assert_eq!(lld.target, None);
}

#[tokio::test]
async fn d_lld_same_prototype_key_from_another_template_is_not_a_candidate() {
    let mut fixture = fixture();
    fixture
        .item_prototypes
        .iter_mut()
        .find(|prototype| prototype.itemid == "601")
        .unwrap_or_else(|| panic!("target prototype"))
        .templateid = Some("999".to_owned());

    let resolution = resolve_with(&fixture, &request(ResolveScope::Dashboard)).await;
    let lld = row(&resolution, "2001", "ds.1.itemids.1");
    assert_eq!(lld.match_status, MatchStatus::UnresolvedLld);
    assert!(lld.candidates.is_empty(), "same key is not same prototype");
    assert_eq!(lld.target, None);
}

// ---------- P2-02E: links, orphans, kept_on_source ----------

#[test]
fn e_links_report_orphans_duplicates_and_dashboard_provider() {
    let model = model();
    let orphans: Vec<&WidgetLink> = model.links.orphaned().collect();
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].reference, "BBBBB");
    assert_eq!(orphans[0].channel, "hostid");
    assert_eq!(orphans[0].consumer.widgetid.as_deref(), Some("2100"));

    let duplicates = model.links.duplicate_definitions();
    assert_eq!(duplicates.len(), 1);
    assert_eq!(duplicates["DDDDD"].len(), 2);

    let dashboard = model
        .links
        .links
        .iter()
        .find(|link| link.channel == "timeperiod")
        .unwrap_or_else(|| panic!("time period link"));
    assert_eq!(dashboard.provider, LinkProvider::Dashboard);
    let cross_page = model
        .links
        .links
        .iter()
        .find(|link| link.reference == "AAAAA")
        .unwrap_or_else(|| panic!("AAAAA link"));
    match &cross_page.provider {
        LinkProvider::Widget(provider) => assert_eq!(provider.widgetid.as_deref(), Some("2000")),
        other => panic!("widget provider expected: {other:?}"),
    }
}

#[test]
fn e_scope_crossings_find_orphans_created_by_the_scope() {
    let model = model();
    let page = ResolveScope::Pages(["1001".to_owned()].into());
    let crossings = model.links.crossing(&page);
    assert_eq!(crossings.len(), 1);
    assert_eq!(crossings[0].side, CrossingSide::ProviderOutside);
    assert_eq!(crossings[0].link.reference, "AAAAA");

    let first = ResolveScope::Widgets(["2000".to_owned()].into());
    let crossings = model.links.crossing(&first);
    assert_eq!(crossings.len(), 1);
    assert_eq!(crossings[0].side, CrossingSide::ConsumerOutside);
    assert!(model.links.crossing(&ResolveScope::Dashboard).is_empty());
}

#[tokio::test]
async fn e_kept_on_source_lists_every_unchanged_reference() {
    let fixture = fixture();
    let resolution = resolve_with(&fixture, &request(ResolveScope::Dashboard)).await;
    assert_eq!(
        kept(&resolution, "2100", "hostids.0").reason,
        ReasonCode::HostFieldNotSupported
    );
    assert_eq!(
        kept(&resolution, "2101", "hostids.0").reason,
        ReasonCode::HostFieldNotSupported
    );
    assert_eq!(
        kept(&resolution, "2001", "problemhosts.0").reason,
        ReasonCode::PatternFieldNotSupported
    );
    assert_eq!(
        kept(&resolution, "2001", "or.0.hosts.0").reason,
        ReasonCode::PatternFieldNotSupported
    );
    assert_eq!(
        kept(&resolution, "2104", "itemid.0").reason,
        ReasonCode::WidgetNotEditable
    );
    assert_eq!(
        kept(&resolution, "2105", "itemid.0").reason,
        ReasonCode::UnknownWidget
    );
    assert!(
        !resolution
            .kept_on_source
            .iter()
            .any(|kept| kept.widget_type == "graph"),
        "classic graphs are editable on 7.4.16"
    );
    // Editable rows are never in the list.
    assert!(resolution.kept_on_source.iter().all(|kept| {
        row(
            &resolution,
            kept.field.widget.widgetid.as_deref().unwrap_or_default(),
            &kept.field.field_name,
        )
        .applicability
            != Applicability::Editable
    }));
    assert_eq!(resolution.summary.kept_on_source, 8);
}

#[tokio::test]
async fn e_hidden_items_and_prototypes_are_unverified_not_guessed() {
    let fixture = fixture();
    let resolution = resolve_with(&fixture, &request(ResolveScope::Dashboard)).await;
    let reasons: Vec<(&str, ReasonCode)> = resolution
        .unverified
        .iter()
        .map(|source| {
            (
                source.field.widget.widgetid.as_deref().unwrap_or_default(),
                source.reason,
            )
        })
        .collect();
    assert_eq!(
        reasons,
        vec![
            ("2007", ReasonCode::NotFoundOrNoAccess),
            ("2106", ReasonCode::PrototypeOwnerUnknown)
        ]
    );
}

#[tokio::test]
async fn e_scope_limits_rows_and_counts_outside_references() {
    let fixture = fixture();
    let page = ResolveScope::Pages(["1000".to_owned()].into());
    let resolution = resolve_with(&fixture, &request(page)).await;
    assert!(
        resolution
            .rows
            .iter()
            .all(|row| row.field.widget.page_index == 0)
    );
    // Page 1001: 2 host fields, 2 graphs, clock and vendor items.
    assert_eq!(resolution.summary.out_of_scope, 6);
    assert_eq!(resolution.unverified.len(), 1);
}

// ---------- P2-02F: classic graph ----------

#[tokio::test]
async fn f_classic_graph_is_replaced_by_the_same_template_graph_on_7_4_16() {
    let fixture = fixture();
    let resolution = resolve_with(&fixture, &request(ResolveScope::Dashboard)).await;
    let same = row(&resolution, "2102", "graphid.0");
    assert_eq!(same.match_status, MatchStatus::Exact);
    assert_eq!(
        same.target,
        Some(TargetValue::Graph {
            graphid: "401".to_owned()
        })
    );
    assert_eq!(same.applicability, Applicability::Editable);
    assert!(
        same.will_change(),
        "classic_graph_graphid is proven on 7.4.16"
    );

    // No same-name graph on the target: a decision is needed.
    let missing = row(&resolution, "2103", "graphid.0");
    assert_eq!(missing.match_status, MatchStatus::Missing);
    assert!(missing.reasons.contains(&ReasonCode::GraphNameNotFound));
    assert_eq!(missing.applicability, Applicability::Editable);
    assert!(missing.needs_decision());
}

#[tokio::test]
async fn f_classic_graph_stays_unsupported_without_the_contract() {
    // 7.4.17 is outside the laboratory-proven patch range.
    let version = "7.4.17"
        .parse()
        .unwrap_or_else(|error| panic!("version: {error}"));
    let base =
        VersionedDashboardSnapshot::from_adapter_raw("conn-test".to_owned(), version, dashboard())
            .unwrap_or_else(|error| panic!("snapshot: {error}"));
    let model = source_model(&base, &WidgetCapabilityRegistry::v7_4())
        .unwrap_or_else(|error| panic!("model: {error}"));
    let fixture = fixture();
    let resolution = resolve_replace_host_with(&fixture, &model, &request(ResolveScope::Dashboard))
        .await
        .unwrap_or_else(|error| panic!("resolve: {error}"));
    let same = row(&resolution, "2102", "graphid.0");
    assert_eq!(same.match_status, MatchStatus::Exact);
    assert_eq!(same.applicability, Applicability::Unsupported);
    assert!(!same.will_change());
    assert_eq!(
        kept(&resolution, "2102", "graphid.0").reason,
        ReasonCode::ClassicGraphContractPending
    );
    // Item and svggraph rows are not affected by this contract.
    assert_eq!(
        row(&resolution, "2000", "itemid.0").applicability,
        Applicability::Editable
    );
}

#[tokio::test]
async fn f_simple_graph_item_is_kept_on_source() {
    // `source_type=1`: the classic graph widget draws one item. LAB-02
    // proved its unchanged resend, not a replacement of its `itemid`.
    let raw = json!({
        "dashboardid": "920",
        "name": "Synthetic simple graph dashboard",
        "pages": [
            {"dashboard_pageid": "1200", "name": "Graphs", "widgets": [
                widget("3100", "graph", vec![
                    field("0", "source_type", "1"),
                    field("4", "itemid.0", "101"),
                ]),
                widget("3101", "graph", vec![field("6", "graphid.0", "301")]),
            ]}
        ]
    });
    let model = source_model(&snapshot(raw), &WidgetCapabilityRegistry::v7_4())
        .unwrap_or_else(|error| panic!("model: {error}"));
    let fixture = fixture();
    let resolution = resolve_replace_host_with(&fixture, &model, &request(ResolveScope::Dashboard))
        .await
        .unwrap_or_else(|error| panic!("resolve: {error}"));
    let simple = row(&resolution, "3100", "itemid.0");
    assert_eq!(simple.match_status, MatchStatus::Exact);
    assert_eq!(simple.applicability, Applicability::ReadOnly);
    assert!(!simple.will_change());
    assert_eq!(
        kept(&resolution, "3100", "itemid.0").reason,
        ReasonCode::FieldNotReplaceable
    );
    assert!(row(&resolution, "3101", "graphid.0").will_change());
}

#[tokio::test]
async fn f_same_name_graph_from_another_template_is_only_compatible() {
    let mut fixture = fixture();
    for graph in &mut fixture.graphs {
        if graph.graphid == "401" {
            graph.templateid = Some("799".to_owned());
        }
    }
    let resolution = resolve_with(&fixture, &request(ResolveScope::Dashboard)).await;
    let graph_row = row(&resolution, "2102", "graphid.0");
    assert_eq!(graph_row.match_status, MatchStatus::Compatible);
    assert_eq!(graph_row.differences, vec![Difference::Origin]);
    // O-003: only the same-name graph of the same template replaces a
    // classic graph; this one is evidence, never applied automatically.
    assert_eq!(graph_row.applicability, Applicability::Editable);
    assert!(!graph_row.will_change());
    assert!(graph_row.needs_decision());
}

// ---------- P2-02G: target host, evidence, status independence ----------

#[tokio::test]
async fn g_hidden_target_makes_every_row_denied_without_claiming_permissions() {
    let mut fixture = fixture();
    fixture.hosts.retain(|host| host.hostid != TARGET);
    let resolution = resolve_with(&fixture, &request(ResolveScope::Dashboard)).await;
    assert_eq!(resolution.target_host, None);
    assert!(!resolution.rows.is_empty());
    for row in &resolution.rows {
        if row.applicability == Applicability::ReadOnly
            && matches!(row.source, SourceValue::HostPattern { ref pattern } if is_mask(pattern))
        {
            continue;
        }
        assert_eq!(row.match_status, MatchStatus::Denied, "{:?}", row.field);
        assert!(row.reasons.contains(&ReasonCode::NotFoundOrNoAccess));
        assert_eq!(row.target, None);
    }
    // No items or graphs of an invisible host are read.
    assert!(!fixture.calls().contains(&"item.get:host"));
    assert!(!fixture.calls().contains(&"graph.get:host"));
}

#[tokio::test]
async fn g_match_and_applicability_are_independent() {
    let fixture = fixture();
    let resolution = resolve_with(&fixture, &request(ResolveScope::Dashboard)).await;
    // Exact but not editable (clock), missing but editable (item 104),
    // exact and unknown (vendor widget).
    let clock = row(&resolution, "2104", "itemid.0");
    assert_eq!(
        (clock.match_status, clock.applicability),
        (MatchStatus::Exact, Applicability::ReadOnly)
    );
    let missing = row(&resolution, "2002", "itemid.0");
    assert_eq!(
        (missing.match_status, missing.applicability),
        (MatchStatus::Missing, Applicability::Editable)
    );
    let vendor = row(&resolution, "2105", "itemid.0");
    assert_eq!(
        (vendor.match_status, vendor.applicability),
        (MatchStatus::Exact, Applicability::Unsupported)
    );
    let summary = resolution.summary;
    assert_eq!(
        summary.rows,
        summary.kept_on_source + summary.will_change + summary.needs_decision
    );
}

#[tokio::test]
async fn g_rejects_invalid_or_identical_hosts_and_hidden_source() {
    let fixture = fixture();
    let model = model();
    for (source, target, code) in [
        ("11", "11", "replace_host_same_host"),
        ("x", "12", "resolver_hostid_invalid"),
        ("16", "12", "source_host_not_found_or_no_access"),
    ] {
        let request = ReplaceHostRequest {
            source_hostid: source.to_owned(),
            target_hostid: target.to_owned(),
            scope: ResolveScope::Dashboard,
        };
        let error = resolve_replace_host_with(&fixture, &model, &request)
            .await
            .err()
            .unwrap_or_else(|| panic!("{source}->{target} must fail"));
        assert_eq!(error.code(), code);
    }
}

#[tokio::test]
async fn g_inventory_lists_referenced_hosts_with_counts() {
    let fixture = fixture();
    let model = model();
    let facts = collect_source_facts(&fixture, &model)
        .await
        .unwrap_or_else(|error| panic!("facts: {error}"));
    let inventory = host_inventory(&model, &facts);
    let source = inventory
        .hosts
        .first()
        .unwrap_or_else(|| panic!("source host first"));
    assert_eq!(source.hostid, SOURCE);
    assert_eq!(source.host_fields, 2);
    assert_eq!(source.graphs, 2);
    assert_eq!(source.host_patterns, 3);
    assert_eq!(source.host_masks, 2);
    assert!(inventory.hosts.iter().any(|host| host.hostid == "13"));
    assert!(inventory.hosts.iter().any(|host| host.hostid == TARGET));
    assert_eq!(inventory.unattributed.len(), 2);
}

#[tokio::test]
async fn g_lookups_are_reads_only() {
    let fixture = fixture();
    let _ = resolve_with(&fixture, &request(ResolveScope::Dashboard)).await;
    let calls = fixture.calls();
    assert!(calls.iter().all(|call| call.starts_with("host.get")
        || call.starts_with("item.get")
        || call.starts_with("graph.get")
        || call.starts_with("itemprototype.get")
        || call.starts_with("graphprototype.get")));
}

// ---------- ALLOWLIST-01: owners of prototype fields ----------

fn item_prototype(
    itemid: &str,
    hostid: &str,
    templateid: Option<&str>,
    owner_name: Option<&str>,
) -> ItemPrototypeDetails {
    ItemPrototypeDetails {
        itemid: itemid.to_owned(),
        hostid: hostid.to_owned(),
        key: "pgsql.db.size[{#DBNAME}]".to_owned(),
        name: "DB {#DBNAME} size".to_owned(),
        value_type: 3,
        units: "B".to_owned(),
        templateid: templateid.map(str::to_owned),
        rule_itemid: Some("400".to_owned()),
        owner_name: owner_name.map(str::to_owned),
    }
}

fn graph_prototype(graphid: &str, hostid: &str, templateid: Option<&str>) -> GraphPrototypeDetails {
    GraphPrototypeDetails {
        graphid: graphid.to_owned(),
        name: "DB {#DBNAME}".to_owned(),
        templateid: templateid.map(str::to_owned),
        hostids: vec![hostid.to_owned()],
        rule_itemid: Some("400".to_owned()),
        owner_name: None,
    }
}

fn prototype_dashboard() -> Value {
    json!({
        "dashboardid": "910",
        "name": "Synthetic prototype dashboard",
        "pages": [
            {"dashboard_pageid": "1100", "name": "Prototypes", "widgets": [
                // Inherited item prototype of the source host.
                widget("3000", "vendor.widget", vec![field("5", "itemid.0", "501")]),
                // Prototype of another host.
                widget("3001", "vendor.widget", vec![field("5", "itemid.0", "502")]),
                // Prototype owned by a template, not by the source host.
                widget("3002", "vendor.widget", vec![field("5", "itemid.0", "503")]),
                widget("3003", "graphprototype", vec![field("7", "graphid.0", "351")]),
                widget("3004", "graphprototype", vec![field("7", "graphid.0", "352")]),
                // Hidden or deleted: no lookup returns it.
                widget("3005", "graphprototype", vec![field("7", "graphid.0", "350")]),
                widget("3006", "vendor.widget", vec![field("5", "itemid.0", "590")]),
            ]}
        ]
    })
}

fn prototype_fixture() -> Fixture {
    Fixture {
        hosts: vec![
            host(SOURCE, "db-old-01"),
            host(TARGET, "db-new-01"),
            host("13", "shared-srv"),
        ],
        item_prototypes: vec![
            item_prototype("501", SOURCE, Some("510"), None),
            item_prototype("502", "13", None, None),
            item_prototype("503", "800", None, Some("Synthetic template")),
            item_prototype("510", "800", None, Some("Synthetic PostgreSQL template")),
        ],
        graph_prototypes: vec![
            graph_prototype("351", SOURCE, Some("650")),
            graph_prototype("352", "13", None),
        ],
        ..Fixture::default()
    }
}

#[tokio::test]
async fn g_prototype_owner_is_read_and_source_host_prototypes_are_kept() {
    let fixture = prototype_fixture();
    let model = source_model(
        &snapshot(prototype_dashboard()),
        &WidgetCapabilityRegistry::v7_4(),
    )
    .unwrap_or_else(|error| panic!("model: {error}"));
    let resolution = resolve_replace_host_with(&fixture, &model, &request(ResolveScope::Dashboard))
        .await
        .unwrap_or_else(|error| panic!("resolve: {error}"));

    // Only prototypes of the source host become rows; they are never changed.
    let fields: Vec<&str> = resolution
        .rows
        .iter()
        .map(|row| row.field.widget.widgetid.as_deref().unwrap_or_default())
        .collect();
    assert_eq!(fields, ["3000", "3003"]);
    for row in &resolution.rows {
        assert_ne!(row.applicability, Applicability::Editable);
        assert!(!row.will_change());
        assert!(row.is_kept_on_source());
        assert!(row.target.is_none());
        assert!(!row.reasons.is_empty(), "a kept row always says why");
    }
    let item_row = row(&resolution, "3000", "itemid.0");
    assert_eq!(
        item_row.source,
        SourceValue::ItemPrototype {
            itemid: "501".to_owned()
        }
    );
    let Some(Evidence::Item(evidence)) = &item_row.source_evidence else {
        panic!("item prototype evidence");
    };
    assert_eq!(evidence.key, "pgsql.db.size[{#DBNAME}]");
    assert_eq!(
        evidence.origin,
        ItemOrigin::Template {
            template_itemid: "510".to_owned(),
            template_name: Some("Synthetic PostgreSQL template".to_owned()),
        }
    );
    let graph_row = row(&resolution, "3003", "graphid.0");
    assert_eq!(
        graph_row.source,
        SourceValue::GraphPrototype {
            graphid: "351".to_owned()
        }
    );
    let Some(Evidence::Graph(graph_evidence)) = &graph_row.source_evidence else {
        panic!("graph prototype evidence");
    };
    assert_eq!(graph_evidence.template_graphid.as_deref(), Some("650"));
    assert_eq!(
        kept(&resolution, "3000", "itemid.0").source,
        item_row.source
    );

    // Invisible prototypes stay unverified, never guessed.
    let unverified: Vec<(&str, ReasonCode)> = resolution
        .unverified
        .iter()
        .map(|source| {
            (
                source.field.widget.widgetid.as_deref().unwrap_or_default(),
                source.reason,
            )
        })
        .collect();
    assert_eq!(
        unverified,
        vec![
            ("3005", ReasonCode::PrototypeOwnerUnknown),
            ("3006", ReasonCode::PrototypeOwnerUnknown)
        ]
    );

    // Only read lookups, each prototype kind once (plus the template owner).
    let calls = fixture.calls();
    assert!(calls.iter().all(|call| call.contains(".get:")));
    assert_eq!(
        calls
            .iter()
            .filter(|call| **call == "graphprototype.get:ids")
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| **call == "itemprototype.get:ids")
            .count(),
        2,
        "prototypes, then the template prototypes of inherited ones"
    );
}

#[tokio::test]
async fn g_prototypes_are_attributed_to_their_host_in_the_inventory() {
    let fixture = prototype_fixture();
    let model = source_model(
        &snapshot(prototype_dashboard()),
        &WidgetCapabilityRegistry::v7_4(),
    )
    .unwrap_or_else(|error| panic!("model: {error}"));
    let facts = collect_source_facts(&fixture, &model)
        .await
        .unwrap_or_else(|error| panic!("facts: {error}"));
    let inventory = host_inventory(&model, &facts);
    let by_host = |hostid: &str| {
        inventory
            .hosts
            .iter()
            .find(|host| host.hostid == hostid)
            .unwrap_or_else(|| panic!("host {hostid}"))
    };
    assert_eq!((by_host(SOURCE).items, by_host(SOURCE).graphs), (1, 1));
    assert_eq!((by_host("13").items, by_host("13").graphs), (1, 1));
    // Template-owned prototype (503) and the two invisible ones.
    let unattributed: Vec<&str> = inventory
        .unattributed
        .iter()
        .map(|source| source.field.widget.widgetid.as_deref().unwrap_or_default())
        .collect();
    assert_eq!(unattributed, ["3002", "3005", "3006"]);
}
