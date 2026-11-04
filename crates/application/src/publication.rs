// SPDX-License-Identifier: MIT OR Apache-2.0

//! Publication screens of a ReplaceHost draft (P2-04E/F, P1-08D/E/G):
//!
//! - [`plan_replace_host_copy`]: «Сохранить копию» of a ReplaceHost draft —
//!   the Save as copy plan of P2-06 ([`crate::plan_save_as_copy`]) after
//!   the intent-level checks of the replacement (unresolved bindings and
//!   the unconfirmed «останутся на старом узле» list block it; the update
//!   contracts of the source dashboard do not apply to a new dashboard);
//! - [`remote_changes`]: the conflict comparison «На сервере: база →
//!   Zabbix сейчас» (P1-08D), by page and widget IDs of Base, with the
//!   widgets that overlap the local changes;
//! - [`refetch_replace_host`]: «Скопировать заново» (P2-04E): a new copy of
//!   the same source read now, with the replacement calculated again on the
//!   new base; this draft is never changed, decisions are made again by
//!   hand (no automatic merge, O-014).
//!
//! Every server access here is a read through a [`ReadOnlyGateway`]; the
//! only write path stays the closed publisher.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use zdashstitch_zabbix::{ReadOnlyGateway, ReadWriteGateway, VersionedDashboardSnapshot};

use crate::create_dashboard::{CreatePlanError, SaveAsCopyRequest, VerifiedCreatePlan};
use crate::publisher::{CompatibilityFinding, CompatibilityReport, FindingCode};
use crate::replace_host::{ReplaceHostPreview, ReplaceScope, SessionContext};
use crate::resolver::{ReplaceHostRequest, ResolveScope};
use crate::workcopy::{ImportOutcome, ImportRequest, ImportScope, WorkcopyError, WorkcopyService};
use crate::{LocalSession, PublishJournal, plan_save_as_copy};

/// Findings of the replacement itself that also block a copy: the new
/// dashboard would otherwise keep unresolved references or an unconfirmed
/// list of widgets on the old host.
const COPY_BLOCKING_CODES: [FindingCode; 9] = [
    FindingCode::TargetHostNotVisible,
    FindingCode::BindingUnresolved,
    FindingCode::SourceUnverified,
    FindingCode::DecisionInvalid,
    FindingCode::BindingStale,
    FindingCode::BindingInvalid,
    FindingCode::KeptOnSourceUnconfirmed,
    FindingCode::KeptOnSourceConfirmationStale,
    FindingCode::ScopeLinkCrossing,
];

/// The blocking findings of `preview` that a copy inherits.
#[must_use]
pub fn copy_blockers(preview: &ReplaceHostPreview) -> Vec<CompatibilityFinding> {
    preview
        .report
        .blockers()
        .filter(|finding| COPY_BLOCKING_CODES.contains(&finding.code))
        .cloned()
        .collect()
}

/// «Сохранить копию» of a ReplaceHost draft: the stored replacement must
/// have no blocker a copy inherits ([`copy_blockers`], offline preview of
/// the stored operations); then [`plan_save_as_copy`] verifies the create.
/// A draft without a replacement is copied as it is.
///
/// `request.base_fingerprint` is the canonical Base fingerprint the
/// publication screens show (`ReplaceHostPreview::base_fingerprint`); it
/// must equal the current Base and is then bound to the stored Base
/// reference [`plan_save_as_copy`] compares (the revision binds both).
pub async fn plan_replace_host_copy(
    service: &WorkcopyService,
    journal: &PublishJournal,
    session: &LocalSession,
    gateway: &ReadWriteGateway,
    request: &SaveAsCopyRequest,
) -> Result<VerifiedCreatePlan, CreatePlanError> {
    let (file, base) = service
        .load_with_base(&request.local_id)
        .map_err(CreatePlanError::Workcopy)?;
    if base.fingerprint() != request.base_fingerprint || file.revision != request.revision {
        return Err(CreatePlanError::Invalidated(
            crate::PlanInvalidReason::DraftChanged,
        ));
    }
    let request = SaveAsCopyRequest {
        base_fingerprint: file.base.fingerprint,
        ..request.clone()
    };
    match service.replace_host_preview(&request.local_id, SessionContext::Offline) {
        Ok(preview) => {
            let blockers = copy_blockers(&preview);
            if !blockers.is_empty() {
                return Err(CreatePlanError::Incompatible(CompatibilityReport {
                    checks: Vec::new(),
                    findings: blockers,
                }));
            }
        }
        Err(error) if error.code() == "replace_host_not_calculated" => {}
        Err(error) => return Err(CreatePlanError::Workcopy(error)),
    }
    plan_save_as_copy(service, journal, session, gateway, &request).await
}

