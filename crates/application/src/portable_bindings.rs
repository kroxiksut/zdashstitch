// SPDX-License-Identifier: MIT OR Apache-2.0

//! Validation of explicit portable metric assignments against read-only facts.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use zdashstitch_core::formats::{
    Cardinality, CatalogReference, HostSelection, MetricCatalog, MetricOrigin, MetricSelector,
    Role, Selection, ValueType, ZabbixObjectKind, ZabbixWidgetFieldValue,
};
use zdashstitch_zabbix::{ItemFlags, VersionedDashboardSnapshot};

use crate::{
    resolver::{SourceFacts, SourceKind as ResolverSourceKind, SourceModel},
    workcopy::WorkcopyError,
};

/// Explicit choices are local input only; server IDs never enter the bundle.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableItemChoice {
    pub itemid: String,
    pub catalog: String,
    pub metric: String,
    pub role: String,
    pub slot: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableHostChoice {
    pub hostid: String,
    pub role: String,
    pub slot: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortablePatternChoice {
    pub page_index: usize,
    pub widget_index: usize,
    pub field_name: String,
    pub role: String,
}

pub(crate) struct PortableBindings {
    pub fields: BTreeMap<String, ZabbixWidgetFieldValue>,
    pub hosts: BTreeMap<String, ZabbixWidgetFieldValue>,
    pub graphs: BTreeMap<(u16, String), ZabbixWidgetFieldValue>,
    pub patterns: BTreeMap<(usize, usize, String), ZabbixWidgetFieldValue>,
    pub roles: BTreeMap<String, Role>,
    pub catalogs: BTreeMap<String, MetricCatalog>,
    pub references: Vec<CatalogReference>,
}

/// Evidence is scoped to the exact source bytes. The snapshot digest covers
/// the source connection and version as well as positional field addresses.
#[derive(Clone, Copy)]
pub(crate) struct SnapshotBindings<'a> {
    bindings: &'a PortableBindings,
    source_fingerprint: &'a str,
}

impl PortableBindings {
    pub(crate) fn for_snapshot<'a>(
        &'a self,
        snapshot: &'a VersionedDashboardSnapshot,
    ) -> SnapshotBindings<'a> {
        SnapshotBindings {
            bindings: self,
            source_fingerprint: snapshot.storage_fingerprint(),
        }
    }
}

impl<'a> SnapshotBindings<'a> {
    pub(crate) fn get(
        self,
        snapshot: &VersionedDashboardSnapshot,
    ) -> Result<&'a PortableBindings, WorkcopyError> {
        if self.source_fingerprint != snapshot.storage_fingerprint() {
            return Err(WorkcopyError::new("portable_conversion_binding_required"));
        }
        Ok(self.bindings)
    }
}

pub(crate) fn add_pattern_choices(
    bindings: &mut PortableBindings,
    model: &SourceModel,
    choices: &[PortablePatternChoice],
) -> Result<(), WorkcopyError> {
    let invalid = || WorkcopyError::new("portable_conversion_binding_required");
    let mut required = BTreeMap::new();
    for (_, source) in model.sources() {
        let pattern = match &source.kind {
            ResolverSourceKind::HostPattern { pattern, .. }
            | ResolverSourceKind::ItemPattern { pattern, .. } => pattern,
            _ => continue,
        };
        let key = (
            source.field.widget.page_index,
            source.field.widget.widget_index,
            source.field.field_name.clone(),
        );
        if required.insert(key, pattern).is_some() {
            return Err(invalid());
        }
    }
    if required.len() != choices.len() {
        return Err(invalid());
    }
    for choice in choices {
        let key = (
            choice.page_index,
            choice.widget_index,
            choice.field_name.clone(),
        );
        let pattern = required.remove(&key).ok_or_else(invalid)?;
        bindings
            .roles
            .entry(choice.role.clone())
            .or_insert_with(|| Role {
                cardinality: Cardinality::Many,
                description: None,
                slots: None,
            });
        bindings.patterns.insert(
            key,
            ZabbixWidgetFieldValue::DynamicPattern {
                value: pattern.clone(),
                role: choice.role.clone(),
            },
        );
    }
    Ok(())
}

