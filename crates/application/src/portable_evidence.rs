// SPDX-License-Identifier: MIT OR Apache-2.0

//! Read-only source evidence for P4-08. No server IDs enter a portable file.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use zdashstitch_zabbix::{AdapterRegistry, ItemFlags, ReadOnlyGateway, VersionedDashboardSnapshot};

use crate::{
    PortableConversionOutcome,
    offline_dashboards::OfflineDashboardStore,
    offline_pages::OfflinePageStore,
    portable_bindings::{
        PortableHostChoice, PortableItemChoice, PortablePatternChoice, add_graphs,
        add_host_choices, add_pattern_choices, metric_matches, validate_choices,
    },
    portable_conversion::{
        append_converted_blueprints, apply_workcopy_name, convert_dashboard_bound,
        convert_page_bound,
    },
    replace_host::materialize_existing_widgets,
    resolver::{SourceFacts, SourceKind, collect_source_facts, source_model},
    workcopy::{DraftKind, WorkcopyError, WorkcopyService},
};

/// Counts only: identifiers, item names, and keys are never returned here.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PortableEvidenceSummary {
    pub revision: u64,
    pub referenced_items: usize,
    pub visible_items: usize,
    pub missing_items: usize,
    pub missing_hosts: usize,
    pub plain_items: usize,
    pub discovered_items: usize,
    pub other_items: usize,
    pub inherited_items_with_visible_template: usize,
}

#[derive(Serialize)]
pub struct PortableBindingCandidate {
    pub catalog: String,
    pub catalog_name: String,
    pub metric: String,
    pub metric_name: String,
    pub lld_prototype: bool,
}

#[derive(Serialize)]
pub struct PortableBindingItem {
    pub itemid: String,
    pub name: Option<String>,
    pub host: Option<String>,
    pub hostid: Option<String>,
    pub candidates: Vec<PortableBindingCandidate>,
}

#[derive(Serialize)]
pub struct PortableBindingSetup {
    pub revision: u64,
    pub items: Vec<PortableBindingItem>,
    pub hosts: Vec<PortableBindingHost>,
    pub unresolved_graphs: usize,
    pub patterns: Vec<PortableBindingPattern>,
    pub catalogs: BTreeMap<String, zdashstitch_core::formats::MetricCatalog>,
}

#[derive(Serialize)]
pub struct PortableBindingHost {
    pub hostid: String,
    pub name: Option<String>,
}

#[derive(Serialize)]
pub struct PortableBindingPattern {
    pub page_index: usize,
    pub widget_index: usize,
    pub field_name: String,
    pub value: String,
}

