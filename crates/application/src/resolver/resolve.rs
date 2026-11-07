// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-02B/C/D/F/G: resolving every reference of a source host on a target
//! host with evidence. Pure: input is the source model plus facts read
//! earlier; nothing is read or written here.
//!
//! Rules (O-003, P1-IN-02, P1-IN-03):
//!
//! - an item is matched only by exact `key_` on the target host, then
//!   `value_type`, units and template origin are compared; a similar name is
//!   never evidence;
//! - an LLD instance without the same key on the target is
//!   `unresolved_lld`; instances of the same prototype are candidates and
//!   never chosen automatically;
//! - an `svggraph` data-set host pattern without `*` equal to the source
//!   host's visible name (case-insensitive) becomes the target's visible
//!   name; masks and a single `*` are kept on the source with a reason;
//!   item patterns never become item IDs;
//! - a classic graph is matched by the same name from the same template; it
//!   is editable when the adapter supports `classic_graph_graphid` for the
//!   Base version (P2-04H, 7.4.16), otherwise `unsupported`. A same-name
//!   graph of another template (or of none) is only `compatible` and never
//!   applied automatically (`origin_differs` blocks the change);
//! - host fields and patterns outside data sets are kept on the source.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use zdashstitch_zabbix::{GraphDetails, HostDetails, ItemDetails, ItemFlags, ItemPrototypeDetails};

use super::ResolverError;
use super::links::ScopeCrossing;
use super::lookup::{SourceFacts, TargetFacts, check_host_pair};
use super::model::{
    FieldAddress, PatternPlace, Source, SourceKind, SourceModel, SvgDataSet, WidgetAddress,
    WidgetSources,
};
use super::pattern::{is_mask, name_pattern_matches, same_visible_name};
use super::status::{Applicability, Difference, MatchStatus, ReasonCode};

/// Item names listed per pattern as coverage candidates.
pub const MAX_PATTERN_CANDIDATES: usize = 20;
/// LLD instances listed as candidates per unresolved row.
pub const MAX_LLD_CANDIDATES: usize = 50;

/// Part of Base an operation covers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ResolveScope {
    /// Every page (the default for ReplaceHost).
    #[default]
    Dashboard,
    /// Pages by `dashboard_pageid`.
    Pages(BTreeSet<String>),
    /// Widgets by `widgetid`.
    Widgets(BTreeSet<String>),
}

impl ResolveScope {
    #[must_use]
    pub fn contains(&self, widget: &WidgetAddress) -> bool {
        match self {
            Self::Dashboard => true,
            Self::Pages(pages) => widget
                .dashboard_pageid
                .as_ref()
                .is_some_and(|pageid| pages.contains(pageid)),
            Self::Widgets(widgets) => widget
                .widgetid
                .as_ref()
                .is_some_and(|widgetid| widgets.contains(widgetid)),
        }
    }
}

/// ReplaceHost input: explicit source host, target host and scope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaceHostRequest {
    pub source_hostid: String,
    pub target_hostid: String,
    pub scope: ResolveScope,
}

/// The referencing value in Base.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceValue {
    Item {
        itemid: String,
    },
    /// Prototype fields are never replaced (O-003): the row is kept on the
    /// source; the prototype is read only to learn its owner.
    ItemPrototype {
        itemid: String,
    },
    Graph {
        graphid: String,
    },
    GraphPrototype {
        graphid: String,
    },
    Host {
        hostid: String,
    },
    HostPattern {
        pattern: String,
    },
}

/// The value the field would get on the target.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetValue {
    Item { itemid: String },
    Graph { graphid: String },
    Host { hostid: String },
    HostPattern { pattern: String },
}

/// Where an item comes from.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemOrigin {
    /// Created on the host itself.
    Own,
    /// Inherited from a template item.
    Template {
        template_itemid: String,
        /// Server data; `None` when the template is not visible.
        template_name: Option<String>,
    },
    /// LLD instance of a prototype.
    Discovered { prototype_key: String },
}

