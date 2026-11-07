// SPDX-License-Identifier: MIT OR Apache-2.0

//! Read-only revalidation of stored Clone bindings (P3-02G).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use zdashstitch_zabbix::{AdapterRegistry, VersionedDashboardSnapshot};

use super::{
    CloneMappingState, ClonePatternAction, ClonePatternDecision, ClonePatternLookup,
    SourceHostDecision, TargetMetricDecision, TargetMetricResolution, TargetSlot,
    preview_clone_pattern,
};
use crate::resolver::{
    ReplaceHostRequest, ResolutionRow, ResolveScope, ResolverLookup, resolve_replace_host_with,
    source_model,
};
use crate::workcopy::{is_connection_id, is_fingerprint};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloneBindingCheck {
    #[default]
    Checked,
    Stale,
    Invalid,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloneCheckReason {
    ServerVersionChanged,
    BaseChanged,
    LocalChanged,
    SourceChanged,
    TargetNotVisible,
    ResolutionChanged,
    CandidateNotVisible,
    PatternChanged,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloneRevalidationContext {
    pub connection_id: String,
    pub server_identity_fingerprint: String,
    pub server_version: String,
    pub local_fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloneRevalidationError {
    code: &'static str,
    params: BTreeMap<&'static str, String>,
}

impl CloneRevalidationError {
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

impl fmt::Display for CloneRevalidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code)
    }
}

impl std::error::Error for CloneRevalidationError {}

fn mark_resolution(
    resolution: &mut TargetMetricResolution,
    check: CloneBindingCheck,
    reason: CloneCheckReason,
) {
    resolution.check = check;
    resolution.check_reason = Some(reason);
}

fn mark_pattern(
    decision: &mut ClonePatternDecision,
    check: CloneBindingCheck,
    reason: CloneCheckReason,
) {
    decision.check = check;
    decision.check_reason = Some(reason);
}

fn mark_all(state: &mut CloneMappingState, reason: CloneCheckReason) {
    for mapping in &mut state.mappings {
        for reference in &mut mapping.references {
            for resolution in &mut reference.targets {
                mark_resolution(resolution, CloneBindingCheck::Stale, reason);
            }
        }
    }
    for decision in &mut state.pattern_decisions {
        mark_pattern(decision, CloneBindingCheck::Stale, reason);
    }
}

fn fresh_matches(stored: &TargetMetricResolution, fresh: &ResolutionRow) -> bool {
    stored.target == fresh.target
        && stored.match_status == fresh.match_status
        && stored.applicability == fresh.applicability
        && stored.reasons == fresh.reasons
        && stored.differences == fresh.differences
        && stored.evidence == fresh.target_evidence
        && stored.candidates == fresh.candidates
}

fn selected_lld_is_visible(stored: &TargetMetricResolution, fresh: &ResolutionRow) -> bool {
    let Some(TargetMetricDecision::LldInstance { selected }) = &stored.decision else {
        return false;
    };
    fresh
        .candidates
        .iter()
        .any(|candidate| candidate == selected)
        || fresh.target
            == Some(crate::resolver::TargetValue::Item {
                itemid: selected.itemid.clone(),
            })
}

fn all_slots(state: &CloneMappingState) -> Result<Vec<TargetSlot>, CloneRevalidationError> {
    let mut by_id = BTreeMap::new();
    for mapping in &state.mappings {
        if let SourceHostDecision::TargetSlots(slots) = &mapping.decision {
            for slot in slots {
                match by_id.insert(slot.slot_id.clone(), slot.clone()) {
                    Some(previous) if previous != *slot => {
                        return Err(CloneRevalidationError::new(
                            "clone_revalidation_slot_conflict",
                        ));
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(by_id.into_values().collect())
}

fn slots_for_pattern(
    all: &[TargetSlot],
    decision: &ClonePatternDecision,
) -> Result<Vec<TargetSlot>, CloneRevalidationError> {
    let ids = match &decision.action {
        ClonePatternAction::Replace { preview } => preview.checked_slots.clone(),
        ClonePatternAction::KeepAsIs => all.iter().map(|slot| slot.slot_id.clone()).collect(),
    };
    let by_id = all
        .iter()
        .map(|slot| (&slot.slot_id, slot))
        .collect::<BTreeMap<_, _>>();
    ids.into_iter()
        .map(|id| {
            by_id.get(&id).cloned().cloned().ok_or_else(|| {
                CloneRevalidationError::new("clone_revalidation_slot_missing").with("slot_id", id.0)
            })
        })
        .collect()
}

/// Re-read every stored Clone binding. The returned state retains all stored
/// choices and marks them checked/stale/invalid; it never substitutes fresh
/// targets. The caller persists it as a new mapping revision.
pub async fn revalidate_clone_mapping_state<L: ResolverLookup + ClonePatternLookup>(
    lookup: &L,
    base: &VersionedDashboardSnapshot,
    stored: &CloneMappingState,
    context: &CloneRevalidationContext,
) -> Result<CloneMappingState, CloneRevalidationError> {
    revalidate_clone_mapping_from_source(lookup, base, base, stored, context).await
}

pub(crate) async fn revalidate_clone_mapping_from_source<L: ResolverLookup + ClonePatternLookup>(
    lookup: &L,
    base: &VersionedDashboardSnapshot,
    source: &VersionedDashboardSnapshot,
    stored: &CloneMappingState,
    context: &CloneRevalidationContext,
) -> Result<CloneMappingState, CloneRevalidationError> {
    if !is_connection_id(&context.connection_id)
        || !is_fingerprint(&context.server_identity_fingerprint)
        || !is_fingerprint(&context.local_fingerprint)
    {
        return Err(CloneRevalidationError::new(
            "clone_revalidation_context_invalid",
        ));
    }
    if context.connection_id != stored.connection_id {
        return Err(CloneRevalidationError::new("clone_connection_changed"));
    }
    if context.server_identity_fingerprint != stored.server_identity_fingerprint {
        return Err(CloneRevalidationError::new("clone_server_identity_changed"));
    }

    let mut fresh_state = stored.clone();
    if context.server_version != stored.server_version {
        mark_all(&mut fresh_state, CloneCheckReason::ServerVersionChanged);
        return Ok(fresh_state);
    }
    if stored
        .base_fingerprint
        .as_deref()
        .is_none_or(|fingerprint| fingerprint != base.fingerprint())
    {
        mark_all(&mut fresh_state, CloneCheckReason::BaseChanged);
        return Ok(fresh_state);
    }
    if stored.local_fingerprint.as_deref() != Some(&context.local_fingerprint) {
        mark_all(&mut fresh_state, CloneCheckReason::LocalChanged);
        return Ok(fresh_state);
    }

    let registry = AdapterRegistry::widget_capabilities(source.server_version())
        .map_err(|_| CloneRevalidationError::new("clone_revalidation_adapter_missing"))?;
    let model = source_model(source, &registry)
        .map_err(|_| CloneRevalidationError::new("clone_revalidation_source_invalid"))?;

    for mapping in &mut fresh_state.mappings {
        let SourceHostDecision::TargetSlots(slots) = &mapping.decision else {
            continue;
        };
        for slot in slots {
            let request = ReplaceHostRequest {
                source_hostid: mapping.source_host.hostid.clone(),
                target_hostid: slot.host.hostid.clone(),
                scope: ResolveScope::Dashboard,
            };
            let fresh = resolve_replace_host_with(lookup, &model, &request)
                .await
                .map_err(|error| CloneRevalidationError::new(error.code()))?;
            let source_changed = fresh.source_host != mapping.source_host;
            let target_hidden = fresh.target_host.is_none();
            for reference in &mut mapping.references {
                let Some(stored_resolution) = reference
                    .targets
                    .iter_mut()
                    .find(|resolution| resolution.slot_id == slot.slot_id)
                else {
                    continue;
                };
                stored_resolution.check = CloneBindingCheck::Checked;
                stored_resolution.check_reason = None;
                if source_changed {
                    mark_resolution(
                        stored_resolution,
                        CloneBindingCheck::Stale,
                        CloneCheckReason::SourceChanged,
                    );
                    continue;
                }
                if target_hidden {
                    mark_resolution(
                        stored_resolution,
                        CloneBindingCheck::Invalid,
                        CloneCheckReason::TargetNotVisible,
                    );
                    continue;
                }
                let Some(fresh_row) = fresh.rows.iter().find(|row| row.field == reference.field)
                else {
                    mark_resolution(
                        stored_resolution,
                        CloneBindingCheck::Stale,
                        CloneCheckReason::ResolutionChanged,
                    );
                    continue;
                };
                if reference.source_evidence != fresh_row.source_evidence {
                    mark_resolution(
                        stored_resolution,
                        CloneBindingCheck::Stale,
                        CloneCheckReason::SourceChanged,
                    );
                } else if stored_resolution.decision.is_some() {
                    if !selected_lld_is_visible(stored_resolution, fresh_row) {
                        mark_resolution(
                            stored_resolution,
                            CloneBindingCheck::Invalid,
                            CloneCheckReason::CandidateNotVisible,
                        );
                    }
                } else if !fresh_matches(stored_resolution, fresh_row) {
                    mark_resolution(
                        stored_resolution,
                        CloneBindingCheck::Stale,
                        CloneCheckReason::ResolutionChanged,
                    );
                }
            }
        }
    }

    let slots = all_slots(&fresh_state)?;
    for decision in &mut fresh_state.pattern_decisions {
        decision.check = CloneBindingCheck::Checked;
        decision.check_reason = None;
        let checked_slots = slots_for_pattern(&slots, decision)?;
        let target_pattern = match &decision.action {
            ClonePatternAction::Replace { preview } => preview.target_pattern.clone(),
            ClonePatternAction::KeepAsIs => decision.requirement.source_pattern.clone(),
        };
        let preview = match preview_clone_pattern(
            lookup,
            decision.requirement.clone(),
            target_pattern,
            &checked_slots,
        )
        .await
        {
            Ok(preview) => preview,
            Err(error) if error.code() == "clone_pattern_target_not_visible" => {
                mark_pattern(
                    decision,
                    CloneBindingCheck::Invalid,
                    CloneCheckReason::TargetNotVisible,
                );
                continue;
            }
            Err(error) => return Err(CloneRevalidationError::new(error.code())),
        };
        if let ClonePatternAction::Replace { preview: stored } = &decision.action
            && **stored != preview
        {
            mark_pattern(
                decision,
                CloneBindingCheck::Stale,
                CloneCheckReason::PatternChanged,
            );
        }
    }

    let mut seen = BTreeSet::new();
    if fresh_state
        .pattern_decisions
        .iter()
        .any(|decision| !seen.insert(&decision.requirement.field))
    {
        return Err(CloneRevalidationError::new(
            "clone_pattern_decision_invalid",
        ));
    }
    Ok(fresh_state)
}
