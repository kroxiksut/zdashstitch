// SPDX-License-Identifier: MIT OR Apache-2.0

//! Closed 8.0.0 update codec for independently verified Graph shapes.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::graph_8_0::PortableUpdateSources;
use super::{
    CreateRejection, CreateRejectionCode, VerifiedDashboardCreate, graph_8_0, shape, text,
};
use crate::dashboard_access::VerifiedAccess;
use crate::gateway::WriteMethod;
use crate::{
    DashboardAccessSelection, DashboardAccessState, DashboardPublishOutcome, ReadMethod,
    ReadWriteGateway, VersionedDashboardSnapshot, WidgetCapabilityRegistry, WidgetFieldType,
    ZabbixError, is_zabbix_id,
};
use std::sync::atomic::{AtomicBool, Ordering};

mod composition;

pub const MAX_PORTABLE_APPEND_WIDGETS: usize = 256;

pub enum PortableGraphChange {
    AppendPage {
        after_pageid: Option<String>,
    },
    AppendWidget {
        pageid: String,
        x: u32,
        y: u32,
    },
    AppendWidgets {
        pageid: String,
        placements: Vec<(u32, u32)>,
    },
    UpdateCreated {
        pageid: String,
        widgetid: String,
    },
    UpdateGraphLabels {
        pages: Vec<LinkedGraphPage>,
    },
    UpdateGraphComposition {
        pages: Vec<LinkedGraphCompositionPage>,
    },
    ChangeAccess(DashboardAccessSelection),
}

pub struct LinkedGraphPage {
    pub pageid: String,
    pub widgetids: Vec<String>,
}

pub struct LinkedGraphCompositionPage {
    pub pageid: Option<String>,
    pub widgetids: Vec<Option<String>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NewGraphIds {
    pub new_page: bool,
    pub new_widgets: usize,
}

/// Positions used to compare content while the server allocates IDs for additions.
/// Reconciliation accepts this value only for independently proven 8.0.0 records.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PortableUpdateReadback {
    Exact,
    UpdatedGraph,
    AddedPage {
        page: usize,
    },
    InsertedPage {
        page: usize,
        page_count: usize,
    },
    AddedWidget {
        page: usize,
        widget: usize,
    },
    AddedPageWidgets {
        page: usize,
        page_count: usize,
        widgets: usize,
    },
    AddedWidgets {
        page: usize,
        first_widget: usize,
        widgets: usize,
    },
    LinkedComposition {
        pages: Vec<NewGraphIds>,
    },
}

impl PortableUpdateReadback {
    pub fn fingerprint(&self, snapshot: &VersionedDashboardSnapshot) -> Option<String> {
        if !graph_8_0::exact_version(snapshot.server_version()) {
            return None;
        }
        if *self == Self::Exact {
            return Some(snapshot.fingerprint().to_owned());
        }
        let raw = snapshot.raw_api_document();
        supported_target(raw, raw["userid"].as_str()?)?;
        let mut raw = raw.clone();
        let pages = raw["pages"].as_array_mut()?;
        if let Self::LinkedComposition { pages: links } = self {
            composition::normalize_ids(pages, links)?;
            return snapshot_fingerprint(snapshot, raw);
        }
        let added = *self != Self::UpdatedGraph;
        let (page, first_widget, count) = match *self {
            Self::Exact => return None,
            Self::UpdatedGraph => {
                let [page] = pages.as_mut_slice() else {
                    return None;
                };
                (page, 0, 1)
            }
            Self::AddedPage { page }
            | Self::InsertedPage { page, .. }
            | Self::AddedPageWidgets { page, .. } => {
                let (page_count, widgets) = match *self {
                    Self::AddedPageWidgets {
                        page_count,
                        widgets,
                        ..
                    } => (page_count, widgets),
                    Self::InsertedPage { page_count, .. } => (page_count, 1),
                    _ => (page.checked_add(1)?, 1),
                };
                if pages.len() != page_count {
                    return None;
                }
                let page = pages.get_mut(page)?;
                page.as_object_mut()?
                    .remove("dashboard_pageid")
                    .filter(|id| id.as_str().is_some_and(is_zabbix_id))?;
                (page, 0, widgets)
            }
            Self::AddedWidget { page, widget } => (pages.get_mut(page)?, widget, 1),
            Self::AddedWidgets {
                page,
                first_widget,
                widgets,
            } => (pages.get_mut(page)?, first_widget, widgets),
            Self::LinkedComposition { .. } => return None,
        };
        let widgets = page["widgets"].as_array_mut()?;
        if widgets.len() != first_widget.checked_add(count)? {
            return None;
        }
        for widget in &mut widgets[first_widget..] {
            if added {
                widget
                    .as_object_mut()?
                    .remove("widgetid")
                    .filter(|id| id.as_str().is_some_and(is_zabbix_id))?;
            }
            // The Graph codec independently verified this field-order normalization.
            widget["fields"]
                .as_array_mut()?
                .sort_by_cached_key(Value::to_string);
        }
        snapshot_fingerprint(snapshot, raw)
    }
}