/// Item evidence shown to the user (P1-05D: key, type, units, template).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ItemEvidence {
    pub itemid: String,
    pub key: String,
    /// Server data, shown but never used as matching evidence.
    pub name: String,
    pub value_type: u8,
    pub units: String,
    pub origin: ItemOrigin,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphEvidence {
    pub graphid: String,
    pub name: String,
    pub template_graphid: Option<String>,
    pub discovered: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Evidence {
    Item(ItemEvidence),
    Graph(GraphEvidence),
}

/// How one item pattern of a data set behaves on source and target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatternCoverage {
    pub pattern: String,
    /// Items of the source host whose visible name matches.
    pub source_matches: u32,
    pub target_matches: u32,
    /// Names of matching target items, at most [`MAX_PATTERN_CANDIDATES`];
    /// evidence only, never turned into item IDs.
    pub target_candidates: Vec<String>,
}

/// One reference of the source host inside the scope and its resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolutionRow {
    pub field: FieldAddress,
    pub widget_type: String,
    pub source: SourceValue,
    /// `None` when there is no single target.
    pub target: Option<TargetValue>,
    pub match_status: MatchStatus,
    pub applicability: Applicability,
    /// Sorted, unique.
    pub reasons: Vec<ReasonCode>,
    pub differences: Vec<Difference>,
    pub source_evidence: Option<Evidence>,
    pub target_evidence: Option<Evidence>,
    /// Options for a manual decision (LLD instances of the same prototype,
    /// several matches); never selected automatically.
    pub candidates: Vec<ItemEvidence>,
    /// Item patterns of the data set, for data-set host pattern rows.
    pub pattern_coverage: Vec<PatternCoverage>,
}

impl ResolutionRow {
    /// The publication would change this field to `target`.
    #[must_use]
    pub fn will_change(&self) -> bool {
        self.applicability == Applicability::Editable
            && self.match_status.is_resolved()
            && self.target.is_some()
            && !self
                .reasons
                .iter()
                .any(|reason| reason.blocks_change_of(&self.widget_type))
    }

    /// An editable reference without an applicable target: needs a decision
    /// (candidate, LLD instance, keep as is) before publication.
    #[must_use]
    pub fn needs_decision(&self) -> bool {
        self.applicability == Applicability::Editable && !self.will_change()
    }

    /// The reference stays on the source host whatever is decided.
    #[must_use]
    pub fn is_kept_on_source(&self) -> bool {
        self.applicability != Applicability::Editable
    }
}

/// A reference to the source host that ReplaceHost leaves unchanged (O-003):
/// the plan lists it and publication requires confirming the list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeptOnSource {
    pub field: FieldAddress,
    pub widget_type: String,
    pub source: SourceValue,
    pub reason: ReasonCode,
}

/// A source in scope whose owner cannot be determined, so it may or may not
/// reference the source host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnverifiedSource {
    pub field: FieldAddress,
    pub widget_type: String,
    pub kind: SourceKind,
    pub reason: ReasonCode,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResolutionSummary {
    pub rows: u32,
    pub will_change: u32,
    pub needs_decision: u32,
    pub kept_on_source: u32,
    pub unverified: u32,
    /// References of the source host outside the scope (P1-05A warning).
    pub out_of_scope: u32,
    /// Widgets in scope with at least one changing field.
    pub changed_widgets: u32,
}

/// Result of resolving ReplaceHost on a source model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaceHostResolution {
    pub dashboardid: String,
    pub snapshot_fingerprint: String,
    pub source_host: HostDetails,
    pub target_host: Option<HostDetails>,
    pub rows: Vec<ResolutionRow>,
    pub kept_on_source: Vec<KeptOnSource>,
    pub unverified: Vec<UnverifiedSource>,
    pub scope_crossings: Vec<ScopeCrossing>,
    pub summary: ResolutionSummary,
}

