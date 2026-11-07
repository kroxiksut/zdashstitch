// SPDX-License-Identifier: MIT OR Apache-2.0

//! Read-only lookups of the resolver and the facts they produce.
//!
//! [`ResolverLookup`] is implemented only for
//! [`zdashstitch_zabbix::ReadOnlyGateway`]; tests implement it with
//! fixtures. A write gateway cannot be passed:
//!
//! ```compile_fail
//! async fn no_write_gateway(
//!     writer: &zdashstitch_zabbix::ReadWriteGateway,
//!     model: &zdashstitch_application::resolver::SourceModel,
//! ) {
//!     let _ = zdashstitch_application::resolver::collect_source_facts(writer, model).await;
//! }
//! ```
//!
//! Control: the same call compiles with the read-only gateway.
//!
//! ```no_run
//! async fn read_gateway(
//!     reader: &zdashstitch_zabbix::ReadOnlyGateway,
//!     model: &zdashstitch_application::resolver::SourceModel,
//! ) {
//!     let _ = zdashstitch_application::resolver::collect_source_facts(reader, model).await;
//! }
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;

use zdashstitch_zabbix::{
    GraphDetails, GraphPrototypeDetails, HostDetails, HostObjects, ItemDetails,
    ItemPrototypeDetails, ReadOnlyGateway, ZabbixError, is_zabbix_id,
};

use super::ResolverError;
use super::model::{FieldAddress, SourceKind, SourceModel};
use super::pattern::{is_mask, same_visible_name};

/// Read-only server lookups the resolver needs. Every method maps to one
/// P1-IN-08 allowlisted read (`host.get`, `item.get`, `graph.get`,
/// `itemprototype.get`, `graphprototype.get`). Hidden objects are absent from
/// answers, never errors.
pub trait ResolverLookup {
    fn hosts_by_ids(
        &self,
        hostids: &[String],
    ) -> impl Future<Output = Result<Vec<HostDetails>, ZabbixError>> + Send;

    /// Hosts whose visible name equals one of `names` (case-insensitive);
    /// names never contain `*`.
    fn hosts_by_visible_names(
        &self,
        names: &[String],
    ) -> impl Future<Output = Result<Vec<HostDetails>, ZabbixError>> + Send;

    fn items_by_ids(
        &self,
        itemids: &[String],
    ) -> impl Future<Output = Result<Vec<ItemDetails>, ZabbixError>> + Send;

    fn host_items(
        &self,
        hostid: &str,
    ) -> impl Future<Output = Result<HostObjects<ItemDetails>, ZabbixError>> + Send;

    fn graphs_by_ids(
        &self,
        graphids: &[String],
    ) -> impl Future<Output = Result<Vec<GraphDetails>, ZabbixError>> + Send;

    fn host_graphs(
        &self,
        hostid: &str,
    ) -> impl Future<Output = Result<HostObjects<GraphDetails>, ZabbixError>> + Send;

    fn item_prototypes_by_ids(
        &self,
        itemids: &[String],
    ) -> impl Future<Output = Result<Vec<ItemPrototypeDetails>, ZabbixError>> + Send;

    fn graph_prototypes_by_ids(
        &self,
        graphids: &[String],
    ) -> impl Future<Output = Result<Vec<GraphPrototypeDetails>, ZabbixError>> + Send;
}

impl ResolverLookup for ReadOnlyGateway {
    fn hosts_by_ids(
        &self,
        hostids: &[String],
    ) -> impl Future<Output = Result<Vec<HostDetails>, ZabbixError>> + Send {
        ReadOnlyGateway::hosts_by_ids(self, hostids)
    }

    fn hosts_by_visible_names(
        &self,
        names: &[String],
    ) -> impl Future<Output = Result<Vec<HostDetails>, ZabbixError>> + Send {
        ReadOnlyGateway::hosts_by_visible_names(self, names)
    }

    fn items_by_ids(
        &self,
        itemids: &[String],
    ) -> impl Future<Output = Result<Vec<ItemDetails>, ZabbixError>> + Send {
        ReadOnlyGateway::items_by_ids(self, itemids)
    }

    fn host_items(
        &self,
        hostid: &str,
    ) -> impl Future<Output = Result<HostObjects<ItemDetails>, ZabbixError>> + Send {
        ReadOnlyGateway::host_items(self, hostid)
    }

