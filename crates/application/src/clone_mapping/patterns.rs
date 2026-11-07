// SPDX-License-Identifier: MIT OR Apache-2.0

//! Explicit Clone decisions for `svggraph` host masks and item patterns.
//!
//! Preview reads only the already selected target hosts and their items. It
//! follows the Zabbix 7.4 frontend matching rule (`*`, case-insensitive) and
//! never turns a similar name into a decision.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::Future;

use serde::{Deserialize, Serialize};
use zdashstitch_zabbix::{
    HostDetails, HostObjects, ItemDetails, ReadOnlyGateway, ZabbixError, is_zabbix_id,
};

use super::{CloneBindingCheck, CloneCheckReason, TargetSlot, TargetSlotId};
use crate::resolver::{
    FieldAddress, MAX_PATTERN_CANDIDATES, SourceKind, SourceModel, name_pattern_matches,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClonePatternKind {
    Host,
    Item,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClonePatternRequirement {
    pub field: FieldAddress,
    pub widget_type: String,
    pub kind: ClonePatternKind,
    pub source_pattern: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClonePatternMatch {
    pub slot_id: TargetSlotId,
    pub object_id: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClonePatternPreview {
    pub requirement: ClonePatternRequirement,
    pub target_pattern: String,
    pub checked_slots: Vec<TargetSlotId>,
    pub match_count: u32,
    pub matches: Vec<ClonePatternMatch>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ClonePatternAction {
    Replace { preview: Box<ClonePatternPreview> },
    KeepAsIs,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClonePatternDecision {
    pub requirement: ClonePatternRequirement,
    pub action: ClonePatternAction,
    #[serde(default)]
    pub check: CloneBindingCheck,
    #[serde(default)]
    pub check_reason: Option<CloneCheckReason>,
}

impl ClonePatternDecision {
    #[must_use]
    pub fn keep_as_is(requirement: ClonePatternRequirement) -> Self {
        Self {
            requirement,
            action: ClonePatternAction::KeepAsIs,
            check: CloneBindingCheck::Checked,
            check_reason: None,
        }
    }

    pub fn replace(preview: ClonePatternPreview) -> Result<Self, ClonePatternError> {
        let decision = Self {
            requirement: preview.requirement.clone(),
            action: ClonePatternAction::Replace {
                preview: Box::new(preview),
            },
            check: CloneBindingCheck::Checked,
            check_reason: None,
        };
        if !decision.is_well_formed() {
            return Err(ClonePatternError::new("clone_pattern_decision_invalid"));
        }
        Ok(decision)
    }

    pub(crate) fn is_well_formed(&self) -> bool {
        if !requirement_is_well_formed(&self.requirement) {
            return false;
        }
        if (self.check == CloneBindingCheck::Checked && self.check_reason.is_some())
            || (self.check != CloneBindingCheck::Checked && self.check_reason.is_none())
        {
            return false;
        }
        match &self.action {
            ClonePatternAction::KeepAsIs => true,
            ClonePatternAction::Replace { preview } => {
                preview.requirement == self.requirement
                    && valid_pattern(&preview.target_pattern)
                    && unique_slots(&preview.checked_slots)
                    && !preview.checked_slots.is_empty()
                    && preview.match_count >= preview.matches.len() as u32
                    && preview.truncated == (preview.match_count as usize > preview.matches.len())
                    && preview.matches.iter().all(|matched| {
                        preview.checked_slots.contains(&matched.slot_id)
                            && is_zabbix_id(&matched.object_id)
                            && !matched.name.is_empty()
                            && !matched.name.chars().any(char::is_control)
                            && name_pattern_matches(&preview.target_pattern, &matched.name)
                    })
            }
        }
    }
}

fn valid_pattern(pattern: &str) -> bool {
    !pattern.is_empty() && pattern.len() <= 255 && !pattern.chars().any(char::is_control)
}

fn requirement_is_well_formed(requirement: &ClonePatternRequirement) -> bool {
    if requirement.widget_type != "svggraph" || !valid_pattern(&requirement.source_pattern) {
        return false;
    }
    let parts = requirement.field.field_name.split('.').collect::<Vec<_>>();
    match (requirement.kind, parts.as_slice()) {
        (ClonePatternKind::Host, ["problemhosts", position]) => position.parse::<u32>().is_ok(),
        (ClonePatternKind::Host, ["ds" | "or", set, "hosts", position])
        | (ClonePatternKind::Item, ["ds" | "or", set, "items", position]) => {
            set.parse::<u32>().is_ok() && position.parse::<u32>().is_ok()
        }
        _ => false,
    }
}

fn unique_slots(slots: &[TargetSlotId]) -> bool {
    let mut found = BTreeSet::new();
    slots.iter().all(|slot| found.insert(slot))
}

/// Every decision required by P3-02I, in snapshot order. Exact host names
/// are handled by ordinary mapping; masks and all item patterns are explicit.
#[must_use]
pub fn pattern_requirements(model: &SourceModel) -> Vec<ClonePatternRequirement> {
    model
        .sources()
        .filter_map(|(widget, source)| {
            let (kind, source_pattern) = match &source.kind {
                SourceKind::HostPattern { pattern, .. } if pattern.contains('*') => {
                    (ClonePatternKind::Host, pattern.clone())
                }
                SourceKind::ItemPattern { pattern, .. } => {
                    (ClonePatternKind::Item, pattern.clone())
                }
                _ => return None,
            };
            Some(ClonePatternRequirement {
                field: source.field.clone(),
                widget_type: widget.widget_type.clone(),
                kind,
                source_pattern,
            })
        })
        .collect()
}

/// Only the read-only gateway implements production preview lookups.
pub trait ClonePatternLookup {
    fn hosts_by_ids(
        &self,
        hostids: &[String],
    ) -> impl Future<Output = Result<Vec<HostDetails>, ZabbixError>> + Send;

    fn host_items(
        &self,
        hostid: &str,
    ) -> impl Future<Output = Result<HostObjects<ItemDetails>, ZabbixError>> + Send;
}

impl ClonePatternLookup for ReadOnlyGateway {
    fn hosts_by_ids(
        &self,
        hostids: &[String],
    ) -> impl Future<Output = Result<Vec<HostDetails>, ZabbixError>> + Send {
        ReadOnlyGateway::hosts_by_ids(self, hostids)
    }

    fn host_items(
        &self,
        hostid: &str,
    ) -> impl Future<Output = Result<HostObjects<ItemDetails>, ZabbixError>> + Send {
        ReadOnlyGateway::host_items(self, hostid)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClonePatternError {
    code: &'static str,
    params: BTreeMap<&'static str, String>,
}

impl ClonePatternError {
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

impl fmt::Display for ClonePatternError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code)
    }
}

impl std::error::Error for ClonePatternError {}

/// Preview a proposed pattern against the frozen target slots. Missing hosts
/// and truncated item inventories fail closed and are never called matches.
pub async fn preview_clone_pattern<L: ClonePatternLookup>(
    lookup: &L,
    requirement: ClonePatternRequirement,
    target_pattern: String,
    target_slots: &[TargetSlot],
) -> Result<ClonePatternPreview, ClonePatternError> {
    if !requirement_is_well_formed(&requirement) || !valid_pattern(&target_pattern) {
        return Err(ClonePatternError::new("clone_pattern_invalid"));
    }
    if target_slots.is_empty() {
        return Err(ClonePatternError::new("clone_pattern_targets_empty"));
    }
    let checked_slots = target_slots
        .iter()
        .map(|slot| slot.slot_id.clone())
        .collect::<Vec<_>>();
    if !unique_slots(&checked_slots) {
        return Err(ClonePatternError::new("clone_pattern_target_duplicate"));
    }
    let hostids = target_slots
        .iter()
        .map(|slot| slot.host.hostid.clone())
        .collect::<Vec<_>>();
    let visible = lookup
        .hosts_by_ids(&hostids)
        .await
        .map_err(|_| ClonePatternError::new("clone_pattern_preview_read_failed"))?;
    let visible = visible
        .into_iter()
        .map(|host| (host.hostid.clone(), host))
        .collect::<BTreeMap<_, _>>();
    if hostids.iter().any(|hostid| !visible.contains_key(hostid)) {
        return Err(ClonePatternError::new("clone_pattern_target_not_visible"));
    }

    let mut matches = Vec::new();
    for slot in target_slots {
        match requirement.kind {
            ClonePatternKind::Host => {
                let host = &visible[&slot.host.hostid];
                if name_pattern_matches(&target_pattern, &host.name) {
                    matches.push(ClonePatternMatch {
                        slot_id: slot.slot_id.clone(),
                        object_id: host.hostid.clone(),
                        name: host.name.clone(),
                    });
                }
            }
            ClonePatternKind::Item => {
                let items = lookup
                    .host_items(&slot.host.hostid)
                    .await
                    .map_err(|_| ClonePatternError::new("clone_pattern_preview_read_failed"))?;
                if items.truncated {
                    return Err(ClonePatternError::new("clone_pattern_preview_truncated")
                        .with("slot_id", slot.slot_id.0.clone()));
                }
                for item in items.objects {
                    if name_pattern_matches(&target_pattern, &item.name) {
                        matches.push(ClonePatternMatch {
                            slot_id: slot.slot_id.clone(),
                            object_id: item.itemid,
                            name: item.name,
                        });
                    }
                }
            }
        }
    }
    matches.sort_by(|left, right| {
        let slots = checked_slots
            .iter()
            .position(|slot| slot == &left.slot_id)
            .cmp(&checked_slots.iter().position(|slot| slot == &right.slot_id));
        slots
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.object_id.cmp(&right.object_id))
    });
    let match_count = u32::try_from(matches.len()).unwrap_or(u32::MAX);
    let truncated = matches.len() > MAX_PATTERN_CANDIDATES;
    matches.truncate(MAX_PATTERN_CANDIDATES);
    Ok(ClonePatternPreview {
        requirement,
        target_pattern,
        checked_slots,
        match_count,
        matches,
        truncated,
    })
}

pub(crate) fn decisions_by_field(
    requirements: &[ClonePatternRequirement],
    decisions: &[ClonePatternDecision],
) -> Result<BTreeMap<FieldAddress, ClonePatternDecision>, ClonePatternError> {
    let required = requirements
        .iter()
        .map(|requirement| (requirement.field.clone(), requirement))
        .collect::<BTreeMap<_, _>>();
    if required.len() != requirements.len() {
        return Err(ClonePatternError::new(
            "clone_pattern_requirement_duplicate",
        ));
    }
    let mut found = BTreeMap::new();
    for decision in decisions {
        if !decision.is_well_formed()
            || decision.check != CloneBindingCheck::Checked
            || required.get(&decision.requirement.field) != Some(&&decision.requirement)
            || found
                .insert(decision.requirement.field.clone(), decision.clone())
                .is_some()
        {
            return Err(ClonePatternError::new("clone_pattern_decision_invalid"));
        }
    }
    if found.len() != required.len() {
        return Err(ClonePatternError::new("clone_pattern_decision_required"));
    }
    Ok(found)
}
