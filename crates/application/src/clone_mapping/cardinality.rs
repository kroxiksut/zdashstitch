// SPDX-License-Identifier: MIT OR Apache-2.0

//! Pure cardinality planning for CloneDashboard (P3-02C).
//!
//! This module decides which existing `svggraph` series stay, move or are
//! removed and which target slots need a new series. It never edits Zabbix
//! fields: P3-02F consumes the plan and performs the typed transformation.

use std::collections::{BTreeMap, BTreeSet};

use super::{TargetMetricResolution, TargetSlot, TargetSlotId};
use crate::resolver::{Applicability, Difference, FieldAddress, TargetValue};

/// How one `svggraph` data set selects its series.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SeriesBindingKind {
    /// A target host becomes `ds.N.hosts.M`; the data-set item pattern stays
    /// separate and is resolved by P3-02I.
    HostPattern,
    /// A target item becomes `ds.N.itemids.M`.
    ExplicitItem,
}

/// One series currently present in Base/Local, bound to a stable slot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSeries {
    pub slot_id: TargetSlotId,
    pub field: FieldAddress,
}

/// Cardinality input for one `svggraph` data set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SvgDatasetCardinality {
    pub dataset_index: u32,
    pub binding: SeriesBindingKind,
    pub source_series: Vec<SourceSeries>,
    /// The source row whose supported data-set settings P3-02F will copy for
    /// every added series. Required only when the target adds a slot.
    pub template_slot_id: Option<TargetSlotId>,
    /// One result per target slot, in exactly the same order.
    pub target_resolutions: Vec<TargetMetricResolution>,
    /// Adapter/layout limit already proven for this widget and server.
    pub max_series: usize,
}

/// Pure request. `source_slot_ids` describes the cardinality before Clone;
/// target slots are the frozen explicit order selected by the user.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CardinalityRequest {
    pub widget_type: String,
    pub source_slot_ids: Vec<TargetSlotId>,
    pub target_slots: Vec<TargetSlot>,
    /// Empty for single-host widget types.
    pub datasets: Vec<SvgDatasetCardinality>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum CardinalityBlockerCode {
    EmptyMandatoryRole,
    DuplicateSlot,
    DuplicateHost,
    SingleHostWidget,
    TemplateSeriesMissing,
    MetricMissing,
    IncompatibleValueType,
    IncompatibleUnits,
    TargetNotEditable,
    SeriesLimit,
}

/// Stable data-only blocker. The UI maps `code` to a localized message and
/// uses the optional identifiers as parameters.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CardinalityBlocker {
    pub code: CardinalityBlockerCode,
    pub dataset_index: Option<u32>,
    pub slot_id: Option<TargetSlotId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SeriesOrigin {
    /// The same source series survives; only its position may change.
    Existing { source_field: FieldAddress },
    /// P3-02F must clone supported settings from this existing series.
    Added { template_field: FieldAddress },
}

