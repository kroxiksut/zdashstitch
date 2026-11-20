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
use serde_json::Value;
use sha2::{Digest, Sha256};
use zdashstitch_zabbix::{
    AdapterProfile, HostDetails, HostObjects, ItemDetails, MAX_HOST_SEARCH, ReadOnlyGateway,
    SvgGraphItemMatch, SvgGraphItemTagFilter, VersionedDashboardSnapshot, ZabbixError,
    is_zabbix_id,
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
    /// 8.0 evidence covers every visible match and selected host, including
    /// objects outside the selected slots. Legacy selected-only records omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_fingerprint: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct VisiblePatternMatch {
    pub hostid: String,
    pub host_name: String,
    pub itemid: Option<String>,
    pub item_name: Option<String>,
    pub selected: bool,
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
        if preview.truncated {
            return Err(ClonePatternError::new("clone_pattern_preview_truncated"));
        }
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
                    && preview.visible_fingerprint.as_ref().is_none_or(|value| {
                        !preview.truncated && crate::workcopy::is_fingerprint(value)
                    })
                    && preview.matches.iter().all(|matched| {
                        preview.checked_slots.contains(&matched.slot_id)
                            && is_zabbix_id(&matched.object_id)
                            && !matched.name.is_empty()
                            && !matched.name.chars().any(char::is_control)
                            && (preview.visible_fingerprint.is_some()
                                || name_pattern_matches(&preview.target_pattern, &matched.name))
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
                SourceKind::HostPattern { pattern, .. }
                    if model.profile == AdapterProfile::V8_0 || pattern.contains('*') =>
                {
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

    fn visible_hosts_8_0(
        &self,
        _pattern: &str,
    ) -> impl Future<Output = Result<HostObjects<HostDetails>, ZabbixError>> + Send {
        async {
            Err(ZabbixError::UnsupportedVersion(
                "8.0 svggraph host search".to_owned(),
            ))
        }
    }

    fn visible_items_8_0(
        &self,
        _hostids: &[String],
        _pattern: &str,
        _filter: &SvgGraphItemTagFilter,
    ) -> impl Future<Output = Result<HostObjects<SvgGraphItemMatch>, ZabbixError>> + Send {
        async {
            Err(ZabbixError::UnsupportedVersion(
                "8.0 svggraph item search".to_owned(),
            ))
        }
    }
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

    async fn visible_hosts_8_0(
        &self,
        pattern: &str,
    ) -> Result<HostObjects<HostDetails>, ZabbixError> {
        if self.capabilities().version.to_string() == "8.0.0"
            && self.portable_pattern_inventory_role().await?.is_some()
        {
            return self.clone_svggraph_host_matches(pattern).await;
        }
        if pattern != "*" {
            return self.svggraph_host_matches(pattern).await;
        }
        // All-host reviews use the existing bounded visibility inventory.
        let found = self.search_hosts("", MAX_HOST_SEARCH).await?;
        Ok(HostObjects {
            truncated: found.truncated,
            objects: found
                .objects
                .into_iter()
                .map(|host| HostDetails {
                    hostid: host.hostid,
                    host: host.host,
                    name: host.name,
                })
                .collect(),
        })
    }

    async fn visible_items_8_0(
        &self,
        hostids: &[String],
        pattern: &str,
        filter: &SvgGraphItemTagFilter,
    ) -> Result<HostObjects<SvgGraphItemMatch>, ZabbixError> {
        if self.capabilities().version.to_string() == "8.0.0"
            && self.portable_pattern_inventory_role().await?.is_some()
        {
            return self
                .clone_svggraph_item_matches(hostids, pattern, filter)
                .await;
        }
        self.svggraph_item_matches(hostids, pattern, filter).await
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
        visible_fingerprint: None,
    })
}

pub(crate) fn snapshot_pattern_fields<'a>(
    source: &'a VersionedDashboardSnapshot,
    requirement: &ClonePatternRequirement,
) -> Result<&'a [Value], ClonePatternError> {
    let address = &requirement.field.widget;
    source.raw_api_document()["pages"][address.page_index]["widgets"][address.widget_index]
        ["fields"].as_array().map(Vec::as_slice)
        .ok_or_else(|| ClonePatternError::new("clone_pattern_invalid"))
}

