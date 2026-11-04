// SPDX-License-Identifier: MIT OR Apache-2.0

//! Inputs of the ReplaceHost wizard before a calculation exists (P2-03E,
//! P1-05A/B/C): the hosts the draft references with every reference and its
//! expected outcome (09-01 «Зависимости»), and the target host search (09-02
//! «Новый узел», single choice). Read-only; nothing is stored.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;

use serde_json::Value;
use zdashstitch_zabbix::{HostListing, HostObjects, ReadOnlyGateway, ZabbixError, is_zabbix_id};

use super::service::draft_page;
use crate::resolver::{
    Applicability, ReasonCode, ResolverLookup, SourceKind, collect_source_facts, host_inventory,
    is_mask, name_pattern_matches, same_visible_name, source_model,
};
use crate::workcopy::{DraftKind, WorkcopyError, WorkcopyService, is_workcopy_id};

/// Longest target host search text, in characters.
pub const MAX_TARGET_SEARCH_CHARS: usize = zdashstitch_zabbix::MAX_HOST_SEARCH_CHARS;
/// Hosts returned by one target search.
pub const TARGET_SEARCH_LIMIT: usize = 50;

/// A widget of the draft scope (server data), for scope choice and grouping.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InventoryWidget {
    pub widget_index: usize,
    /// Zabbix ID: used in the scope of the calculation only.
    pub widgetid: Option<String>,
    /// Server data; may be empty.
    pub name: String,
    pub widget_type: String,
}

/// A page of the draft scope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InventoryPage {
    /// 0-based position in Base.
    pub page_index: usize,
    pub dashboard_pageid: Option<String>,
    /// Server data; may be empty.
    pub name: String,
    pub widgets: Vec<InventoryWidget>,
}

/// How a widget field refers to a host (P1-05B «как ссылается»).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ReferenceKind {
    /// Item ID owned by the host.
    Item,
    /// Classic graph drawing the host.
    Graph,
    /// Host ID field.
    HostField,
    /// `svggraph` host pattern equal to the visible name.
    HostPattern,
    /// `svggraph` host mask with `*` matching the visible name.
    HostMask,
}

impl ReferenceKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Graph => "graph",
            Self::HostField => "host",
            Self::HostPattern => "host_pattern",
            Self::HostMask => "host_mask",
        }
    }
}

/// What ReplaceHost would do with a reference, before a target is chosen
/// (P1-05B «что будет», O-003). The real result depends on the target and is
/// shown by the calculation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExpectedOutcome {
    /// Editable: changes when the target has a match.
    Changes,
    /// Stays on the replaced host (unsupported widget or field, mask).
    KeptOnSource,
}

impl ExpectedOutcome {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Changes => "changes",
            Self::KeptOnSource => "kept_on_source",
        }
    }
}

/// One reference of a host in the draft scope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostReference {
    pub page_index: usize,
    pub widget_index: usize,
    /// Zabbix field name, e.g. `itemid.0`, `ds.0.hosts.0`.
    pub field: String,
    pub widget_type: String,
    pub kind: ReferenceKind,
    pub outcome: ExpectedOutcome,
    /// Why the reference stays; `None` for `changes`.
    pub reason: Option<ReasonCode>,
}

/// A host referenced by the draft with all its references in Base order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InventoryHost {
    pub hostid: String,
    /// Server data: visible and technical name.
    pub name: String,
    pub host: String,
    pub references: Vec<HostReference>,
    /// Distinct widgets with a reference.
    pub widgets: u32,
}

/// A reference whose owner host cannot be read (hidden or deleted object,
/// prototype, pattern naming no visible host).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnattributedReference {
    pub page_index: usize,
    pub widget_index: usize,
    pub field: String,
    pub widget_type: String,
    /// Source kind code (`item`, `graph`, `host`, `host_pattern`,
    /// `item_prototype`, `graph_prototype`).
    pub kind: &'static str,
}