fn snapshot_fingerprint(snapshot: &VersionedDashboardSnapshot, raw: Value) -> Option<String> {
    VersionedDashboardSnapshot::from_adapter_raw(
        snapshot.connection_id().to_owned(),
        snapshot.server_version().clone(),
        raw,
    )
    .ok()
    .map(|snapshot| snapshot.fingerprint().to_owned())
}

/// Non-deserializable, exact-version authority for one update, with one send reservation.
pub struct VerifiedPortableUpdate {
    target: VersionedDashboardSnapshot,
    graphs: Vec<VerifiedDashboardCreate>,
    params: Value,
    expected: String,
    readback: PortableUpdateReadback,
    differences: Vec<PortableUpdateDifference>,
    repeats_append: bool,
    access: Option<ScopedAccessUpdate>,
}

struct ScopedAccessUpdate {
    before: DashboardAccessState,
    verified: VerifiedAccess,
    authority: String,
    attempted: AtomicBool,
}

pub enum PortableUpdateDifference {
    AddPage {
        name: String,
        number: usize,
    },
    AddWidget {
        page: String,
        name: String,
        x: u32,
        y: u32,
    },
    RemovePage {
        name: String,
        number: usize,
    },
    RemoveWidget {
        page: String,
        name: String,
        x: u32,
        y: u32,
    },
    MovePage {
        name: String,
        from: usize,
        to: usize,
    },
    Geometry {
        page: String,
        widget: String,
        before: [u32; 4],
        after: [u32; 4],
    },
    Name {
        object: &'static str,
        before: String,
        after: String,
    },
    Field {
        page: String,
        widget: String,
        name: String,
        before: Option<String>,
        after: String,
    },
}

impl VerifiedPortableUpdate {
    pub fn access_change(&self) -> Option<(&DashboardAccessState, &DashboardAccessState)> {
        self.access
            .as_ref()
            .map(|change| (&change.before, &change.verified.state))
    }

    pub fn target(&self) -> &VersionedDashboardSnapshot {
        &self.target
    }

    pub fn expected_fingerprint(&self) -> &str {
        &self.expected
    }

    pub const fn readback(&self) -> &PortableUpdateReadback {
        &self.readback
    }

    pub fn differences(&self) -> &[PortableUpdateDifference] {
        &self.differences
    }

    pub const fn repeats_append(&self) -> bool {
        self.repeats_append
    }
}

fn repeats_append(
    before: &Value,
    after: &Value,
    readback: &PortableUpdateReadback,
) -> Option<bool> {
    let page = match *readback {
        PortableUpdateReadback::AddedPage { page }
        | PortableUpdateReadback::InsertedPage { page, .. }
        | PortableUpdateReadback::AddedPageWidgets { page, .. } => page,
        _ => return Some(false),
    };
    let candidate = super::canonical_page(after["pages"].as_array()?.get(page)?)?;
    for previous in before["pages"].as_array()? {
        if previous["name"] == candidate["name"] && super::canonical_page(previous)? == candidate {
            return Some(true);
        }
    }
    Some(false)
}

fn supported_target(raw: &Value, account: &str) -> Option<()> {
    if DashboardAccessState::read(raw)?.owner != account {
        return None;
    }
    let pages = raw["pages"].as_array()?;
    if !(1..=50).contains(&pages.len()) {
        return None;
    }
    let mut pages_seen = BTreeSet::new();
    let mut widgets_seen = BTreeSet::new();
    let registry = WidgetCapabilityRegistry::v8_0();
    let graph = registry.find("svggraph")?;
    for page in pages {
        let pageid = page["dashboard_pageid"]
            .as_str()
            .filter(|id| is_zabbix_id(id))?;
        if !pages_seen.insert(pageid) {
            return None;
        }
        let widgets = page["widgets"].as_array()?;
        if widgets.len() > MAX_PORTABLE_APPEND_WIDGETS {
            return None;
        }
        for widget in widgets {
            let widgetid = widget["widgetid"].as_str().filter(|id| is_zabbix_id(id))?;
            if !widgets_seen.insert(widgetid) {
                return None;
            }
            let fields = widget["fields"].as_array()?;
            match widget["type"].as_str() {
                Some("clock") if fields.is_empty() => {}
                Some("svggraph") => {
                    let mut names = BTreeSet::new();
                    for field in fields {
                        let name = field["name"].as_str()?;
                        let kind = WidgetFieldType::try_from(
                            text(field.get("type"))?.parse::<u16>().ok()?,
                        )
                        .ok()?;
                        if !names.insert(name)
                            || name == "reference"
                            || name.ends_with("._reference")
                            || !graph.knows_field(name, kind)
                            || !field["value"].is_string()
                        {
                            return None;
                        }
                    }
                }
                _ => return None,
            }
        }
    }
    Some(())
}

