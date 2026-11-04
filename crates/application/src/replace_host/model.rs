// SPDX-License-Identifier: MIT OR Apache-2.0

//! Data of a `replace_host` operation as stored in the workcopy log and shown
//! to the screens. Local-only (O-018): never exported, carries server IDs and
//! server names. Serde names are stable snake_case codes.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use zdashstitch_zabbix::is_zabbix_id;

use crate::resolver::{
    Applicability, CrossingSide, Difference, Evidence, ItemEvidence, ItemOrigin, MatchStatus,
    ReasonCode, ReplaceHostResolution, ResolveScope, SourceKind, SourceValue, TargetValue,
};

/// Value of a referencing widget field, before or after the replacement.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FieldValue {
    Item {
        itemid: String,
    },
    Graph {
        graphid: String,
    },
    Host {
        hostid: String,
    },
    /// `svggraph` host pattern: a visible host name, possibly a mask.
    HostPattern {
        pattern: String,
    },
    ItemPrototype {
        itemid: String,
    },
    GraphPrototype {
        graphid: String,
    },
}

impl FieldValue {
    /// The string stored in the Zabbix field.
    #[must_use]
    pub fn raw(&self) -> &str {
        match self {
            Self::Item { itemid } | Self::ItemPrototype { itemid } => itemid,
            Self::Graph { graphid } | Self::GraphPrototype { graphid } => graphid,
            Self::Host { hostid } => hostid,
            Self::HostPattern { pattern } => pattern,
        }
    }

    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Item { .. } => "item",
            Self::Graph { .. } => "graph",
            Self::Host { .. } => "host",
            Self::HostPattern { .. } => "host_pattern",
            Self::ItemPrototype { .. } => "item_prototype",
            Self::GraphPrototype { .. } => "graph_prototype",
        }
    }

    fn from_source(value: &SourceValue) -> Self {
        match value {
            SourceValue::Item { itemid } => Self::Item {
                itemid: itemid.clone(),
            },
            SourceValue::ItemPrototype { itemid } => Self::ItemPrototype {
                itemid: itemid.clone(),
            },
            SourceValue::Graph { graphid } => Self::Graph {
                graphid: graphid.clone(),
            },
            SourceValue::GraphPrototype { graphid } => Self::GraphPrototype {
                graphid: graphid.clone(),
            },
            SourceValue::Host { hostid } => Self::Host {
                hostid: hostid.clone(),
            },
            SourceValue::HostPattern { pattern } => Self::HostPattern {
                pattern: pattern.clone(),
            },
        }
    }

    fn from_target(value: &TargetValue) -> Self {
        match value {
            TargetValue::Item { itemid } => Self::Item {
                itemid: itemid.clone(),
            },
            TargetValue::Graph { graphid } => Self::Graph {
                graphid: graphid.clone(),
            },
            TargetValue::Host { hostid } => Self::Host {
                hostid: hostid.clone(),
            },
            TargetValue::HostPattern { pattern } => Self::HostPattern {
                pattern: pattern.clone(),
            },
        }
    }

    fn from_kind(kind: &SourceKind) -> Option<Self> {
        Some(match kind {
            SourceKind::Item { itemid } => Self::Item {
                itemid: itemid.clone(),
            },
            SourceKind::ItemPrototype { itemid } => Self::ItemPrototype {
                itemid: itemid.clone(),
            },
            SourceKind::Graph { graphid } => Self::Graph {
                graphid: graphid.clone(),
            },
            SourceKind::GraphPrototype { graphid } => Self::GraphPrototype {
                graphid: graphid.clone(),
            },
            SourceKind::Host { hostid } => Self::Host {
                hostid: hostid.clone(),
            },
            SourceKind::HostPattern { pattern, .. } => Self::HostPattern {
                pattern: pattern.clone(),
            },
            _ => return None,
        })
    }

    fn ids_valid(&self) -> bool {
        match self {
            Self::HostPattern { pattern } => !pattern.is_empty(),
            other => is_zabbix_id(other.raw()),
        }
    }
}