/// Resolves every reference of `request.source_hostid` in scope. Pure.
///
/// Errors: `resolver_hostid_invalid`, `replace_host_same_host`,
/// `source_host_not_found_or_no_access` (the source host is needed for its
/// visible name), `resolver_facts_mismatch` (target facts read for other
/// hosts).
pub fn resolve_replace_host(
    model: &SourceModel,
    request: &ReplaceHostRequest,
    source: &SourceFacts,
    target: &TargetFacts,
) -> Result<ReplaceHostResolution, ResolverError> {
    check_host_pair(&request.source_hostid, &request.target_hostid)?;
    let source_host = target
        .source_host
        .clone()
        .ok_or_else(|| ResolverError::new("source_host_not_found_or_no_access"))?;
    if source_host.hostid != request.source_hostid
        || target
            .target_host
            .as_ref()
            .is_some_and(|host| host.hostid != request.target_hostid)
    {
        return Err(ResolverError::new("resolver_facts_mismatch"));
    }
    let context = Context {
        request,
        source,
        target,
        source_host: &source_host,
    };

    let mut rows = Vec::new();
    let mut unverified = Vec::new();
    let mut out_of_scope = 0u32;
    for widget in &model.widgets {
        let in_scope = request.scope.contains(&widget.widget);
        for item in &widget.sources {
            match context.relation(item) {
                Relation::Unrelated => {}
                Relation::Unknown(reason) => {
                    if in_scope {
                        unverified.push(UnverifiedSource {
                            field: item.field.clone(),
                            widget_type: widget.widget_type.clone(),
                            kind: item.kind.clone(),
                            reason,
                        });
                    }
                }
                Relation::SourceHost => {
                    if in_scope {
                        if let Some(row) = context.row(widget, item) {
                            rows.push(row);
                        }
                    } else {
                        out_of_scope = out_of_scope.saturating_add(1);
                    }
                }
            }
        }
    }

    let kept_on_source: Vec<KeptOnSource> = rows
        .iter()
        .filter(|row| row.is_kept_on_source())
        .map(|row| KeptOnSource {
            field: row.field.clone(),
            widget_type: row.widget_type.clone(),
            source: row.source.clone(),
            reason: kept_reason(row),
        })
        .collect();
    let changed_widgets: BTreeSet<&WidgetAddress> = rows
        .iter()
        .filter(|row| row.will_change())
        .map(|row| &row.field.widget)
        .collect();
    let summary = ResolutionSummary {
        rows: count(rows.len()),
        will_change: count(rows.iter().filter(|row| row.will_change()).count()),
        needs_decision: count(rows.iter().filter(|row| row.needs_decision()).count()),
        kept_on_source: count(kept_on_source.len()),
        unverified: count(unverified.len()),
        out_of_scope,
        changed_widgets: count(changed_widgets.len()),
    };
    Ok(ReplaceHostResolution {
        dashboardid: model.dashboardid.clone(),
        snapshot_fingerprint: model.snapshot_fingerprint.clone(),
        source_host,
        target_host: target.target_host.clone(),
        rows,
        kept_on_source,
        unverified,
        scope_crossings: model.links.crossing(&request.scope),
        summary,
    })
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// The reason a kept row is listed with: the applicability reason first
/// (masks, host fields, pending contracts).
fn kept_reason(row: &ResolutionRow) -> ReasonCode {
    const PRIORITY: [ReasonCode; 9] = [
        ReasonCode::HostWildcardAll,
        ReasonCode::HostMask,
        ReasonCode::ClassicGraphContractPending,
        ReasonCode::HostFieldNotSupported,
        ReasonCode::PatternFieldNotSupported,
        ReasonCode::WidgetNotEditable,
        ReasonCode::UnknownWidget,
        ReasonCode::UnknownField,
        ReasonCode::FieldNotReplaceable,
    ];
    PRIORITY
        .into_iter()
        .find(|reason| row.reasons.contains(reason))
        .unwrap_or(ReasonCode::FieldNotReplaceable)
}

enum Relation {
    SourceHost,
    Unrelated,
    Unknown(ReasonCode),
}

struct Context<'a> {
    request: &'a ReplaceHostRequest,
    source: &'a SourceFacts,
    target: &'a TargetFacts,
    source_host: &'a HostDetails,
}

