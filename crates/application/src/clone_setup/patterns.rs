// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use zdashstitch_zabbix::ReadOnlyGateway;

use crate::clone_mapping::{
    ClonePatternDecision, ClonePatternPreview, ClonePatternRequirement, SourceHostDecision,
    TargetSlot, preview_clone_pattern,
};
use crate::resolver::FieldAddress;
use crate::workcopy::WorkcopyError;

use super::{CloneSetupRequest, SetupMappings, fingerprint};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClonePatternChoice {
    field: FieldAddress,
    decision: PatternAction,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum PatternAction {
    Keep,
    Replace {
        target_pattern: String,
        fingerprint: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClonePatternReviewRequest {
    pub(super) setup: CloneSetupRequest,
    field: FieldAddress,
    target_pattern: String,
}

#[derive(Serialize)]
pub struct ClonePatternReview {
    fingerprint: String,
    match_count: u32,
    matches: Vec<NamedMatch>,
    truncated: bool,
}

#[derive(Serialize)]
struct NamedMatch {
    host: String,
    name: String,
}

fn selected_slots(setup: &SetupMappings) -> Vec<TargetSlot> {
    setup
        .mappings
        .iter()
        .flat_map(|mapping| match &mapping.decision {
            SourceHostDecision::TargetSlots(slots) => slots.as_slice(),
            SourceHostDecision::KeepSource => &[],
        })
        .cloned()
        .collect()
}

async fn fresh_preview(
    reader: &ReadOnlyGateway,
    request: &CloneSetupRequest,
    requirement: &ClonePatternRequirement,
    proposed: &str,
    slots: &[TargetSlot],
) -> Result<(ClonePatternPreview, String), WorkcopyError> {
    let preview = preview_clone_pattern(reader, requirement.clone(), proposed.to_owned(), slots)
        .await
        .map_err(|error| WorkcopyError::new(error.code()))?;
    let digest = fingerprint(&(
        reader.identity().fingerprint(),
        request.revision,
        &request.source_fingerprint,
        slots,
        &preview,
    ))?;
    Ok((preview, digest))
}

pub(super) async fn review(
    reader: &ReadOnlyGateway,
    request: &ClonePatternReviewRequest,
    setup: &SetupMappings,
) -> Result<ClonePatternReview, WorkcopyError> {
    let requirement = setup
        .patterns
        .iter()
        .find(|row| row.field == request.field)
        .ok_or_else(|| WorkcopyError::new("clone_pattern_decision_invalid"))?;
    let slots = selected_slots(setup);
    let (preview, fingerprint) = fresh_preview(
        reader,
        &request.setup,
        requirement,
        &request.target_pattern,
        &slots,
    )
    .await?;
    let names = slots
        .iter()
        .map(|slot| (&slot.slot_id, &slot.host.name))
        .collect::<BTreeMap<_, _>>();
    let matches = preview
        .matches
        .into_iter()
        .map(|item| {
            let host = names
                .get(&item.slot_id)
                .ok_or_else(|| WorkcopyError::new("clone_pattern_decision_invalid"))?;
            Ok(NamedMatch {
                host: (*host).clone(),
                name: item.name,
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(ClonePatternReview {
        fingerprint,
        match_count: preview.match_count,
        matches,
        truncated: preview.truncated,
    })
}

pub(super) async fn apply(
    reader: &ReadOnlyGateway,
    request: &CloneSetupRequest,
    setup: &SetupMappings,
) -> Result<Vec<ClonePatternDecision>, WorkcopyError> {
    let invalid = || WorkcopyError::new("clone_pattern_decision_invalid");
    if request.keep_patterns {
        if !request.pattern_choices.is_empty() {
            return Err(invalid());
        }
        return Ok(setup
            .patterns
            .iter()
            .cloned()
            .map(ClonePatternDecision::keep_as_is)
            .collect());
    }
    let mut choices = BTreeMap::new();
    for choice in &request.pattern_choices {
        if choices.insert(&choice.field, &choice.decision).is_some() {
            return Err(invalid());
        }
    }
    let slots = selected_slots(setup);
    let mut decisions = Vec::new();
    for requirement in &setup.patterns {
        let action = choices
            .remove(&requirement.field)
            .ok_or_else(|| WorkcopyError::new("clone_pattern_decision_required"))?;
        let decision = match action {
            PatternAction::Keep => ClonePatternDecision::keep_as_is(requirement.clone()),
            PatternAction::Replace {
                target_pattern,
                fingerprint,
            } => {
                let (preview, fresh) =
                    fresh_preview(reader, request, requirement, target_pattern, &slots).await?;
                if &fresh != fingerprint {
                    return Err(WorkcopyError::new("clone_pattern_preview_stale"));
                }
                ClonePatternDecision::replace(preview)
                    .map_err(|error| WorkcopyError::new(error.code()))?
            }
        };
        decisions.push(decision);
    }
    if !choices.is_empty() {
        return Err(invalid());
    }
    Ok(decisions)
}