#[cfg(test)]
#[test]
fn append_targets_require_recognized_fields_without_widget_links() {
    let mut target = json!({
        "userid":"1", "private":"1", "users":[], "userGroups":[],
        "pages":[{"dashboard_pageid":"2","widgets":[
            {"widgetid":"3","type":"clock","fields":[]},
            {"widgetid":"4","type":"svggraph","fields":[
                {"type":"1","name":"ds.0.color.0","value":"ABCDEF"}
            ]}
        ]}]
    });
    assert_eq!(supported_target(&target, "1"), Some(()));
    for name in ["reference", "time_period._reference", "vendor.unknown"] {
        target["pages"][0]["widgets"][1]["fields"][0]["name"] = json!(name);
        assert_eq!(supported_target(&target, "1"), None);
    }
}

fn rect(widget: &Value) -> Option<[u32; 4]> {
    let unsigned = |key| text(widget.get(key))?.parse::<u32>().ok();
    let (x, y, width, height) = (
        unsigned("x")?,
        unsigned("y")?,
        unsigned("width")?,
        unsigned("height")?,
    );
    let (right, bottom) = (x.checked_add(width)?, y.checked_add(height)?);
    (width > 0 && height > 0 && right <= 72 && bottom <= 64).then_some([x, y, right, bottom])
}

const GEOMETRY: [&str; 4] = ["x", "y", "width", "height"];

fn geometry(widget: &Value) -> Option<[u32; 4]> {
    let [x, y, right, bottom] = rect(widget)?;
    Some([x, y, right - x, bottom - y])
}

fn patch_geometry(current: &mut Value, desired: &Value, patch: &mut Value) {
    for key in GEOMETRY {
        if current[key] != desired[key] {
            patch[key] = desired[key].clone();
            current[key] = desired[key].clone();
        }
    }
}

fn geometry_difference(
    differences: &mut Vec<PortableUpdateDifference>,
    old: &Value,
    new: &Value,
    page: &str,
) -> Option<()> {
    let before = geometry(old)?;
    let after = geometry(new)?;
    if before != after {
        differences.push(PortableUpdateDifference::Geometry {
            page: page.to_owned(),
            widget: new["name"].as_str()?.to_owned(),
            before,
            after,
        });
    }
    Some(())
}

fn field_differences(
    differences: &mut Vec<PortableUpdateDifference>,
    old: &Value,
    new: &Value,
    page: &str,
) -> Option<()> {
    let before = old["fields"].as_array()?;
    for field in new["fields"].as_array()? {
        let name = field["name"].as_str()?;
        let old = before.iter().find(|row| row["name"] == name);
        if old.is_none_or(|old| old["value"] != field["value"]) {
            differences.push(PortableUpdateDifference::Field {
                page: page.to_owned(),
                widget: new["name"].as_str()?.to_owned(),
                name: name.to_owned(),
                before: old.and_then(|old| text(old.get("value"))),
                after: text(field.get("value"))?,
            });
        }
    }
    Some(())
}

pub(super) fn has_room(widgets: &[Value], added: &Value) -> Option<bool> {
    let [left, top, right, bottom] = rect(added)?;
    for widget in widgets {
        let [x, y, r, b] = rect(widget)?;
        if left < r && x < right && top < b && y < bottom {
            return Some(false);
        }
    }
    Some(true)
}

fn page_position(pages: &[Value], pageid: &str) -> Option<usize> {
    pages
        .iter()
        .position(|page| page["dashboard_pageid"] == pageid)
}

fn placeholder_ids<'a>(rows: impl Iterator<Item = &'a str>, count: usize) -> Option<Vec<String>> {
    let present: BTreeSet<_> = rows.collect();
    Some(
        (1..=present.len().checked_add(count)?)
            .map(|number| number.to_string())
            .filter(|id| !present.contains(id.as_str()))
            .take(count)
            .collect(),
    )
}