/// 09-01: the draft scope and the hosts it references.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaceHostInventory {
    pub local_id: String,
    pub kind: DraftKind,
    /// Pages of the draft scope (one for a page draft), Base order.
    pub pages: Vec<InventoryPage>,
    /// Sorted by reference count (descending), then visible name.
    pub hosts: Vec<InventoryHost>,
    pub unattributed: Vec<UnattributedReference>,
}

/// A host offered as the new host (09-02), with its coverage of the
/// templates linked to the replaced host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetHostCandidate {
    pub hostid: String,
    /// Server data.
    pub name: String,
    pub host: String,
    pub groups: Vec<String>,
    /// Names of the templates linked to the host.
    pub templates: Vec<String>,
    /// Templates of the replaced host that are linked to this host too.
    pub source_templates_found: u32,
}

/// Result of a target host search.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetHostSearch {
    /// Sorted by visible name; never contains the replaced host.
    pub hosts: Vec<TargetHostCandidate>,
    /// More hosts matched than one search returns.
    pub truncated: bool,
    /// Names of the templates linked to the replaced host; `None` when the
    /// replaced host is not visible.
    pub source_templates: Option<Vec<String>>,
    /// `not_visible_or_denied` when nothing matched: Zabbix does not tell
    /// «no host» and «no permission» apart (P1-IN-08).
    pub empty_reason: Option<&'static str>,
}

/// Read-only host search for the target picker. Implemented only for
/// [`ReadOnlyGateway`]; tests implement it with fixtures.
pub trait HostSearchLookup {
    fn search_hosts(
        &self,
        search: &str,
        limit: usize,
    ) -> impl Future<Output = Result<HostObjects<HostListing>, ZabbixError>> + Send;

    fn host_listings(
        &self,
        hostids: &[String],
    ) -> impl Future<Output = Result<Vec<HostListing>, ZabbixError>> + Send;
}

impl HostSearchLookup for ReadOnlyGateway {
    fn search_hosts(
        &self,
        search: &str,
        limit: usize,
    ) -> impl Future<Output = Result<HostObjects<HostListing>, ZabbixError>> + Send {
        ReadOnlyGateway::search_hosts(self, search, limit)
    }

    fn host_listings(
        &self,
        hostids: &[String],
    ) -> impl Future<Output = Result<Vec<HostListing>, ZabbixError>> + Send {
        ReadOnlyGateway::host_listings(self, hostids)
    }
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn id(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|id| is_zabbix_id(id))
        .map(str::to_owned)
}

/// Pages and widgets of the draft scope from the raw Base document.
fn scope_pages(raw: &Value, page: Option<usize>) -> Vec<InventoryPage> {
    raw.get("pages")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .enumerate()
        .filter(|(index, _)| page.is_none_or(|draft| draft == *index))
        .map(|(page_index, page)| InventoryPage {
            page_index,
            dashboard_pageid: id(page, "dashboard_pageid"),
            name: text(page, "name"),
            widgets: page
                .get("widgets")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .enumerate()
                .map(|(widget_index, widget)| InventoryWidget {
                    widget_index,
                    widgetid: id(widget, "widgetid"),
                    name: text(widget, "name"),
                    widget_type: text(widget, "type"),
                })
                .collect(),
        })
        .collect()
}

impl WorkcopyService {
    /// Connection profile the draft was copied through: calculations and
    /// host searches run only through that connection.
    pub fn draft_connection_id(&self, local_id: &str) -> Result<String, WorkcopyError> {
        if !is_workcopy_id(local_id) {
            return Err(WorkcopyError::new("workcopy_id_invalid"));
        }
        Ok(self.load(local_id)?.source.connection_id)
    }

