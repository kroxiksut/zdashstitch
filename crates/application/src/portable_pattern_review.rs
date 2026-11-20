// SPDX-License-Identifier: MIT OR Apache-2.0

//! Read-only target review of portable dynamic patterns. A review is not a
//! publication plan and must be repeated and bound to the future P5 plan.

use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, sync::Mutex};
use zdashstitch_core::formats::{
    Cardinality, ProjectRoot, TypedPortableBundle, Widget, ZabbixWidgetField,
    ZabbixWidgetFieldValue,
};
use zdashstitch_zabbix::{
    HostDetails, HostObjects, ItemDetails, ReadOnlyGateway, SvgGraphItemMatch,
    SvgGraphItemTagFilter, ZabbixError,
};

pub use crate::clone_mapping::VisiblePatternMatch;
use crate::{
    clone_mapping::{
        ClonePatternKind, ClonePatternLookup, ClonePatternPreview, ClonePatternRequirement,
        TargetSlot, preview_clone_pattern, preview_clone_pattern_8_0,
    },
    resolver::{FieldAddress, WidgetAddress},
    workcopy::{
        WorkcopyError, WorkcopyService,
        store::{LocalStore, read_bounded, write_atomic},
    },
};

const BINDINGS_PATH: &[&str] = &["bindings"];
const BINDINGS_FILE: &str = "portable-pattern-v1.json";
const MAX_BINDINGS_BYTES: usize = 1024 * 1024;
const MAX_BINDINGS: usize = 2048;
static PATTERN_BINDINGS_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortablePatternReviewRequest {
    pub revision: u64,
    pub page_index: usize,
    pub widget_index: usize,
    pub field_index: usize,
    pub targets: Vec<TargetSlot>,
}

#[derive(Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SavedPattern {
    pub(crate) local_id: String,
    pub(crate) server: String,
    pub(crate) review: PortablePatternReviewRequest,
    pub(crate) fingerprint: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct PatternFile {
    format: String,
    bindings: Vec<SavedPattern>,
}