pub(crate) fn add_graphs(
    bindings: &mut PortableBindings,
    graphids: &BTreeSet<String>,
    prototype_ids: &BTreeSet<String>,
    facts: &SourceFacts,
) -> Result<(), WorkcopyError> {
    let invalid = || WorkcopyError::new("portable_conversion_binding_required");
    for id in graphids {
        let graph = facts
            .graphs
            .get(id)
            .filter(|graph| &graph.graphid == id)
            .ok_or_else(invalid)?;
        bindings.graphs.insert(
            (6, id.clone()),
            ZabbixWidgetFieldValue::Unresolved {
                object_kind: ZabbixObjectKind::Graph,
                display_name: graph.name.clone(),
            },
        );
    }
    for id in prototype_ids {
        let graph = facts
            .graph_prototypes
            .get(id)
            .filter(|graph| &graph.graphid == id)
            .ok_or_else(invalid)?;
        bindings.graphs.insert(
            (7, id.clone()),
            ZabbixWidgetFieldValue::Unresolved {
                object_kind: ZabbixObjectKind::GraphPrototype,
                display_name: graph.name.clone(),
            },
        );
    }
    Ok(())
}

pub(crate) fn add_host_choices(
    bindings: &mut PortableBindings,
    hostids: &BTreeSet<String>,
    facts: &SourceFacts,
    choices: &[PortableHostChoice],
) -> Result<(), WorkcopyError> {
    let invalid = || WorkcopyError::new("portable_conversion_binding_required");
    let mut seen = BTreeSet::new();
    for choice in choices {
        if !hostids.contains(&choice.hostid)
            || !seen.insert(choice.hostid.clone())
            || facts
                .hosts
                .get(&choice.hostid)
                .is_none_or(|h| h.hostid != choice.hostid)
        {
            return Err(invalid());
        }
        let value = ZabbixWidgetFieldValue::Host {
            role: choice.role.clone(),
            selection: Selection::Slot,
            slot_id: Some(choice.slot.clone()),
        };
        if bindings
            .hosts
            .get(&choice.hostid)
            .is_some_and(|previous| previous != &value)
            || bindings
                .hosts
                .iter()
                .any(|(id, previous)| id != &choice.hostid && previous == &value)
        {
            return Err(invalid());
        }
        let role = bindings
            .roles
            .entry(choice.role.clone())
            .or_insert_with(|| Role {
                cardinality: Cardinality::Many,
                description: None,
                slots: Some(Vec::new()),
            });
        let slots = role.slots.as_mut().ok_or_else(invalid)?;
        if !slots.contains(&choice.slot) {
            slots.push(choice.slot.clone());
        }
        bindings.hosts.insert(choice.hostid.clone(), value);
    }
    if hostids.iter().any(|id| !bindings.hosts.contains_key(id)) {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) fn metric_matches(
    item: &zdashstitch_zabbix::ItemDetails,
    metric: &zdashstitch_core::formats::Metric,
    facts: &SourceFacts,
) -> bool {
    let expected_key = match item.flags {
        ItemFlags::Plain
            if !matches!(
                metric.origin,
                MetricOrigin::LldPrototype | MetricOrigin::DiscoveredItem
            ) =>
        {
            &item.key
        }
        ItemFlags::Plain => return false,
        ItemFlags::Discovered => {
            let Some(link) = &item.discovery else {
                return false;
            };
            if metric.origin != MetricOrigin::LldPrototype
                || facts
                    .item_prototypes
                    .get(&link.parent_itemid)
                    .is_none_or(|proto| {
                        proto.itemid != link.parent_itemid
                            || proto.hostid != item.hostid
                            || proto.key != link.prototype_key
                            || proto.value_type != item.value_type
                    })
            {
                return false;
            }
            &link.prototype_key
        }
        ItemFlags::Other(_) => return false,
    };
    let kind = match item.value_type {
        0 => ValueType::NumericFloat,
        1 => ValueType::Character,
        2 => ValueType::Log,
        3 => ValueType::NumericUnsigned,
        4 => ValueType::Text,
        5 => ValueType::Binary,
        // JSON catalogs are readable, but portable binding/publication has no
        // independently verified widget contract for value_type 6.
        _ => return false,
    };
    matches!(&metric.selector, MetricSelector::Key(key) if key == expected_key)
        && metric.value_type == kind
}

pub(crate) fn validate_choices(
    itemids: &BTreeSet<String>,
    facts: &SourceFacts,
    catalogs: &BTreeMap<String, MetricCatalog>,
    choices: &[PortableItemChoice],
) -> Result<PortableBindings, WorkcopyError> {
    let invalid = || WorkcopyError::new("portable_conversion_binding_required");
    if choices.len() != itemids.len() {
        return Err(invalid());
    }
    let mut fields = BTreeMap::new();
    let mut slot_hosts = BTreeMap::<(String, String), String>::new();
    let mut host_slots = BTreeMap::<String, (String, String)>::new();
    let mut roles = BTreeMap::<String, Role>::new();
    let mut used_catalogs = BTreeMap::new();
    for choice in choices {
        if !itemids.contains(&choice.itemid) || fields.contains_key(&choice.itemid) {
            return Err(invalid());
        }
        let item = facts
            .items
            .get(&choice.itemid)
            .filter(|item| item.itemid == choice.itemid)
            .ok_or_else(invalid)?;
        if item.key.is_empty()
            || facts
                .hosts
                .get(&item.hostid)
                .is_none_or(|host| host.hostid != item.hostid)
        {
            return Err(invalid());
        }
        let catalog = catalogs
            .get(&choice.catalog)
            .filter(|catalog| catalog.catalog_id == choice.catalog)
            .ok_or_else(invalid)?;
        let metric = catalog
            .metrics
            .iter()
            .find(|metric| metric.id == choice.metric)
            .ok_or_else(invalid)?;
        if !metric_matches(item, metric, facts) {
            return Err(invalid());
        }
        let slot_key = (choice.role.clone(), choice.slot.clone());
        if slot_hosts
            .get(&slot_key)
            .is_some_and(|host| host != &item.hostid)
            || host_slots
                .get(&item.hostid)
                .is_some_and(|slot| slot != &slot_key)
        {
            return Err(invalid());
        }
        slot_hosts.insert(slot_key.clone(), item.hostid.clone());
        host_slots.insert(item.hostid.clone(), slot_key);
        let role = roles.entry(choice.role.clone()).or_insert_with(|| Role {
            cardinality: Cardinality::Many,
            description: None,
            slots: Some(Vec::new()),
        });
        let slots = role.slots.as_mut().ok_or_else(invalid)?;
        if !slots.contains(&choice.slot) {
            slots.push(choice.slot.clone());
        }
        used_catalogs.insert(choice.catalog.clone(), catalog.clone());
        let hosts = HostSelection {
            role: choice.role.clone(),
            selection: Selection::Slot,
            slot_id: Some(choice.slot.clone()),
        };
        let value =
            if item.flags == ItemFlags::Discovered && metric.origin == MetricOrigin::LldPrototype {
                ZabbixWidgetFieldValue::LldInstance {
                    catalog: choice.catalog.clone(),
                    metric: choice.metric.clone(),
                    hosts,
                    source_key: item.key.clone(),
                }
            } else {
                ZabbixWidgetFieldValue::Metric {
                    catalog: choice.catalog.clone(),
                    metric: choice.metric.clone(),
                    hosts,
                }
            };
        fields.insert(choice.itemid.clone(), value);
    }
    let references = used_catalogs
        .keys()
        .map(|id| CatalogReference {
            id: id.clone(),
            file: format!("metrics/{id}.yaml"),
        })
        .collect();
    let hosts = host_slots
        .into_iter()
        .map(|(hostid, (role, slot))| {
            (
                hostid,
                ZabbixWidgetFieldValue::Host {
                    role,
                    selection: Selection::Slot,
                    slot_id: Some(slot),
                },
            )
        })
        .collect();
    Ok(PortableBindings {
        fields,
        hosts,
        graphs: BTreeMap::new(),
        patterns: BTreeMap::new(),
        roles,
        catalogs: used_catalogs,
        references,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        offline_pages::OfflinePageStore,
        portable_conversion::{convert_dashboard_bound, convert_page_bound},
    };
    use serde_json::json;
    use zdashstitch_core::formats::{
        CatalogSource, ExportProfile, Metric, MetricOrigin, MetricsFormat, SourceKind,
    };
    use zdashstitch_zabbix::{HostDetails, ItemDetails, VersionedDashboardSnapshot, ZabbixVersion};

    #[test]
    fn visible_classic_graph_becomes_unresolved_without_server_id() {
        let mut facts = SourceFacts::default();
        facts.graphs.insert(
            "60001".into(),
            zdashstitch_zabbix::GraphDetails {
                graphid: "60001".into(),
                name: "CPU load".into(),
                templateid: None,
                discovered: false,
                hostids: vec!["40001".into()],
            },
        );
        let mut bindings = validate_choices(&BTreeSet::new(), &facts, &BTreeMap::new(), &[])
            .unwrap_or_else(|e| panic!("{e}"));
        let graphids = ["60001".to_owned()].into_iter().collect();
        add_graphs(&mut bindings, &graphids, &BTreeSet::new(), &facts)
            .unwrap_or_else(|e| panic!("{e}"));
        let version: ZabbixVersion = "7.4.16".parse().unwrap_or_else(|e| panic!("{e}"));
        let raw = json!({"dashboardid":"7","name":"D","pages":[{"dashboard_pageid":"8","name":"P","widgets":[{"widgetid":"9","type":"graph","name":"CPU","x":"0","y":"0","width":"12","height":"4","fields":[{"name":"graphid.0","type":6,"value":"60001"}]}]}]});
        let snapshot = VersionedDashboardSnapshot::from_adapter_raw("test".into(), version, raw)
            .unwrap_or_else(|e| panic!("{e}"));
        let bundle = convert_page_bound(
            &snapshot,
            "8",
            "portable".into(),
            Some(bindings.for_snapshot(&snapshot)),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let value = serde_json::to_value(&bundle).unwrap_or_default();
        let zdashstitch_core::formats::ProjectRoot::Page(page) = &bundle.project.root else {
            panic!("page")
        };
        assert!(
            crate::compatibility::validate_portable_widget_for_create(
                &page.widgets[0],
                &zdashstitch_zabbix::WidgetCapabilityRegistry::v7_4()
            )
            .is_err()
        );
        assert_eq!(
            value["project"]["root"]["widgets"][0]["fields"][0]["value"],
            json!({"kind":"unresolved","object_kind":"graph","display_name":"CPU load"})
        );
        assert!(!value.to_string().contains("60001"));
        facts.graphs.clear();
        assert!(add_graphs(&mut bindings, &graphids, &BTreeSet::new(), &facts).is_err());
    }

    #[test]
    fn explicit_binding_is_validated_saved_and_reopened() {
        let mut facts = SourceFacts::default();
        facts.items.insert(
            "30001".into(),
            ItemDetails {
                itemid: "30001".into(),
                hostid: "40001".into(),
                key: "vm.memory.size[available]".into(),
                name: "Available memory".into(),
                value_type: 3,
                units: "B".into(),
                flags: ItemFlags::Plain,
                enabled: None,
                templateid: None,
                discovery: None,
                owner_name: None,
            },
        );
        facts.hosts.insert(
            "40001".into(),
            HostDetails {
                hostid: "40001".into(),
                host: "host".into(),
                name: "Host".into(),
            },
        );
        let catalog = MetricCatalog {
            format: MetricsFormat::V1,
            catalog_id: "memory".into(),
            export_profile: ExportProfile::PortableWithPublicKeys,
            source: CatalogSource {
                kind: SourceKind::Manual,
                export_format_version: None,
                source_server_version: None,
                template_name: None,
                template_uuid: None,
            },
            diagnostics: vec![],
            metrics: vec![Metric {
                id: "available".into(),
                name: "Available memory".into(),
                selector: MetricSelector::Key("vm.memory.size[available]".into()),
                value_type: ValueType::NumericUnsigned,
                units: "B".into(),
                origin: MetricOrigin::HostItem,
                uuid: None,
                diagnostics: None,
            }],
        };
        let catalogs = [("memory".to_owned(), catalog)].into_iter().collect();
        let ids = ["30001".to_owned()].into_iter().collect();
        let choice = PortableItemChoice {
            itemid: "30001".into(),
            catalog: "memory".into(),
            metric: "available".into(),
            role: "servers".into(),
            slot: "first".into(),
        };
        let bindings = validate_choices(&ids, &facts, &catalogs, std::slice::from_ref(&choice))
            .unwrap_or_else(|e| panic!("{e}"));
        let mut host_only = validate_choices(&BTreeSet::new(), &facts, &catalogs, &[])
            .unwrap_or_else(|e| panic!("{e}"));
        let hostids = ["40001".to_owned()].into_iter().collect();
        let host_choice = PortableHostChoice {
            hostid: "40001".into(),
            role: "servers".into(),
            slot: "first".into(),
        };
        assert!(
            add_host_choices(
                &mut host_only,
                &hostids,
                &facts,
                std::slice::from_ref(&host_choice)
            )
            .is_ok()
        );
        assert!(
            add_host_choices(
                &mut host_only,
                &hostids,
                &facts,
                &[PortableHostChoice {
                    slot: "other".into(),
                    ..host_choice
                }]
            )
            .is_err()
        );
        let version: ZabbixVersion = "7.4.16".parse().unwrap_or_else(|e| panic!("{e}"));
        let raw = json!({"dashboardid":"7","name":"D","pages":[{"dashboard_pageid":"8","name":"P","widgets":[{"widgetid":"9","type":"item","name":"Memory","x":"0","y":"0","width":"12","height":"4","fields":[{"name":"itemid.0","type":4,"value":"30001"}]},{"widgetid":"10","type":"problems","name":"Problems","x":"12","y":"0","width":"12","height":"4","fields":[{"name":"hostids.0","type":3,"value":"40001"}]}]}]});
        let snapshot =
            VersionedDashboardSnapshot::from_adapter_raw("test".into(), version, raw.clone())
                .unwrap_or_else(|e| panic!("{e}"));
        let bundle = convert_page_bound(
            &snapshot,
            "8",
            "portable".into(),
            Some(bindings.for_snapshot(&snapshot)),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let wire = serde_json::to_string(&bundle).unwrap_or_default();
        assert!(!wire.contains("30001") && !wire.contains("40001"));
        assert!(wire.contains("hostids.0") && wire.contains("\"slot_id\":\"first\""));
        let doc = serde_json::to_value(&bundle).unwrap_or_default();
        assert_eq!(
            doc["project"]["root"]["widgets"][1]["fields"][0]["value"],
            json!({"kind":"host", "role":"servers", "selection":"slot", "slot_id":"first"})
        );
        let mut graph_raw = raw.clone();
        graph_raw["pages"][0]["widgets"][0]["type"] = json!("svggraph");
        graph_raw["pages"][0]["widgets"][0]["fields"][0]["name"] = json!("ds.0.itemids.0");
        let version: ZabbixVersion = "7.4.16".parse().unwrap_or_else(|e| panic!("{e}"));
        let graph_snapshot =
            VersionedDashboardSnapshot::from_adapter_raw("test".into(), version, graph_raw)
                .unwrap_or_else(|e| panic!("{e}"));
        let graph_bundle = convert_page_bound(
            &graph_snapshot,
            "8",
            "portable".into(),
            Some(bindings.for_snapshot(&graph_snapshot)),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let graph_doc = serde_json::to_value(graph_bundle).unwrap_or_default();
        assert_eq!(
            graph_doc["project"]["root"]["widgets"][0]["fields"][0]["value"]["kind"],
            "metric"
        );
        let mut unmatched = raw.clone();
        unmatched["pages"][0]["widgets"][1]["fields"][0]["value"] = json!("49999");
        let version: ZabbixVersion = "7.4.16".parse().unwrap_or_else(|e| panic!("{e}"));
        let unmatched =
            VersionedDashboardSnapshot::from_adapter_raw("test".into(), version, unmatched)
                .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            convert_page_bound(
                &unmatched,
                "8",
                "portable".into(),
                Some(bindings.for_snapshot(&unmatched))
            )
            .is_err()
        );
        let mut eight_raw = raw.clone();
        eight_raw["pages"][0]["widgets"][1]["type"] = json!("svggraph");
        eight_raw["pages"][0]["widgets"][1]["fields"] = json!([
            {"name":"override_hostid.0","type":3,"value":"40001"},
            {"name":"show_hostnames","type":0,"value":"1"}
        ]);
        let eight = VersionedDashboardSnapshot::from_adapter_raw(
            "test".into(),
            "8.0.2".parse().unwrap_or_else(|e| panic!("{e}")),
            eight_raw.clone(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let other_connection = VersionedDashboardSnapshot::from_adapter_raw(
            "other-test".into(),
            eight.server_version().clone(),
            eight_raw,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        for (mismatched, source) in [(&eight, &snapshot), (&other_connection, &eight)] {
            assert_eq!(
                convert_page_bound(
                    mismatched,
                    "8",
                    "portable".into(),
                    Some(bindings.for_snapshot(source))
                )
                .err()
                .map(|error| error.code()),
                Some("portable_conversion_binding_required")
            );
        }
        let eight_bundle = convert_page_bound(
            &eight,
            "8",
            "portable".into(),
            Some(bindings.for_snapshot(&eight)),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(eight_bundle.catalogs, bundle.catalogs);
        assert_eq!(eight_bundle.project.roles, bundle.project.roles);
        let eight_doc = serde_json::to_value(&eight_bundle).unwrap_or_default();
        assert_eq!(
            eight_doc["project"]["root"]["widgets"][0]["fields"][0]["value"]["kind"],
            "metric"
        );
        assert_eq!(
            eight_doc["project"]["root"]["widgets"][1]["fields"][0]["value"]["slot_id"],
            "first"
        );
        let eight_wire = eight_doc.to_string();
        assert!(!eight_wire.contains("30001") && !eight_wire.contains("40001"));
        let root = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store = OfflinePageStore::new(root.path().to_path_buf());
        let saved = store
            .save_converted(bundle)
            .unwrap_or_else(|e| panic!("{e}"));
        let reopened = store
            .get(&saved.local_id)
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("missing"));
        assert_eq!(reopened.bundle, saved.bundle);
        let bytes = zdashstitch_core::formats::build_typed_package_bytes(&reopened.bundle)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            zdashstitch_core::formats::read_typed_package_bytes(&bytes)
                .unwrap_or_else(|e| panic!("{e}")),
            reopened.bundle
        );
        assert_eq!(snapshot.raw_api_document(), &raw);
        let dashboard_bundle = convert_dashboard_bound(
            &snapshot,
            "portable".into(),
            Some(bindings.for_snapshot(&snapshot)),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let dashboards =
            crate::offline_dashboards::OfflineDashboardStore::new(root.path().to_path_buf());
        let saved_dashboard = dashboards
            .save_converted(dashboard_bundle)
            .unwrap_or_else(|e| panic!("{e}"));
        let reopened_dashboard = dashboards
            .get(&saved_dashboard.local_id)
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("missing"));
        assert_eq!(reopened_dashboard.bundle, saved_dashboard.bundle);
        let mut hidden = facts.clone();
        hidden.hosts.clear();
        assert!(validate_choices(&ids, &hidden, &catalogs, std::slice::from_ref(&choice)).is_err());
        let mut discovered = facts.clone();
        discovered
            .items
            .get_mut("30001")
            .unwrap_or_else(|| panic!("missing"))
            .flags = ItemFlags::Discovered;
        assert!(
            validate_choices(&ids, &discovered, &catalogs, std::slice::from_ref(&choice)).is_err()
        );
        discovered
            .items
            .get_mut("30001")
            .unwrap_or_else(|| panic!("missing"))
            .discovery = Some(zdashstitch_zabbix::ItemDiscoveryLink {
            parent_itemid: "50001".into(),
            prototype_key: "vm.memory.size[{#TYPE}]".into(),
            currently_discovered: None,
        });
        discovered.item_prototypes.insert(
            "50001".into(),
            zdashstitch_zabbix::ItemPrototypeDetails {
                itemid: "50001".into(),
                hostid: "40001".into(),
                key: "vm.memory.size[{#TYPE}]".into(),
                name: "Memory".into(),
                value_type: 3,
                units: "B".into(),
                templateid: None,
                rule_itemid: None,
                owner_name: None,
            },
        );
        let mut lld_catalogs = catalogs.clone();
        lld_catalogs
            .get_mut("memory")
            .unwrap_or_else(|| panic!("missing"))
            .metrics[0]
            .origin = MetricOrigin::DiscoveredItem;
        assert!(
            validate_choices(
                &ids,
                &discovered,
                &lld_catalogs,
                std::slice::from_ref(&choice)
            )
            .is_err()
        );
        let prototype_metric = &mut lld_catalogs
            .get_mut("memory")
            .unwrap_or_else(|| panic!("missing"))
            .metrics[0];
        prototype_metric.origin = MetricOrigin::LldPrototype;
        prototype_metric.selector = MetricSelector::Key("vm.memory.size[{#TYPE}]".into());
        let derived = validate_choices(
            &ids,
            &discovered,
            &lld_catalogs,
            std::slice::from_ref(&choice),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let instance = derived
            .catalogs
            .values()
            .next()
            .unwrap_or_else(|| panic!("missing derived catalog"));
        assert_eq!(
            instance.metrics[0].selector,
            MetricSelector::Key("vm.memory.size[{#TYPE}]".into())
        );
        assert_eq!(instance.metrics[0].origin, MetricOrigin::LldPrototype);
        assert!(
            matches!(&derived.fields["30001"], ZabbixWidgetFieldValue::LldInstance { catalog, metric, source_key, .. } if catalog == "memory" && metric == "available" && source_key == "vm.memory.size[available]")
        );
        let bundle = convert_page_bound(
            &snapshot,
            "8",
            "portable".into(),
            Some(derived.for_snapshot(&snapshot)),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let bytes = zdashstitch_core::formats::build_typed_package_bytes(&bundle)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            zdashstitch_core::formats::read_typed_package_bytes(&bytes)
                .unwrap_or_else(|e| panic!("{e}")),
            bundle
        );
        let mut bad_prototype = bundle.clone();
        bad_prototype
            .catalogs
            .get_mut("memory")
            .unwrap_or_else(|| panic!("catalog"))
            .metrics[0]
            .origin = MetricOrigin::TemplateItem;
        assert_eq!(
            bad_prototype.validate().err().map(|error| error.code()),
            Some("widget_lld_prototype")
        );
        assert!(
            !serde_json::to_string(&bundle)
                .unwrap_or_default()
                .contains("30001")
        );
        let mut two_discovered = discovered.clone();
        let mut second_instance = two_discovered
            .items
            .get("30001")
            .unwrap_or_else(|| panic!("missing"))
            .clone();
        second_instance.itemid = "30002".into();
        second_instance.key = "vm.memory.size[used]".into();
        two_discovered.items.insert("30002".into(), second_instance);
        let mut second_choice = choice.clone();
        second_choice.itemid = "30002".into();
        let two_ids = ["30001".to_owned(), "30002".to_owned()]
            .into_iter()
            .collect();
        let two = validate_choices(
            &two_ids,
            &two_discovered,
            &lld_catalogs,
            &[choice.clone(), second_choice],
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(two.catalogs.len(), 1);
        assert_ne!(two.fields["30001"], two.fields["30002"]);
        discovered
            .item_prototypes
            .get_mut("50001")
            .unwrap_or_else(|| panic!("missing"))
            .hostid = "49999".into();
        assert!(
            validate_choices(
                &ids,
                &discovered,
                &lld_catalogs,
                std::slice::from_ref(&choice)
            )
            .is_err()
        );
        assert!(validate_choices(&ids, &facts, &catalogs, &[]).is_err());
        assert!(
            validate_choices(&ids, &facts, &catalogs, &[choice.clone(), choice.clone()]).is_err()
        );
        let mut multi = facts.clone();
        let mut second = multi
            .items
            .get("30001")
            .unwrap_or_else(|| panic!("missing"))
            .clone();
        second.itemid = "30002".into();
        second.hostid = "40002".into();
        multi.items.insert("30002".into(), second);
        multi.hosts.insert(
            "40002".into(),
            HostDetails {
                hostid: "40002".into(),
                host: "host2".into(),
                name: "Host 2".into(),
            },
        );
        let ids_two = ["30001".to_owned(), "30002".to_owned()]
            .into_iter()
            .collect();
        let mut choice_two = choice.clone();
        choice_two.itemid = "30002".into();
        assert!(
            validate_choices(
                &ids_two,
                &multi,
                &catalogs,
                &[choice.clone(), choice_two.clone()]
            )
            .is_err()
        );
        choice_two.slot = "second".into();
        let two = validate_choices(&ids_two, &multi, &catalogs, &[choice.clone(), choice_two])
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(two.roles["servers"].slots.as_ref().map(Vec::len), Some(2));
        facts
            .items
            .get_mut("30001")
            .unwrap_or_else(|| panic!("missing"))
            .key = "other".into();
        assert!(validate_choices(&ids, &facts, &catalogs, &[choice]).is_err());
    }
}
