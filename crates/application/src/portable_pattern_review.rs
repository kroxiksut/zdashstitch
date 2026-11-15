// SPDX-License-Identifier: MIT OR Apache-2.0

//! Read-only target review of portable dynamic patterns. A review is not a
//! publication plan and must be repeated and bound to the future P5 plan.

use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use zdashstitch_core::formats::{
    Cardinality, ProjectRoot, TypedPortableBundle, Widget, ZabbixWidgetFieldValue,
};

use crate::{
    clone_mapping::{
        ClonePatternKind, ClonePatternLookup, ClonePatternPreview, ClonePatternRequirement,
        TargetSlot, preview_clone_pattern,
    },
    resolver::{FieldAddress, WidgetAddress},
    workcopy::{WorkcopyError, WorkcopyService},
};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortablePatternReviewRequest {
    pub revision: u64,
    pub page_index: usize,
    pub widget_index: usize,
    pub field_index: usize,
    pub targets: Vec<TargetSlot>,
}

#[derive(serde::Serialize)]
pub struct PortablePatternReviewResponse {
    pub preview: ClonePatternPreview,
    /// A review fingerprint is not a plan or permission to publish.
    pub fingerprint: String,
}

impl WorkcopyService {
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
        let preview = preview_portable_pattern(
            reader,
            &bundle,
            request.page_index,
            request.widget_index,
            request.field_index,
            &request.targets,
        )
        .await?;
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
        ))
        .map_err(|_| WorkcopyError::new("portable_pattern_review_invalid"))?;
        Ok(PortablePatternReviewResponse {
            preview,
            fingerprint: format!("{:x}", Sha256::digest(encoded)),
        })
    }
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
    for target in targets {
        if !hostids.insert(&target.host.hostid)
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
    let review = preview_clone_pattern(lookup, requirement, value.clone(), targets)
        .await
        .map_err(|error| WorkcopyError::new(error.code()))?;
    if review.truncated {
        return Err(WorkcopyError::new("clone_pattern_preview_truncated"));
    }
    Ok(review)
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
        async fn hosts_by_ids(&self, _: &[String]) -> Result<Vec<HostDetails>, ZabbixError> {
            Ok(if self.visible {
                vec![HostDetails {
                    hostid: "10".into(),
                    host: "db-01".into(),
                    name: "DB-01".into(),
                }]
            } else {
                vec![]
            })
        }
        async fn host_items(&self, _: &str) -> Result<HostObjects<ItemDetails>, ZabbixError> {
            Ok(HostObjects {
                truncated: self.truncated,
                objects: vec![ItemDetails {
                    itemid: "20".into(),
                    hostid: "10".into(),
                    key: "cpu".into(),
                    name: "CPU load".into(),
                    value_type: 0,
                    units: "".into(),
                    flags: ItemFlags::Plain,
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
}