fn read_pattern_file(store: &LocalStore) -> Result<PatternFile, WorkcopyError> {
    let empty = || PatternFile {
        format: "portable-pattern-bindings/v1".into(),
        bindings: Vec::new(),
    };
    let Some(dir) = store.path(BINDINGS_PATH, false)? else {
        return Ok(empty());
    };
    let Some(raw) = read_bounded(&dir, BINDINGS_FILE, MAX_BINDINGS_BYTES)? else {
        return Ok(empty());
    };
    let file: PatternFile = serde_json::from_slice(&raw)
        .map_err(|_| WorkcopyError::new("portable_pattern_review_invalid"))?;
    if file.format != "portable-pattern-bindings/v1"
        || file.bindings.len() > MAX_BINDINGS
        || file.bindings.iter().any(|b| {
            b.server.len() != 64
                || b.fingerprint.len() != 64
                || !b.server.bytes().all(|c| c.is_ascii_hexdigit())
                || !b.fingerprint.bytes().all(|c| c.is_ascii_hexdigit())
        })
    {
        return Err(WorkcopyError::new("portable_pattern_review_invalid"));
    }
    Ok(file)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortablePatternReopenRequest {
    pub revision: u64,
    pub page_index: usize,
    pub widget_index: usize,
    pub field_index: usize,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortablePatternConfirmRequest {
    pub review: PortablePatternReviewRequest,
    pub fingerprint: String,
    /// Explicit acknowledgement that the displayed visible matches include
    /// objects outside the selected slots.
    #[serde(default)]
    pub acknowledge_visible_outside: bool,
}

#[derive(serde::Serialize)]
pub struct PortablePatternReviewResponse {
    pub preview: ClonePatternPreview,
    /// Matches visible to this account, including outside the selected slots.
    /// Other Zabbix users may have different visibility.
    pub visible_matches: Vec<VisiblePatternMatch>,
    /// A review fingerprint is not a plan or permission to publish.
    pub fingerprint: String,
}

impl WorkcopyService {
    fn saved_pattern(
        &self,
        server: &str,
        local_id: &str,
        request: &PortablePatternReopenRequest,
    ) -> Result<SavedPattern, WorkcopyError> {
        let _guard = PATTERN_BINDINGS_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_unavailable"))?;
        let mut matches = read_pattern_file(&self.store)?
            .bindings
            .into_iter()
            .filter(|b| {
                b.local_id == local_id
                    && b.server == server
                    && b.review.revision == request.revision
                    && b.review.page_index == request.page_index
                    && b.review.widget_index == request.widget_index
                    && b.review.field_index == request.field_index
            });
        let saved = matches
            .next()
            .ok_or_else(|| WorkcopyError::new("portable_pattern_review_invalid"))?;
        if matches.next().is_some() {
            return Err(WorkcopyError::new("portable_pattern_review_invalid"));
        }
        Ok(saved)
    }

    pub(crate) async fn verify_saved_pattern_for_plan(
        &self,
        reader: &zdashstitch_zabbix::ReadOnlyGateway,
        local_id: &str,
        request: &PortablePatternReopenRequest,
    ) -> Result<(SavedPattern, PortablePatternReviewResponse), WorkcopyError> {
        let before = self.saved_pattern(reader.identity().fingerprint(), local_id, request)?;
        let current = self
            .reopen_saved_portable_pattern(reader, local_id, request)
            .await?;
        let after = self.saved_pattern(reader.identity().fingerprint(), local_id, request)?;
        if before != after || before.fingerprint != current.fingerprint {
            return Err(WorkcopyError::new("portable_pattern_review_changed"));
        }
        Ok((before, current))
    }

    pub(crate) fn portable_pattern_state_matches(&self, expected: &[SavedPattern]) -> bool {
        if expected.is_empty() {
            return true;
        }
        let Ok(_guard) = PATTERN_BINDINGS_LOCK.lock() else {
            return false;
        };
        let Ok(file) = read_pattern_file(&self.store) else {
            return false;
        };
        expected.iter().all(|entry| {
            let mut matches = file.bindings.iter().filter(|b| {
                b.local_id == entry.local_id
                    && b.server == entry.server
                    && b.review.page_index == entry.review.page_index
                    && b.review.widget_index == entry.review.widget_index
                    && b.review.field_index == entry.review.field_index
            });
            matches.next() == Some(entry) && matches.next().is_none()
        })
    }
    /// Confirm only the currently visible review. This is not a publish grant.
    pub async fn confirm_saved_portable_pattern(
        &self,
        reader: &zdashstitch_zabbix::ReadOnlyGateway,
        local_id: &str,
        request: &PortablePatternConfirmRequest,
    ) -> Result<PortablePatternReviewResponse, WorkcopyError> {
        let current = self
            .preview_saved_portable_pattern(reader, local_id, &request.review)
            .await?;
        if current.fingerprint != request.fingerprint {
            return Err(WorkcopyError::new("portable_pattern_review_changed"));
        }
        if current.visible_matches.iter().any(|item| !item.selected)
            && !request.acknowledge_visible_outside
        {
            return Err(WorkcopyError::new("clone_pattern_decision_required"));
        }
        let _guard = PATTERN_BINDINGS_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        let mut file = read_pattern_file(&self.store)?;
        file.bindings.retain(|old| {
            !(old.local_id == local_id
                && old.server == reader.identity().fingerprint()
                && old.review.page_index == request.review.page_index
                && old.review.widget_index == request.review.widget_index
                && old.review.field_index == request.review.field_index)
        });
        if file.bindings.len() >= MAX_BINDINGS {
            return Err(WorkcopyError::new("storage_limit_reached"));
        }
        file.bindings.push(SavedPattern {
            local_id: local_id.into(),
            server: reader.identity().fingerprint().into(),
            review: request.review.clone(),
            fingerprint: current.fingerprint.clone(),
        });
        let raw =
            serde_json::to_vec(&file).map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        if raw.len() > MAX_BINDINGS_BYTES {
            return Err(WorkcopyError::new("storage_limit_reached"));
        }
        let dir = self
            .store
            .path(BINDINGS_PATH, true)?
            .ok_or_else(|| WorkcopyError::new("storage_unavailable"))?;
        write_atomic(&dir, BINDINGS_FILE, &raw)?;
        Ok(current)
    }

    /// A saved confirmation is only a local hint. Read the target again and
    /// reject stale or concurrently changed choices before returning it.
    pub async fn reopen_saved_portable_pattern(
        &self,
        reader: &zdashstitch_zabbix::ReadOnlyGateway,
        local_id: &str,
        request: &PortablePatternReopenRequest,
    ) -> Result<PortablePatternReviewResponse, WorkcopyError> {
        let find = || self.saved_pattern(reader.identity().fingerprint(), local_id, request);
        let saved = find()?;
        let current = self
            .preview_saved_portable_pattern(reader, local_id, &saved.review)
            .await?;
        if current.fingerprint != saved.fingerprint || find()? != saved {
            return Err(WorkcopyError::new("portable_pattern_review_changed"));
        }
        Ok(current)
    }
    /// Read a saved local draft, review against the active read-only target,
    /// then reject if its revision changed during the network reads.
    pub async fn preview_saved_portable_pattern(
        &self,
        reader: &zdashstitch_zabbix::ReadOnlyGateway,
        local_id: &str,
        request: &PortablePatternReviewRequest,
    ) -> Result<PortablePatternReviewResponse, WorkcopyError> {
        let load = || -> Result<(u64, TypedPortableBundle), WorkcopyError> {
            let root = self.store.root_path();
            if local_id.starts_with("pg-") {
                let page = crate::offline_pages::OfflinePageStore::new(root)
                    .get(local_id)?
                    .ok_or_else(|| WorkcopyError::new("offline_page_not_found"))?;
                Ok((page.revision, page.bundle))
            } else if local_id.starts_with("db-") {
                let dashboard = crate::offline_dashboards::OfflineDashboardStore::new(root)
                    .get(local_id)?
                    .ok_or_else(|| WorkcopyError::new("offline_dashboard_not_found"))?;
                Ok((dashboard.revision, dashboard.bundle))
            } else {
                Err(WorkcopyError::new("workcopy_id_invalid"))
            }
        };
        let (actual, bundle) = load()?;
        if actual != request.revision {
            return Err(WorkcopyError::new("workcopy_revision_conflict"));
        }
        let (preview, visible_matches) = if reader.capabilities().version.major == 8 {
            if !matches!(reader.matches_live_version().await, Ok(true)) {
                return Err(WorkcopyError::new("portable_pattern_review_changed"));
            }
            let (requirement, fields) = portable_pattern_input(
                &bundle,
                request.page_index,
                request.widget_index,
                request.field_index,
                &request.targets,
            )?;
            let fields: Vec<_> = fields.iter()
                .filter(|field| field.name.contains(".item_tags"))
                .map(|field| {
                    let value = match &field.value {
                        ZabbixWidgetFieldValue::Integer { value } => serde_json::json!(value),
                        ZabbixWidgetFieldValue::String { value } => serde_json::json!(value),
                        _ => serde_json::Value::Null,
                    };
                    serde_json::json!({"name": field.name, "type": field.field_type, "value": value})
                }).collect();
            let target_pattern = requirement.source_pattern.clone();
            let review = preview_clone_pattern_8_0(
                &PortablePatternLookup(reader),
                requirement,
                target_pattern,
                &request.targets,
                &fields,
            )
            .await
            .map_err(|error| WorkcopyError::new(error.code()))?;
            if !matches!(reader.matches_live_version().await, Ok(true)) {
                return Err(WorkcopyError::new("portable_pattern_review_changed"));
            }
            review
        } else {
            let preview = preview_portable_pattern(
                reader,
                &bundle,
                request.page_index,
                request.widget_index,
                request.field_index,
                &request.targets,
            )
            .await?;
            let visible = visible_portable_matches_7_4(reader, &preview, &request.targets).await?;
            (preview, visible)
        };
        let (after_revision, after) = load()?;
        if after_revision != request.revision || after != bundle {
            return Err(WorkcopyError::new("workcopy_revision_conflict"));
        }
        let encoded = serde_json::to_vec(&(
            "zdashstitch-portable-pattern-review-v1",
            reader.identity().fingerprint(),
            reader.capabilities().version.to_string(),
            request.revision,
            &bundle,
            request.page_index,
            request.widget_index,
            request.field_index,
            &request.targets,
            &preview,
            &visible_matches,
        ))
        .map_err(|_| WorkcopyError::new("portable_pattern_review_invalid"))?;
        Ok(PortablePatternReviewResponse {
            preview,
            visible_matches,
            fingerprint: format!("{:x}", Sha256::digest(encoded)),
        })
    }
}

struct PortablePatternLookup<'a>(&'a ReadOnlyGateway);

impl ClonePatternLookup for PortablePatternLookup<'_> {
    async fn hosts_by_ids(&self, ids: &[String]) -> Result<Vec<HostDetails>, ZabbixError> {
        self.0.hosts_by_ids(ids).await
    }

    async fn host_items(&self, hostid: &str) -> Result<HostObjects<ItemDetails>, ZabbixError> {
        self.0.portable_host_items(hostid).await
    }

    async fn visible_hosts_8_0(
        &self,
        pattern: &str,
    ) -> Result<HostObjects<HostDetails>, ZabbixError> {
        self.0.portable_svggraph_host_matches(pattern).await
    }

    async fn visible_items_8_0(
        &self,
        hostids: &[String],
        pattern: &str,
        filter: &SvgGraphItemTagFilter,
    ) -> Result<HostObjects<SvgGraphItemMatch>, ZabbixError> {
        self.0
            .portable_svggraph_item_matches(hostids, pattern, filter)
            .await
    }
}

