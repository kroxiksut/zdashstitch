// SPDX-License-Identifier: MIT OR Apache-2.0

//! Scoped 8.0.0 updates of explicit numeric item references.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Value, json};

use super::{
    MAX_REPLACE_HOST_LLD_CANDIDATES, PublicationContext, WidgetFieldReplacement, value_as_i64,
};
use crate::capabilities::is_eight_zero_item_update_version;
use crate::create::graph_8_0::current_lld_origin;
use crate::create::graph_8_0::patterns::{PatternInputs, PatternSpec, read_clone_inputs};
use crate::{
    ItemDetails, ItemFlags, ReadMethod, ReadOnlyGateway, ReadWriteGateway, Result,
    VersionedDashboardSnapshot, is_zabbix_id,
};

#[derive(Clone, Eq, PartialEq)]
pub(super) struct ItemUpdateEvidence {
    roleid: String,
    items: Vec<Value>,
    hosts: Vec<Value>,
    lld: Option<LldEvidence>,
    patterns: Option<Arc<PatternUpdateEvidence>>,
}

#[derive(Eq, PartialEq)]
struct PatternUpdateEvidence {
    before: PatternInputs,
    after: PatternInputs,
    outside: usize,
}

#[derive(Clone, Eq, PartialEq)]
struct LldEvidence {
    origin: (String, String),
    items: [Value; 2],
    candidates: Vec<ItemDetails>,
}

#[derive(Clone)]
pub(super) struct ScopedItemUpdate {
    source_fingerprint: String,
    replacements: Vec<WidgetFieldReplacement>,
    pub(super) evidence: ItemUpdateEvidence,
}

impl ScopedItemUpdate {
    pub(super) fn outside_pattern_matches(&self) -> Option<usize> {
        self.evidence
            .patterns
            .as_ref()
            .map(|pattern| pattern.outside)
    }

    pub(super) fn covers(
        &self,
        source: &VersionedDashboardSnapshot,
        replacements: &[WidgetFieldReplacement],
    ) -> bool {
        self.source_fingerprint == source.fingerprint() && replacements == self.replacements
    }
}

fn has_exact_keys(object: &Value, keys: &[&str]) -> bool {
    object.as_object().is_some_and(|object| {
        object.len() == keys.len() && keys.iter().all(|k| object.contains_key(*k))
    })
}

pub(super) fn rows(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

#[derive(Clone, Copy)]
pub(super) enum Shape<'a> {
    Item,
    ExplicitGraph {
        stationary: [Option<&'a str>; 2],
    },
    LinkedGraphs {
        stationary: &'a str,
        provider: Provider<'a>,
    },
}

#[derive(Clone, Copy)]
pub(super) enum Provider<'a> {
    Ordinary,
    Lld { source: &'a str, target: &'a str },
    Pattern { widget: &'a Value, target: &'a str },
}

pub(super) fn is_page(page: &Value) -> bool {
    has_exact_keys(
        page,
        &["dashboard_pageid", "name", "display_period", "widgets"],
    ) && page["dashboard_pageid"].as_str().is_some_and(is_zabbix_id)
        && page["name"].is_string()
        && value_as_i64(&page["display_period"]).is_some()
        && page["widgets"].is_array()
}

pub(super) fn is_widget(widget: &Value, kind: &str) -> bool {
    has_exact_keys(
        widget,
        &[
            "widgetid",
            "type",
            "name",
            "x",
            "y",
            "width",
            "height",
            "view_mode",
            "fields",
        ],
    ) && widget["widgetid"].as_str().is_some_and(is_zabbix_id)
        && widget["type"] == kind
        && widget["name"].is_string()
        && ["x", "y", "width", "height", "view_mode"]
            .into_iter()
            .all(|name| value_as_i64(&widget[name]).is_some())
        && widget["fields"].is_array()
}

fn is_clock(widget: &Value) -> bool {
    is_widget(widget, "clock") && rows(&widget["fields"]).is_empty()
}