// ---------------------------------------------------------------- remote diff

/// What changed on the server since the snapshot (P1-08D).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RemoteChangeKind {
    /// Name, default page period or slideshow start.
    DashboardProperties,
    /// Owner, sharing type or recipients.
    Access,
    PageAdded,
    PageRemoved,
    PageRenamed,
    /// Page period or order.
    PageChanged,
    WidgetAdded,
    WidgetRemoved,
    /// Type, name, fields or view mode.
    WidgetChanged,
    /// Only position or size.
    WidgetMoved,
}

impl RemoteChangeKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DashboardProperties => "dashboard_properties",
            Self::Access => "access",
            Self::PageAdded => "page_added",
            Self::PageRemoved => "page_removed",
            Self::PageRenamed => "page_renamed",
            Self::PageChanged => "page_changed",
            Self::WidgetAdded => "widget_added",
            Self::WidgetRemoved => "widget_removed",
            Self::WidgetChanged => "widget_changed",
            Self::WidgetMoved => "widget_moved",
        }
    }
}

/// One row of «На сервере: база → Zabbix сейчас». Names are server data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteChange {
    pub kind: RemoteChangeKind,
    /// Page name (Zabbix now, or Base when removed); `None` for dashboard
    /// rows.
    pub page_name: Option<String>,
    /// 1-based page position (Zabbix now, or Base when removed).
    pub page_number: Option<u32>,
    pub widget_name: Option<String>,
    pub widget_type: Option<String>,
    pub widgetid: Option<String>,
    /// The widget is also changed by this draft.
    pub overlaps: bool,
}

/// The comparison; `missing` when the dashboard is gone or not visible.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RemoteDiff {
    pub missing: bool,
    pub changes: Vec<RemoteChange>,
    /// Rows beyond [`MAX_REMOTE_CHANGES`] are counted, not listed.
    pub truncated: usize,
    pub overlapping_widgets: usize,
}

/// Upper bound of listed remote changes.
pub const MAX_REMOTE_CHANGES: usize = 500;

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

/// Widget fields as a sorted multiset, so a different field order is not
/// a change.
fn fields(widget: &Value) -> Vec<String> {
    let mut fields: Vec<String> = array(widget, "fields")
        .iter()
        .map(|field| {
            format!(
                "{}\u{0}{}\u{0}{}",
                scalar(field.get("type")),
                text(field, "name"),
                scalar(field.get("value"))
            )
        })
        .collect();
    fields.sort_unstable();
    fields
}