fn make_change(
    target: &VersionedDashboardSnapshot,
    graphs: &[VerifiedDashboardCreate],
    change: PortableGraphChange,
    sources: Option<PortableUpdateSources>,
) -> Option<(Value, Value, PortableUpdateReadback)> {
    if let PortableGraphChange::UpdateGraphComposition { pages } = change {
        let [graph] = graphs else {
            return None;
        };
        return composition::make_change(target, graph, pages);
    }
    let mut expected = target.raw_api_document().clone();
    let pages = expected["pages"].as_array_mut()?;
    let mut payload_pages: Vec<_> = pages
        .iter()
        .map(|page| json!({"dashboard_pageid":page["dashboard_pageid"]}))
        .collect();
    let graph = graphs.first()?;
    let mut new_page = graph.create_params["pages"][0].clone();
    let mut new_widget = new_page["widgets"][0].clone();
    let mut additions = Vec::new();
    for graph in graphs {
        let [page] = graph.create_params["pages"].as_array()?.as_slice() else {
            return None;
        };
        additions.extend(page["widgets"].as_array()?.iter().cloned());
    }
    let widget_ids = placeholder_ids(
        pages
            .iter()
            .flat_map(|page| page["widgets"].as_array().into_iter().flatten())
            .filter_map(|widget| widget["widgetid"].as_str()),
        additions.len(),
    )?;
    let change = match change {
        PortableGraphChange::AppendWidget { pageid, x, y } => PortableGraphChange::AppendWidgets {
            pageid,
            placements: vec![(x, y)],
        },
        change => change,
    };
    let readback = match change {
        PortableGraphChange::AppendPage { after_pageid } if pages.len() < 50 => {
            let index = if let Some(after_pageid) = after_pageid {
                page_position(pages, &after_pageid)? + 1
            } else {
                pages.len()
            };
            let count = additions.len();
            if !(1..=MAX_PORTABLE_APPEND_WIDGETS).contains(&count) {
                return None;
            }
            for (widget_index, widget) in additions.iter().enumerate() {
                if !has_room(&additions[..widget_index], widget)? {
                    return None;
                }
            }
            new_page["widgets"] = json!(additions);
            payload_pages.insert(index, new_page.clone());
            new_page["dashboard_pageid"] = json!(
                placeholder_ids(
                    pages
                        .iter()
                        .filter_map(|page| page["dashboard_pageid"].as_str()),
                    1
                )?
                .pop()?
            );
            for (widget, id) in new_page["widgets"]
                .as_array_mut()?
                .iter_mut()
                .zip(widget_ids)
            {
                widget["widgetid"] = json!(id);
            }
            pages.insert(index, new_page);
            if count > 1 {
                PortableUpdateReadback::AddedPageWidgets {
                    page: index,
                    page_count: pages.len(),
                    widgets: count,
                }
            } else if index + 1 == pages.len() {
                PortableUpdateReadback::AddedPage { page: index }
            } else {
                PortableUpdateReadback::InsertedPage {
                    page: index,
                    page_count: pages.len(),
                }
            }
        }
        PortableGraphChange::AppendWidgets { pageid, placements } => {
            if placements.len() != additions.len() {
                return None;
            }
            let index = page_position(pages, &pageid)?;
            let widgets = pages[index]["widgets"].as_array_mut()?;
            let widget = widgets.len();
            if widget.checked_add(additions.len())? > MAX_PORTABLE_APPEND_WIDGETS {
                return None;
            }
            let mut wire: Vec<_> = widgets
                .iter()
                .map(|widget| json!({"widgetid":widget["widgetid"]}))
                .collect();
            let count = additions.len();
            for ((mut addition, (x, y)), server_id) in
                additions.drain(..).zip(placements).zip(widget_ids)
            {
                addition["x"] = json!(x.to_string());
                addition["y"] = json!(y.to_string());
                if !has_room(widgets, &addition)? {
                    return None;
                }
                wire.push(addition.clone());
                addition["widgetid"] = json!(server_id);
                widgets.push(addition);
            }
            payload_pages[index]["widgets"] = json!(wire);
            if count == 1 {
                PortableUpdateReadback::AddedWidget {
                    page: index,
                    widget,
                }
            } else {
                PortableUpdateReadback::AddedWidgets {
                    page: index,
                    first_widget: widget,
                    widgets: count,
                }
            }
        }
        PortableGraphChange::UpdateCreated { pageid, widgetid } => {
            if graphs.len() != 1 || additions.len() != 1 {
                return None;
            }
            let [page] = pages.as_mut_slice() else {
                return None;
            };
            if page["dashboard_pageid"] != pageid {
                return None;
            }
            let [widget] = page["widgets"].as_array_mut()?.as_mut_slice() else {
                return None;
            };
            if widget["widgetid"] != widgetid {
                return None;
            }
            let mut existing = widget.clone();
            existing.as_object_mut()?.remove("widgetid");
            existing["name"] = new_widget["name"].clone();
            let before = existing.as_object_mut()?.remove("fields")?;
            let mut attributes = new_widget.clone();
            let after = attributes.as_object_mut()?.remove("fields")?;
            let sources = sources?;
            let geometry_changed = geometry(&existing)? != geometry(&attributes)?;
            if geometry_changed {
                if !matches!(sources, PortableUpdateSources::Ordinary)
                    || allowed_graph_fields(
                        &before,
                        &after,
                        PortableUpdateSources::PatternColor("ds.0.color.0"),
                    )
                    .is_none()
                {
                    return None;
                }
                for key in GEOMETRY {
                    existing[key] = attributes[key].clone();
                    widget[key] = attributes[key].clone();
                }
            }
            if existing != attributes {
                return None;
            }
            let fields_changed = allowed_graph_fields(&before, &after, sources)?;
            widget["name"] = new_widget["name"].clone();
            if fields_changed {
                widget["fields"] = after;
            }
            new_widget["widgetid"] = json!(widgetid);
            payload_pages[0]["name"] = new_page["name"].clone();
            payload_pages[0]["widgets"] = json!([new_widget]);
            page["name"] = new_page["name"].clone();
            expected["name"] = json!(graph.name);
            PortableUpdateReadback::UpdatedGraph
        }
        PortableGraphChange::UpdateGraphLabels { pages: links } => {
            if graphs.len() != 1 || links.len() != pages.len() {
                return None;
            }
            let intended = graph.create_params["pages"].as_array()?;
            if intended.len() != pages.len() {
                return None;
            }
            for (index, ((page, intent), link)) in
                pages.iter_mut().zip(intended).zip(links).enumerate()
            {
                if page["dashboard_pageid"] != link.pageid
                    || page["display_period"] != intent["display_period"]
                {
                    return None;
                }
                let widgets = page["widgets"].as_array_mut()?;
                let desired = intent["widgets"].as_array()?;
                if widgets.len() != desired.len() || desired.len() != link.widgetids.len() {
                    return None;
                }
                let mut wire = Vec::with_capacity(widgets.len());
                for widget in widgets {
                    let id = widget["widgetid"].as_str()?.to_owned();
                    let position = link.widgetids.iter().position(|expected| *expected == id)?;
                    let desired = desired.get(position)?;
                    let mut old = widget.clone();
                    old.as_object_mut()?.remove("widgetid")?;
                    old["name"] = desired["name"].clone();
                    let mut intended_widget = desired.clone();
                    old["fields"]
                        .as_array_mut()?
                        .sort_by_cached_key(Value::to_string);
                    intended_widget["fields"]
                        .as_array_mut()?
                        .sort_by_cached_key(Value::to_string);
                    if old != intended_widget {
                        return None;
                    }
                    let mut patch = json!({"widgetid":id});
                    if widget["name"] != desired["name"] {
                        widget["name"] = desired["name"].clone();
                        patch["name"] = widget["name"].clone();
                    }
                    wire.push(patch);
                }
                if page["name"] != intent["name"] {
                    page["name"] = intent["name"].clone();
                    payload_pages[index]["name"] = page["name"].clone();
                }
                payload_pages[index]["widgets"] = json!(wire);
            }
            expected["name"] = json!(graph.name);
            PortableUpdateReadback::Exact
        }
        PortableGraphChange::AppendPage { .. }
        | PortableGraphChange::AppendWidget { .. }
        | PortableGraphChange::UpdateGraphComposition { .. }
        | PortableGraphChange::ChangeAccess(_) => {
            return None;
        }
    };
    let mut payload = json!({"dashboardid":target.dashboard_id(), "pages":payload_pages});
    if matches!(
        readback,
        PortableUpdateReadback::Exact | PortableUpdateReadback::UpdatedGraph
    ) {
        payload["name"] = expected["name"].clone();
    }
    Some((payload, expected, readback))
}