fn matches_item(widget: &Value, replacement: &WidgetFieldReplacement) -> bool {
    let [field] = rows(&widget["fields"]) else {
        return false;
    };
    is_widget(widget, "item")
        && widget["widgetid"] == replacement.widgetid
        && replacement.page_index == 0
        && replacement.field_name == "itemid.0"
        && has_exact_keys(field, &["name", "type", "value"])
        && field["name"] == "itemid.0"
        && value_as_i64(&field["type"]) == Some(4)
        && field["value"] == replacement.expected_value
}

pub(super) fn graph_row<'a>(widget: &'a Value, name: &str, kind: i64) -> Option<&'a str> {
    let fields = rows(&widget["fields"]);
    let mut matching = fields.iter().filter(|field| field["name"] == name);
    let field = matching.next()?;
    (matching.next().is_none()
        && has_exact_keys(field, &["name", "type", "value"])
        && value_as_i64(&field["type"]) == Some(kind))
    .then(|| field["value"].as_str())?
}

fn graph_color(widget: &Value, name: &str) -> bool {
    graph_row(widget, name, 1)
        .is_some_and(|color| color.len() == 6 && color.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn explicit_graph_stationary<'a>(
    widget: &'a Value,
    replacement: &WidgetFieldReplacement,
) -> Option<[Option<&'a str>; 2]> {
    let count = match rows(&widget["fields"]).len() {
        5 => 2,
        7 => 3,
        _ => return None,
    };
    if !is_widget(widget, "svggraph")
        || widget["widgetid"] != replacement.widgetid
        || value_as_i64(&widget["view_mode"]) != Some(0)
        || replacement.page_index != 0
        || graph_row(widget, "ds.0.dataset_type", 0) != Some("0")
        || graph_row(widget, &replacement.field_name, 4) != Some(&replacement.expected_value)
    {
        return None;
    }
    let mut stationary = [None; 2];
    let mut index = 0;
    let mut seen = BTreeSet::from([replacement.replacement_value.as_str()]);
    for row in 0..count {
        let name = format!("ds.0.itemids.{row}");
        let item = graph_row(widget, &name, 4)?;
        if !is_zabbix_id(item)
            || !seen.insert(item)
            || !graph_color(widget, &format!("ds.0.color.{row}"))
        {
            return None;
        }
        if name != replacement.field_name {
            *stationary.get_mut(index)? = Some(item);
            index += 1;
        }
    }
    (index == count - 1).then_some(stationary)
}

fn graph_stationary<'a>(
    widget: &'a Value,
    replacement: &WidgetFieldReplacement,
) -> Option<&'a str> {
    let other = match replacement.field_name.as_str() {
        "ds.0.itemids.0" => "ds.0.itemids.1",
        "ds.0.itemids.1" => "ds.0.itemids.0",
        _ => return None,
    };
    if !is_widget(widget, "svggraph")
        || rows(&widget["fields"]).len() != 6
        || replacement.page_index != 1
        || widget["widgetid"] != replacement.widgetid
        || graph_row(widget, "ds.0.dataset_type", 0) != Some("0")
        || graph_row(widget, &replacement.field_name, 4) != Some(&replacement.expected_value)
        || ["ds.0.color.0", "ds.0.color.1"]
            .into_iter()
            .any(|name| !graph_color(widget, name))
    {
        return None;
    }
    graph_row(widget, other, 4).filter(|id| {
        is_zabbix_id(id)
            && *id != replacement.expected_value
            && *id != replacement.replacement_value
    })
}

fn linked(provider: &Value, consumer: &Value) -> bool {
    let reference = graph_row(provider, "reference", 1)
        .filter(|value| value.len() == 5 && value.bytes().all(|byte| byte.is_ascii_uppercase()));
    reference.is_some_and(|value| {
        graph_row(consumer, "time_period._reference", 1)
            .and_then(|value| value.strip_suffix("._timeperiod"))
            == Some(value)
    })
}

fn same_pair(left: &WidgetFieldReplacement, right: &WidgetFieldReplacement) -> bool {
    left.expected_value == right.expected_value && left.replacement_value == right.replacement_value
}

