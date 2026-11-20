// SPDX-License-Identifier: MIT OR Apache-2.0
// Synthetic documents exercise local boundaries only; no 8.0 server contract.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use zdashstitch_zabbix::{
    AdapterRegistry, DependencyKind, DependencyState, VersionedDashboardSnapshot, WidgetFieldType,
    ZabbixError, ZabbixVersion,
};

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/zabbix/8.0")
        .join(name);
    let bytes = fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

fn version(raw: &str) -> ZabbixVersion {
    raw.parse()
        .unwrap_or_else(|error| panic!("parse test version {raw}: {error}"))
}

#[test]
fn synthetic_dashboard_preserves_new_and_opaque_fields_without_authority() {
    let raw = fixture("dashboard-get-synthetic.json");
    let snapshot = VersionedDashboardSnapshot::from_adapter_raw(
        "synthetic-connection".to_owned(),
        version("8.0.0"),
        raw.clone(),
    )
    .unwrap_or_else(|error| panic!("synthetic snapshot: {error}"));
    assert_eq!(snapshot.adapter_id(), "zabbix-8.0");
    assert_eq!(snapshot.raw_api_document(), &raw);
    let analysis = snapshot
        .analyze()
        .unwrap_or_else(|error| panic!("synthetic analysis: {error}"));
    assert_eq!(analysis.widget_count, 7);
    assert_eq!(analysis.page_count, 1);
    assert_eq!(analysis.widgets[3].widget_type, "scatterplot");
    assert_eq!(analysis.widgets[3].state, DependencyState::Unknown);
    assert_eq!(analysis.widgets[4].state, DependencyState::Unknown);
    assert!(
        analysis
            .dependencies
            .iter()
            .all(|dependency| dependency.state != DependencyState::SupportedForModification)
    );
    assert!(
        analysis
            .dependencies
            .iter()
            .any(|dependency| dependency.field_name == "itemid._reference")
    );
    assert!(analysis.dependencies.iter().any(|dependency| {
        dependency.widget_type == "vendor.synthetic"
            && dependency.field_name == "opaque_item"
            && dependency.state == DependencyState::Unknown
    }));
    assert!(
        !analysis
            .dependencies
            .iter()
            .any(|dependency| dependency.field_name == "ds.0.item_tags.0.tag")
    );
    assert_eq!(
        snapshot.raw_api_document()["pages"][0]["widgets"][4]["vendor_extension"]["must_survive"],
        true
    );
    assert!(analysis.dependencies.iter().any(|dependency| {
        dependency.kind == DependencyKind::ItemPrototype
            && dependency.state == DependencyState::DiscoveredReadOnly
    }));
    assert!(analysis.dependencies.iter().any(|dependency| {
        dependency.kind == DependencyKind::GraphPrototype
            && dependency.state == DependencyState::DiscoveredReadOnly
    }));
    let registry = AdapterRegistry::widget_capabilities(&version("8.0.0"))
        .unwrap_or_else(|error| panic!("8.0 registry: {error}"));
    let graph = registry
        .find("svggraph")
        .unwrap_or_else(|| panic!("svggraph"));
    assert!(graph.knows_field("show_hostnames", WidgetFieldType::Integer));
    assert!(graph.knows_field("ds.12.item_tags.4.tag", WidgetFieldType::String));
    assert!(!graph.knows_field("ds.12.item_tags.4.tag", WidgetFieldType::Item));
    assert!(!graph.field_editable("show_hostnames", WidgetFieldType::Integer));
    let classic = registry
        .find("graph")
        .unwrap_or_else(|| panic!("classic graph"));
    assert!(!classic.knows_field("show_hostnames", WidgetFieldType::Integer));
}

#[test]
fn synthetic_inventory_keeps_json_and_lld_distinct_with_experimental_read_gate() {
    let raw = fixture("item-inventory-synthetic.json");
    let items = raw["items"].as_array().unwrap_or_else(|| panic!("items"));
    let prototypes = raw["itemprototypes"]
        .as_array()
        .unwrap_or_else(|| panic!("itemprototypes"));
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["value_type"], "0");
    assert_eq!(items[1]["value_type"], "6");
    assert_eq!(items[2]["flags"], "4");
    assert_eq!(prototypes[0]["value_type"], "6");
    assert_eq!(prototypes[0]["flags"], "2");
    let capabilities = AdapterRegistry::resolve_server(version("8.0.0"))
        .unwrap_or_else(|error| panic!("8.0 connection profile: {error}"));
    assert_eq!(
        capabilities.read_inventory,
        zdashstitch_zabbix::CapabilityState::Experimental
    );
}

#[test]
fn malformed_synthetic_dashboard_cases_fail_closed() {
    let cases = fixture("dashboard-negative-synthetic.json");
    let cases = cases.as_array().unwrap_or_else(|| panic!("cases"));
    assert_eq!(cases.len(), 4);
    for case in cases {
        let name = case["case"].as_str().unwrap_or_else(|| panic!("case name"));
        let raw = case["document"].clone();
        let outcome = VersionedDashboardSnapshot::from_adapter_raw(
            "synthetic-connection".to_owned(),
            version("8.0.0"),
            raw,
        );
        match name {
            "missing_dashboard_id" | "pages_not_array" => {
                assert!(
                    matches!(outcome, Err(ZabbixError::SnapshotShape(_))),
                    "{name}"
                );
            }
            "missing_widget_type" | "fields_not_array" => {
                let snapshot = outcome.unwrap_or_else(|error| panic!("{name}: {error}"));
                assert!(
                    matches!(snapshot.analyze(), Err(ZabbixError::SnapshotShape(_))),
                    "{name}"
                );
            }
            _ => panic!("unasserted negative case: {name}"),
        }
    }
}

#[test]
fn seven_four_fixture_remains_bound_to_its_profile() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/zabbix/7.4/dashboard-get-mixed.json");
    let bytes = fs::read(&path).unwrap_or_else(|error| panic!("read 7.4 fixture: {error}"));
    let mut dashboards: Vec<Value> =
        serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("parse 7.4 fixture: {error}"));
    let raw = dashboards.pop().unwrap_or_else(|| panic!("7.4 dashboard"));
    let snapshot = VersionedDashboardSnapshot::from_adapter_raw(
        "synthetic-connection".to_owned(),
        version("7.4.16"),
        raw.clone(),
    )
    .unwrap_or_else(|error| panic!("7.4 snapshot: {error}"));
    assert_eq!(snapshot.adapter_id(), "zabbix-7.4");
    assert_eq!(snapshot.raw_api_document(), &raw);
    assert_eq!(
        snapshot
            .analyze()
            .unwrap_or_else(|error| panic!("7.4 analysis: {error}"))
            .widget_count,
        4
    );
    assert_eq!(
        AdapterRegistry::widget_capabilities(&version("7.4.16"))
            .unwrap_or_else(|error| panic!("7.4 registry: {error}"))
            .capabilities()
            .len(),
        32
    );
}