/// Address of one widget field in Base: the key of rows and decisions.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FieldKey {
    /// 0-based page position in Base.
    pub page_index: usize,
    /// 0-based widget position on its page.
    pub widget_index: usize,
    /// Zabbix field name, e.g. `itemid.0`, `ds.1.hosts.0`.
    pub field: String,
}

/// Item facts kept as evidence (P1-05D: key, type, units, template).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ItemFacts {
    pub itemid: String,
    pub key: String,
    /// Server data, shown but never matching evidence.
    pub name: String,
    pub value_type: u8,
    pub units: String,
    /// Template item the item is inherited from.
    pub template_itemid: Option<String>,
    /// Prototype key of an LLD instance.
    pub prototype_key: Option<String>,
}

impl ItemFacts {
    fn from_evidence(evidence: &ItemEvidence) -> Self {
        let (template_itemid, prototype_key) = match &evidence.origin {
            ItemOrigin::Own => (None, None),
            ItemOrigin::Template {
                template_itemid, ..
            } => (Some(template_itemid.clone()), None),
            ItemOrigin::Discovered { prototype_key } => (None, Some(prototype_key.clone())),
        };
        Self {
            itemid: evidence.itemid.clone(),
            key: evidence.key.clone(),
            name: evidence.name.clone(),
            value_type: evidence.value_type,
            units: evidence.units.clone(),
            template_itemid,
            prototype_key,
        }
    }

    fn from_option(evidence: Option<&Evidence>) -> Option<Self> {
        match evidence {
            Some(Evidence::Item(item)) => Some(Self::from_evidence(item)),
            _ => None,
        }
    }

    /// `value_type` 0 (float) and 3 (unsigned) are numeric.
    #[must_use]
    pub const fn is_numeric(&self) -> bool {
        matches!(self.value_type, 0 | 3)
    }
}

/// A host with its server data.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostRef {
    pub hostid: String,
    /// Visible name (server data).
    pub name: String,
}

/// Item-pattern coverage of a data-set host pattern (evidence only).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PatternCount {
    pub pattern: String,
    pub source_matches: u32,
    pub target_matches: u32,
}

/// One reference of the source host in scope with its calculated
/// resolution (a P2-02 row frozen at calculation time).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub page_index: usize,
    pub dashboard_pageid: Option<String>,
    pub widget_index: usize,
    pub widgetid: Option<String>,
    pub field: String,
    pub widget_type: String,
    pub source: FieldValue,
    /// Calculated target; `None` when there is no single target.
    pub target: Option<FieldValue>,
    pub match_status: MatchStatus,
    pub applicability: Applicability,
    /// Sorted, unique.
    pub reasons: Vec<ReasonCode>,
    pub differences: Vec<Difference>,
    /// Why the reference stays on the source host; `Some` exactly for
    /// non-editable rows.
    pub kept_reason: Option<ReasonCode>,
    pub source_item: Option<ItemFacts>,
    pub target_item: Option<ItemFacts>,
    /// Options for a manual decision; never chosen automatically.
    pub candidates: Vec<ItemFacts>,
    pub pattern_coverage: Vec<PatternCount>,
}

impl Binding {
    #[must_use]
    pub fn key(&self) -> FieldKey {
        FieldKey {
            page_index: self.page_index,
            widget_index: self.widget_index,
            field: self.field.clone(),
        }
    }

    /// The calculation alone changes this field (same rule as
    /// [`crate::resolver::ResolutionRow::will_change`]).
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

    #[must_use]
    pub fn is_editable(&self) -> bool {
        self.applicability == Applicability::Editable
    }
}

/// A reference in scope whose owner cannot be read: it may or may not point
/// at the source host.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UnverifiedReference {
    pub page_index: usize,
    pub dashboard_pageid: Option<String>,
    pub widget_index: usize,
    pub widgetid: Option<String>,
    pub field: String,
    pub widget_type: String,
    pub source: FieldValue,
    pub reason: ReasonCode,
}

impl UnverifiedReference {
    #[must_use]
    pub fn key(&self) -> FieldKey {
        FieldKey {
            page_index: self.page_index,
            widget_index: self.widget_index,
            field: self.field.clone(),
        }
    }
}

