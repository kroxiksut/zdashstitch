// SPDX-License-Identifier: MIT OR Apache-2.0

//! Pure host/metric mapping model for CloneDashboard (P3-02A).
//!
//! The model reuses the resolver vocabulary and evidence. It contains no
//! transport, credentials, persistence or write planning. Target slot order
//! is the order of the vectors; no host or metric is selected by a similar
//! display name.

use serde::{Deserialize, Serialize};
use zdashstitch_zabbix::HostDetails;

use crate::resolver::{
    Applicability, Difference, Evidence, FieldAddress, ItemEvidence, MatchStatus, ReasonCode,
    SourceValue, TargetValue,
};

mod cardinality;
mod lld;
mod patterns;
mod revalidation;
mod storage;
mod target_area;
mod transform;

pub use cardinality::{
    CardinalityBlocker, CardinalityBlockerCode, CardinalityPlan, CardinalityRequest,
    PlannedDataset, PlannedSeries, RemovedSeries, SeriesBindingKind, SeriesOrigin, SourceSeries,
    SvgDatasetCardinality, plan_cardinality,
};
pub use lld::{CloneLldDecisionError, TargetMetricDecision};
pub use patterns::{
    ClonePatternAction, ClonePatternDecision, ClonePatternError, ClonePatternKind,
    ClonePatternLookup, ClonePatternMatch, ClonePatternPreview, ClonePatternRequirement,
    pattern_requirements, preview_clone_pattern,
};
pub(crate) use revalidation::revalidate_clone_mapping_from_source;
pub use revalidation::{
    CloneBindingCheck, CloneCheckReason, CloneRevalidationContext, CloneRevalidationError,
    revalidate_clone_mapping_state,
};
pub use storage::{
    CloneMappingRecord, CloneMappingState, RemovedCloneMapping, SaveCloneMappingRequest,
};
pub use target_area::{
    FrozenTargetSelection, TargetAreaCandidate, TargetAreaLookup, TargetAreaSearch,
    TargetSelectionError, search_target_groups, search_target_hosts,
};
pub use transform::{
    CloneCardinalityTransform, CloneTransformError, complete_clone_series_colors, materialize_clone,
};

/// Stable local identity of a source role in one mapping operation.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SourceRoleId(pub String);

/// Stable local identity of an ordered target slot.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct TargetSlotId(pub String);

/// One explicitly selected host in the target area.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetSlot {
    pub slot_id: TargetSlotId,
    pub host: HostDetails,
}

/// What to do with every reference owned by one source host.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceHostDecision {
    /// Resolve the references against these slots in their explicit order.
    TargetSlots(Vec<TargetSlot>),
    /// Preserve the source references intentionally (for example, a shared
    /// Zabbix server or load balancer). This is not a warning or an error.
    KeepSource,
}

/// Match result for one source metric in one target slot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetMetricResolution {
    pub slot_id: TargetSlotId,
    pub target: Option<TargetValue>,
    pub match_status: MatchStatus,
    pub applicability: Applicability,
    /// Sorted, unique stable codes.
    pub reasons: Vec<ReasonCode>,
    pub differences: Vec<Difference>,
    pub evidence: Option<Evidence>,
    /// Manual choices, never selected automatically.
    pub candidates: Vec<ItemEvidence>,
    /// Explicit decision applied to this target. `None` for every automatic
    /// exact/compatible result and for an unresolved row.
    #[serde(default)]
    pub decision: Option<TargetMetricDecision>,
    #[serde(default)]
    pub check: CloneBindingCheck,
    #[serde(default)]
    pub check_reason: Option<CloneCheckReason>,
}

impl TargetMetricResolution {
    /// Zabbix cannot distinguish an absent object from one hidden by the
    /// caller's permissions. Keep the combined UI reason and do not claim a
    /// proven permission denial.
    #[must_use]
    pub fn not_found_or_no_access(slot_id: TargetSlotId, applicability: Applicability) -> Self {
        Self {
            slot_id,
            target: None,
            match_status: MatchStatus::Denied,
            applicability,
            reasons: vec![ReasonCode::NotFoundOrNoAccess],
            differences: Vec::new(),
            evidence: None,
            candidates: Vec::new(),
            decision: None,
            check: CloneBindingCheck::Checked,
            check_reason: None,
        }
    }
}

/// One source reference and its independent result in every target slot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceMapping {
    pub field: FieldAddress,
    pub widget_type: String,
    pub source: SourceValue,
    pub source_evidence: Option<Evidence>,
    /// Same order as [`SourceHostDecision::TargetSlots`]. Empty only for an
    /// explicit [`SourceHostDecision::KeepSource`].
    pub targets: Vec<TargetMetricResolution>,
}