fn plain_pattern_provider(widget: &Value, patch: &WidgetFieldReplacement) -> bool {
    let exact_host = |name: &str| {
        !name.is_empty()
            && name.len() <= 255
            && name.is_ascii()
            && !name.contains('*')
            && !name.chars().any(char::is_control)
    };
    is_widget(widget, "svggraph")
        && widget["widgetid"] == patch.widgetid
        && patch.page_index == 1
        && patch.field_name == "ds.0.hosts.0"
        && rows(&widget["fields"]).len() == 6
        && graph_row(widget, "ds.0.dataset_type", 0) == Some("1")
        && graph_row(widget, "ds.0.hosts.0", 1) == Some(&patch.expected_value)
        && exact_host(&patch.expected_value)
        && exact_host(&patch.replacement_value)
        && graph_row(widget, "ds.0.hosts.1", 1).is_some_and(exact_host)
        && graph_row(widget, "ds.0.items.0", 1)
            .is_some_and(|mask| mask.is_ascii() && mask.contains('*'))
        && graph_row(widget, "ds.0.color", 1).is_some()
}

pub(super) fn contract_shape<'a>(
    source: &'a VersionedDashboardSnapshot,
    replacements: &'a [WidgetFieldReplacement],
) -> Option<Shape<'a>> {
    let replacement = replacements.first()?;
    let raw = source.raw_api_document();
    let pages = rows(&raw["pages"]);
    if !is_eight_zero_item_update_version(source.server_version())
        || pages.len() > 3
        || !is_zabbix_id(&replacement.expected_value)
        || !is_zabbix_id(&replacement.replacement_value)
        || replacement.expected_value == replacement.replacement_value
        || !is_zabbix_id(source.dashboard_id())
        || value_as_i64(&raw["private"]) != Some(1)
        || !raw["users"].as_array().is_some_and(Vec::is_empty)
        || !raw["userGroups"].as_array().is_some_and(Vec::is_empty)
    {
        return None;
    }
    if let ([page], [_]) = (pages, replacements) {
        let [graph] = rows(&page["widgets"]) else {
            return None;
        };
        if value_as_i64(&raw["display_period"]) != Some(30)
            || value_as_i64(&raw["auto_start"]) != Some(1)
            || value_as_i64(&page["display_period"]) != Some(0)
        {
            return None;
        }
        return explicit_graph_stationary(graph, replacement)
            .map(|stationary| Shape::ExplicitGraph { stationary });
    }
    let [first, second, rest @ ..] = pages else {
        return None;
    };
    let mut page_ids = BTreeSet::new();
    let mut widget_ids = BTreeSet::new();
    if !pages.iter().all(|page| {
        is_page(page)
            && page_ids.insert(page["dashboard_pageid"].as_str())
            && rows(&page["widgets"]).len() <= 2
            && rows(&page["widgets"])
                .iter()
                .all(|widget| widget_ids.insert(widget["widgetid"].as_str()))
    }) {
        return None;
    }
    let first_widgets = rows(&first["widgets"]);
    let second_widgets = rows(&second["widgets"]);
    match (replacements, first_widgets, second_widgets, rest) {
        ([_], [item], [clock], []) if matches_item(item, replacement) && is_clock(clock) => {
            Some(Shape::Item)
        }
        ([_, second_patch, third_patch], [item, clock], [left, right], rest)
            if matches_item(item, replacement)
                && is_clock(clock)
                && matches!(rest, [] | [_])
                && rest
                    .iter()
                    .all(|page| matches!(rows(&page["widgets"]), [clock] if is_clock(clock))) =>
        {
            let [left_patch, right_patch] = if left["widgetid"] == second_patch.widgetid {
                [second_patch, third_patch]
            } else {
                [third_patch, second_patch]
            };
            let stationary = graph_stationary(right, right_patch)?;
            if rest.len() == 1
                && linked(left, right)
                && right_patch.field_name == "ds.0.itemids.1"
                && same_pair(right_patch, replacement)
                && plain_pattern_provider(left, left_patch)
            {
                return Some(Shape::LinkedGraphs {
                    stationary,
                    provider: Provider::Pattern {
                        widget: left,
                        target: &left_patch.replacement_value,
                    },
                });
            }
            if graph_stationary(left, left_patch)? != stationary {
                return None;
            }
            if same_pair(left_patch, replacement)
                && same_pair(right_patch, replacement)
                && (linked(left, right) || linked(right, left))
            {
                return Some(Shape::LinkedGraphs {
                    stationary,
                    provider: Provider::Ordinary,
                });
            }
            let old = left_patch.expected_value.as_str();
            let new = left_patch.replacement_value.as_str();
            if rest.len() == 1
                && linked(left, right)
                && left_patch.field_name == "ds.0.itemids.0"
                && right_patch.field_name == "ds.0.itemids.1"
                && same_pair(right_patch, replacement)
                && is_zabbix_id(old)
                && is_zabbix_id(new)
                && old != new
                && [old, new].into_iter().all(|id| {
                    id != replacement.expected_value && id != replacement.replacement_value
                })
            {
                return Some(Shape::LinkedGraphs {
                    stationary,
                    provider: Provider::Lld {
                        source: old,
                        target: new,
                    },
                });
            }
            None
        }
        _ => None,
    }
}