fn allowed_graph_fields(
    before: &Value,
    after: &Value,
    sources: PortableUpdateSources,
) -> Option<bool> {
    let mut before: Vec<_> = before.as_array()?.iter().collect();
    let mut after: Vec<_> = after.as_array()?.iter().collect();
    before.sort_by_key(|field| field["name"].as_str());
    after.sort_by_key(|field| field["name"].as_str());
    if matches!(sources, PortableUpdateSources::ThreeOrdinary)
        && before.len() == 5
        && after.len() == 7
    {
        after.retain(|field| {
            !matches!(
                field["name"].as_str(),
                Some("ds.0.color.2" | "ds.0.itemids.2")
            )
        });
        return (before == after).then_some(true);
    }
    if before.len() != after.len() {
        return None;
    }
    let mut changed = Vec::new();
    for (old, new) in before.into_iter().zip(after) {
        let name = new["name"].as_str()?;
        if old["name"] != name || old["type"] != new["type"] {
            return None;
        }
        if old["value"] != new["value"] {
            changed.push(name);
        }
    }
    if changed.is_empty() {
        return Some(false);
    }
    let verified = match sources {
        PortableUpdateSources::Ordinary => {
            changed == ["ds.0.itemids.0"] || changed == ["ds.0.color.0"]
        }
        PortableUpdateSources::ThreeOrdinary => false,
        PortableUpdateSources::Discovered => changed == ["ds.0.itemids.0", "ds.0.itemids.1"],
        PortableUpdateSources::PatternColor(field) => changed == [field],
    };
    verified.then_some(true)
}

