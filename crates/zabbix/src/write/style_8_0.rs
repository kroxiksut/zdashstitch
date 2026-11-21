// SPDX-License-Identifier: MIT OR Apache-2.0

//! Visual field amendments on the private, single-page laboratory shapes.

use std::collections::BTreeSet;

use crate::capabilities::is_eight_zero_item_update_version;
use crate::{VersionedDashboardSnapshot, is_zabbix_id};

use super::item_update_8_0::{graph_row, is_page, is_widget, rows};
use super::{WidgetFieldReplacement, value_as_i64};

pub(super) fn color(value: &str) -> bool {
    value.len() == 6
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
}

pub(super) fn contract_shape(
    snapshot: &VersionedDashboardSnapshot,
    replacements: &[WidgetFieldReplacement],
) -> bool {
    let raw = snapshot.raw_api_document();
    let [page] = rows(&raw["pages"]) else {
        return false;
    };
    if replacements.is_empty()
        || !is_eight_zero_item_update_version(snapshot.server_version())
        || !is_zabbix_id(snapshot.dashboard_id())
        || value_as_i64(&raw["private"]) != Some(1)
        || !raw["users"].as_array().is_some_and(Vec::is_empty)
        || !raw["userGroups"].as_array().is_some_and(Vec::is_empty)
        || !is_page(page)
    {
        return false;
    }
    let widgets = rows(&page["widgets"]);
    let Some(widget) = widgets
        .iter()
        .find(|widget| widget["widgetid"] == replacements[0].widgetid)
    else {
        return false;
    };
    let kind = widget["type"].as_str().unwrap_or_default();
    if !is_widget(widget, kind)
        || value_as_i64(&widget["view_mode"]) != Some(0)
        || widgets.len() > 2
        || widgets
            .iter()
            .filter(|other| other["widgetid"] == widget["widgetid"])
            .count()
            != 1
        || widgets
            .iter()
            .filter(|other| other["widgetid"] != widget["widgetid"])
            .any(|other| {
                !is_widget(other, "clock")
                    || !matches!(rows(&other["fields"]).len(), 0 | 1)
                    || (rows(&other["fields"]).len() == 1
                        && graph_row(other, "clock_type", 0) != Some("1"))
            })
    {
        return false;
    }
    let fields = rows(&widget["fields"]);
    let count = match kind {
        "item"
            if fields.len() == 3
                && graph_row(widget, "itemid.0", 4).is_some_and(is_zabbix_id)
                && graph_row(widget, "show.0", 0) == Some("2") =>
        {
            1
        }
        "svggraph"
            if matches!(fields.len(), 3 | 5 | 7)
                && graph_row(widget, "ds.0.dataset_type", 0) == Some("0") =>
        {
            let count = (fields.len() - 1) / 2;
            let mut items = BTreeSet::new();
            for slot in 0..count {
                if !graph_row(widget, &format!("ds.0.itemids.{slot}"), 4)
                    .is_some_and(|id| is_zabbix_id(id) && items.insert(id))
                    || !graph_row(widget, &format!("ds.0.color.{slot}"), 1).is_some_and(color)
                {
                    return false;
                }
            }
            count
        }
        _ => return false,
    };
    replacements.len() <= count
        && replacements.iter().all(|replacement| {
            replacement.page_index == 0
                && widget["widgetid"] == replacement.widgetid
                && match kind {
                    "item" => replacement.field_name == "bg_color",
                    _ => (0..count)
                        .any(|slot| replacement.field_name == format!("ds.0.color.{slot}")),
                }
                && graph_row(widget, &replacement.field_name, 1)
                    == Some(&replacement.expected_value)
        })
}