impl WorkcopyService {
    /// Candidates are suggestions from exact key/type matches, never automatic choices.
    pub async fn portable_binding_setup(
        &self,
        reader: &ReadOnlyGateway,
        connection_id: &str,
        local_id: &str,
    ) -> Result<PortableBindingSetup, WorkcopyError> {
        let (file, base, operations) = self.load_log(local_id)?;
        if file.source.connection_id != connection_id {
            return Err(WorkcopyError::new("workcopy_other_connection"));
        }
        let local = materialize_existing_widgets(base, &operations)?;
        if local.server_version() != &reader.capabilities().version {
            return Err(WorkcopyError::new("clone_intent_server_changed"));
        }
        let fingerprint = local.storage_fingerprint().to_owned();
        let scoped = if file.kind == DraftKind::Page {
            let page_id = &file
                .source
                .page
                .as_ref()
                .ok_or_else(|| WorkcopyError::new("workcopy_corrupt"))?
                .dashboard_pageid;
            let mut raw = local.raw_api_document().clone();
            let pages = raw["pages"]
                .as_array_mut()
                .ok_or_else(|| WorkcopyError::new("snapshot_corrupt"))?;
            pages.retain(|page| page["dashboard_pageid"].as_str() == Some(page_id));
            if pages.len() != 1 {
                return Err(WorkcopyError::new("workcopy_corrupt"));
            }
            VersionedDashboardSnapshot::from_adapter_raw(
                connection_id.to_owned(),
                local.server_version().clone(),
                raw,
            )
            .map_err(|_| WorkcopyError::new("snapshot_corrupt"))?
        } else {
            local
        };
        let registry = AdapterRegistry::widget_capabilities(scoped.server_version())
            .map_err(|_| WorkcopyError::new("clone_preview_adapter_missing"))?;
        let model = source_model(&scoped, &registry)
            .map_err(|error| WorkcopyError::from_resolver(&error))?;
        let facts = collect_source_facts(reader, &model)
            .await
            .map_err(|error| WorkcopyError::from_resolver(&error))?;
        let library = crate::catalog_library::CatalogLibrary::new(self.store.root_path());
        let mut seen = BTreeSet::new();
        let mut available = Vec::new();
        for summary in library.list()? {
            if seen.insert(summary.catalog_id.clone()) {
                let template = library
                    .get(&summary.catalog_id, summary.version)?
                    .ok_or_else(|| WorkcopyError::new("library_corrupt"))?;
                available.push((summary.name, template.catalog));
            }
        }
        let mut catalogs = BTreeMap::new();
        let mut items = Vec::new();
        for itemid in model.itemids() {
            let item = facts.items.get(&itemid);
            let host = item.and_then(|i| facts.hosts.get(&i.hostid));
            let mut candidates = Vec::new();
            if let Some(item) =
                item.filter(|i| i.itemid == itemid && host.is_some_and(|h| h.hostid == i.hostid))
            {
                for (name, catalog) in &available {
                    for metric in catalog
                        .metrics
                        .iter()
                        .filter(|m| metric_matches(item, m, &facts))
                    {
                        candidates.push(PortableBindingCandidate {
                            catalog: catalog.catalog_id.clone(),
                            catalog_name: name.clone(),
                            metric: metric.id.clone(),
                            metric_name: metric.name.clone(),
                            lld_prototype: metric.origin
                                == zdashstitch_core::formats::MetricOrigin::LldPrototype,
                        });
                        catalogs.insert(catalog.catalog_id.clone(), catalog.clone());
                    }
                }
            }
            items.push(PortableBindingItem {
                itemid,
                name: item.map(|i| i.name.clone()),
                host: host.map(|h| h.name.clone()),
                hostid: host.map(|h| h.hostid.clone()),
                candidates,
            });
        }
        let (current, base, operations) = self.load_log(local_id)?;
        if current.revision != file.revision
            || materialize_existing_widgets(base, &operations)?.storage_fingerprint() != fingerprint
        {
            return Err(WorkcopyError::new("workcopy_revision_conflict"));
        }
        let hosts = model
            .host_field_ids()
            .into_iter()
            .map(|hostid| PortableBindingHost {
                name: facts
                    .hosts
                    .get(&hostid)
                    .filter(|h| h.hostid == hostid)
                    .map(|h| h.name.clone()),
                hostid,
            })
            .collect();
        let patterns = model
            .sources()
            .filter_map(|(_, source)| {
                let value = match &source.kind {
                    SourceKind::HostPattern { pattern, .. }
                    | SourceKind::ItemPattern { pattern, .. } => pattern.clone(),
                    _ => return None,
                };
                Some(PortableBindingPattern {
                    page_index: source.field.widget.page_index,
                    widget_index: source.field.widget.widget_index,
                    field_name: source.field.field_name.clone(),
                    value,
                })
            })
            .collect();
        Ok(PortableBindingSetup {
            revision: file.revision,
            items,
            hosts,
            unresolved_graphs: model.graphids().len() + model.graph_prototype_ids().len(),
            patterns,
            catalogs,
        })
    }
    /// Create a separate local draft from explicit, verified item choices.
    /// The Zabbix gateway is read-only and the original copy is never changed.
    #[allow(clippy::too_many_arguments)]
    pub async fn make_portable_with_choices(
        &self,
        reader: &ReadOnlyGateway,
        connection_id: &str,
        local_id: &str,
        expected_revision: u64,
        catalogs: &std::collections::BTreeMap<String, zdashstitch_core::formats::MetricCatalog>,
        choices: &[PortableItemChoice],
        host_choices: &[PortableHostChoice],
        pattern_choices: &[PortablePatternChoice],
    ) -> Result<PortableConversionOutcome, WorkcopyError> {
        let (file, base, operations) = self.load_log(local_id)?;
        if file.source.connection_id != connection_id {
            return Err(WorkcopyError::new("workcopy_other_connection"));
        }
        if file.revision != expected_revision {
            return Err(WorkcopyError::new("workcopy_revision_conflict"));
        }
        let local = materialize_existing_widgets(base, &operations)?;
        if local.server_version() != &reader.capabilities().version {
            return Err(WorkcopyError::new("clone_intent_server_changed"));
        }
        let fingerprint = local.storage_fingerprint().to_owned();
        let page_id = match file.kind {
            DraftKind::Dashboard => None,
            DraftKind::Page => Some(
                file.source
                    .page
                    .as_ref()
                    .ok_or_else(|| WorkcopyError::new("workcopy_corrupt"))?
                    .dashboard_pageid
                    .clone(),
            ),
        };
        let scoped = if let Some(id) = &page_id {
            let mut raw = local.raw_api_document().clone();
            let pages = raw["pages"]
                .as_array_mut()
                .ok_or_else(|| WorkcopyError::new("snapshot_corrupt"))?;
            pages.retain(|page| page["dashboard_pageid"].as_str() == Some(id));
            if pages.len() != 1 {
                return Err(WorkcopyError::new("workcopy_corrupt"));
            }
            VersionedDashboardSnapshot::from_adapter_raw(
                connection_id.to_owned(),
                local.server_version().clone(),
                raw,
            )
            .map_err(|_| WorkcopyError::new("snapshot_corrupt"))?
        } else {
            local.clone()
        };
        let registry = AdapterRegistry::widget_capabilities(scoped.server_version())
            .map_err(|_| WorkcopyError::new("clone_preview_adapter_missing"))?;
        let model = source_model(&scoped, &registry)
            .map_err(|error| WorkcopyError::from_resolver(&error))?;
        let facts = collect_source_facts(reader, &model)
            .await
            .map_err(|error| WorkcopyError::from_resolver(&error))?;
        let mut bindings = validate_choices(&model.itemids(), &facts, catalogs, choices)?;
        add_host_choices(&mut bindings, &model.host_field_ids(), &facts, host_choices)?;
        add_graphs(
            &mut bindings,
            &model.graphids(),
            &model.graph_prototype_ids(),
            &facts,
        )?;
        add_pattern_choices(&mut bindings, &model, pattern_choices)?;
        let evidence = bindings.for_snapshot(&scoped);
        let mut bundle = match &page_id {
            Some(id) => convert_page_bound(&scoped, id, "portable".to_owned(), Some(evidence))?,
            None => convert_dashboard_bound(&scoped, "portable".to_owned(), Some(evidence))?,
        };
        append_converted_blueprints(&mut bundle, &scoped, page_id.as_deref(), &operations)?;
        apply_workcopy_name(&mut bundle, &file.name)?;
        let (current, current_base, current_operations) = self.load_log(local_id)?;
        let current_local = materialize_existing_widgets(current_base, &current_operations)?;
        if current.revision != expected_revision
            || current_local.storage_fingerprint() != fingerprint
        {
            return Err(WorkcopyError::new("workcopy_revision_conflict"));
        }
        let root = self.store.root_path();
        let saved_id = match file.kind {
            DraftKind::Page => OfflinePageStore::new(root).save_converted(bundle)?.local_id,
            DraftKind::Dashboard => {
                OfflineDashboardStore::new(root)
                    .save_converted(bundle)?
                    .local_id
            }
        };
        Ok(PortableConversionOutcome {
            local_id: saved_id,
            kind: file.kind,
        })
    }
    /// Read current source facts through the read-only gateway. This is not a
    /// conversion plan and does not authorize a save or any Zabbix write.
    pub async fn portable_source_evidence(
        &self,
        reader: &ReadOnlyGateway,
        connection_id: &str,
        local_id: &str,
    ) -> Result<PortableEvidenceSummary, WorkcopyError> {
        let (file, base, operations) = self.load_log(local_id)?;
        if file.source.connection_id != connection_id {
            return Err(WorkcopyError::new("workcopy_other_connection"));
        }
        let local = materialize_existing_widgets(base, &operations)?;
        if local.server_version() != &reader.capabilities().version {
            return Err(WorkcopyError::new("clone_intent_server_changed"));
        }
        let local_fingerprint = local.storage_fingerprint().to_owned();
        let scoped = match file.kind {
            DraftKind::Dashboard => local,
            DraftKind::Page => {
                let page_id = &file
                    .source
                    .page
                    .as_ref()
                    .ok_or_else(|| WorkcopyError::new("workcopy_corrupt"))?
                    .dashboard_pageid;
                let mut raw = local.raw_api_document().clone();
                let pages = raw["pages"]
                    .as_array_mut()
                    .ok_or_else(|| WorkcopyError::new("snapshot_corrupt"))?;
                pages.retain(|page| page["dashboard_pageid"].as_str() == Some(page_id));
                if pages.len() != 1 {
                    return Err(WorkcopyError::new("workcopy_corrupt"));
                }
                VersionedDashboardSnapshot::from_adapter_raw(
                    connection_id.to_owned(),
                    local.server_version().clone(),
                    raw,
                )
                .map_err(|_| WorkcopyError::new("snapshot_corrupt"))?
            }
        };
        let registry = AdapterRegistry::widget_capabilities(scoped.server_version())
            .map_err(|_| WorkcopyError::new("clone_preview_adapter_missing"))?;
        let model = source_model(&scoped, &registry)
            .map_err(|error| WorkcopyError::from_resolver(&error))?;
        let facts = collect_source_facts(reader, &model)
            .await
            .map_err(|error| WorkcopyError::from_resolver(&error))?;
        // The workcopy can change while the network request is in flight.
        let (current, current_base, current_operations) = self.load_log(local_id)?;
        let current_local = materialize_existing_widgets(current_base, &current_operations)?;
        if current.revision != file.revision
            || current_local.storage_fingerprint() != local_fingerprint
        {
            return Err(WorkcopyError::new("workcopy_revision_conflict"));
        }
        Ok(summarize(file.revision, &model.itemids(), &facts))
    }
}