async fn read_lld_evidence(
    reader: &ReadOnlyGateway,
    source: &str,
    target: &str,
    ordinary_source: &Value,
    ordinary_target: &Value,
) -> Result<Option<LldEvidence>> {
    let rows = reader.read_inventory_rows(ReadMethod::ItemGet, json!({
        "itemids": [source, target], "output": ["itemid", "hostid", "key_", "units", "value_type", "status", "flags"],
        "selectDiscoveryData": ["parent_itemid", "key_", "status"],
    })).await?;
    let get = |id, ordinary: &Value| {
        rows.iter().find(|row| {
            row["itemid"] == id
                && row["hostid"] == ordinary["hostid"]
                && row["key_"].as_str().is_some_and(|key| !key.is_empty())
                && row["units"].is_string()
                && value_as_i64(&row["value_type"]) == Some(3)
                && value_as_i64(&row["status"]) == Some(0)
                && value_as_i64(&row["flags"]) == Some(4)
        })
    };
    let (Some(before), Some(after)) = (get(source, ordinary_source), get(target, ordinary_target))
    else {
        return Ok(None);
    };
    if rows.len() != 2 || before["units"] != after["units"] {
        return Ok(None);
    }
    let Some(origin) = current_lld_origin(reader, before).await? else {
        return Ok(None);
    };
    if current_lld_origin(reader, after).await?.as_ref() != Some(&origin) {
        return Ok(None);
    }
    let Some(hostid) = after["hostid"].as_str() else {
        return Ok(None);
    };
    let visible = reader.portable_host_items(hostid).await?;
    if visible.truncated
        || visible.objects.iter().any(|item| {
            item.flags == ItemFlags::Discovered
                && (item.enabled.is_none()
                    || item
                        .discovery
                        .as_ref()
                        .is_none_or(|link| link.currently_discovered.is_none()))
        })
    {
        return Ok(None);
    }
    let parent = &after["discoveryData"]["parent_itemid"];
    let mut candidates: Vec<_> = visible
        .objects
        .into_iter()
        .filter(|item| {
            item.flags == ItemFlags::Discovered
                && item.enabled == Some(true)
                && item.discovery.as_ref().is_some_and(|link| {
                    parent.as_str() == Some(link.parent_itemid.as_str())
                        && link.currently_discovered == Some(true)
                })
        })
        .collect();
    candidates.sort_by(|left, right| left.itemid.cmp(&right.itemid));
    if candidates.len() > MAX_REPLACE_HOST_LLD_CANDIDATES
        || candidates
            .windows(2)
            .any(|rows| rows[0].itemid == rows[1].itemid)
        || !candidates.iter().any(|item| item.itemid == target)
    {
        return Ok(None);
    }
    Ok(Some(LldEvidence {
        origin,
        items: [before.clone(), after.clone()],
        candidates,
    }))
}

/// Changed item references move after the unchanged fields on 8.0.0.
/// Changed string host selectors keep their position.
pub(super) fn project_readback(
    raw: &mut Value,
    replacements: &[WidgetFieldReplacement],
) -> Option<()> {
    for patch in replacements {
        let widget = raw["pages"][patch.page_index]["widgets"]
            .as_array_mut()?
            .iter_mut()
            .find(|widget| widget["widgetid"] == patch.widgetid)?;
        let fields = widget["fields"].as_array_mut()?;
        let position = fields
            .iter()
            .position(|field| field["name"] == patch.field_name)?;
        if value_as_i64(&fields[position]["type"]) == Some(4) {
            let changed = fields.remove(position);
            fields.push(changed);
        }
    }
    Some(())
}