fn differences(
    before: &Value,
    after: &Value,
    readback: &PortableUpdateReadback,
) -> Option<Vec<PortableUpdateDifference>> {
    use PortableUpdateDifference::*;
    match *readback {
        PortableUpdateReadback::LinkedComposition { .. } => composition::differences(before, after),
        PortableUpdateReadback::AddedPage { page }
        | PortableUpdateReadback::InsertedPage { page, .. } => Some(vec![AddPage {
            name: after["pages"][page]["name"].as_str()?.to_owned(),
            number: page + 1,
        }]),
        PortableUpdateReadback::AddedPageWidgets { page, .. } => {
            let page_value = &after["pages"][page];
            let name = page_value["name"].as_str()?.to_owned();
            let mut changes = vec![AddPage {
                name: name.clone(),
                number: page + 1,
            }];
            changes.extend(
                page_value["widgets"]
                    .as_array()?
                    .iter()
                    .map(|graph| {
                        Some(AddWidget {
                            page: name.clone(),
                            name: graph["name"].as_str()?.to_owned(),
                            x: text(graph.get("x"))?.parse().ok()?,
                            y: text(graph.get("y"))?.parse().ok()?,
                        })
                    })
                    .collect::<Option<Vec<_>>>()?,
            );
            Some(changes)
        }
        PortableUpdateReadback::AddedWidget { page, widget }
        | PortableUpdateReadback::AddedWidgets {
            page,
            first_widget: widget,
            ..
        } => {
            let page = &after["pages"][page];
            page["widgets"]
                .as_array()?
                .get(widget..)?
                .iter()
                .map(|graph| {
                    Some(AddWidget {
                        page: page["name"].as_str()?.to_owned(),
                        name: graph["name"].as_str()?.to_owned(),
                        x: text(graph.get("x"))?.parse().ok()?,
                        y: text(graph.get("y"))?.parse().ok()?,
                    })
                })
                .collect()
        }
        PortableUpdateReadback::Exact => {
            let mut rows = Vec::new();
            let mut name = |object, old: &Value, new: &Value| -> Option<()> {
                let before = old["name"].as_str()?;
                let after = new["name"].as_str()?;
                if before != after {
                    rows.push(Name {
                        object,
                        before: before.into(),
                        after: after.into(),
                    });
                }
                Some(())
            };
            name("dashboard", before, after)?;
            for (old_page, new_page) in before["pages"]
                .as_array()?
                .iter()
                .zip(after["pages"].as_array()?)
            {
                name("page", old_page, new_page)?;
                for (old, new) in old_page["widgets"]
                    .as_array()?
                    .iter()
                    .zip(new_page["widgets"].as_array()?)
                {
                    name("widget", old, new)?;
                }
            }
            Some(rows)
        }
        PortableUpdateReadback::UpdatedGraph => {
            let mut rows = Vec::new();
            let old_page = &before["pages"][0];
            let new_page = &after["pages"][0];
            let old_widget = &old_page["widgets"][0];
            let new_widget = &new_page["widgets"][0];
            for (object, old, new) in [
                ("dashboard", before, after),
                ("page", old_page, new_page),
                ("widget", old_widget, new_widget),
            ] {
                let before = old["name"].as_str()?;
                let after = new["name"].as_str()?;
                if before != after {
                    rows.push(Name {
                        object,
                        before: before.to_owned(),
                        after: after.to_owned(),
                    });
                }
            }
            geometry_difference(
                &mut rows,
                old_widget,
                new_widget,
                new_page["name"].as_str()?,
            )?;
            field_differences(
                &mut rows,
                old_widget,
                new_widget,
                new_page["name"].as_str()?,
            )?;
            Some(rows)
        }
    }
}