async fn visible_portable_matches_7_4(
    reader: &zdashstitch_zabbix::ReadOnlyGateway,
    preview: &ClonePatternPreview,
    targets: &[TargetSlot],
) -> Result<Vec<VisiblePatternMatch>, WorkcopyError> {
    // Check the entire bounded inventory visible to the current account,
    // not just the selected slots. Fail closed when that inventory or any
    // per-host item inventory is incomplete.
    let inventory = reader
        .search_hosts("", zdashstitch_zabbix::MAX_HOST_SEARCH)
        .await
        .map_err(|_| WorkcopyError::new("portable_pattern_review_invalid"))?;
    if inventory.truncated {
        return Err(WorkcopyError::new("clone_pattern_preview_truncated"));
    }
    let mut seen_hosts = BTreeSet::new();
    if inventory
        .objects
        .iter()
        .any(|host| !seen_hosts.insert(&host.hostid))
    {
        return Err(WorkcopyError::new("portable_pattern_review_invalid"));
    }
    let selected: BTreeSet<_> = targets
        .iter()
        .map(|slot| slot.host.hostid.as_str())
        .collect();
    if selected
        .iter()
        .any(|id| !inventory.objects.iter().any(|host| host.hostid == *id))
    {
        return Err(WorkcopyError::new("portable_pattern_review_changed"));
    }
    let mut visible_matches = Vec::new();
    for host in inventory.objects {
        match preview.requirement.kind {
            ClonePatternKind::Host => {
                if crate::resolver::name_pattern_matches(&preview.target_pattern, &host.name) {
                    visible_matches.push(VisiblePatternMatch {
                        selected: selected.contains(host.hostid.as_str()),
                        hostid: host.hostid,
                        host_name: host.name,
                        itemid: None,
                        item_name: None,
                    });
                }
            }
            ClonePatternKind::Item => {
                let items = reader
                    .host_items(&host.hostid)
                    .await
                    .map_err(|_| WorkcopyError::new("portable_pattern_review_invalid"))?;
                if items.truncated {
                    return Err(WorkcopyError::new("clone_pattern_preview_truncated"));
                }
                for item in items.objects {
                    if item.hostid != host.hostid {
                        return Err(WorkcopyError::new("portable_pattern_review_invalid"));
                    }
                    if crate::resolver::name_pattern_matches(&preview.target_pattern, &item.name) {
                        visible_matches.push(VisiblePatternMatch {
                            selected: selected.contains(host.hostid.as_str()),
                            hostid: host.hostid.clone(),
                            host_name: host.name.clone(),
                            itemid: Some(item.itemid),
                            item_name: Some(item.name),
                        });
                    }
                    if visible_matches.len() > 500 {
                        return Err(WorkcopyError::new("clone_pattern_preview_truncated"));
                    }
                }
            }
        }
        if visible_matches.len() > 500 {
            return Err(WorkcopyError::new("clone_pattern_preview_truncated"));
        }
    }
    visible_matches.sort_by(|a, b| {
        (&a.hostid, &a.itemid, &a.host_name, &a.item_name).cmp(&(
            &b.hostid,
            &b.itemid,
            &b.host_name,
            &b.item_name,
        ))
    });
    let selected_review: BTreeSet<_> = preview
        .matches
        .iter()
        .map(|item| (item.object_id.as_str(), item.name.as_str()))
        .collect();
    let selected_inventory: BTreeSet<_> = visible_matches
        .iter()
        .filter(|item| item.selected)
        .map(|item| match (&item.itemid, &item.item_name) {
            (Some(id), Some(name)) => (id.as_str(), name.as_str()),
            _ => (item.hostid.as_str(), item.host_name.as_str()),
        })
        .collect();
    if selected_review != selected_inventory {
        return Err(WorkcopyError::new("portable_pattern_review_changed"));
    }
    Ok(visible_matches)
}

