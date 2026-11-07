// SPDX-License-Identifier: MIT OR Apache-2.0

//! Manual LLD-instance decisions for CloneDashboard (P3-02J).
//!
//! Resolver candidates are evidence only. A Clone mapping becomes usable
//! only after an explicit decision names one of the frozen candidates of the
//! same prototype; no first/best/similar candidate is selected automatically.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::{ReferenceMapping, TargetMetricResolution, TargetSlotId};
use crate::resolver::{
    Applicability, Difference, Evidence, ItemEvidence, ItemOrigin, MatchStatus, ReasonCode,
    TargetValue,
};

/// Explicit user choice for one target metric resolution.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetMetricDecision {
    /// One frozen LLD candidate of the same template prototype.
    LldInstance { selected: ItemEvidence },
}

/// Stable data-only failure while recording a manual LLD choice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloneLldDecisionError {
    code: &'static str,
    params: BTreeMap<&'static str, String>,
}

impl CloneLldDecisionError {
    fn new(code: &'static str) -> Self {
        Self {
            code,
            params: BTreeMap::new(),
        }
    }

    fn with(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.params.insert(key, value.into());
        self
    }

    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    #[must_use]
    pub fn params(&self) -> &BTreeMap<&'static str, String> {
        &self.params
    }
}

impl fmt::Display for CloneLldDecisionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code)
    }
}

impl std::error::Error for CloneLldDecisionError {}

fn discovered_prototype(evidence: &ItemEvidence) -> Option<&str> {
    match &evidence.origin {
        ItemOrigin::Discovered { prototype_key } => Some(prototype_key),
        ItemOrigin::Own | ItemOrigin::Template { .. } => None,
    }
}

fn valid_decision(source: Option<&Evidence>, resolution: &TargetMetricResolution) -> bool {
    let Some(TargetMetricDecision::LldInstance { selected }) = &resolution.decision else {
        return resolution.decision.is_none()
            && !(resolution.match_status == MatchStatus::UnresolvedLld
                && resolution.target.is_some());
    };
    let Some(Evidence::Item(source)) = source else {
        return false;
    };
    let (Some(source_prototype), Some(target_prototype)) =
        (discovered_prototype(source), discovered_prototype(selected))
    else {
        return false;
    };
    resolution.match_status == MatchStatus::UnresolvedLld
        && resolution.applicability == Applicability::Editable
        && source_prototype == target_prototype
        && resolution
            .candidates
            .iter()
            .any(|candidate| candidate == selected)
        && resolution.target
            == Some(TargetValue::Item {
                itemid: selected.itemid.clone(),
            })
        && resolution.evidence == Some(Evidence::Item(selected.clone()))
}

pub(crate) fn reference_lld_decisions_well_formed(reference: &ReferenceMapping) -> bool {
    reference
        .targets
        .iter()
        .all(|resolution| valid_decision(reference.source_evidence.as_ref(), resolution))
}

impl TargetMetricResolution {
    /// Whether this row names a concrete target for Clone materialization.
    /// An unresolved LLD row is concrete only after an explicit decision.
    #[must_use]
    pub fn is_resolved_for_clone(&self) -> bool {
        self.match_status.is_resolved()
            || matches!(
                self.decision,
                Some(TargetMetricDecision::LldInstance { .. })
            )
    }
}

impl ReferenceMapping {
    /// Record a manual LLD-instance choice from the frozen candidate list.
    /// The source and candidate must both be discovered items of the same
    /// prototype. The original `unresolved_lld` status remains visible as
    /// provenance; [`TargetMetricResolution::is_resolved_for_clone`] reflects
    /// the applied decision.
    pub fn choose_lld_instance(
        &mut self,
        slot_id: &TargetSlotId,
        itemid: &str,
    ) -> Result<(), CloneLldDecisionError> {
        let Some(Evidence::Item(source)) = &self.source_evidence else {
            return Err(CloneLldDecisionError::new(
                "clone_lld_source_not_discovered",
            ));
        };
        let Some(source_prototype) = discovered_prototype(source) else {
            return Err(CloneLldDecisionError::new(
                "clone_lld_source_not_discovered",
            ));
        };
        let resolution = self
            .targets
            .iter_mut()
            .find(|resolution| &resolution.slot_id == slot_id)
            .ok_or_else(|| {
                CloneLldDecisionError::new("clone_target_slot_not_found")
                    .with("slot_id", &slot_id.0)
            })?;
        if resolution.match_status != MatchStatus::UnresolvedLld
            || resolution.applicability != Applicability::Editable
            || resolution.target.is_some()
            || resolution.decision.is_some()
        {
            return Err(CloneLldDecisionError::new(
                "clone_lld_decision_not_applicable",
            ));
        }
        let selected = resolution
            .candidates
            .iter()
            .find(|candidate| candidate.itemid == itemid)
            .cloned()
            .ok_or_else(|| {
                CloneLldDecisionError::new("clone_lld_candidate_not_visible").with("itemid", itemid)
            })?;
        if discovered_prototype(&selected) != Some(source_prototype) {
            return Err(CloneLldDecisionError::new(
                "clone_lld_candidate_prototype_mismatch",
            ));
        }

        let mut differences = Vec::new();
        if source.value_type != selected.value_type {
            differences.push(Difference::ValueType);
        }
        if source.units != selected.units {
            differences.push(Difference::Units);
        }
        resolution.target = Some(TargetValue::Item {
            itemid: selected.itemid.clone(),
        });
        resolution.evidence = Some(Evidence::Item(selected.clone()));
        resolution.differences = differences;
        resolution.reasons.retain(|reason| {
            !matches!(
                reason,
                ReasonCode::ValueTypeDiffers
                    | ReasonCode::ValueTypeIncompatible
                    | ReasonCode::UnitsDiffer
            )
        });
        resolution.decision = Some(TargetMetricDecision::LldInstance { selected });
        Ok(())
    }
}