    /// P1-05A/B `bindings.inventory`: the hosts the draft scope references,
    /// each with its references and the expected outcome, plus references
    /// whose owner cannot be read. Read-only (`host.get`, `item.get`,
    /// `graph.get`); stores nothing.
    pub async fn replace_host_inventory<L: ResolverLookup>(
        &self,
        lookup: &L,
        local_id: &str,
    ) -> Result<ReplaceHostInventory, WorkcopyError> {
        if !is_workcopy_id(local_id) {
            return Err(WorkcopyError::new("workcopy_id_invalid"));
        }
        let (file, base) = self.load_with_base(local_id)?;
        let raw = base.raw_api_document();
        let page = draft_page(&file, raw)?.map(|(index, _)| index);
        let model = source_model(&base, &self.registry)
            .map_err(|error| WorkcopyError::from_resolver(&error))?;
        let facts = collect_source_facts(lookup, &model)
            .await
            .map_err(|error| WorkcopyError::from_resolver(&error))?;
        let in_scope = |page_index: usize| page.is_none_or(|draft| draft == page_index);

        let mut references: BTreeMap<&str, Vec<HostReference>> = BTreeMap::new();
        for widget in &model.widgets {
            if !in_scope(widget.widget.page_index) {
                continue;
            }
            for source in &widget.sources {
                let mut owners: Vec<(&str, ReferenceKind)> = Vec::new();
                match &source.kind {
                    SourceKind::Item { itemid } => {
                        if let Some(item) = facts.items.get(itemid) {
                            owners.push((item.hostid.as_str(), ReferenceKind::Item));
                        }
                    }
                    SourceKind::Graph { graphid } => {
                        if let Some(graph) = facts.graphs.get(graphid) {
                            owners.extend(
                                graph
                                    .hostids
                                    .iter()
                                    .map(|hostid| (hostid.as_str(), ReferenceKind::Graph)),
                            );
                        }
                    }
                    SourceKind::Host { hostid } => {
                        owners.push((hostid.as_str(), ReferenceKind::HostField));
                    }
                    SourceKind::HostPattern { pattern, .. } => {
                        let mask = is_mask(pattern);
                        for host in facts.hosts.values() {
                            if mask && name_pattern_matches(pattern, &host.name) {
                                owners.push((host.hostid.as_str(), ReferenceKind::HostMask));
                            } else if !mask && same_visible_name(pattern, &host.name) {
                                owners.push((host.hostid.as_str(), ReferenceKind::HostPattern));
                            }
                        }
                    }
                    _ => {}
                }
                for (hostid, kind) in owners {
                    if !facts.hosts.contains_key(hostid) {
                        continue;
                    }
                    let (outcome, reason) = if kind == ReferenceKind::HostMask {
                        let reason = if matches!(&source.kind, SourceKind::HostPattern { pattern, .. } if pattern.trim() == "*")
                        {
                            ReasonCode::HostWildcardAll
                        } else {
                            ReasonCode::HostMask
                        };
                        (ExpectedOutcome::KeptOnSource, Some(reason))
                    } else if source.applicability == Applicability::Editable {
                        (ExpectedOutcome::Changes, None)
                    } else {
                        (
                            ExpectedOutcome::KeptOnSource,
                            Some(
                                source
                                    .applicability_reason
                                    .unwrap_or(ReasonCode::FieldNotReplaceable),
                            ),
                        )
                    };
                    references.entry(hostid).or_default().push(HostReference {
                        page_index: source.field.widget.page_index,
                        widget_index: source.field.widget.widget_index,
                        field: source.field.field_name.clone(),
                        widget_type: widget.widget_type.clone(),
                        kind,
                        outcome,
                        reason,
                    });
                }
            }
        }

        // Host order and the unattributed list follow the resolver inventory.
        let inventory = host_inventory(&model, &facts);
        let hosts = inventory
            .hosts
            .iter()
            .filter_map(|host| {
                let references = references.remove(host.hostid.as_str())?;
                let widgets: BTreeSet<(usize, usize)> = references
                    .iter()
                    .map(|reference| (reference.page_index, reference.widget_index))
                    .collect();
                Some(InventoryHost {
                    hostid: host.hostid.clone(),
                    name: host.name.clone(),
                    host: host.host.clone(),
                    widgets: u32::try_from(widgets.len()).unwrap_or(u32::MAX),
                    references,
                })
            })
            .collect();
        let unattributed = inventory
            .unattributed
            .iter()
            .filter(|source| in_scope(source.field.widget.page_index))
            .map(|source| UnattributedReference {
                page_index: source.field.widget.page_index,
                widget_index: source.field.widget.widget_index,
                field: source.field.field_name.clone(),
                widget_type: source.widget_type.clone(),
                kind: source.kind.as_str(),
            })
            .collect();
        Ok(ReplaceHostInventory {
            local_id: file.workcopy_id.clone(),
            kind: file.kind,
            pages: scope_pages(raw, page),
            hosts,
            unattributed,
        })
    }