async fn read_pattern_evidence(
    reader: &ReadOnlyGateway,
    widget: &Value,
    target: &str,
    pairs: [(&str, &str); 3],
) -> Result<Option<PatternUpdateEvidence>> {
    let mut fields: Vec<_> = rows(&widget["fields"])
        .iter()
        .filter(|row| row["name"] != "reference")
        .cloned()
        .collect();
    let Some(before_spec) = PatternSpec::from_fields(&fields, 0) else {
        return Ok(None);
    };
    let Some(host) = fields.iter_mut().find(|row| row["name"] == "ds.0.hosts.0") else {
        return Ok(None);
    };
    host["value"] = json!(target);
    let Some(after_spec) = PatternSpec::from_fields(&fields, 0) else {
        return Ok(None);
    };
    let Some(before) = read_clone_inputs(reader, &before_spec).await? else {
        return Ok(None);
    };
    let Some(after) = read_clone_inputs(reader, &after_spec).await? else {
        return Ok(None);
    };
    let [source, target, stationary] = pairs;
    let Some(outside) =
        before.replacement_outside_matches(&after, [source, stationary], [target, stationary])
    else {
        return Ok(None);
    };
    Ok(Some(PatternUpdateEvidence {
        before,
        after,
        outside,
    }))
}

impl ReadWriteGateway {
    /// Binds the verified 8.0.0 operation to a fresh owned dashboard, the
    /// signed-in role and all item references required by the shape. The
    /// application still has to build and confirm its ordinary `VerifiedPlan`.
    pub async fn verified_item_update_context(
        &self,
        source: &VersionedDashboardSnapshot,
        replacement: &WidgetFieldReplacement,
    ) -> Result<Option<PublicationContext>> {
        self.verified_replace_host_context(source, std::slice::from_ref(replacement))
            .await
    }

    /// Bind one of the proven 8.0.0 shapes to the live role, complete item
    /// visibility and editable source. It grants no general write authority.
    pub async fn verified_replace_host_context(
        &self,
        source: &VersionedDashboardSnapshot,
        replacements: &[WidgetFieldReplacement],
    ) -> Result<Option<PublicationContext>> {
        if self.capabilities().version != *source.server_version()
            || source.raw_api_document()["userid"].as_str()
                != Some(self.reader().identity().account_userid())
        {
            return Ok(None);
        }
        let Some(evidence) = self
            .eight_zero_item_update_evidence(source, replacements)
            .await?
        else {
            return Ok(None);
        };
        let fresh = self
            .reader()
            .dashboard_snapshot(source.connection_id(), source.dashboard_id())
            .await?;
        if fresh.is_none_or(|fresh| fresh.fingerprint() != source.fingerprint()) {
            return Ok(None);
        }
        let mut context = self.publication_context();
        context.scoped_item_update = Some(ScopedItemUpdate {
            source_fingerprint: source.fingerprint().to_owned(),
            replacements: replacements.to_vec(),
            evidence,
        });
        Ok(Some(context))
    }

