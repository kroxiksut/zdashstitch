// SPDX-License-Identifier: MIT OR Apache-2.0

//! Scoped, offline style changes to a workcopy. The raw source and its
//! server identities are only used inside application storage.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zdashstitch_zabbix::{
    VersionedDashboardSnapshot, WidgetFieldReplacement, is_zabbix_id, write_requirements,
};

use super::{
    WorkcopyError, WorkcopyService, is_fingerprint, now_ms,
    operations::{self, Operation},
};
use crate::reusable_library::{ReusableDocument, ReusableLibrary};
use zdashstitch_core::formats::ReusableWidgetType;

pub(crate) const APPLY_STYLE: &str = "apply_style_profile";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StyleEntry {
    pub seq: u64,
    pub op: String,
    pub op_version: u32,
    pub recorded_at_ms: u64,
    pub base_fingerprint: String,
    pub profile_id: String,
    pub profile_version: u32,
    pub profile_name_at_apply: String,
    pub fields: Vec<CopyStyleChange>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CopyStyleChange {
    pub page: u32,
    pub widget: u32,
    pub field: String,
    pub before: String,
    pub after: String,
}

impl StyleEntry {
    pub fn well_formed(&self, base_fingerprint: &str) -> bool {
        let mut keys = BTreeSet::new();
        is_fingerprint(&self.base_fingerprint)
            && self.base_fingerprint == base_fingerprint
            && self.profile_version > 0
            && self.profile_id.len() == 36
            && (1..=120).contains(&self.profile_name_at_apply.chars().count())
            && !self.fields.is_empty()
            && self.fields.len() <= 50 * 256 * 3
            && self.fields.iter().all(|change| {
                keys.insert((change.page, change.widget, change.field.as_str()))
                    && match change.field.as_str() {
                        "ds.0.color.0" | "ds.0.color.1" | "ds.0.color.2" => {
                            uppercase_color(&change.before) && uppercase_color(&change.after)
                        }
                        "bg_color" => item_color(&change.before) && item_color(&change.after),
                        _ => false,
                    }
                    && change.before != change.after
            })
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopyStyleRequest {
    pub expected_revision: u64,
    pub profile_id: String,
    pub profile_revision: u64,
    pub page: u32,
    #[serde(default)]
    pub widgets: Option<Vec<u32>>,
}

#[derive(Debug, Serialize)]
pub struct CopyStylePreview {
    pub revision: u64,
    pub changes: Vec<CopyStyleChange>,
    pub skipped: u32,
}

#[derive(Debug, Serialize)]
pub struct CopyStyleState {
    pub revision: u64,
    pub has_style: bool,
    pub undo: bool,
    pub publication: Option<CopyStylePublication>,
}

#[derive(Debug, Serialize)]
pub struct CopyStylePublication {
    pub base_fingerprint: String,
    pub local_fingerprint: String,
    pub changes: Vec<CopyStyleChange>,
}

pub(crate) fn style_replacements(
    base: &VersionedDashboardSnapshot,
    operations: &[Operation],
) -> Result<Vec<WidgetFieldReplacement>, WorkcopyError> {
    effective_style_changes(operations)?
        .into_iter()
        .map(|change| {
            let page_index = usize::try_from(change.page).map_err(|_| corrupt())?;
            let widget_index = usize::try_from(change.widget).map_err(|_| corrupt())?;
            let widgetid =
                base.raw_api_document()["pages"][page_index]["widgets"][widget_index]["widgetid"]
                    .as_str()
                    .filter(|value| is_zabbix_id(value))
                    .ok_or_else(corrupt)?;
            Ok(WidgetFieldReplacement {
                page_index,
                widgetid: widgetid.to_owned(),
                field_name: change.field,
                expected_value: change.before,
                replacement_value: change.after,
            })
        })
        .collect()
}

fn effective_style_changes(
    operations: &[Operation],
) -> Result<Vec<CopyStyleChange>, WorkcopyError> {
    let mut fields: BTreeMap<_, CopyStyleChange> = BTreeMap::new();
    for operation in operations {
        let Operation::Style(entry) = operation else {
            return Err(WorkcopyError::new("reusable_workcopy_publish_unsupported"));
        };
        for change in &entry.fields {
            fields
                .entry((change.page, change.widget, change.field.as_str()))
                .and_modify(|value| value.after.clone_from(&change.after))
                .or_insert_with(|| change.clone());
        }
    }
    Ok(fields
        .into_values()
        .filter(|value| value.before != value.after)
        .collect())
}

impl WorkcopyService {
    pub fn workcopy_style_state(&self, local_id: &str) -> Result<CopyStyleState, WorkcopyError> {
        let (file, base, log) = self.load_log(local_id)?;
        let has_style = log.iter().any(|op| matches!(op, Operation::Style(_)));
        let publication = if has_style
            && base.server_version().to_string() == "8.0.0"
            && log.iter().all(|op| matches!(op, Operation::Style(_)))
            && style_replacements(&base, &log)
                .is_ok_and(|changes| write_requirements(&base, &changes).is_ok())
        {
            let local = overlay_style(base.clone(), &log)?;
            let changes = effective_style_changes(&log)?;
            (!changes.is_empty()).then(|| CopyStylePublication {
                base_fingerprint: base.fingerprint().to_owned(),
                local_fingerprint: local.fingerprint().to_owned(),
                changes,
            })
        } else {
            None
        };
        Ok(CopyStyleState {
            revision: file.revision,
            has_style,
            undo: matches!(log.last(), Some(Operation::Style(_))),
            publication,
        })
    }

    pub fn undo_workcopy_style(
        &self,
        local_id: &str,
        revision: u64,
    ) -> Result<CopyStyleState, WorkcopyError> {
        let (_, _, mut log) = self.load_log(local_id)?;
        if !matches!(log.pop(), Some(Operation::Style(_))) {
            return Err(WorkcopyError::new("reusable_history_unavailable"));
        }
        self.commit_operations(local_id, revision, &log)?;
        self.workcopy_style_state(local_id)
    }

    pub fn workcopy_style(
        &self,
        local_id: &str,
        request: &CopyStyleRequest,
        apply: bool,
    ) -> Result<CopyStylePreview, WorkcopyError> {
        let (file, base, mut log) = self.load_log(local_id)?;
        if file.revision != request.expected_revision {
            return Err(WorkcopyError::new("workcopy_revision_conflict"));
        }
        let local = crate::replace_host::materialize_existing_widgets(base, &log)?;
        let version = local.server_version().to_string();
        let family = match version.as_str() {
            "7.4.16" => "7.4",
            "8.0.0" => "8.0",
            _ => return Err(unsupported()),
        };
        let profile = ReusableLibrary::new(self.store.root_path()).get(&request.profile_id)?;
        let ReusableDocument::StyleProfile(style) = &profile.asset else {
            return Err(unsupported());
        };
        if profile.revision != request.profile_revision {
            return Err(WorkcopyError::new("reusable_conflict"));
        }
        if style.adapter != family
            || style.declared_compatibility.as_ref().is_some_and(|range| {
                version
                    .parse()
                    .map_or(true, |version| !range.contains(&version))
            })
        {
            return Err(unsupported());
        }
        let palette = style
            .widget_types
            .get(&ReusableWidgetType::Svggraph)
            .and_then(|section| section.series_palette.as_ref());
        let background = style
            .widget_types
            .get(&ReusableWidgetType::Item)
            .and_then(|section| section.widget_fields.get("bg_color"))
            .and_then(Value::as_str)
            .filter(|color| family == "8.0" && item_color(color));
        if palette.is_none() && background.is_none() {
            return Err(unsupported());
        }
        let raw_pages = local
            .raw_api_document()
            .get("pages")
            .and_then(Value::as_array)
            .ok_or_else(corrupt)?;
        let page_index = if let Some(selector) = &file.source.page {
            if request.page != 0 {
                return Err(selection());
            }
            raw_pages
                .iter()
                .position(|page| {
                    page["dashboard_pageid"].as_str() == Some(selector.dashboard_pageid.as_str())
                })
                .ok_or_else(corrupt)?
        } else {
            usize::try_from(request.page).map_err(|_| selection())?
        };
        let page = raw_pages.get(page_index).ok_or_else(selection)?;
        let widgets = page
            .get("widgets")
            .and_then(Value::as_array)
            .ok_or_else(corrupt)?;
        let selected: BTreeSet<u32> = request.widgets.as_ref().map_or_else(
            || (0..u32::try_from(widgets.len()).unwrap_or_default()).collect(),
            |widgets| widgets.iter().copied().collect(),
        );
        if request
            .widgets
            .as_ref()
            .is_some_and(|ids| ids.len() != selected.len())
        {
            return Err(selection());
        }
        let mut skipped = 0;
        let mut changes = Vec::new();
        for index in selected {
            let widget = widgets
                .get(usize::try_from(index).map_err(|_| selection())?)
                .ok_or_else(selection)?;
            if let (Some(palette), Some(colors)) = (
                palette,
                graph_colors(widget).filter(|colors| family == "8.0" || colors.len() > 1),
            ) {
                for (position, (field, before)) in colors.into_iter().enumerate() {
                    let new = &palette[position % palette.len()];
                    if new != before {
                        changes.push(CopyStyleChange {
                            page: u32::try_from(page_index).map_err(|_| corrupt())?,
                            widget: index,
                            field: field.to_owned(),
                            before: before.to_owned(),
                            after: new.clone(),
                        });
                    }
                }
            } else if let (Some(after), Some(before)) = (background, item_background(widget)) {
                if after != before {
                    changes.push(CopyStyleChange {
                        page: u32::try_from(page_index).map_err(|_| corrupt())?,
                        widget: index,
                        field: "bg_color".to_owned(),
                        before: before.to_owned(),
                        after: after.to_owned(),
                    });
                }
            } else {
                skipped += 1;
            }
        }
        if !apply || changes.is_empty() {
            return Ok(CopyStylePreview {
                revision: file.revision,
                changes,
                skipped,
            });
        }
        let seq = log
            .last()
            .map_or(0, Operation::seq)
            .checked_add(1)
            .ok_or_else(corrupt)?;
        let recorded_at_ms = now_ms().max(file.updated_at_ms);
        log.push(Operation::Style(StyleEntry {
            seq,
            op: APPLY_STYLE.to_owned(),
            op_version: operations::OP_VERSION_1,
            recorded_at_ms,
            base_fingerprint: file.base.fingerprint.clone(),
            profile_id: style.profile_id.clone(),
            profile_version: style.version,
            profile_name_at_apply: style.name.clone(),
            fields: changes.clone(),
        }));
        let file = self.commit_operations(local_id, request.expected_revision, &log)?;
        Ok(CopyStylePreview {
            revision: file.revision,
            changes,
            skipped,
        })
    }
}

pub(crate) fn overlay_style(
    source: VersionedDashboardSnapshot,
    operations: &[Operation],
) -> Result<VersionedDashboardSnapshot, WorkcopyError> {
    if !operations
        .iter()
        .any(|operation| matches!(operation, Operation::Style(_)))
    {
        return Ok(source);
    }
    let mut raw = source.raw_api_document().clone();
    for entry in operations.iter().filter_map(|operation| match operation {
        Operation::Style(entry) => Some(entry),
        _ => None,
    }) {
        for change in &entry.fields {
            let widget = raw
                .get_mut("pages")
                .and_then(Value::as_array_mut)
                .and_then(|pages| pages.get_mut(usize::try_from(change.page).ok()?))
                .and_then(|page| page.get_mut("widgets"))
                .and_then(Value::as_array_mut)
                .and_then(|widgets| widgets.get_mut(usize::try_from(change.widget).ok()?))
                .ok_or_else(corrupt)?;
            let accepted = if change.field == "bg_color" {
                source.server_version().to_string() == "8.0.0" && item_background(widget).is_some()
            } else {
                graph_colors(widget)
                    .is_some_and(|colors| source.server_version().major == 8 || colors.len() > 1)
            };
            if !accepted {
                return Err(corrupt());
            }
            let field = widget
                .get_mut("fields")
                .and_then(Value::as_array_mut)
                .and_then(|fields| {
                    fields
                        .iter_mut()
                        .find(|field| field["name"].as_str() == Some(change.field.as_str()))
                })
                .ok_or_else(corrupt)?;
            if field["value"].as_str() != Some(change.before.as_str()) {
                return Err(corrupt());
            }
            field["value"] = Value::String(change.after.clone());
        }
    }
    VersionedDashboardSnapshot::from_adapter_raw(
        source.connection_id().to_owned(),
        source.server_version().clone(),
        raw,
    )
    .map_err(|_| corrupt())
}

/// An explicit dataset with up to three numeric item rows and an existing
/// uppercase hex color per source. Callers gate row counts by version.
/// A style operation never adds a field or changes another part of the Graph.
fn graph_colors(widget: &Value) -> Option<Vec<(&str, &str)>> {
    if widget["type"].as_str() != Some("svggraph") {
        return None;
    }
    let fields = widget.get("fields")?.as_array()?;
    let count = match fields.len() {
        3 => 1,
        5 => 2,
        7 => 3,
        _ => return None,
    };
    let find = |name: &str, kind: &str| {
        fields.iter().find(|field| {
            field["name"].as_str() == Some(name)
                && (field["type"].as_str() == Some(kind)
                    || field["type"]
                        .as_u64()
                        .is_some_and(|n| n.to_string() == kind))
        })
    };
    let dataset = &find("ds.0.dataset_type", "0")?["value"];
    if dataset.as_str() != Some("0") && dataset.as_i64() != Some(0) {
        return None;
    }
    let mut colors = Vec::with_capacity(count);
    for slot in 0..count {
        let item = find(&format!("ds.0.itemids.{slot}"), "4")?["value"].as_str()?;
        if !is_zabbix_id(item) {
            return None;
        }
        let field = find(&format!("ds.0.color.{slot}"), "1")?;
        let color = field["value"].as_str()?;
        if !uppercase_color(color) {
            return None;
        }
        colors.push((field["name"].as_str()?, color));
    }
    Some(colors)
}

fn item_background(widget: &Value) -> Option<&str> {
    if widget["type"] != "item" || !api_integer(&widget["view_mode"], 0) {
        return None;
    }
    let fields = widget["fields"].as_array()?;
    if fields.len() != 3 {
        return None;
    }
    let field = |name, kind| {
        fields
            .iter()
            .find(|field| field["name"] == name && api_integer(&field["type"], kind))
            .map(|field| &field["value"])
    };
    if !field("itemid.0", 4)?.as_str().is_some_and(is_zabbix_id)
        || !api_integer(field("show.0", 0)?, 2)
    {
        return None;
    }
    field("bg_color", 1)?
        .as_str()
        .filter(|color| item_color(color))
}

fn api_integer(value: &Value, expected: i64) -> bool {
    value.as_i64() == Some(expected)
        || value.as_str().and_then(|value| value.parse().ok()) == Some(expected)
}

fn item_color(value: &str) -> bool {
    value.is_empty() || uppercase_color(value)
}

fn uppercase_color(value: &str) -> bool {
    value.len() == 6
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
}
fn corrupt() -> WorkcopyError {
    WorkcopyError::new("workcopy_corrupt")
}
fn selection() -> WorkcopyError {
    WorkcopyError::new("reusable_selection_invalid")
}
fn unsupported() -> WorkcopyError {
    WorkcopyError::new("reusable_adapter_unsupported")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn overlay_is_bound_to_existing_graph_color_and_keeps_other_fields() {
        let raw = json!({
            "dashboardid":"5000", "name":"Synthetic style fixture",
            "pages":[{"dashboard_pageid":"5001", "widgets":[
                {"type":"svggraph","widgetid":"5002","x":"3","y":"4","width":"25","height":"6","fields":[
                    {"type":"0","name":"ds.0.dataset_type","value":"0"},
                    {"type":"4","name":"ds.0.itemids.0","value":"6000"},
                    {"type":"1","name":"ds.0.color.0","value":"123ABC"},
                    {"type":"4","name":"ds.0.itemids.1","value":"6001"},
                    {"type":"1","name":"ds.0.color.1","value":"321CBA"}
                ]},
                {"type":"opaque","raw":"kept"}
            ]}]
        });
        let version = "8.0.0".parse().unwrap_or_else(|error| panic!("{error}"));
        let base = VersionedDashboardSnapshot::from_adapter_raw(
            "conn-synthetic".to_owned(),
            version,
            raw.clone(),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let mut entry = StyleEntry {
            seq: 1,
            op: APPLY_STYLE.to_owned(),
            op_version: 1,
            recorded_at_ms: 1,
            base_fingerprint: base.storage_fingerprint().to_owned(),
            profile_id: "45d10c01-1a20-47f7-98f2-4bdad3475de3".to_owned(),
            profile_version: 3,
            profile_name_at_apply: "Synthetic palette".to_owned(),
            fields: vec![CopyStyleChange {
                page: 0,
                widget: 0,
                field: "ds.0.color.1".to_owned(),
                before: "321CBA".to_owned(),
                after: "FFFFFF".to_owned(),
            }],
        };
        assert!(entry.well_formed(base.storage_fingerprint()));
        assert!(!entry.well_formed(&"a".repeat(64)));
        let local = overlay_style(base.clone(), &[Operation::Style(entry.clone())])
            .unwrap_or_else(|error| panic!("{error}"));
        let mut expected = raw.clone();
        expected["pages"][0]["widgets"][0]["fields"][4]["value"] = json!("FFFFFF");
        assert_eq!(local.raw_api_document(), &expected);
        assert_eq!(base.raw_api_document(), &raw);

        entry.fields[0].before = "FFFFFF".to_owned();
        assert!(overlay_style(base, &[Operation::Style(entry)]).is_err());
    }
}