    /// P1-05C `bindings.targets.search` for ReplaceHost (single choice):
    /// visible hosts whose name contains `search`, without the replaced
    /// host, with coverage of the replaced host's templates. Read-only
    /// (`host.get`). Errors: `workcopy_id_invalid`, `resolver_hostid_invalid`,
    /// `search_invalid` (param `max`), Zabbix read codes.
    pub async fn search_replace_host_targets<L: HostSearchLookup>(
        &self,
        lookup: &L,
        local_id: &str,
        source_hostid: &str,
        search: &str,
    ) -> Result<TargetHostSearch, WorkcopyError> {
        if !is_workcopy_id(local_id) {
            return Err(WorkcopyError::new("workcopy_id_invalid"));
        }
        if !is_zabbix_id(source_hostid) {
            return Err(WorkcopyError::new("resolver_hostid_invalid"));
        }
        let search = search.trim();
        if search.chars().count() > MAX_TARGET_SEARCH_CHARS || search.chars().any(char::is_control)
        {
            return Err(WorkcopyError::new("search_invalid")
                .with("max", MAX_TARGET_SEARCH_CHARS.to_string()));
        }
        // The draft must exist; its Base is not needed.
        self.load(local_id)?;
        let source = lookup
            .host_listings(&[source_hostid.to_owned()])
            .await
            .map_err(|error| WorkcopyError::from_zabbix(&error))?
            .into_iter()
            .find(|host| host.hostid == source_hostid);
        let found = lookup
            .search_hosts(search, TARGET_SEARCH_LIMIT + 1)
            .await
            .map_err(|error| WorkcopyError::from_zabbix(&error))?;
        let source_ids: BTreeSet<&str> = source
            .as_ref()
            .map(|host| {
                host.templates
                    .iter()
                    .map(|template| template.templateid.as_str())
                    .collect()
            })
            .unwrap_or_default();
        let mut hosts: Vec<TargetHostCandidate> = found
            .objects
            .iter()
            .filter(|host| host.hostid != source_hostid)
            .map(|host| TargetHostCandidate {
                hostid: host.hostid.clone(),
                name: host.name.clone(),
                host: host.host.clone(),
                groups: host.groups.clone(),
                templates: host
                    .templates
                    .iter()
                    .map(|template| template.name.clone())
                    .collect(),
                source_templates_found: u32::try_from(
                    host.templates
                        .iter()
                        .filter(|template| source_ids.contains(template.templateid.as_str()))
                        .count(),
                )
                .unwrap_or(u32::MAX),
            })
            .collect();
        let truncated = found.truncated || hosts.len() > TARGET_SEARCH_LIMIT;
        hosts.truncate(TARGET_SEARCH_LIMIT);
        let empty_reason = hosts.is_empty().then_some("not_visible_or_denied");
        Ok(TargetHostSearch {
            hosts,
            truncated,
            source_templates: source.map(|host| {
                host.templates
                    .into_iter()
                    .map(|template| template.name)
                    .collect()
            }),
            empty_reason,
        })
    }
}