impl ReadWriteGateway {
    async fn verify_eight_zero_access_update(
        &self,
        target: VersionedDashboardSnapshot,
        selection: DashboardAccessSelection,
    ) -> std::result::Result<VerifiedPortableUpdate, CreateRejection> {
        let unverified = || {
            CreateRejection::new(CreateRejectionCode::ContractUnverified)
                .with("contract", crate::dashboard_access::CONTRACT)
        };
        let reader = self.reader();
        let context = self.publication_context();
        if !crate::dashboard_access::exact_version(target.server_version())
            || !context.is_live_authenticated()
            || !self
                .portable_target_is_editable(target.dashboard_id())
                .await
            || !matches!(reader.dashboard_snapshot(target.connection_id(), target.dashboard_id()).await,
                Ok(Some(current)) if current.fingerprint() == target.fingerprint())
        {
            return Err(unverified());
        }
        let mut expected = target.raw_api_document().clone();
        let before = DashboardAccessState::read(&expected).ok_or_else(unverified)?;
        let verified = reader
            .verify_dashboard_access(&selection, Some(&before))
            .await
            .ok()
            .flatten()
            .ok_or_else(unverified)?;
        let mut params = json!({"dashboardid": target.dashboard_id()});
        verified.apply(&mut params);
        for (key, value) in params.as_object().ok_or_else(unverified)? {
            expected[key] = value.clone();
        }
        let expected = snapshot_fingerprint(&target, expected).ok_or_else(unverified)?;
        Ok(VerifiedPortableUpdate {
            target,
            graphs: Vec::new(),
            params,
            expected,
            readback: PortableUpdateReadback::Exact,
            differences: Vec::new(),
            repeats_append: false,
            access: Some(ScopedAccessUpdate {
                before,
                verified,
                authority: context.binding_fingerprint().to_owned(),
                attempted: AtomicBool::new(false),
            }),
        })
    }

    async fn portable_target_is_editable(&self, dashboardid: &str) -> bool {
        if !is_zabbix_id(dashboardid) || !graph_8_0::exact_version(&self.capabilities().version) {
            return false;
        }
        self.reader()
            .read_inventory_rows(
                ReadMethod::DashboardGet,
                json!({"output":["dashboardid"],"dashboardids":[dashboardid],"editable":true}),
            )
            .await
            .is_ok_and(|rows| matches!(rows.as_slice(), [row] if row["dashboardid"] == dashboardid))
    }