/// A widget link that crosses the scope boundary (P1-05B «осиротевшая»).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LinkCrossing {
    pub page_index: usize,
    pub widget_index: usize,
    pub widgetid: Option<String>,
    /// Consumer field, e.g. `override_hostid._reference`.
    pub field: String,
    pub reference: String,
    pub side: CrossingSide,
}

/// Part of Base the operation covers.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReplaceScope {
    Dashboard,
    /// Sorted, unique `dashboard_pageid`s.
    Pages {
        dashboard_pageids: Vec<String>,
    },
    /// Sorted, unique `widgetid`s.
    Widgets {
        widgetids: Vec<String>,
    },
}

impl ReplaceScope {
    #[must_use]
    pub fn from_resolve(scope: &ResolveScope) -> Self {
        match scope {
            ResolveScope::Dashboard => Self::Dashboard,
            ResolveScope::Pages(pages) => Self::Pages {
                dashboard_pageids: pages.iter().cloned().collect(),
            },
            ResolveScope::Widgets(widgets) => Self::Widgets {
                widgetids: widgets.iter().cloned().collect(),
            },
        }
    }

    #[must_use]
    pub fn to_resolve(&self) -> ResolveScope {
        match self {
            Self::Dashboard => ResolveScope::Dashboard,
            Self::Pages { dashboard_pageids } => {
                ResolveScope::Pages(dashboard_pageids.iter().cloned().collect())
            }
            Self::Widgets { widgetids } => {
                ResolveScope::Widgets(widgetids.iter().cloned().collect())
            }
        }
    }

    /// Non-empty, sorted, unique, numeric IDs.
    pub(crate) fn is_well_formed(&self) -> bool {
        let ids = match self {
            Self::Dashboard => return true,
            Self::Pages { dashboard_pageids } => dashboard_pageids,
            Self::Widgets { widgetids } => widgetids,
        };
        !ids.is_empty()
            && ids.iter().all(|id| is_zabbix_id(id))
            && ids.windows(2).all(|pair| pair[0] < pair[1])
    }
}

/// The frozen result of one read-only calculation (P2-02 resolver output).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Calculation {
    /// Base the calculation was made on: its storage fingerprint, equal to
    /// the workcopy's `base.fingerprint`.
    pub base_fingerprint: String,
    /// Server version of the read.
    pub server_version: String,
    pub calculated_at_ms: u64,
    pub source_host: HostRef,
    /// `None`: target missing or hidden; every row is `denied`.
    pub target_host: Option<HostRef>,
    pub bindings: Vec<Binding>,
    pub unverified: Vec<UnverifiedReference>,
    /// References of the source host outside the scope.
    pub out_of_scope: u32,
    pub scope_crossings: Vec<LinkCrossing>,
}