impl Context<'_> {
    fn relation(&self, source: &Source) -> Relation {
        let source_hostid = self.request.source_hostid.as_str();
        match &source.kind {
            SourceKind::Item { itemid } => match self.source.items.get(itemid) {
                Some(item) if item.hostid == source_hostid => Relation::SourceHost,
                Some(_) => Relation::Unrelated,
                None => Relation::Unknown(ReasonCode::NotFoundOrNoAccess),
            },
            SourceKind::Graph { graphid } => match self.source.graphs.get(graphid) {
                Some(graph) if graph.hostids.iter().any(|id| id == source_hostid) => {
                    Relation::SourceHost
                }
                Some(_) => Relation::Unrelated,
                None => Relation::Unknown(ReasonCode::NotFoundOrNoAccess),
            },
            SourceKind::Host { hostid } if hostid == source_hostid => Relation::SourceHost,
            SourceKind::HostPattern { pattern, .. }
                if name_pattern_matches(pattern, &self.source_host.name) =>
            {
                Relation::SourceHost
            }
            // Owner of a prototype (host or template) is read with
            // `itemprototype.get` / `graphprototype.get`; a prototype that is
            // not visible stays unverified.
            SourceKind::ItemPrototype { itemid } => match self.source.item_prototypes.get(itemid) {
                Some(prototype) if prototype.hostid == source_hostid => Relation::SourceHost,
                Some(_) => Relation::Unrelated,
                None => Relation::Unknown(ReasonCode::PrototypeOwnerUnknown),
            },
            SourceKind::GraphPrototype { graphid } => {
                match self.source.graph_prototypes.get(graphid) {
                    Some(prototype) if prototype.hostids.iter().any(|id| id == source_hostid) => {
                        Relation::SourceHost
                    }
                    Some(_) => Relation::Unrelated,
                    None => Relation::Unknown(ReasonCode::PrototypeOwnerUnknown),
                }
            }
            _ => Relation::Unrelated,
        }
    }

    fn row(&self, widget: &WidgetSources, source: &Source) -> Option<ResolutionRow> {
        let mut row = ResolutionRow {
            field: source.field.clone(),
            widget_type: widget.widget_type.clone(),
            source: SourceValue::Item {
                itemid: String::new(),
            },
            target: None,
            match_status: MatchStatus::Denied,
            applicability: source.applicability,
            reasons: source.applicability_reason.into_iter().collect(),
            differences: Vec::new(),
            source_evidence: None,
            target_evidence: None,
            candidates: Vec::new(),
            pattern_coverage: Vec::new(),
        };
        match &source.kind {
            SourceKind::Item { itemid } => {
                let item = self.source.items.get(itemid)?;
                row.source = SourceValue::Item {
                    itemid: itemid.clone(),
                };
                self.resolve_item(&mut row, item);
            }
            SourceKind::Graph { graphid } => {
                let graph = self.source.graphs.get(graphid)?;
                row.source = SourceValue::Graph {
                    graphid: graphid.clone(),
                };
                self.resolve_graph(&mut row, graph);
            }
            SourceKind::ItemPrototype { itemid } => {
                let prototype = self.source.item_prototypes.get(itemid)?;
                row.source = SourceValue::ItemPrototype {
                    itemid: itemid.clone(),
                };
                self.keep_prototype(&mut row);
                row.source_evidence = Some(Evidence::Item(self.prototype_evidence(prototype)));
            }
            SourceKind::GraphPrototype { graphid } => {
                let prototype = self.source.graph_prototypes.get(graphid)?;
                row.source = SourceValue::GraphPrototype {
                    graphid: graphid.clone(),
                };
                self.keep_prototype(&mut row);
                row.source_evidence = Some(Evidence::Graph(GraphEvidence {
                    graphid: prototype.graphid.clone(),
                    name: prototype.name.clone(),
                    template_graphid: prototype.templateid.clone(),
                    discovered: false,
                }));
            }
            SourceKind::Host { hostid } => {
                row.source = SourceValue::Host {
                    hostid: hostid.clone(),
                };
                if let Some(target) = &self.target.target_host {
                    row.target = Some(TargetValue::Host {
                        hostid: target.hostid.clone(),
                    });
                    row.match_status = MatchStatus::Exact;
                } else {
                    row.reasons.push(ReasonCode::NotFoundOrNoAccess);
                }
            }
            SourceKind::HostPattern { pattern, place } => {
                row.source = SourceValue::HostPattern {
                    pattern: pattern.clone(),
                };
                self.resolve_host_pattern(&mut row, widget, pattern, *place);
            }
            _ => return None,
        }
        row.reasons.sort_unstable();
        row.reasons.dedup();
        row.differences.sort_unstable();
        row.differences.dedup();
        Some(row)
    }

    /// A prototype of the source host is referenced, but ReplaceHost does not
    /// change prototype fields (O-003); there is no target to choose.
    fn keep_prototype(&self, row: &mut ResolutionRow) {
        if row.applicability == Applicability::Editable {
            row.applicability = Applicability::ReadOnly;
        }
        row.match_status = MatchStatus::Ambiguous;
        if row.reasons.is_empty() {
            row.reasons.push(ReasonCode::FieldNotReplaceable);
        }
    }

    fn prototype_evidence(&self, prototype: &ItemPrototypeDetails) -> ItemEvidence {
        let origin = match &prototype.templateid {
            Some(template_itemid) => ItemOrigin::Template {
                template_itemid: template_itemid.clone(),
                template_name: self.source.template_names.get(template_itemid).cloned(),
            },
            None => ItemOrigin::Own,
        };
        ItemEvidence {
            itemid: prototype.itemid.clone(),
            key: prototype.key.clone(),
            name: prototype.name.clone(),
            value_type: prototype.value_type,
            units: prototype.units.clone(),
            origin,
        }
    }

    fn resolve_item(&self, row: &mut ResolutionRow, item: &ItemDetails) {
        row.source_evidence = Some(Evidence::Item(self.item_evidence(item)));
        if self.target.target_host.is_none() {
            row.match_status = MatchStatus::Denied;
            row.reasons.push(ReasonCode::NotFoundOrNoAccess);
            return;
        }
        let same_key: Vec<&ItemDetails> = self
            .target
            .target_items
            .iter()
            .filter(|candidate| candidate.key == item.key)
            .collect();
        match same_key.as_slice() {
            [found] => {
                row.target = Some(TargetValue::Item {
                    itemid: found.itemid.clone(),
                });
                row.target_evidence = Some(Evidence::Item(self.item_evidence(found)));
                compare_items(row, item, found);
            }
            [] => {
                if item.flags == ItemFlags::Discovered {
                    row.match_status = MatchStatus::UnresolvedLld;
                    row.reasons.push(ReasonCode::LldInstanceMissing);
                    row.candidates = self.lld_candidates(item);
                } else {
                    row.match_status = MatchStatus::Missing;
                    row.reasons.push(ReasonCode::KeyNotFound);
                }
            }
            several => {
                row.match_status = MatchStatus::Ambiguous;
                row.reasons.push(ReasonCode::SeveralTargetCandidates);
                row.candidates = several
                    .iter()
                    .map(|candidate| self.item_evidence(candidate))
                    .collect();
            }
        }
        if row.widget_type == "svggraph"
            && let Some(Evidence::Item(target)) = &row.target_evidence
            && !is_numeric(target.value_type)
        {
            row.reasons.push(ReasonCode::ValueTypeIncompatible);
        }
    }

    /// Discovered items of the target created from the same template
    /// prototype. A matching prototype key alone is not evidence: unrelated
    /// discovery rules may expose the same text. Evidence for a manual choice
    /// only; this function never selects a candidate.
    fn lld_candidates(&self, item: &ItemDetails) -> Vec<ItemEvidence> {
        let Some(link) = &item.discovery else {
            return Vec::new();
        };
        let Some(source_prototype) = self.source.item_prototypes.get(&link.parent_itemid) else {
            return Vec::new();
        };
        let source_identity = source_prototype
            .templateid
            .as_deref()
            .unwrap_or(source_prototype.itemid.as_str());
        let mut candidates: Vec<ItemEvidence> = self
            .target
            .target_items
            .iter()
            .filter(|candidate| {
                let Some(other) = candidate.discovery.as_ref() else {
                    return false;
                };
                let Some(target_prototype) =
                    self.target.target_item_prototypes.get(&other.parent_itemid)
                else {
                    return false;
                };
                let target_identity = target_prototype
                    .templateid
                    .as_deref()
                    .unwrap_or(target_prototype.itemid.as_str());
                candidate.flags == ItemFlags::Discovered
                    && source_prototype.key == link.prototype_key
                    && target_prototype.key == other.prototype_key
                    && source_prototype.key == target_prototype.key
                    && source_identity == target_identity
            })
            .map(|candidate| self.item_evidence(candidate))
            .collect();
        candidates.sort_by(|left, right| left.key.cmp(&right.key));
        candidates.truncate(MAX_LLD_CANDIDATES);
        candidates
    }

    fn resolve_graph(&self, row: &mut ResolutionRow, graph: &GraphDetails) {
        row.source_evidence = Some(Evidence::Graph(graph_evidence(graph)));
        if self.target.target_host.is_none() {
            row.match_status = MatchStatus::Denied;
            row.reasons.push(ReasonCode::NotFoundOrNoAccess);
            return;
        }
        let same_name: Vec<&GraphDetails> = self
            .target
            .target_graphs
            .iter()
            .filter(|candidate| candidate.name == graph.name)
            .collect();
        let same_template: Vec<&GraphDetails> = same_name
            .iter()
            .copied()
            .filter(|candidate| {
                graph.templateid.is_some() && candidate.templateid == graph.templateid
            })
            .collect();
        let (chosen, origin_differs) = match (same_template.as_slice(), same_name.as_slice()) {
            ([found], _) => (Some(*found), false),
            ([], [found]) => (Some(*found), true),
            ([], []) => (None, false),
            _ => {
                row.match_status = MatchStatus::Ambiguous;
                row.reasons.push(ReasonCode::SeveralTargetCandidates);
                return;
            }
        };
        match chosen {
            Some(found) => {
                row.target = Some(TargetValue::Graph {
                    graphid: found.graphid.clone(),
                });
                row.target_evidence = Some(Evidence::Graph(graph_evidence(found)));
                if origin_differs {
                    row.match_status = MatchStatus::Compatible;
                    row.differences.push(Difference::Origin);
                    row.reasons.push(ReasonCode::OriginDiffers);
                } else {
                    row.match_status = MatchStatus::Exact;
                }
            }
            None if graph.discovered => {
                row.match_status = MatchStatus::UnresolvedLld;
                row.reasons.push(ReasonCode::LldInstanceMissing);
            }
            None => {
                row.match_status = MatchStatus::Missing;
                row.reasons.push(ReasonCode::GraphNameNotFound);
            }
        }
    }

    fn resolve_host_pattern(
        &self,
        row: &mut ResolutionRow,
        widget: &WidgetSources,
        pattern: &str,
        place: PatternPlace,
    ) {
        if is_mask(pattern) {
            // O-003: masks are never rewritten by ReplaceHost.
            row.applicability = Applicability::ReadOnly;
            row.match_status = MatchStatus::Ambiguous;
            row.reasons.push(if pattern.trim() == "*" {
                ReasonCode::HostWildcardAll
            } else {
                ReasonCode::HostMask
            });
            return;
        }
        debug_assert!(same_visible_name(pattern, &self.source_host.name));
        let Some(target_host) = &self.target.target_host else {
            row.match_status = MatchStatus::Denied;
            row.reasons.push(ReasonCode::NotFoundOrNoAccess);
            return;
        };
        row.target = Some(TargetValue::HostPattern {
            pattern: target_host.name.clone(),
        });
        row.match_status = MatchStatus::Exact;
        if is_mask(&target_host.name) {
            row.match_status = MatchStatus::Ambiguous;
            row.reasons.push(ReasonCode::TargetNameIsMask);
            return;
        }
        let PatternPlace::DataSet(index) = place else {
            return;
        };
        let Some(dataset) = widget.datasets.iter().find(|set| set.index == index) else {
            return;
        };
        self.apply_coverage(row, dataset);
    }

    /// Item patterns stay as they are; their match counts on both hosts are
    /// evidence of what the series will show after the host change.
    fn apply_coverage(&self, row: &mut ResolutionRow, dataset: &SvgDataSet) {
        let mut source_total = 0u32;
        let mut target_total = 0u32;
        let mut differs = false;
        for pattern in &dataset.item_patterns {
            let source_matches = count(
                self.target
                    .source_items
                    .iter()
                    .filter(|item| name_pattern_matches(pattern, &item.name))
                    .count(),
            );
            let mut target_names: Vec<&str> = self
                .target
                .target_items
                .iter()
                .filter(|item| name_pattern_matches(pattern, &item.name))
                .map(|item| item.name.as_str())
                .collect();
            target_names.sort_unstable();
            let target_matches = count(target_names.len());
            differs |= source_matches != target_matches;
            source_total = source_total.saturating_add(source_matches);
            target_total = target_total.saturating_add(target_matches);
            row.pattern_coverage.push(PatternCoverage {
                pattern: pattern.clone(),
                source_matches,
                target_matches,
                target_candidates: target_names
                    .into_iter()
                    .take(MAX_PATTERN_CANDIDATES)
                    .map(str::to_owned)
                    .collect(),
            });
        }
        if target_total == 0 && source_total > 0 {
            row.match_status = MatchStatus::Missing;
            row.reasons.push(ReasonCode::ItemPatternsWithoutTargetItems);
        } else if differs {
            row.match_status = MatchStatus::Compatible;
            row.differences.push(Difference::PatternCoverage);
            row.reasons.push(ReasonCode::PatternCoverageDiffers);
        }
    }

    fn item_evidence(&self, item: &ItemDetails) -> ItemEvidence {
        let origin = match (&item.templateid, &item.discovery) {
            (Some(template_itemid), _) => ItemOrigin::Template {
                template_itemid: template_itemid.clone(),
                template_name: self.source.template_names.get(template_itemid).cloned(),
            },
            (None, Some(link)) if item.flags == ItemFlags::Discovered => ItemOrigin::Discovered {
                prototype_key: link.prototype_key.clone(),
            },
            _ => ItemOrigin::Own,
        };
        ItemEvidence {
            itemid: item.itemid.clone(),
            key: item.key.clone(),
            name: item.name.clone(),
            value_type: item.value_type,
            units: item.units.clone(),
            origin,
        }
    }
}