/// Evaluate one stored pattern only on the explicitly supplied target slots.
/// A hidden host, an incomplete host inventory or a truncated result fails.
/// The return value carries no authority to publish.
pub async fn preview_portable_pattern<L: ClonePatternLookup>(
    lookup: &L,
    bundle: &TypedPortableBundle,
    page_index: usize,
    widget_index: usize,
    field_index: usize,
    targets: &[TargetSlot],
) -> Result<ClonePatternPreview, WorkcopyError> {
    let (requirement, _) =
        portable_pattern_input(bundle, page_index, widget_index, field_index, targets)?;
    let value = requirement.source_pattern.clone();
    let review = preview_clone_pattern(lookup, requirement, value, targets)
        .await
        .map_err(|error| WorkcopyError::new(error.code()))?;
    if review.truncated {
        return Err(WorkcopyError::new("clone_pattern_preview_truncated"));
    }
    Ok(review)
}

fn portable_pattern_input<'a>(
    bundle: &'a TypedPortableBundle,
    page_index: usize,
    widget_index: usize,
    field_index: usize,
    targets: &[TargetSlot],
) -> Result<(ClonePatternRequirement, &'a [ZabbixWidgetField]), WorkcopyError> {
    let invalid = || WorkcopyError::new("portable_pattern_review_invalid");
    bundle.validate().map_err(|_| invalid())?;
    let page = match &bundle.project.root {
        ProjectRoot::Page(page) if page_index == 0 => page,
        ProjectRoot::Dashboard(dashboard) => dashboard.pages.get(page_index).ok_or_else(invalid)?,
        _ => return Err(invalid()),
    };
    let Widget::Zabbix {
        widget_type,
        fields,
        ..
    } = page.widgets.get(widget_index).ok_or_else(invalid)?
    else {
        return Err(invalid());
    };
    let field = fields.get(field_index).ok_or_else(invalid)?;
    let ZabbixWidgetFieldValue::DynamicPattern { value, role } = &field.value else {
        return Err(invalid());
    };
    if widget_type != "svggraph" || !crate::compatibility::is_svggraph_pattern_field(&field.name) {
        return Err(invalid());
    }
    let role = bundle.project.roles.get(role).ok_or_else(invalid)?;
    if targets.is_empty()
        || targets.len() > 256
        || (role.cardinality == Cardinality::One && targets.len() != 1)
    {
        return Err(invalid());
    }
    let mut hostids = BTreeSet::new();
    let mut slotids = BTreeSet::new();
    for target in targets {
        if !hostids.insert(&target.host.hostid)
            || !slotids.insert(&target.slot_id.0)
            || role
                .slots
                .as_ref()
                .is_some_and(|slots| !slots.contains(&target.slot_id.0))
        {
            return Err(invalid());
        }
    }
    if role
        .slots
        .as_ref()
        .is_some_and(|slots| slots.len() != targets.len())
    {
        return Err(invalid());
    }
    let kind = if field.name.split('.').any(|part| part == "items") {
        ClonePatternKind::Item
    } else {
        ClonePatternKind::Host
    };
    let requirement = ClonePatternRequirement {
        field: FieldAddress {
            widget: WidgetAddress {
                page_index,
                dashboard_pageid: None,
                widget_index,
                widgetid: None,
            },
            field_name: field.name.clone(),
        },
        widget_type: widget_type.clone(),
        kind,
        source_pattern: value.clone(),
    };
    Ok((requirement, fields))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clone_mapping::TargetSlotId;
    use zdashstitch_zabbix::{HostDetails, HostObjects, ItemDetails, ItemFlags, ZabbixError};

    struct Fixture {
        truncated: bool,
        visible: bool,
    }
    impl ClonePatternLookup for Fixture {
        async fn hosts_by_ids(&self, ids: &[String]) -> Result<Vec<HostDetails>, ZabbixError> {
            Ok(if self.visible {
                ids.iter()
                    .map(|id| HostDetails {
                        hostid: id.clone(),
                        host: format!("db-{id}"),
                        name: format!("DB-{id}"),
                    })
                    .collect()
            } else {
                vec![]
            })
        }
        async fn host_items(&self, hostid: &str) -> Result<HostObjects<ItemDetails>, ZabbixError> {
            Ok(HostObjects {
                truncated: self.truncated,
                objects: vec![ItemDetails {
                    itemid: "20".into(),
                    hostid: hostid.into(),
                    key: "cpu".into(),
                    name: "CPU load".into(),
                    value_type: 0,
                    units: "".into(),
                    flags: ItemFlags::Plain,
                    enabled: None,
                    templateid: None,
                    discovery: None,
                    owner_name: None,
                }],
            })
        }
    }

    #[tokio::test]
    async fn target_review_reads_selected_hosts_and_rejects_incomplete_data() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap_or_else(|| panic!("root"));
        let bundle = zdashstitch_core::formats::load_typed_project_file(
            &root.join("fixtures/formats/v2/positive/dynamic-pattern.yaml"),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let slot = TargetSlot {
            slot_id: TargetSlotId("first".into()),
            host: HostDetails {
                hostid: "10".into(),
                host: "ignored".into(),
                name: "ignored".into(),
            },
        };
        let visible = Fixture {
            truncated: false,
            visible: true,
        };
        let host =
            preview_portable_pattern(&visible, &bundle, 0, 0, 0, std::slice::from_ref(&slot))
                .await
                .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(host.match_count, 1);
        let item =
            preview_portable_pattern(&visible, &bundle, 0, 0, 1, std::slice::from_ref(&slot))
                .await
                .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(item.match_count, 1);
        assert!(
            preview_portable_pattern(
                &Fixture {
                    truncated: true,
                    visible: true
                },
                &bundle,
                0,
                0,
                1,
                std::slice::from_ref(&slot)
            )
            .await
            .is_err()
        );
        assert!(
            preview_portable_pattern(
                &Fixture {
                    truncated: false,
                    visible: false
                },
                &bundle,
                0,
                0,
                0,
                std::slice::from_ref(&slot)
            )
            .await
            .is_err()
        );
        assert!(
            preview_portable_pattern(&visible, &bundle, 0, 0, 1, &[slot.clone(), slot])
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn multiple_hosts_require_distinct_slots_and_complete_roles() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap_or_else(|| panic!("root"));
        let mut bundle = zdashstitch_core::formats::load_typed_project_file(
            &root.join("fixtures/formats/v2/positive/dynamic-pattern.yaml"),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let host = |slot: &str, id: &str| TargetSlot {
            slot_id: TargetSlotId(slot.into()),
            host: HostDetails {
                hostid: id.into(),
                host: "untrusted".into(),
                name: "untrusted".into(),
            },
        };
        let fixture = Fixture {
            truncated: false,
            visible: true,
        };
        let targets = [host("first", "10"), host("second", "11")];
        let review = preview_portable_pattern(&fixture, &bundle, 0, 0, 0, &targets)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(review.match_count, 2);
        assert_eq!(review.checked_slots.len(), 2);
        assert!(
            preview_portable_pattern(
                &fixture,
                &bundle,
                0,
                0,
                0,
                &[host("first", "10"), host("first", "11")]
            )
            .await
            .is_err()
        );
        bundle
            .project
            .roles
            .get_mut("hosts")
            .unwrap_or_else(|| panic!("role"))
            .slots = Some(vec!["first".into(), "second".into()]);
        assert!(
            preview_portable_pattern(&fixture, &bundle, 0, 0, 0, &targets[..1])
                .await
                .is_err()
        );
        assert!(
            preview_portable_pattern(&fixture, &bundle, 0, 0, 0, &targets)
                .await
                .is_ok()
        );
    }
}