    pub async fn verify_eight_zero_portable_update(
        &self,
        graphs: Vec<VerifiedDashboardCreate>,
        target: VersionedDashboardSnapshot,
        change: PortableGraphChange,
    ) -> std::result::Result<VerifiedPortableUpdate, CreateRejection> {
        let unverified = || {
            CreateRejection::new(CreateRejectionCode::ContractUnverified)
                .with("contract", "update_portable_graph_8_0_0")
        };
        if let PortableGraphChange::ChangeAccess(selection) = change {
            if !graphs.is_empty() {
                return Err(unverified());
            }
            return self
                .verify_eight_zero_access_update(target, selection)
                .await;
        }
        if !graph_8_0::exact_version(target.server_version())
            || !(1..=MAX_PORTABLE_APPEND_WIDGETS).contains(&graphs.len())
            || !self
                .portable_target_is_editable(target.dashboard_id())
                .await
        {
            return Err(unverified());
        }
        let context = self.publication_context();
        let multi_widget = matches!(
            &change,
            PortableGraphChange::UpdateGraphLabels { .. }
                | PortableGraphChange::UpdateGraphComposition { .. }
        );
        let linked_update =
            multi_widget || matches!(&change, PortableGraphChange::UpdateCreated { .. });
        if !linked_update {
            let access = target.dashboard_access_state().ok_or_else(unverified)?;
            if !access.private || !access.users.is_empty() || !access.groups.is_empty() {
                return Err(unverified());
            }
        }
        let mut scope = None;
        let mut role = None;
        for graph in &graphs {
            if graph.server_version() != target.server_version()
                || graph.connection_id() != target.connection_id()
                || graph.authority_fingerprint() != context.binding_fingerprint()
            {
                return Err(unverified());
            }
            let field_count = graph.create_params["pages"][0]["widgets"][0]["fields"]
                .as_array()
                .ok_or_else(unverified)?
                .len();
            let proof = graph.scoped_graph.as_ref().ok_or_else(unverified)?;
            if !multi_widget
                && matches!(&change, PortableGraphChange::AppendPage { .. })
                && let Some(widgets) = graph.create_params["pages"][0]["widgets"]
                    .as_array()
                    .filter(|widgets| widgets.len() > 1)
            {
                if graphs.len() != 1
                    || !widgets.iter().all(|widget| {
                        widget["fields"]
                            .as_array()
                            .is_some_and(|fields| proof.proves_two_ordinary_rows(fields))
                    })
                {
                    return Err(unverified());
                }
            } else if !multi_widget {
                scope = Some(
                    proof
                        .portable_update_sources(field_count)
                        .ok_or_else(unverified)?,
                );
            }
            if role
                .replace(proof.role_id())
                .is_some_and(|previous| previous != proof.role_id())
            {
                return Err(unverified());
            }
        }
        supported_target(
            target.raw_api_document(),
            self.reader().identity().account_userid(),
        )
        .ok_or_else(unverified)?;
        let (params, raw, readback) =
            make_change(&target, &graphs, change, scope).ok_or_else(|| shape("target"))?;
        let differences = differences(target.raw_api_document(), &raw, &readback)
            .ok_or_else(|| shape("target"))?;
        let repeats_append = repeats_append(target.raw_api_document(), &raw, &readback)
            .ok_or_else(|| shape("target"))?;
        let candidate = VersionedDashboardSnapshot::from_adapter_raw(
            target.connection_id().to_owned(),
            target.server_version().clone(),
            raw,
        )
        .map_err(|_| shape("target"))?;
        let expected = readback
            .fingerprint(&candidate)
            .ok_or_else(|| shape("target"))?;
        if readback.fingerprint(&target).as_deref() == Some(&expected) {
            return Err(shape("target"));
        }
        Ok(VerifiedPortableUpdate {
            target,
            graphs,
            params,
            expected,
            readback,
            differences,
            repeats_append,
            access: None,
        })
    }

    pub async fn publish_verified_portable_update(
        &self,
        update: &VerifiedPortableUpdate,
        before_send: impl FnOnce() -> bool + Send,
    ) -> DashboardPublishOutcome {
        use DashboardPublishOutcome::*;
        let target = &update.target;
        if !self
            .portable_target_is_editable(target.dashboard_id())
            .await
        {
            return FailedBeforeSend;
        }
        let context = self.publication_context();
        if let Some(access) = &update.access
            && (access.authority != context.binding_fingerprint()
                || !context.is_live_authenticated()
                || !access.verified.recheck(self.reader()).await)
        {
            return FailedBeforeSend;
        }
        for graph in &update.graphs {
            let Some(proof) = &graph.scoped_graph else {
                return FailedBeforeSend;
            };
            if graph.authority_fingerprint() != context.binding_fingerprint()
                || !proof.recheck(self).await
            {
                return FailedBeforeSend;
            }
        }
        let read = async || {
            self.reader()
                .dashboard_snapshot(target.connection_id(), target.dashboard_id())
                .await
        };
        match read().await {
            Ok(Some(current)) if current.fingerprint() == target.fingerprint() => {}
            Ok(_) => return Conflict,
            Err(_) => return FailedBeforeSend,
        }
        if update
            .access
            .as_ref()
            .is_some_and(|access| access.attempted.swap(true, Ordering::AcqRel))
            || !update.graphs.iter().all(|graph| {
                graph
                    .scoped_graph
                    .as_ref()
                    .is_some_and(graph_8_0::ScopedGraphCreate::reserve_send)
            })
            || !before_send()
        {
            return FailedBeforeSend;
        }
        let session = self.session();
        let response = session
            .transport
            .write::<Value>(
                WriteMethod::DashboardUpdate,
                update.params.clone(),
                &session.token,
                &self.grant,
            )
            .await;
        let acknowledged = match response {
            Ok(body) => body["dashboardids"]
                .as_array()
                .is_some_and(|ids| ids == &[json!(target.dashboard_id())]),
            Err(ZabbixError::Api { .. }) => return FailedAfterResponse,
            Err(_) => false,
        };
        if matches!(self.reader().matches_live_version().await, Ok(true))
            && let Ok(Some(after)) = read().await
            && update.readback.fingerprint(&after).as_deref() == Some(update.expected_fingerprint())
        {
            if acknowledged {
                Verified
            } else {
                ReconciledVerified
            }
        } else if acknowledged {
            FailedAfterResponse
        } else {
            UnknownOutcome
        }
    }
}