    pub(super) async fn eight_zero_item_update_evidence(
        &self,
        source: &VersionedDashboardSnapshot,
        replacements: &[WidgetFieldReplacement],
    ) -> Result<Option<ItemUpdateEvidence>> {
        let Some(shape) = contract_shape(source, replacements) else {
            return Ok(None);
        };
        let replacement = &replacements[0];
        let reader = self.reader();
        let read_role = || async {
            match shape {
                Shape::LinkedGraphs {
                    provider: Provider::Pattern { .. },
                    ..
                } => reader.eight_zero_pattern_create_role().await,
                _ => reader.eight_zero_update_role().await,
            }
        };
        let Some(roleid) = read_role().await? else {
            return Ok(None);
        };
        let mut targets = vec![
            replacement.expected_value.as_str(),
            replacement.replacement_value.as_str(),
        ];
        let mut output = vec!["itemid", "hostid", "key_", "value_type", "status", "flags"];
        let graph = !matches!(shape, Shape::Item);
        match shape {
            Shape::Item => {}
            Shape::ExplicitGraph { stationary } => targets.extend(stationary.into_iter().flatten()),
            Shape::LinkedGraphs { stationary, .. } => targets.push(stationary),
        }
        if graph {
            output.push("units");
        }
        let mut items = self
            .reader()
            .read_inventory_rows(
                ReadMethod::ItemGet,
                json!({
                    "itemids": targets,
                    "output": output
                }),
            )
            .await?;
        items.sort_by(|a, b| a["itemid"].as_str().cmp(&b["itemid"].as_str()));
        targets.sort_unstable();
        let has_item = |item: &Value, id: &str| {
            item["itemid"].as_str() == Some(id)
                && item["hostid"].as_str().is_some_and(is_zabbix_id)
                && item["key_"].as_str().is_some_and(|key| !key.is_empty())
                && value_as_i64(&item["value_type"]) == Some(3)
                && value_as_i64(&item["status"]) == Some(0)
                && value_as_i64(&item["flags"]) == Some(0)
        };
        if items.len() != targets.len()
            || items
                .iter()
                .zip(targets)
                .any(|(item, id)| !has_item(item, id))
            || items.iter().any(|item| item["key_"] != items[0]["key_"])
            || (graph
                && items
                    .iter()
                    .any(|item| !item["units"].is_string() || item["units"] != items[0]["units"]))
        {
            return Ok(None);
        }
        let mut hostids: Vec<_> = items
            .iter()
            .filter_map(|item| item["hostid"].as_str())
            .collect();
        hostids.sort_unstable();
        if hostids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Ok(None);
        }
        let mut hosts = Vec::new();
        if graph {
            hosts = self
                .reader()
                .read_inventory_rows(
                    ReadMethod::HostGet,
                    json!({"hostids":hostids,"output":["hostid","status"]}),
                )
                .await?;
            hosts.sort_by(|left, right| left["hostid"].as_str().cmp(&right["hostid"].as_str()));
            if hosts.len() != hostids.len()
                || hosts.iter().zip(hostids).any(|(host, id)| {
                    host["hostid"].as_str() != Some(id) || value_as_i64(&host["status"]) != Some(0)
                })
            {
                return Ok(None);
            }
        }
        let lld = if let Shape::LinkedGraphs {
            provider: Provider::Lld { source, target },
            ..
        } = shape
        {
            let find = |id: &str| items.iter().find(|item| item["itemid"] == id);
            let (Some(ordinary_source), Some(ordinary_target)) = (
                find(&replacement.expected_value),
                find(&replacement.replacement_value),
            ) else {
                return Ok(None);
            };
            let Some(evidence) = read_lld_evidence(
                self.reader(),
                source,
                target,
                ordinary_source,
                ordinary_target,
            )
            .await?
            else {
                return Ok(None);
            };
            Some(evidence)
        } else {
            None
        };
        let patterns = if let Shape::LinkedGraphs {
            stationary,
            provider: Provider::Pattern { widget, target },
        } = shape
        {
            let pair = |id: &str| {
                let row = items.iter().find(|row| row["itemid"] == id)?;
                Some((row["hostid"].as_str()?, row["itemid"].as_str()?))
            };
            let (Some(source), Some(destination), Some(stationary)) = (
                pair(&replacement.expected_value),
                pair(&replacement.replacement_value),
                pair(stationary),
            ) else {
                return Ok(None);
            };
            let Some(evidence) = read_pattern_evidence(
                self.reader(),
                widget,
                target,
                [source, destination, stationary],
            )
            .await?
            else {
                return Ok(None);
            };
            if evidence.before.role_id() != roleid {
                return Ok(None);
            }
            Some(Arc::new(evidence))
        } else {
            None
        };
        if !self.dashboard_is_editable(source.dashboard_id()).await?
            || read_role().await?.as_deref() != Some(&roleid)
        {
            return Ok(None);
        }
        Ok(Some(ItemUpdateEvidence {
            roleid,
            items,
            hosts,
            lld,
            patterns,
        }))
    }
}
