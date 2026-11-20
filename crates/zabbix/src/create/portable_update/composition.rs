// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use super::*;

pub(super) fn normalize_ids(pages: &mut [Value], links: &[NewGraphIds]) -> Option<()> {
    if pages.len() != links.len() {
        return None;
    }
    for (page, link) in pages.iter_mut().zip(links) {
        if link.new_page {
            page.as_object_mut()?.remove("dashboard_pageid")?;
        }
        let widgets = page["widgets"].as_array_mut()?;
        let first_new = widgets.len().checked_sub(link.new_widgets)?;
        if link.new_page && first_new != 0 {
            return None;
        }
        for widget in &mut widgets[first_new..] {
            widget.as_object_mut()?.remove("widgetid")?;
            widget["fields"]
                .as_array_mut()?
                .sort_by_cached_key(Value::to_string);
        }
    }
    Some(())
}

pub(super) fn make_change(
    target: &VersionedDashboardSnapshot,
    graph: &VerifiedDashboardCreate,
    links: Vec<LinkedGraphCompositionPage>,
) -> Option<(Value, Value, PortableUpdateReadback)> {
    let before = target.raw_api_document();
    if !graph_8_0::graph_readback_supported(before) {
        return None;
    }
    let desired = graph.create_params["pages"].as_array()?;
    if desired.len() != links.len() {
        return None;
    }
    let old_pages = by_id(before["pages"].as_array()?, "dashboard_pageid")?;
    let mut new_pages = placeholder_ids(old_pages.keys().copied(), desired.len())?.into_iter();
    let old_widget_ids = old_pages.values().flat_map(|page| {
        page["widgets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|widget| widget["widgetid"].as_str())
    });
    let new_widget_count = links
        .iter()
        .flat_map(|link| &link.widgetids)
        .filter(|id| id.is_none())
        .count();
    let mut new_widgets = placeholder_ids(old_widget_ids, new_widget_count)?.into_iter();
    let mut used_pages = BTreeSet::new();
    let mut wire_pages = Vec::with_capacity(links.len());
    let mut expected_pages = Vec::with_capacity(links.len());
    let mut ids = Vec::with_capacity(links.len());
    for (desired, link) in desired.iter().zip(links) {
        let requested = desired["widgets"].as_array()?;
        if requested.len() != link.widgetids.len() {
            return None;
        }
        let mut expected;
        let mut wire;
        let new = link.pageid.is_none();
        let allocated;
        if let Some(pageid) = link.pageid {
            if !used_pages.insert(pageid.clone()) {
                return None;
            }
            let old = *old_pages.get(pageid.as_str())?;
            if old["display_period"] != desired["display_period"] {
                return None;
            }
            let mut retained = BTreeMap::new();
            let mut additions = Vec::new();
            for (widgetid, widget) in link.widgetids.into_iter().zip(requested) {
                if let Some(widgetid) = widgetid {
                    if retained.insert(widgetid, widget).is_some() {
                        return None;
                    }
                } else {
                    additions.push(widget);
                }
            }
            expected = old.clone();
            wire = json!({"dashboard_pageid":pageid});
            let mut updated = Vec::with_capacity(requested.len());
            let mut patches = Vec::with_capacity(requested.len());
            for existing in old["widgets"].as_array()? {
                let id = existing["widgetid"].as_str()?;
                let Some(desired) = retained.remove(id) else {
                    continue;
                };
                let mut prior = existing.clone();
                prior.as_object_mut()?.remove("widgetid")?;
                prior["fields"]
                    .as_array_mut()?
                    .sort_by_cached_key(Value::to_string);
                let mut comparable = desired.clone();
                comparable["fields"]
                    .as_array_mut()?
                    .sort_by_cached_key(Value::to_string);
                let mut piece = existing.clone();
                let mut patch = json!({"widgetid":id});
                if piece["name"] != desired["name"] {
                    piece["name"] = desired["name"].clone();
                    patch["name"] = piece["name"].clone();
                }
                prior["name"] = desired["name"].clone();
                if geometry(&prior)? != geometry(desired)?
                    || prior["fields"] != comparable["fields"]
                {
                    if !graph
                        .scoped_graph
                        .as_ref()?
                        .proves_two_ordinary_rows(desired["fields"].as_array()?)
                    {
                        return None;
                    }
                    if allowed_graph_fields(
                        &prior["fields"],
                        &comparable["fields"],
                        PortableUpdateSources::PatternColor("ds.0.color.0"),
                    )? {
                        patch["fields"] = desired["fields"].clone();
                        piece["fields"] = desired["fields"].clone();
                        prior["fields"] = comparable["fields"].clone();
                    }
                    patch_geometry(&mut piece, desired, &mut patch);
                    for key in GEOMETRY {
                        prior[key] = desired[key].clone();
                    }
                }
                if prior != comparable || !has_room(&updated, &piece)? {
                    return None;
                }
                updated.push(piece);
                patches.push(patch);
            }
            if !retained.is_empty() {
                return None;
            }
            allocated = additions.len();
            for desired in additions {
                if !has_room(&updated, desired)? {
                    return None;
                }
                patches.push(desired.clone());
                let mut desired = desired.clone();
                desired["widgetid"] = json!(new_widgets.next()?);
                updated.push(desired);
            }
            if old["name"] != desired["name"] {
                wire["name"] = desired["name"].clone();
                expected["name"] = desired["name"].clone();
            }
            if expected["widgets"].as_array()? != &updated {
                wire["widgets"] = json!(patches);
                expected["widgets"] = json!(updated);
            }
        } else {
            if link.widgetids.iter().any(Option::is_some) {
                return None;
            }
            wire = desired.clone();
            expected = wire.clone();
            expected["dashboard_pageid"] = json!(new_pages.next()?);
            allocated = requested.len();
            for widget in expected["widgets"].as_array_mut()? {
                widget["widgetid"] = json!(new_widgets.next()?);
            }
        }
        wire_pages.push(wire);
        expected_pages.push(expected);
        ids.push(NewGraphIds {
            new_page: new,
            new_widgets: allocated,
        });
    }
    let mut expected = before.clone();
    expected["pages"] = json!(expected_pages);
    expected["name"] = json!(graph.name);
    Some((
        json!({"dashboardid":target.dashboard_id(),"name":graph.name,"pages":wire_pages}),
        expected,
        PortableUpdateReadback::LinkedComposition { pages: ids },
    ))
}

fn by_id<'a>(rows: &'a [Value], key: &str) -> Option<BTreeMap<&'a str, &'a Value>> {
    let mapped = rows
        .iter()
        .map(|row| Some((row[key].as_str()?, row)))
        .collect::<Option<BTreeMap<_, _>>>()?;
    (rows.len() == mapped.len()).then_some(mapped)
}

