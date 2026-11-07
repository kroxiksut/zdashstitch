// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use zdashstitch_zabbix::ReadOnlyGateway;

use crate::clone_mapping::{SourceHostDecision, SourceHostMapping, TargetSlotId};
use crate::resolver::{Evidence, FieldAddress, ItemEvidence, ItemOrigin, MatchStatus};
use crate::workcopy::WorkcopyError;

use super::{CloneSetupRequest, fingerprint};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CloneLldChoice {
    field: FieldAddress,
    slot_id: TargetSlotId,
    itemid: String,
    fingerprint: String,
}

#[derive(Serialize)]
pub struct CloneLldRow {
    field: FieldAddress,
    slot_id: TargetSlotId,
    source: ItemEvidence,
    source_host: String,
    target_host: String,
    prototype_key: Option<String>,
    candidates: Vec<ItemEvidence>,
    fingerprint: String,
}

pub(super) fn rows(
    reader: &ReadOnlyGateway,
    request: &CloneSetupRequest,
    mappings: &[SourceHostMapping],
) -> Result<Vec<CloneLldRow>, WorkcopyError> {
    let mut rows = Vec::new();
    for mapping in mappings {
        let SourceHostDecision::TargetSlots(slots) = &mapping.decision else {
            continue;
        };
        for reference in &mapping.references {
            for target in &reference.targets {
                if target.match_status != MatchStatus::UnresolvedLld {
                    continue;
                }
                let Some(Evidence::Item(source)) = &reference.source_evidence else {
                    return Err(WorkcopyError::new("clone_lld_source_not_discovered"));
                };
                let slot = slots
                    .iter()
                    .find(|slot| slot.slot_id == target.slot_id)
                    .ok_or_else(|| WorkcopyError::new("clone_target_slot_not_found"))?;
                let fingerprint = fingerprint(&(
                    reader.identity().fingerprint(),
                    request.revision,
                    &request.source_fingerprint,
                    &mapping.source_host,
                    &reference.field,
                    source,
                    slot,
                    &target.candidates,
                ))?;
                rows.push(CloneLldRow {
                    field: reference.field.clone(),
                    slot_id: slot.slot_id.clone(),
                    source: source.clone(),
                    source_host: mapping.source_host.name.clone(),
                    target_host: slot.host.name.clone(),
                    prototype_key: match &source.origin {
                        ItemOrigin::Discovered { prototype_key } => Some(prototype_key.clone()),
                        _ => None,
                    },
                    candidates: target.candidates.clone(),
                    fingerprint,
                });
            }
        }
    }
    Ok(rows)
}

pub(super) fn apply(
    reader: &ReadOnlyGateway,
    request: &CloneSetupRequest,
    mappings: &mut [SourceHostMapping],
) -> Result<(), WorkcopyError> {
    let fresh = rows(reader, request, mappings)?;
    let mut choices = BTreeMap::new();
    for choice in &request.lld_choices {
        if choices
            .insert((&choice.field, &choice.slot_id), choice)
            .is_some()
        {
            return Err(WorkcopyError::new("clone_lld_selection_stale"));
        }
    }
    for row in &fresh {
        let choice = choices
            .remove(&(&row.field, &row.slot_id))
            .ok_or_else(|| WorkcopyError::new("clone_lld_selection_required"))?;
        if choice.fingerprint != row.fingerprint {
            return Err(WorkcopyError::new("clone_lld_selection_stale"));
        }
        let reference = mappings
            .iter_mut()
            .flat_map(|mapping| &mut mapping.references)
            .find(|reference| {
                reference.field == row.field
                    && reference
                        .targets
                        .iter()
                        .any(|target| target.slot_id == row.slot_id)
            })
            .ok_or_else(|| WorkcopyError::new("clone_lld_selection_stale"))?;
        reference
            .choose_lld_instance(&row.slot_id, &choice.itemid)
            .map_err(|error| WorkcopyError::new(error.code()))?;
    }
    if !choices.is_empty() {
        return Err(WorkcopyError::new("clone_lld_selection_stale"));
    }
    Ok(())
}