fn scalar(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn same(left: &Value, right: &Value, keys: &[&str]) -> bool {
    keys.iter()
        .all(|key| scalar(left.get(*key)) == scalar(right.get(*key)))
}

const GEOMETRY: [&str; 4] = ["x", "y", "width", "height"];
const WIDGET_CONTENT: [&str; 3] = ["type", "name", "view_mode"];

fn page_number(index: usize) -> Option<u32> {
    u32::try_from(index + 1).ok()
}

/// Compares Base with Remote by page and widget IDs (P1-08B: existing
/// dashboards are matched by the IDs of Base). `local_widgetids` are the
/// widgets this draft changes; a remote change of one of them overlaps.
#[must_use]
pub fn remote_changes(
    base: &VersionedDashboardSnapshot,
    remote: Option<&VersionedDashboardSnapshot>,
    local_widgetids: &BTreeSet<String>,
) -> RemoteDiff {
    let Some(remote) = remote else {
        return RemoteDiff {
            missing: true,
            ..RemoteDiff::default()
        };
    };
    let (base, remote) = (base.raw_api_document(), remote.raw_api_document());
    let mut changes = Vec::new();
    let dashboard_row = |kind| RemoteChange {
        kind,
        page_name: None,
        page_number: None,
        widget_name: None,
        widget_type: None,
        widgetid: None,
        overlaps: false,
    };
    if !same(base, remote, &["name", "display_period", "auto_start"]) {
        changes.push(dashboard_row(RemoteChangeKind::DashboardProperties));
    }
    if !same(base, remote, &["userid", "private"])
        || array(base, "users") != array(remote, "users")
        || array(base, "userGroups") != array(remote, "userGroups")
    {
        changes.push(dashboard_row(RemoteChangeKind::Access));
    }
    let pages = |raw: &Value| -> BTreeMap<String, (usize, Value)> {
        array(raw, "pages")
            .iter()
            .enumerate()
            .map(|(index, page)| {
                (
                    text(page, "dashboard_pageid").to_owned(),
                    (index, page.clone()),
                )
            })
            .collect()
    };
    let (base_pages, remote_pages) = (pages(base), pages(remote));
    let page_row = |kind, index: usize, page: &Value| RemoteChange {
        kind,
        page_name: Some(text(page, "name").to_owned()),
        page_number: page_number(index),
        widget_name: None,
        widget_type: None,
        widgetid: None,
        overlaps: false,
    };
    let widget_row = |kind, index: usize, page: &Value, widget: &Value| {
        let widgetid = text(widget, "widgetid").to_owned();
        RemoteChange {
            kind,
            page_name: Some(text(page, "name").to_owned()),
            page_number: page_number(index),
            widget_name: Some(text(widget, "name").to_owned()),
            widget_type: Some(text(widget, "type").to_owned()),
            overlaps: local_widgetids.contains(&widgetid),
            widgetid: Some(widgetid),
        }
    };
    let widgets = |page: &Value| -> BTreeMap<String, Value> {
        array(page, "widgets")
            .iter()
            .map(|widget| (text(widget, "widgetid").to_owned(), widget.clone()))
            .collect()
    };
    for (pageid, (index, page)) in &remote_pages {
        let Some((base_index, base_page)) = base_pages.get(pageid) else {
            changes.push(page_row(RemoteChangeKind::PageAdded, *index, page));
            for widget in array(page, "widgets") {
                changes.push(widget_row(
                    RemoteChangeKind::WidgetAdded,
                    *index,
                    page,
                    widget,
                ));
            }
            continue;
        };
        if text(page, "name") != text(base_page, "name") {
            changes.push(page_row(RemoteChangeKind::PageRenamed, *index, page));
        } else if index != base_index || !same(page, base_page, &["display_period"]) {
            changes.push(page_row(RemoteChangeKind::PageChanged, *index, page));
        }
        let (now, before) = (widgets(page), widgets(base_page));
        for (widgetid, widget) in &now {
            match before.get(widgetid) {
                None => changes.push(widget_row(
                    RemoteChangeKind::WidgetAdded,
                    *index,
                    page,
                    widget,
                )),
                Some(old)
                    if !same(widget, old, &WIDGET_CONTENT) || fields(widget) != fields(old) =>
                {
                    changes.push(widget_row(
                        RemoteChangeKind::WidgetChanged,
                        *index,
                        page,
                        widget,
                    ));
                }
                Some(old) if !same(widget, old, &GEOMETRY) => changes.push(widget_row(
                    RemoteChangeKind::WidgetMoved,
                    *index,
                    page,
                    widget,
                )),
                Some(_) => {}
            }
        }
        for (widgetid, widget) in &before {
            if !now.contains_key(widgetid) {
                changes.push(widget_row(
                    RemoteChangeKind::WidgetRemoved,
                    *index,
                    page,
                    widget,
                ));
            }
        }
    }
    for (pageid, (index, page)) in &base_pages {
        if !remote_pages.contains_key(pageid) {
            changes.push(page_row(RemoteChangeKind::PageRemoved, *index, page));
            for widget in array(page, "widgets") {
                changes.push(widget_row(
                    RemoteChangeKind::WidgetRemoved,
                    *index,
                    page,
                    widget,
                ));
            }
        }
    }
    changes.sort_by(|left, right| {
        (left.page_number, left.kind, &left.widgetid).cmp(&(
            right.page_number,
            right.kind,
            &right.widgetid,
        ))
    });
    let overlapping_widgets = changes
        .iter()
        .filter(|change| change.overlaps)
        .filter_map(|change| change.widgetid.as_deref())
        .collect::<BTreeSet<_>>()
        .len();
    let truncated = changes.len().saturating_sub(MAX_REMOTE_CHANGES);
    changes.truncate(MAX_REMOTE_CHANGES);
    RemoteDiff {
        missing: false,
        changes,
        truncated,
        overlapping_widgets,
    }
}

/// The widgets a ReplaceHost preview changes, by `widgetid`.
#[must_use]
pub fn changed_widgetids(preview: &ReplaceHostPreview) -> BTreeSet<String> {
    preview
        .changes
        .iter()
        .filter_map(|change| change.widgetid.clone())
        .collect()
}

/// Reads Remote of the draft's source dashboard and compares it with Base
/// ([`remote_changes`]). Read-only.
pub async fn read_remote_changes(
    service: &WorkcopyService,
    reader: &ReadOnlyGateway,
    local_id: &str,
    local_widgetids: &BTreeSet<String>,
) -> Result<RemoteDiff, WorkcopyError> {
    let base = service.base_snapshot(local_id)?;
    let remote = reader
        .dashboard_snapshot(base.connection_id(), base.dashboard_id())
        .await
        .map_err(|error| WorkcopyError::from_zabbix(&error))?;
    Ok(remote_changes(&base, remote.as_ref(), local_widgetids))
}

// ---------------------------------------------------------------- refetch

/// Result of «Скопировать заново».
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefetchOutcome {
    /// The new copy; the old draft is unchanged.
    pub import: ImportOutcome,
    /// The replacement was calculated again on the new base.
    pub replayed: bool,
    /// Why it was not (a workcopy or resolver code), when there was one to
    /// replay.
    pub replay_code: Option<&'static str>,
}

fn resolve_scope(scope: &ReplaceScope) -> ResolveScope {
    match scope {
        ReplaceScope::Dashboard => ResolveScope::Dashboard,
        ReplaceScope::Pages { dashboard_pageids } => {
            ResolveScope::Pages(dashboard_pageids.iter().cloned().collect())
        }
        ReplaceScope::Widgets { widgetids } => {
            ResolveScope::Widgets(widgetids.iter().cloned().collect())
        }
    }
}

impl WorkcopyService {
    /// «Скопировать заново» (P2-04E, P1-08D): a new copy of the same source
    /// (dashboard or page) read now through `reader`, named `name` (the UI
    /// picks «… (копия N)», P2-08A), and the ReplaceHost of `local_id`
    /// (source host, target host, scope) calculated again on the new base.
    /// Decisions and the confirmation of the kept list are not carried
    /// over: they belong to the old base. `local_id` is never changed.
    ///
    /// Errors: those of [`Self::import`] and `workcopy_other_connection`.
    /// A failed recalculation keeps the new copy and is reported in
    /// [`RefetchOutcome::replay_code`].
    pub async fn refetch_replace_host(
        &self,
        reader: &ReadOnlyGateway,
        connection_id: &str,
        local_id: &str,
        name: Option<String>,
        session: SessionContext<'_>,
    ) -> Result<RefetchOutcome, WorkcopyError> {
        let file = self.load(local_id)?;
        if file.source.connection_id != connection_id {
            return Err(WorkcopyError::new("workcopy_other_connection"));
        }
        let replay = match self.replace_host_preview(local_id, SessionContext::Offline) {
            Ok(preview) => Some(preview.operation),
            Err(error) if error.code() == "replace_host_not_calculated" => None,
            Err(error) => return Err(error),
        };
        let scope = match &file.source.page {
            Some(page) => ImportScope::Page {
                dashboard_pageid: page.dashboard_pageid.clone(),
            },
            None => ImportScope::Dashboard,
        };
        let import = self
            .import(
                reader,
                connection_id,
                &ImportRequest {
                    dashboardid: file.source.dashboardid.clone(),
                    scope,
                    name,
                },
            )
            .await?;
        let Some(operation) = replay else {
            return Ok(RefetchOutcome {
                import,
                replayed: false,
                replay_code: None,
            });
        };
        let Some(target) = operation.target_host else {
            return Ok(RefetchOutcome {
                import,
                replayed: false,
                replay_code: Some("target_host_not_visible"),
            });
        };
        let request = ReplaceHostRequest {
            source_hostid: operation.source_host.hostid,
            target_hostid: target.hostid,
            scope: resolve_scope(&operation.scope),
        };
        let replayed = self
            .calculate_replace_host(reader, &import.local_id, &request, session)
            .await;
        Ok(match replayed {
            Ok(_) => RefetchOutcome {
                import,
                replayed: true,
                replay_code: None,
            },
            Err(error) => RefetchOutcome {
                import,
                replayed: false,
                replay_code: Some(error.code()),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use zdashstitch_zabbix::ZabbixVersion;

    use super::*;

    fn snapshot(raw: Value) -> VersionedDashboardSnapshot {
        let version: ZabbixVersion = "7.4.16"
            .parse()
            .unwrap_or_else(|error| panic!("version: {error}"));
        VersionedDashboardSnapshot::from_adapter_raw("conn".to_owned(), version, raw)
            .unwrap_or_else(|error| panic!("snapshot: {error}"))
    }

    fn widget(widgetid: &str, name: &str, x: &str, value: &str) -> Value {
        json!({
            "widgetid": widgetid, "type": "item", "name": name,
            "x": x, "y": "0", "width": "12", "height": "3", "view_mode": "0",
            "fields": [{"type": "4", "name": "itemid.0", "value": value}]
        })
    }

    fn dashboard(pages: Value) -> Value {
        json!({
            "dashboardid": "7", "name": "Synthetic", "userid": "1", "private": "1",
            "display_period": "30", "auto_start": "1", "users": [], "userGroups": [],
            "pages": pages
        })
    }

    #[test]
    fn remote_changes_are_matched_by_ids_and_overlap_local_changes() {
        let base = snapshot(dashboard(json!([
            {"dashboard_pageid": "11", "name": "One", "display_period": "0",
             "widgets": [widget("101", "A", "0", "1"), widget("102", "B", "12", "2"),
                         widget("103", "C", "24", "3")]},
            {"dashboard_pageid": "12", "name": "Two", "display_period": "0", "widgets": []}
        ])));
        let remote = snapshot(dashboard(json!([
            {"dashboard_pageid": "11", "name": "One", "display_period": "0",
             // 102 changed, 103 moved, 101 untouched (order differs), 104 added.
             "widgets": [widget("103", "C", "36", "3"), widget("102", "B", "12", "9"),
                         widget("101", "A", "0", "1"), widget("104", "D", "48", "4")]},
            {"dashboard_pageid": "12", "name": "Renamed", "display_period": "0", "widgets": []}
        ])));
        let local: BTreeSet<String> = ["102".to_owned(), "101".to_owned()].into();
        let diff = remote_changes(&base, Some(&remote), &local);
        let kinds: Vec<(&str, Option<&str>, bool)> = diff
            .changes
            .iter()
            .map(|change| {
                (
                    change.kind.as_str(),
                    change.widgetid.as_deref(),
                    change.overlaps,
                )
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("widget_added", Some("104"), false),
                ("widget_changed", Some("102"), true),
                ("widget_moved", Some("103"), false),
                ("page_renamed", None, false),
            ]
        );
        assert_eq!(diff.overlapping_widgets, 1);
        assert!(!diff.missing);
        assert!(remote_changes(&base, None, &local).missing);
        assert!(
            remote_changes(&base, Some(&base), &local)
                .changes
                .is_empty()
        );
    }
}