fn summarize(
    revision: u64,
    ids: &BTreeSet<String>,
    facts: &SourceFacts,
) -> PortableEvidenceSummary {
    let items: Vec<_> = ids
        .iter()
        .filter_map(|id| facts.items.get(id).filter(|item| &item.itemid == id))
        .collect();
    PortableEvidenceSummary {
        revision,
        referenced_items: ids.len(),
        visible_items: items.len(),
        missing_items: ids.len() - items.len(),
        missing_hosts: items
            .iter()
            .filter(|item| {
                facts
                    .hosts
                    .get(&item.hostid)
                    .is_none_or(|host| host.hostid != item.hostid)
            })
            .count(),
        plain_items: items
            .iter()
            .filter(|item| item.flags == ItemFlags::Plain)
            .count(),
        discovered_items: items
            .iter()
            .filter(|item| item.flags == ItemFlags::Discovered)
            .count(),
        other_items: items
            .iter()
            .filter(|item| matches!(item.flags, ItemFlags::Other(_)))
            .count(),
        inherited_items_with_visible_template: items
            .iter()
            .filter(|item| {
                item.templateid
                    .as_ref()
                    .is_some_and(|id| facts.template_names.contains_key(id))
            })
            .count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zdashstitch_zabbix::{HostDetails, ItemDetails};

    #[test]
    fn reports_missing_sources_without_disclosing_values() {
        let mut facts = SourceFacts::default();
        facts.items.insert(
            "10".into(),
            ItemDetails {
                itemid: "10".into(),
                hostid: "20".into(),
                key: "private.key".into(),
                name: "private name".into(),
                value_type: 0,
                units: "".into(),
                flags: ItemFlags::Plain,
                enabled: None,
                templateid: Some("30".into()),
                discovery: None,
                owner_name: None,
            },
        );
        let ids = ["10".to_owned(), "11".to_owned()].into_iter().collect();
        let summary = summarize(3, &ids, &facts);
        assert_eq!(
            (
                summary.visible_items,
                summary.missing_items,
                summary.missing_hosts
            ),
            (1, 1, 1)
        );
        assert_eq!(summary.inherited_items_with_visible_template, 0);
        let serialized = serde_json::to_string(&summary).unwrap_or_default();
        assert!(!serialized.contains("private"));
        facts.hosts.insert(
            "20".into(),
            HostDetails {
                hostid: "20".into(),
                host: "host".into(),
                name: "host".into(),
            },
        );
        facts.template_names.insert("30".into(), "template".into());
        let summary = summarize(3, &ids, &facts);
        assert_eq!(summary.missing_hosts, 0);
        assert_eq!(summary.inherited_items_with_visible_template, 1);
    }
}