    fn graphs_by_ids(
        &self,
        graphids: &[String],
    ) -> impl Future<Output = Result<Vec<GraphDetails>, ZabbixError>> + Send {
        ReadOnlyGateway::graphs_by_ids(self, graphids)
    }

    fn host_graphs(
        &self,
        hostid: &str,
    ) -> impl Future<Output = Result<HostObjects<GraphDetails>, ZabbixError>> + Send {
        ReadOnlyGateway::host_graphs(self, hostid)
    }

    fn item_prototypes_by_ids(
        &self,
        itemids: &[String],
    ) -> impl Future<Output = Result<Vec<ItemPrototypeDetails>, ZabbixError>> + Send {
        ReadOnlyGateway::item_prototypes_by_ids(self, itemids)
    }

    fn graph_prototypes_by_ids(
        &self,
        graphids: &[String],
    ) -> impl Future<Output = Result<Vec<GraphPrototypeDetails>, ZabbixError>> + Send {
        ReadOnlyGateway::graph_prototypes_by_ids(self, graphids)
    }
}

/// What the server says about the sources of a model (P2-02A/B/F).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceFacts {
    /// Visible referenced items by `itemid`; hidden or deleted ones absent.
    pub items: BTreeMap<String, ItemDetails>,
    /// Visible referenced classic graphs by `graphid`.
    pub graphs: BTreeMap<String, GraphDetails>,
    /// Visible referenced item prototypes by `itemid`; hidden or deleted ones
    /// absent (their owner stays unverified).
    pub item_prototypes: BTreeMap<String, ItemPrototypeDetails>,
    /// Visible referenced graph prototypes by `graphid`.
    pub graph_prototypes: BTreeMap<String, GraphPrototypeDetails>,
    /// Hosts behind host fields, item owners, graph hosts and exact host
    /// patterns, by `hostid`.
    pub hosts: BTreeMap<String, HostDetails>,
    /// Template name per template item or item prototype ID (`templateid`),
    /// when the template is visible.
    pub template_names: BTreeMap<String, String>,
}

/// What the server says about the source and target hosts of ReplaceHost
/// (P2-02G).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TargetFacts {
    pub source_host: Option<HostDetails>,
    /// `None`: missing or hidden; every row becomes `denied`.
    pub target_host: Option<HostDetails>,
    /// All items of the source host; read only when the model has item
    /// patterns (coverage evidence).
    pub source_items: Vec<ItemDetails>,
    /// All items of the target host.
    pub target_items: Vec<ItemDetails>,
    /// Parent prototypes of discovered target items, by prototype item ID.
    /// Missing entries stay missing/hidden; callers must fail closed rather
    /// than matching LLD instances by a similar key alone.
    pub target_item_prototypes: BTreeMap<String, ItemPrototypeDetails>,
    /// Classic graphs of the target host; read only when the model has
    /// graph sources.
    pub target_graphs: Vec<GraphDetails>,
}

fn lookup_error(error: &ZabbixError) -> ResolverError {
    ResolverError::from_zabbix(error)
}

