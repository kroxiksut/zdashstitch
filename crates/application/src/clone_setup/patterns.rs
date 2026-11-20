// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use zdashstitch_zabbix::ReadOnlyGateway;

use crate::clone_mapping::{
    ClonePatternDecision, ClonePatternPreview, ClonePatternRequirement, SourceHostDecision,
    TargetSlot, VisiblePatternMatch, preview_clone_pattern, preview_clone_pattern_8_0,
    snapshot_pattern_fields,
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
        #[serde(default)]
        acknowledge_visible_outside: bool,
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
    outside: bool,
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
    source: &zdashstitch_zabbix::VersionedDashboardSnapshot,
) -> Result<(ClonePatternPreview, Vec<VisiblePatternMatch>, String), WorkcopyError> {
    let error = |error: crate::clone_mapping::ClonePatternError| WorkcopyError::new(error.code());
    let (preview, visible) = if source.server_version().major == 8 {
        preview_clone_pattern_8_0(
            reader,
            requirement.clone(),
            proposed.to_owned(),
            slots,
            snapshot_pattern_fields(source, requirement).map_err(error)?,
        )
        .await
        .map_err(error)?
    } else {
        (
            preview_clone_pattern(reader, requirement.clone(), proposed.to_owned(), slots)
                .await
                .map_err(error)?,
            Vec::new(),
        )
    };
    let digest = fingerprint(&(
        reader.identity().fingerprint(),
        request.revision,
        &request.source_fingerprint,
        slots,
        &preview,
    ))?;
    Ok((preview, visible, digest))
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
    let (preview, visible, fingerprint) = fresh_preview(
        reader,
        &request.setup,
        requirement,
        &request.target_pattern,
        &slots,
        &setup.source,
    )
    .await?;
    let names = slots
        .iter()
        .map(|slot| (&slot.slot_id, &slot.host.name))
        .collect::<BTreeMap<_, _>>();
    let matches = if preview.visible_fingerprint.is_some() {
        visible
            .into_iter()
            .map(|item| NamedMatch {
                host: item.host_name,
                name: item.item_name.unwrap_or_default(),
                outside: !item.selected,
            })
            .collect()
    } else {
        preview
            .matches
            .into_iter()
            .map(|item| {
                let host = names
                    .get(&item.slot_id)
                    .ok_or_else(|| WorkcopyError::new("clone_pattern_decision_invalid"))?;
                Ok(NamedMatch {
                    host: (*host).clone(),
                    name: item.name,
                    outside: false,
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    Ok(ClonePatternReview {
        fingerprint,
        match_count: if preview.visible_fingerprint.is_some() {
            matches.len() as u32
        } else {
            preview.match_count
        },
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
    let visible_review = setup.source.server_version().major == 8;
    if request.keep_patterns {
        if !request.pattern_choices.is_empty() || (visible_review && !setup.patterns.is_empty()) {
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
            PatternAction::Keep if !visible_review => {
                ClonePatternDecision::keep_as_is(requirement.clone())
            }
            PatternAction::Keep => {
                return Err(WorkcopyError::new("clone_pattern_decision_required"));
            }
            PatternAction::Replace {
                target_pattern,
                fingerprint,
                acknowledge_visible_outside,
            } => {
                let (preview, visible, fresh) = fresh_preview(
                    reader,
                    request,
                    requirement,
                    target_pattern,
                    &slots,
                    &setup.source,
                )
                .await?;
                if &fresh != fingerprint {
                    return Err(WorkcopyError::new("clone_pattern_preview_stale"));
                }
                if visible.iter().any(|item| !item.selected) && !acknowledge_visible_outside {
                    return Err(WorkcopyError::new("clone_pattern_decision_required"));
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