/// References grouped under a semantic source role and concrete source
/// host. Role IDs are local identities; visible names remain evidence only.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceHostMapping {
    pub role_id: SourceRoleId,
    pub source_host: HostDetails,
    pub decision: SourceHostDecision,
    pub references: Vec<ReferenceMapping>,
}

/// A reference intentionally retained by an explicit `keep_source` host
/// decision. Unlike unresolved or unsupported rows, it needs no warning.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExplicitKeptReference {
    pub role_id: SourceRoleId,
    pub source_hostid: String,
    pub field: FieldAddress,
    pub widget_type: String,
    pub source: SourceValue,
}

impl SourceHostMapping {
    /// Materialize the informational `kept_on_source` rows for an explicit
    /// host decision. Other decisions return no rows.
    #[must_use]
    pub fn explicit_kept_on_source(&self) -> Vec<ExplicitKeptReference> {
        if self.decision != SourceHostDecision::KeepSource {
            return Vec::new();
        }
        self.references
            .iter()
            .map(|reference| ExplicitKeptReference {
                role_id: self.role_id.clone(),
                source_hostid: self.source_host.hostid.clone(),
                field: reference.field.clone(),
                widget_type: reference.widget_type.clone(),
                source: reference.source.clone(),
            })
            .collect()
    }

    pub(crate) fn is_well_formed(&self) -> bool {
        fn local_id(value: &str) -> bool {
            !value.is_empty()
                && value.len() <= 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        }
        fn source_id(value: &SourceValue) -> bool {
            match value {
                SourceValue::Item { itemid } | SourceValue::ItemPrototype { itemid } => {
                    zdashstitch_zabbix::is_zabbix_id(itemid)
                }
                SourceValue::Graph { graphid } | SourceValue::GraphPrototype { graphid } => {
                    zdashstitch_zabbix::is_zabbix_id(graphid)
                }
                SourceValue::Host { hostid } => zdashstitch_zabbix::is_zabbix_id(hostid),
                SourceValue::HostPattern { pattern } => {
                    !pattern.is_empty() && !pattern.chars().any(char::is_control)
                }
            }
        }
        fn target_id(value: &TargetValue) -> bool {
            match value {
                TargetValue::Item { itemid } => zdashstitch_zabbix::is_zabbix_id(itemid),
                TargetValue::Graph { graphid } => zdashstitch_zabbix::is_zabbix_id(graphid),
                TargetValue::Host { hostid } => zdashstitch_zabbix::is_zabbix_id(hostid),
                TargetValue::HostPattern { pattern } => {
                    !pattern.is_empty() && !pattern.chars().any(char::is_control)
                }
            }
        }

        if !local_id(&self.role_id.0)
            || !zdashstitch_zabbix::is_zabbix_id(&self.source_host.hostid)
            || self.references.iter().any(|reference| {
                reference.field.field_name.is_empty()
                    || reference.field.field_name.chars().any(char::is_control)
                    || !source_id(&reference.source)
                    || !lld::reference_lld_decisions_well_formed(reference)
                    || reference.targets.iter().any(|target| {
                        !local_id(&target.slot_id.0)
                            || target
                                .target
                                .as_ref()
                                .is_some_and(|value| !target_id(value))
                            || (target.check == CloneBindingCheck::Checked
                                && target.check_reason.is_some())
                            || (target.check != CloneBindingCheck::Checked
                                && target.check_reason.is_none())
                    })
            })
        {
            return false;
        }
        match &self.decision {
            SourceHostDecision::KeepSource => self
                .references
                .iter()
                .all(|reference| reference.targets.is_empty()),
            SourceHostDecision::TargetSlots(slots) => {
                if slots.is_empty() {
                    return false;
                }
                let mut slot_ids = std::collections::BTreeSet::new();
                let mut hostids = std::collections::BTreeSet::new();
                if slots.iter().any(|slot| {
                    !local_id(&slot.slot_id.0)
                        || !zdashstitch_zabbix::is_zabbix_id(&slot.host.hostid)
                        || !slot_ids.insert(slot.slot_id.0.as_str())
                        || !hostids.insert(slot.host.hostid.as_str())
                }) {
                    return false;
                }
                self.references.iter().all(|reference| {
                    reference.targets.len() == slots.len()
                        && reference
                            .targets
                            .iter()
                            .zip(slots)
                            .all(|(target, slot)| target.slot_id == slot.slot_id)
                })
            }
        }
    }
}

#[cfg(test)]
mod tests;