/// Reads the facts about every source of `model`: items, graphs, their
/// hosts, hosts behind host fields and exact host patterns, and template
/// names of inherited items.
pub async fn collect_source_facts<L: ResolverLookup>(
    lookup: &L,
    model: &SourceModel,
) -> Result<SourceFacts, ResolverError> {
    let mut facts = SourceFacts::default();

    let itemids: Vec<String> = model.itemids().into_iter().collect();
    if !itemids.is_empty() {
        for item in lookup
            .items_by_ids(&itemids)
            .await
            .map_err(|error| lookup_error(&error))?
        {
            facts.items.insert(item.itemid.clone(), item);
        }
    }

    // `selectItemDiscovery` names the parent prototype of every discovered
    // item. Read those prototypes as evidence: a prototype key by itself is
    // not enough to prove that two instances came from the same template.
    let discovery_prototypeids: Vec<String> = facts
        .items
        .values()
        .filter_map(|item| {
            item.discovery
                .as_ref()
                .map(|link| link.parent_itemid.clone())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if !discovery_prototypeids.is_empty() {
        for prototype in lookup
            .item_prototypes_by_ids(&discovery_prototypeids)
            .await
            .map_err(|error| lookup_error(&error))?
        {
            facts
                .item_prototypes
                .insert(prototype.itemid.clone(), prototype);
        }
    }

    let prototype_itemids: Vec<String> = model.item_prototype_ids().into_iter().collect();
    if !prototype_itemids.is_empty() {
        for prototype in lookup
            .item_prototypes_by_ids(&prototype_itemids)
            .await
            .map_err(|error| lookup_error(&error))?
        {
            facts
                .item_prototypes
                .insert(prototype.itemid.clone(), prototype);
        }
    }
    let prototype_graphids: Vec<String> = model.graph_prototype_ids().into_iter().collect();
    if !prototype_graphids.is_empty() {
        for prototype in lookup
            .graph_prototypes_by_ids(&prototype_graphids)
            .await
            .map_err(|error| lookup_error(&error))?
        {
            facts
                .graph_prototypes
                .insert(prototype.graphid.clone(), prototype);
        }
    }

    let templateids: Vec<String> = facts
        .items
        .values()
        .filter_map(|item| item.templateid.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if !templateids.is_empty() {
        for template_item in lookup
            .items_by_ids(&templateids)
            .await
            .map_err(|error| lookup_error(&error))?
        {
            if let Some(name) = template_item.owner_name {
                facts.template_names.insert(template_item.itemid, name);
            }
        }
    }
    // Template prototypes are prototypes too: `item.get` does not return them.
    let template_prototypeids: Vec<String> = facts
        .item_prototypes
        .values()
        .filter_map(|prototype| prototype.templateid.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if !template_prototypeids.is_empty() {
        for template_prototype in lookup
            .item_prototypes_by_ids(&template_prototypeids)
            .await
            .map_err(|error| lookup_error(&error))?
        {
            if let Some(name) = template_prototype.owner_name {
                facts.template_names.insert(template_prototype.itemid, name);
            }
        }
    }

    let graphids: Vec<String> = model.graphids().into_iter().collect();
    if !graphids.is_empty() {
        for graph in lookup
            .graphs_by_ids(&graphids)
            .await
            .map_err(|error| lookup_error(&error))?
        {
            facts.graphs.insert(graph.graphid.clone(), graph);
        }
    }

    let mut hostids = model.host_field_ids();
    hostids.extend(facts.items.values().map(|item| item.hostid.clone()));
    hostids.extend(
        facts
            .graphs
            .values()
            .flat_map(|graph| graph.hostids.iter().cloned()),
    );
    hostids.extend(
        facts
            .item_prototypes
            .values()
            .map(|prototype| prototype.hostid.clone()),
    );
    hostids.extend(
        facts
            .graph_prototypes
            .values()
            .flat_map(|prototype| prototype.hostids.iter().cloned()),
    );
    let hostids: Vec<String> = hostids.into_iter().collect();
    if !hostids.is_empty() {
        for host in lookup
            .hosts_by_ids(&hostids)
            .await
            .map_err(|error| lookup_error(&error))?
        {
            facts.hosts.insert(host.hostid.clone(), host);
        }
    }

    let names: Vec<String> = model.exact_host_patterns().into_iter().collect();
    if !names.is_empty() {
        for host in lookup
            .hosts_by_visible_names(&names)
            .await
            .map_err(|error| lookup_error(&error))?
        {
            facts.hosts.insert(host.hostid.clone(), host);
        }
    }
    Ok(facts)
}

/// Reads the source and target hosts with their items (and graphs when the
/// model has classic graphs). A target that is missing or hidden yields
/// `target_host: None` and no further reads.
pub async fn collect_target_facts<L: ResolverLookup>(
    lookup: &L,
    model: &SourceModel,
    source_hostid: &str,
    target_hostid: &str,
) -> Result<TargetFacts, ResolverError> {
    check_host_pair(source_hostid, target_hostid)?;
    let hosts = lookup
        .hosts_by_ids(&[source_hostid.to_owned(), target_hostid.to_owned()])
        .await
        .map_err(|error| lookup_error(&error))?;
    let mut facts = TargetFacts {
        source_host: hosts
            .iter()
            .find(|host| host.hostid == source_hostid)
            .cloned(),
        target_host: hosts
            .iter()
            .find(|host| host.hostid == target_hostid)
            .cloned(),
        ..TargetFacts::default()
    };
    if facts.target_host.is_none() {
        return Ok(facts);
    }
    facts.target_items = complete(
        lookup
            .host_items(target_hostid)
            .await
            .map_err(|error| lookup_error(&error))?,
    )?;
    let target_prototypeids: Vec<String> = facts
        .target_items
        .iter()
        .filter_map(|item| {
            item.discovery
                .as_ref()
                .map(|link| link.parent_itemid.clone())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if !target_prototypeids.is_empty() {
        for prototype in lookup
            .item_prototypes_by_ids(&target_prototypeids)
            .await
            .map_err(|error| lookup_error(&error))?
        {
            facts
                .target_item_prototypes
                .insert(prototype.itemid.clone(), prototype);
        }
    }
    if model.has_item_patterns() && facts.source_host.is_some() {
        facts.source_items = complete(
            lookup
                .host_items(source_hostid)
                .await
                .map_err(|error| lookup_error(&error))?,
        )?;
    }
    if !model.graphids().is_empty() {
        facts.target_graphs = complete(
            lookup
                .host_graphs(target_hostid)
                .await
                .map_err(|error| lookup_error(&error))?,
        )?;
    }
    Ok(facts)
}

/// A truncated host list would turn «not read» into «missing»: refuse.
fn complete<T>(objects: HostObjects<T>) -> Result<Vec<T>, ResolverError> {
    if objects.truncated {
        return Err(ResolverError::new("resolver_host_objects_limit"));
    }
    Ok(objects.objects)
}

pub(crate) fn check_host_pair(
    source_hostid: &str,
    target_hostid: &str,
) -> Result<(), ResolverError> {
    if !is_zabbix_id(source_hostid) || !is_zabbix_id(target_hostid) {
        return Err(ResolverError::new("resolver_hostid_invalid"));
    }
    if source_hostid == target_hostid {
        return Err(ResolverError::new("replace_host_same_host"));
    }
    Ok(())
}

/// A host referenced by the dashboard, for choosing the host to replace
/// (P1-05A: «заменяемый узел из найденных в панели»).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferencedHost {
    pub hostid: String,
    /// Server data: visible name and technical name.
    pub name: String,
    pub host: String,
    /// Item and item prototype fields whose item belongs to the host.
    pub items: u32,
    /// Classic and prototype graph fields whose graph draws the host.
    pub graphs: u32,
    /// Host fields (`hostid`) naming the host.
    pub host_fields: u32,
    /// Host patterns without `*` equal to the visible name.
    pub host_patterns: u32,
    /// Host masks with `*` that match the visible name.
    pub host_masks: u32,
    /// Distinct widgets with at least one of the references above.
    pub widgets: u32,
}

impl ReferencedHost {
    #[must_use]
    pub const fn references(&self) -> u32 {
        self.items
            .saturating_add(self.graphs)
            .saturating_add(self.host_fields)
            .saturating_add(self.host_patterns)
            .saturating_add(self.host_masks)
    }
}

/// Source whose owner host cannot be determined.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnattributedSource {
    pub field: FieldAddress,
    pub widget_type: String,
    pub kind: SourceKind,
}

/// Hosts referenced by the dashboard and references that cannot be
/// attributed to a host (hidden or deleted objects, prototypes, patterns
/// that name no visible host).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HostInventory {
    /// Sorted by reference count (descending), then visible name.
    pub hosts: Vec<ReferencedHost>,
    pub unattributed: Vec<UnattributedSource>,
}

/// Builds the host inventory of a model from its source facts. Pure.
#[must_use]
pub fn host_inventory(model: &SourceModel, facts: &SourceFacts) -> HostInventory {
    #[derive(Default)]
    struct Counts {
        items: u32,
        graphs: u32,
        host_fields: u32,
        host_patterns: u32,
        host_masks: u32,
        widgets: BTreeSet<usize>,
    }
    #[derive(Clone, Copy)]
    enum Slot {
        Item,
        Graph,
        HostField,
        HostPattern,
        HostMask,
    }
    impl Counts {
        fn add(&mut self, slot: Slot) {
            let counter = match slot {
                Slot::Item => &mut self.items,
                Slot::Graph => &mut self.graphs,
                Slot::HostField => &mut self.host_fields,
                Slot::HostPattern => &mut self.host_patterns,
                Slot::HostMask => &mut self.host_masks,
            };
            *counter = counter.saturating_add(1);
        }
    }
    let mut counts: BTreeMap<&str, Counts> = BTreeMap::new();
    let mut unattributed = Vec::new();
    for (position, widget) in model.widgets.iter().enumerate() {
        for source in &widget.sources {
            let mut owners: Vec<(&str, Slot)> = Vec::new();
            match &source.kind {
                SourceKind::Item { itemid } => match facts.items.get(itemid) {
                    Some(item) if facts.hosts.contains_key(&item.hostid) => {
                        owners.push((item.hostid.as_str(), Slot::Item));
                    }
                    _ => unattributed.push(unattributed_source(widget, source)),
                },
                SourceKind::Graph { graphid } => match facts.graphs.get(graphid) {
                    Some(graph) => {
                        for hostid in &graph.hostids {
                            if facts.hosts.contains_key(hostid) {
                                owners.push((hostid.as_str(), Slot::Graph));
                            }
                        }
                        if owners.is_empty() {
                            unattributed.push(unattributed_source(widget, source));
                        }
                    }
                    None => unattributed.push(unattributed_source(widget, source)),
                },
                SourceKind::Host { hostid } => {
                    if facts.hosts.contains_key(hostid) {
                        owners.push((hostid.as_str(), Slot::HostField));
                    } else {
                        unattributed.push(unattributed_source(widget, source));
                    }
                }
                SourceKind::HostPattern { pattern, .. } => {
                    let mask = is_mask(pattern);
                    for host in facts.hosts.values() {
                        let matched = if mask {
                            super::pattern::name_pattern_matches(pattern, &host.name)
                        } else {
                            same_visible_name(pattern, &host.name)
                        };
                        if matched {
                            if mask {
                                owners.push((host.hostid.as_str(), Slot::HostMask));
                            } else {
                                owners.push((host.hostid.as_str(), Slot::HostPattern));
                            }
                        }
                    }
                    if owners.is_empty() && !mask {
                        unattributed.push(unattributed_source(widget, source));
                    }
                }
                SourceKind::ItemPrototype { itemid } => match facts.item_prototypes.get(itemid) {
                    Some(prototype) if facts.hosts.contains_key(&prototype.hostid) => {
                        owners.push((prototype.hostid.as_str(), Slot::Item));
                    }
                    _ => unattributed.push(unattributed_source(widget, source)),
                },
                SourceKind::GraphPrototype { graphid } => {
                    match facts.graph_prototypes.get(graphid) {
                        Some(prototype) => {
                            for hostid in &prototype.hostids {
                                if facts.hosts.contains_key(hostid) {
                                    owners.push((hostid.as_str(), Slot::Graph));
                                }
                            }
                            if owners.is_empty() {
                                unattributed.push(unattributed_source(widget, source));
                            }
                        }
                        None => unattributed.push(unattributed_source(widget, source)),
                    }
                }
                _ => {}
            }
            for (hostid, slot) in owners {
                let entry = counts.entry(hostid).or_default();
                entry.add(slot);
                entry.widgets.insert(position);
            }
        }
    }
    let mut hosts: Vec<ReferencedHost> = counts
        .into_iter()
        .filter_map(|(hostid, counts)| {
            let host = facts.hosts.get(hostid)?;
            Some(ReferencedHost {
                hostid: host.hostid.clone(),
                name: host.name.clone(),
                host: host.host.clone(),
                items: counts.items,
                graphs: counts.graphs,
                host_fields: counts.host_fields,
                host_patterns: counts.host_patterns,
                host_masks: counts.host_masks,
                widgets: u32::try_from(counts.widgets.len()).unwrap_or(u32::MAX),
            })
        })
        .collect();
    hosts.sort_by(|left, right| {
        right
            .references()
            .cmp(&left.references())
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.hostid.cmp(&right.hostid))
    });
    HostInventory {
        hosts,
        unattributed,
    }
}

fn unattributed_source(
    widget: &super::model::WidgetSources,
    source: &super::model::Source,
) -> UnattributedSource {
    UnattributedSource {
        field: source.field.clone(),
        widget_type: widget.widget_type.clone(),
        kind: source.kind.clone(),
    }
}