fn graph_evidence(graph: &GraphDetails) -> GraphEvidence {
    GraphEvidence {
        graphid: graph.graphid.clone(),
        name: graph.name.clone(),
        template_graphid: graph.templateid.clone(),
        discovered: graph.discovered,
    }
}

/// `value_type` class: 0 float and 3 unsigned are numeric; 1 character,
/// 2 log and 4 text are text; 5 binary stands alone.
const fn value_class(value_type: u8) -> u8 {
    match value_type {
        0 | 3 => 0,
        1 | 2 | 4 => 1,
        _ => 2,
    }
}

const fn is_numeric(value_type: u8) -> bool {
    value_class(value_type) == 0
}

/// Same key found: compare value type, units and origin (P1-IN-03).
fn compare_items(row: &mut ResolutionRow, source: &ItemDetails, target: &ItemDetails) {
    if source.value_type != target.value_type {
        row.differences.push(Difference::ValueType);
        row.reasons.push(
            if value_class(source.value_type) == value_class(target.value_type) {
                ReasonCode::ValueTypeDiffers
            } else {
                ReasonCode::ValueTypeIncompatible
            },
        );
    }
    if source.units != target.units {
        row.differences.push(Difference::Units);
        row.reasons.push(ReasonCode::UnitsDiffer);
    }
    let same_origin = match (&source.templateid, &target.templateid) {
        (Some(left), Some(right)) => left == right,
        (None, None) => match (&source.discovery, &target.discovery) {
            (Some(left), Some(right)) => left.prototype_key == right.prototype_key,
            (None, None) => source.flags == target.flags,
            _ => false,
        },
        _ => false,
    };
    if !same_origin {
        row.differences.push(Difference::Origin);
        row.reasons.push(ReasonCode::OriginDiffers);
    }
    row.match_status = if row.differences.is_empty() {
        MatchStatus::Exact
    } else {
        MatchStatus::Compatible
    };
}