fn rename(
    differences: &mut Vec<PortableUpdateDifference>,
    object: &'static str,
    old: &Value,
    new: &Value,
) -> Option<()> {
    let before = old["name"].as_str()?;
    let after = new["name"].as_str()?;
    if before != after {
        differences.push(PortableUpdateDifference::Name {
            object,
            before: before.into(),
            after: after.into(),
        });
    }
    Some(())
}

pub(super) fn differences(before: &Value, after: &Value) -> Option<Vec<PortableUpdateDifference>> {
    use PortableUpdateDifference::*;
    let mut result = Vec::new();
    rename(&mut result, "dashboard", before, after)?;
    let before = before["pages"].as_array()?;
    let after = after["pages"].as_array()?;
    let old_pages = by_id(before, "dashboard_pageid")?;
    let new_pages = by_id(after, "dashboard_pageid")?;
    for (index, page) in before.iter().enumerate() {
        let old_name = page["name"].as_str()?;
        let matching = new_pages.get(page["dashboard_pageid"].as_str()?);
        if matching.is_none() {
            result.push(RemovePage {
                name: old_name.into(),
                number: index + 1,
            });
        }
        let updated_widgets = matching.map_or(Some(BTreeMap::new()), |matching| {
            by_id(matching["widgets"].as_array()?, "widgetid")
        })?;
        for widget in page["widgets"].as_array()? {
            if !updated_widgets.contains_key(widget["widgetid"].as_str()?) {
                let [x, y, ..] = rect(widget)?;
                result.push(RemoveWidget {
                    page: old_name.into(),
                    name: widget["name"].as_str()?.into(),
                    x,
                    y,
                });
            }
        }
    }
    for (index, page) in after.iter().enumerate() {
        let name = page["name"].as_str()?;
        let id = page["dashboard_pageid"].as_str()?;
        let old_widgets = if let Some(old) = old_pages.get(id) {
            let from = page_position(before, id)?;
            if from != index {
                result.push(MovePage {
                    name: name.into(),
                    from: from + 1,
                    to: index + 1,
                });
            }
            rename(&mut result, "page", old, page)?;
            by_id(old["widgets"].as_array()?, "widgetid")?
        } else {
            result.push(AddPage {
                name: name.into(),
                number: index + 1,
            });
            BTreeMap::new()
        };
        for widget in page["widgets"].as_array()? {
            if let Some(old) = old_widgets.get(widget["widgetid"].as_str()?) {
                rename(&mut result, "widget", old, widget)?;
                geometry_difference(&mut result, old, widget, name)?;
                field_differences(&mut result, old, widget, name)?;
            } else {
                let [x, y, ..] = rect(widget)?;
                result.push(AddWidget {
                    page: name.into(),
                    name: widget["name"].as_str()?.into(),
                    x,
                    y,
                });
            }
        }
    }
    Some(result)
}