/// Review one field against the account's bounded visibility in 8.0.
/// This estimates the scope of that field; the running widget can also apply
/// its other fields and its own limits.
pub(crate) async fn preview_clone_pattern_8_0<L: ClonePatternLookup>(
    lookup: &L,
    requirement: ClonePatternRequirement,
    target_pattern: String,
    slots: &[TargetSlot],
    fields: &[Value],
) -> Result<(ClonePatternPreview, Vec<VisiblePatternMatch>), ClonePatternError> {
    let invalid = || ClonePatternError::new("clone_pattern_invalid");
    let read_failed = |_| ClonePatternError::new("clone_pattern_preview_read_failed");
    let truncated = || ClonePatternError::new("clone_pattern_preview_truncated");
    let hidden = || ClonePatternError::new("clone_pattern_target_not_visible");
    if !requirement_is_well_formed(&requirement) || !valid_pattern(&target_pattern) {
        return Err(invalid());
    }
    if slots.is_empty() {
        return Err(ClonePatternError::new("clone_pattern_targets_empty"));
    }
    let hostids: Vec<_> = slots.iter().map(|slot| slot.host.hostid.clone()).collect();
    let slots_by_host: BTreeMap<_, _> = slots
        .iter()
        .map(|slot| (slot.host.hostid.as_str(), &slot.slot_id))
        .collect();
    let checked_slots: Vec<_> = slots.iter().map(|slot| slot.slot_id.clone()).collect();
    if slots_by_host.len() != slots.len() || !unique_slots(&checked_slots) {
        return Err(ClonePatternError::new("clone_pattern_target_duplicate"));
    }
    let selected = lookup.hosts_by_ids(&hostids).await.map_err(read_failed)?;
    let selected_by_id: BTreeMap<_, _> = selected
        .iter()
        .map(|host| (host.hostid.as_str(), host))
        .collect();
    if selected_by_id.len() != selected.len()
        || hostids
            .iter()
            .any(|id| !selected_by_id.contains_key(id.as_str()))
    {
        return Err(hidden());
    }

    let host_pattern = match requirement.kind {
        ClonePatternKind::Host => &target_pattern,
        ClonePatternKind::Item => "*",
    };
    let hosts = lookup
        .visible_hosts_8_0(host_pattern)
        .await
        .map_err(read_failed)?;
    if hosts.truncated {
        return Err(truncated());
    }
    let host_by_id: BTreeMap<_, _> = hosts
        .objects
        .iter()
        .map(|host| (host.hostid.as_str(), host))
        .collect();
    if host_by_id.len() != hosts.objects.len()
        || host_by_id.iter().any(|(id, host)| {
            selected_by_id
                .get(id)
                .is_some_and(|selected| selected != host)
        })
    {
        return Err(invalid());
    }
    let mut visible_matches = Vec::new();
    match requirement.kind {
        ClonePatternKind::Host => {
            visible_matches.extend(host_by_id.values().map(|host| VisiblePatternMatch {
                hostid: host.hostid.clone(),
                host_name: host.name.clone(),
                itemid: None,
                item_name: None,
                selected: slots_by_host.contains_key(host.hostid.as_str()),
            }));
        }
        ClonePatternKind::Item => {
            if hostids
                .iter()
                .any(|id| !host_by_id.contains_key(id.as_str()))
            {
                return Err(hidden());
            }
            let filter = match requirement
                .field
                .field_name
                .split('.')
                .collect::<Vec<_>>()
                .as_slice()
            {
                ["ds", set, "items", _] => SvgGraphItemTagFilter::from_widget_fields(
                    fields,
                    set.parse().map_err(|_| invalid())?,
                )
                .map_err(|_| invalid())?,
                _ => SvgGraphItemTagFilter::default(),
            };
            let visible_ids: Vec<_> = host_by_id.keys().map(|id| (*id).to_owned()).collect();
            let items = lookup
                .visible_items_8_0(&visible_ids, &target_pattern, &filter)
                .await
                .map_err(read_failed)?;
            if items.truncated {
                return Err(truncated());
            }
            let mut itemids = BTreeSet::new();
            for item in items.objects {
                let host = host_by_id.get(item.hostid.as_str()).ok_or_else(invalid)?;
                if !itemids.insert(item.itemid.clone()) {
                    return Err(invalid());
                }
                visible_matches.push(VisiblePatternMatch {
                    selected: slots_by_host.contains_key(item.hostid.as_str()),
                    hostid: item.hostid,
                    host_name: host.name.clone(),
                    itemid: Some(item.itemid),
                    item_name: Some(item.name),
                });
            }
        }
    }
    visible_matches
        .sort_by(|left, right| (&left.hostid, &left.itemid).cmp(&(&right.hostid, &right.itemid)));
    let matches: Vec<_> = visible_matches
        .iter()
        .filter_map(|item| {
            Some(ClonePatternMatch {
                slot_id: (*slots_by_host.get(item.hostid.as_str())?).clone(),
                object_id: item.itemid.as_ref().unwrap_or(&item.hostid).clone(),
                name: item.item_name.as_ref().unwrap_or(&item.host_name).clone(),
            })
        })
        .collect();
    let digest = serde_json::to_vec(&(&selected_by_id, &visible_matches)).map_err(|_| invalid())?;
    let preview = ClonePatternPreview {
        requirement,
        target_pattern,
        checked_slots,
        match_count: u32::try_from(matches.len()).unwrap_or(u32::MAX),
        matches,
        truncated: false,
        visible_fingerprint: Some(format!("{:x}", Sha256::digest(digest))),
    };
    Ok((preview, visible_matches))
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
            || matches!(&decision.action, ClonePatternAction::Replace { preview } if preview.truncated)
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