impl Calculation {
    /// Freezes a resolver result.
    #[must_use]
    pub fn from_resolution(
        resolution: &ReplaceHostResolution,
        server_version: &str,
        calculated_at_ms: u64,
    ) -> Self {
        let bindings = resolution
            .rows
            .iter()
            .map(|row| Binding {
                page_index: row.field.widget.page_index,
                dashboard_pageid: row.field.widget.dashboard_pageid.clone(),
                widget_index: row.field.widget.widget_index,
                widgetid: row.field.widget.widgetid.clone(),
                field: row.field.field_name.clone(),
                widget_type: row.widget_type.clone(),
                source: FieldValue::from_source(&row.source),
                target: row.target.as_ref().map(FieldValue::from_target),
                match_status: row.match_status,
                applicability: row.applicability,
                reasons: row.reasons.clone(),
                differences: row.differences.clone(),
                kept_reason: resolution
                    .kept_on_source
                    .iter()
                    .find(|kept| kept.field == row.field)
                    .map(|kept| kept.reason),
                source_item: ItemFacts::from_option(row.source_evidence.as_ref()),
                target_item: ItemFacts::from_option(row.target_evidence.as_ref()),
                candidates: row
                    .candidates
                    .iter()
                    .map(ItemFacts::from_evidence)
                    .collect(),
                pattern_coverage: row
                    .pattern_coverage
                    .iter()
                    .map(|coverage| PatternCount {
                        pattern: coverage.pattern.clone(),
                        source_matches: coverage.source_matches,
                        target_matches: coverage.target_matches,
                    })
                    .collect(),
            })
            .collect();
        let unverified = resolution
            .unverified
            .iter()
            .filter_map(|source| {
                Some(UnverifiedReference {
                    page_index: source.field.widget.page_index,
                    dashboard_pageid: source.field.widget.dashboard_pageid.clone(),
                    widget_index: source.field.widget.widget_index,
                    widgetid: source.field.widget.widgetid.clone(),
                    field: source.field.field_name.clone(),
                    widget_type: source.widget_type.clone(),
                    source: FieldValue::from_kind(&source.kind)?,
                    reason: source.reason,
                })
            })
            .collect();
        let scope_crossings = resolution
            .scope_crossings
            .iter()
            .map(|crossing| LinkCrossing {
                page_index: crossing.link.consumer.page_index,
                widget_index: crossing.link.consumer.widget_index,
                widgetid: crossing.link.consumer.widgetid.clone(),
                field: crossing.link.field_name.clone(),
                reference: crossing.link.reference.clone(),
                side: crossing.side,
            })
            .collect();
        Self {
            base_fingerprint: resolution.snapshot_fingerprint.clone(),
            server_version: server_version.to_owned(),
            calculated_at_ms,
            source_host: HostRef {
                hostid: resolution.source_host.hostid.clone(),
                name: resolution.source_host.name.clone(),
            },
            target_host: resolution.target_host.as_ref().map(|host| HostRef {
                hostid: host.hostid.clone(),
                name: host.name.clone(),
            }),
            bindings,
            unverified,
            out_of_scope: resolution.summary.out_of_scope,
            scope_crossings,
        }
    }

    /// Structural checks of a stored calculation: numeric IDs, unique keys,
    /// a field is either a binding or unverified.
    pub(crate) fn is_well_formed(&self, source_hostid: &str, target_hostid: &str) -> bool {
        if self.source_host.hostid != source_hostid
            || self
                .target_host
                .as_ref()
                .is_some_and(|host| host.hostid != target_hostid)
        {
            return false;
        }
        let mut keys = BTreeSet::new();
        let bindings_ok = self.bindings.iter().all(|binding| {
            binding.source.ids_valid()
                && binding.target.as_ref().is_none_or(FieldValue::ids_valid)
                && binding
                    .candidates
                    .iter()
                    .all(|candidate| is_zabbix_id(&candidate.itemid))
                && keys.insert(binding.key())
        });
        bindings_ok
            && self
                .unverified
                .iter()
                .all(|reference| reference.source.ids_valid() && keys.insert(reference.key()))
    }
}

/// A user decision on one row (P1-05D, P1-05H `bindings.decide`).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RowDecision {
    /// One of the offered candidates of an `ambiguous` row.
    ChooseCandidate { itemid: String },
    /// One of the LLD instances of the same prototype offered for an
    /// `unresolved_lld` row (P3-02J).
    ChooseLldInstance { itemid: String },
    /// «Оставить на исходном»: the field keeps its value; the row joins the
    /// list that publication asks to confirm.
    KeepOnSource,
}

impl RowDecision {
    #[must_use]
    pub const fn kind(&self) -> DecisionKind {
        match self {
            Self::ChooseCandidate { .. } => DecisionKind::ChooseCandidate,
            Self::ChooseLldInstance { .. } => DecisionKind::ChooseLldInstance,
            Self::KeepOnSource => DecisionKind::KeepOnSource,
        }
    }

    pub(crate) fn itemid(&self) -> Option<&str> {
        match self {
            Self::ChooseCandidate { itemid } | Self::ChooseLldInstance { itemid } => Some(itemid),
            Self::KeepOnSource => None,
        }
    }
}

/// Decision kinds, for «what can be decided here».
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DecisionKind {
    ChooseCandidate,
    ChooseLldInstance,
    KeepOnSource,
}

impl DecisionKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ChooseCandidate => "choose_candidate",
            Self::ChooseLldInstance => "choose_lld_instance",
            Self::KeepOnSource => "keep_on_source",
        }
    }
}