/// One series in final target-slot order. No color is invented here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedSeries {
    pub slot_id: TargetSlotId,
    pub position: usize,
    pub target: TargetValue,
    pub origin: SeriesOrigin,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemovedSeries {
    pub slot_id: TargetSlotId,
    pub source_field: FieldAddress,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedDataset {
    pub dataset_index: u32,
    pub binding: SeriesBindingKind,
    pub series: Vec<PlannedSeries>,
    pub removed: Vec<RemovedSeries>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CardinalityPlan {
    pub datasets: Vec<PlannedDataset>,
    pub blockers: Vec<CardinalityBlocker>,
}

impl CardinalityPlan {
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.blockers.is_empty()
    }
}

fn blocker(
    code: CardinalityBlockerCode,
    dataset_index: Option<u32>,
    slot_id: Option<&TargetSlotId>,
) -> CardinalityBlocker {
    CardinalityBlocker {
        code,
        dataset_index,
        slot_id: slot_id.cloned(),
    }
}

fn unique_slots(slots: &[TargetSlotId]) -> bool {
    let mut seen = BTreeSet::new();
    slots.iter().all(|slot| seen.insert(&slot.0))
}

fn target_matches_binding(binding: SeriesBindingKind, target: &TargetValue) -> bool {
    matches!(
        (binding, target),
        (
            SeriesBindingKind::HostPattern,
            TargetValue::HostPattern { .. }
        ) | (SeriesBindingKind::ExplicitItem, TargetValue::Item { .. })
    )
}

fn resolution_blockers(
    dataset_index: u32,
    binding: SeriesBindingKind,
    resolution: &TargetMetricResolution,
) -> Vec<CardinalityBlocker> {
    let mut blockers = Vec::new();
    if !resolution.is_resolved_for_clone()
        || resolution
            .target
            .as_ref()
            .is_none_or(|target| !target_matches_binding(binding, target))
    {
        blockers.push(blocker(
            CardinalityBlockerCode::MetricMissing,
            Some(dataset_index),
            Some(&resolution.slot_id),
        ));
    }
    if resolution.differences.contains(&Difference::ValueType) {
        blockers.push(blocker(
            CardinalityBlockerCode::IncompatibleValueType,
            Some(dataset_index),
            Some(&resolution.slot_id),
        ));
    }
    if resolution.differences.contains(&Difference::Units) {
        blockers.push(blocker(
            CardinalityBlockerCode::IncompatibleUnits,
            Some(dataset_index),
            Some(&resolution.slot_id),
        ));
    }
    if resolution.applicability != Applicability::Editable {
        blockers.push(blocker(
            CardinalityBlockerCode::TargetNotEditable,
            Some(dataset_index),
            Some(&resolution.slot_id),
        ));
    }
    blockers
}

/// Build a deterministic cardinality plan without mutating the workcopy.
/// Blockers are returned as data so the UI can show every failing row at
/// once. A plan with blockers must never enter P3-02F or publication.
#[must_use]
pub fn plan_cardinality(request: &CardinalityRequest) -> CardinalityPlan {
    let mut blockers = Vec::new();
    if request.target_slots.is_empty() {
        blockers.push(blocker(
            CardinalityBlockerCode::EmptyMandatoryRole,
            None,
            None,
        ));
    }

    let target_slot_ids = request
        .target_slots
        .iter()
        .map(|slot| slot.slot_id.clone())
        .collect::<Vec<_>>();
    if !unique_slots(&target_slot_ids) || !unique_slots(&request.source_slot_ids) {
        blockers.push(blocker(CardinalityBlockerCode::DuplicateSlot, None, None));
    }
    let mut target_hosts = BTreeSet::new();
    if request
        .target_slots
        .iter()
        .any(|slot| !target_hosts.insert(&slot.host.hostid))
    {
        blockers.push(blocker(CardinalityBlockerCode::DuplicateHost, None, None));
    }

    let grows = request.target_slots.len() > request.source_slot_ids.len();
    if request.widget_type != "svggraph" {
        if grows && matches!(request.widget_type.as_str(), "item" | "graph") {
            blockers.push(blocker(
                CardinalityBlockerCode::SingleHostWidget,
                None,
                None,
            ));
        }
        blockers.sort();
        blockers.dedup();
        return CardinalityPlan {
            datasets: Vec::new(),
            blockers,
        };
    }

    let target_set = target_slot_ids.iter().collect::<BTreeSet<_>>();
    let mut plans = Vec::with_capacity(request.datasets.len());
    for dataset in &request.datasets {
        if request.target_slots.len() > dataset.max_series {
            blockers.push(blocker(
                CardinalityBlockerCode::SeriesLimit,
                Some(dataset.dataset_index),
                None,
            ));
        }

        let source_by_slot = dataset
            .source_series
            .iter()
            .map(|series| (&series.slot_id, series))
            .collect::<BTreeMap<_, _>>();
        if source_by_slot.len() != dataset.source_series.len() {
            blockers.push(blocker(
                CardinalityBlockerCode::DuplicateSlot,
                Some(dataset.dataset_index),
                None,
            ));
        }
        let template = dataset
            .template_slot_id
            .as_ref()
            .and_then(|slot| source_by_slot.get(slot).copied());
        if grows && template.is_none() {
            blockers.push(blocker(
                CardinalityBlockerCode::TemplateSeriesMissing,
                Some(dataset.dataset_index),
                dataset.template_slot_id.as_ref(),
            ));
        }

        let resolution_by_slot = dataset
            .target_resolutions
            .iter()
            .map(|resolution| (&resolution.slot_id, resolution))
            .collect::<BTreeMap<_, _>>();
        if resolution_by_slot.len() != dataset.target_resolutions.len()
            || dataset.target_resolutions.len() != request.target_slots.len()
        {
            blockers.push(blocker(
                CardinalityBlockerCode::MetricMissing,
                Some(dataset.dataset_index),
                None,
            ));
        }

        let mut series = Vec::with_capacity(request.target_slots.len());
        for (position, slot) in request.target_slots.iter().enumerate() {
            let Some(resolution) = resolution_by_slot.get(&slot.slot_id).copied() else {
                blockers.push(blocker(
                    CardinalityBlockerCode::MetricMissing,
                    Some(dataset.dataset_index),
                    Some(&slot.slot_id),
                ));
                continue;
            };
            blockers.extend(resolution_blockers(
                dataset.dataset_index,
                dataset.binding,
                resolution,
            ));
            let Some(target) = resolution.target.clone() else {
                continue;
            };
            let origin = if let Some(existing) = source_by_slot.get(&slot.slot_id) {
                SeriesOrigin::Existing {
                    source_field: existing.field.clone(),
                }
            } else if let Some(template) = template {
                SeriesOrigin::Added {
                    template_field: template.field.clone(),
                }
            } else {
                continue;
            };
            series.push(PlannedSeries {
                slot_id: slot.slot_id.clone(),
                position,
                target,
                origin,
            });
        }
        let removed = dataset
            .source_series
            .iter()
            .filter(|source| !target_set.contains(&source.slot_id))
            .map(|source| RemovedSeries {
                slot_id: source.slot_id.clone(),
                source_field: source.field.clone(),
            })
            .collect();
        plans.push(PlannedDataset {
            dataset_index: dataset.dataset_index,
            binding: dataset.binding,
            series,
            removed,
        });
    }

    blockers.sort();
    blockers.dedup();
    CardinalityPlan {
        datasets: plans,
        blockers,
    }
}
