// SPDX-License-Identifier: MIT OR Apache-2.0

//! A scoped append to an owned private dashboard.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    AdapterRegistry, DashboardAccessState, ItemDetails, ItemFlags, MAX_PORTABLE_APPEND_WIDGETS,
    MAX_PORTABLE_GRAPH_PAGES, PortableUpdateReadback, ReadMethod, ReadWriteGateway, Result,
    VersionedDashboardSnapshot, WriteContract, is_zabbix_id,
};

use super::item_update_8_0::{graph_row, is_page, is_widget};
use super::style_8_0::color;
use super::{VerifiedDashboardUpdate, WriteRejection, WriteRejectionCode, value_as_i64};

/// Source selector captured when the local slot was explicitly resolved.
/// Kept local. Only `itemid` enters the dashboard update payload.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlueprintItemBinding {
    pub itemid: String,
    pub hostid: String,
    pub key: String,
    pub value_type: u8,
    pub units: String,
}

impl std::fmt::Debug for BlueprintItemBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BlueprintItemBinding")
            .finish_non_exhaustive()
    }
}

pub(super) struct BlueprintAppend {
    widget: Value,
    pageid: String,
    bindings: Vec<BlueprintItemBinding>,
    role: String,
    pub(super) readback: PortableUpdateReadback,
    attempted: AtomicBool,
}

impl BlueprintAppend {
    pub(super) fn reserve_send(&self) -> bool {
        !self.attempted.swap(true, Ordering::AcqRel)
    }

    pub(super) fn params(&self, current: &VersionedDashboardSnapshot) -> Option<Value> {
        let target = target_shape(current, &self.pageid)?;
        let mut pages = Vec::with_capacity(target.pages.len());
        for (index, page) in target.pages.iter().enumerate() {
            let mut kept = json!({"dashboard_pageid": page["dashboard_pageid"]});
            if index == target.page {
                let mut widgets: Vec<_> = page["widgets"]
                    .as_array()?
                    .iter()
                    .map(|widget| json!({"widgetid": widget["widgetid"]}))
                    .collect();
                widgets.push(self.widget.clone());
                kept["widgets"] = json!(widgets);
            }
            pages.push(kept);
        }
        Some(json!({
            "dashboardid": current.dashboard_id(),
            "pages": pages
        }))
    }

    pub(super) async fn recheck(
        &self,
        writer: &ReadWriteGateway,
        dashboardid: &str,
    ) -> Result<bool> {
        let reader = writer.reader();
        let hosts: BTreeSet<_> = self
            .bindings
            .iter()
            .map(|binding| &binding.hostid)
            .collect();
        let host_rows = reader
            .read_inventory_rows(
                ReadMethod::HostGet,
                json!({"hostids": hosts, "output": ["hostid", "status"]}),
            )
            .await?;
        if host_rows.len() != hosts.len()
            || host_rows
                .iter()
                .any(|row| value_as_i64(&row["status"]) != Some(0))
            || host_rows
                .iter()
                .filter_map(|row| row["hostid"].as_str())
                .collect::<BTreeSet<_>>()
                != hosts.iter().map(|id| id.as_str()).collect()
        {
            return Ok(false);
        }
        for host in hosts {
            let result = reader.portable_host_items(host).await?;
            if result.truncated {
                return Ok(false);
            }
            for binding in self
                .bindings
                .iter()
                .filter(|binding| &binding.hostid == host)
            {
                if !matches_binding(&result.objects, binding) {
                    return Ok(false);
                }
            }
        }
        Ok(writer.dashboard_is_editable(dashboardid).await?
            && reader.eight_zero_update_role().await?.as_deref() == Some(&self.role))
    }
}

fn matches_binding(items: &[ItemDetails], binding: &BlueprintItemBinding) -> bool {
    let mut candidates = items.iter().filter(|item| item.key == binding.key);
    candidates.next().is_some_and(|item| {
        item.itemid == binding.itemid
            && item.hostid == binding.hostid
            && item.value_type == binding.value_type
            && item.units == binding.units
            && item.flags == ItemFlags::Plain
            && item.enabled == Some(true)
    }) && candidates.next().is_none()
}

struct Target<'a> {
    pages: &'a [Value],
    page: usize,
    widgets: &'a [Value],
    widget_ids: BTreeSet<&'a str>,
}

fn target_shape<'a>(source: &'a VersionedDashboardSnapshot, pageid: &str) -> Option<Target<'a>> {
    let raw = source.raw_api_document();
    let access = DashboardAccessState::read(raw)?;
    let pages = raw["pages"].as_array()?;
    if !crate::capabilities::is_eight_zero_item_update_version(source.server_version())
        || !access.private
        || !access.users.is_empty()
        || !access.groups.is_empty()
        || !(1..=MAX_PORTABLE_GRAPH_PAGES).contains(&pages.len())
    {
        return None;
    }
    let mut page_ids = BTreeSet::new();
    let mut widget_ids = BTreeSet::new();
    let mut target = None;
    for (index, page) in pages.iter().enumerate() {
        let id = page["dashboard_pageid"].as_str()?;
        let widgets = page["widgets"].as_array()?;
        if !is_page(page) || !page_ids.insert(id) || widgets.len() > MAX_PORTABLE_APPEND_WIDGETS {
            return None;
        }
        if id == pageid && widgets.len() < MAX_PORTABLE_APPEND_WIDGETS {
            target = Some((index, widgets.as_slice()));
        }
        for widget in widgets {
            let id = widget["widgetid"].as_str()?;
            if id.starts_with('0')
                || !widget_ids.insert(id)
                || !existing_widget(widget)
                || rectangle(widget).is_none()
            {
                return None;
            }
        }
    }
    let (page, widgets) = target?;
    Some(Target {
        pages,
        page,
        widgets,
        widget_ids,
    })
}

fn existing_widget(widget: &Value) -> bool {
    let Some(kind) = widget["type"].as_str() else {
        return false;
    };
    if !is_widget(widget, kind) || value_as_i64(&widget["view_mode"]) != Some(0) {
        return false;
    }
    let Some(fields) = widget["fields"].as_array() else {
        return false;
    };
    match kind {
        "clock" => {
            fields.is_empty()
                || (fields.len() == 1 && graph_row(widget, "clock_type", 0) == Some("1"))
        }
        "item" | "svggraph" => widget_items(widget).is_some(),
        _ => false,
    }
}

fn rectangle(widget: &Value) -> Option<[i64; 4]> {
    let [x, y, w, h] = ["x", "y", "width", "height"].map(|key| value_as_i64(&widget[key]));
    let (x, y, w, h) = (x?, y?, w?, h?);
    let (right, bottom) = (x.checked_add(w)?, y.checked_add(h)?);
    (x >= 0 && y >= 0 && w > 0 && h > 0 && right <= 72 && bottom <= 64)
        .then_some([x, y, right, bottom])
}

fn shape(
    widget: &Value,
    existing: &[Value],
    bindings: &[BlueprintItemBinding],
) -> Option<WriteContract> {
    let object = widget.as_object()?;
    if object.len() != 8
        || !crate::is_valid_dashboard_name(widget["name"].as_str()?)
        || widget["view_mode"] != "0"
        || ["x", "y", "width", "height"].iter().any(|key| {
            widget[key].as_str().is_none_or(|text| {
                value_as_i64(&widget[key]).is_none_or(|number| text != number.to_string())
            })
        })
        || widget["fields"]
            .as_array()?
            .iter()
            .any(|field| !matches!(field["type"].as_str(), Some("0" | "1" | "4")))
    {
        return None;
    }
    let [x, y, r, b] = rectangle(widget)?;
    for neighbor in existing {
        let [left, top, right, bottom] = rectangle(neighbor)?;
        if r > left && right > x && b > top && bottom > y {
            return None;
        }
    }
    let items = widget_items(widget)?;
    if bindings.len() != items.len()
        || !bindings.iter().zip(items).all(|(binding, item)| {
            item == binding.itemid
                && is_zabbix_id(&binding.hostid)
                && !binding.key.is_empty()
                && matches!(binding.value_type, 0 | 3)
        })
    {
        return None;
    }
    match widget["type"].as_str()? {
        "item" => Some(WriteContract::AppendBlueprintItem),
        "svggraph" => Some(WriteContract::AppendBlueprintGraph),
        _ => None,
    }
}

fn widget_items(widget: &Value) -> Option<Vec<&str>> {
    let count = widget["fields"].as_array()?.len();
    let item_id = |name: &str| {
        graph_row(widget, name, 4).filter(|id| is_zabbix_id(id) && !id.starts_with('0'))
    };
    match widget["type"].as_str()? {
        "item"
            if count == 1
                || (count == 3
                    && graph_row(widget, "show.0", 0) == Some("2")
                    && graph_row(widget, "bg_color", 1)
                        .is_some_and(|value| value.is_empty() || color(value))) =>
        {
            Some(vec![item_id("itemid.0")?])
        }
        "svggraph"
            if matches!(count, 3 | 5 | 7 | 9)
                && graph_row(widget, "ds.0.dataset_type", 0) == Some("0") =>
        {
            let mut items = Vec::new();
            for slot in 0..(count - 1) / 2 {
                let id = item_id(&format!("ds.0.itemids.{slot}"))?;
                if items.contains(&id)
                    || !graph_row(widget, &format!("ds.0.color.{slot}"), 1).is_some_and(color)
                {
                    return None;
                }
                items.push(id);
            }
            Some(items)
        }
        _ => None,
    }
}

/// Local precheck for offering a publication preview. Live authority and
/// binding checks are still required before planning and before sending.
pub fn can_preview_blueprint_append(
    source: &VersionedDashboardSnapshot,
    pageid: &str,
    widget: &Value,
    bindings: &[BlueprintItemBinding],
) -> bool {
    let Some(target) = target_shape(source, pageid) else {
        return false;
    };
    shape(widget, target.widgets, bindings).is_some_and(|contract| {
        AdapterRegistry::resolve_server(source.server_version().clone())
            .is_ok_and(|profile| profile.dashboard_write_contracts.is_verified(contract))
    })
}

impl ReadWriteGateway {
    /// Independently verifies the append shape, live bindings and dashboard
    /// permissions. The result still requires the application's confirmation.
    pub async fn verify_blueprint_append(
        &self,
        source: &VersionedDashboardSnapshot,
        pageid: &str,
        widget: Value,
        bindings: Vec<BlueprintItemBinding>,
        local_fingerprint: String,
    ) -> Result<std::result::Result<VerifiedDashboardUpdate, WriteRejection>> {
        let refusal = |code, step| Err(WriteRejection::new(code).with("step", step));
        let invalid = |step| refusal(WriteRejectionCode::SnapshotShapeInvalid, step);
        let context = self.publication_context();
        let Some(target) = target_shape(source, pageid) else {
            return Ok(invalid("blueprint_target"));
        };
        let Some(contract) = shape(&widget, target.widgets, &bindings) else {
            return Ok(invalid("blueprint_widget"));
        };
        let reader = self.reader();
        if !context
            .capabilities
            .dashboard_write_contracts
            .is_verified(contract)
        {
            return Ok(Err(WriteRejection::new(
                WriteRejectionCode::ContractUnverified,
            )
            .with("contract", contract.as_str())));
        }
        if source.raw_api_document()["userid"].as_str() != Some(reader.identity().account_userid())
        {
            return Ok(refusal(
                WriteRejectionCode::AuthorityOffline,
                "blueprint_owner",
            ));
        }
        let Some(role) = reader.eight_zero_update_role().await? else {
            return Ok(refusal(
                WriteRejectionCode::AuthorityOffline,
                "blueprint_role",
            ));
        };
        let readback = PortableUpdateReadback::AppendedBlueprintAt {
            page: target.page,
            widget: target.widgets.len(),
        };
        let append = BlueprintAppend {
            widget,
            pageid: pageid.to_owned(),
            bindings,
            role,
            readback: readback.clone(),
            attempted: AtomicBool::new(false),
        };
        if !append.recheck(self, source.dashboard_id()).await? {
            return Ok(refusal(
                WriteRejectionCode::SourceValueStale,
                "blueprint_binding_or_permission",
            ));
        }
        let mut raw = source.raw_api_document().clone();
        let mut added = append.widget.clone();
        let Some(id) = (1..=target.widget_ids.len() + 1)
            .map(|id| id.to_string())
            .find(|id| !target.widget_ids.contains(id.as_str()))
        else {
            return Ok(invalid("blueprint_readback"));
        };
        added["widgetid"] = json!(id);
        let Some(widgets) = raw["pages"][target.page]["widgets"].as_array_mut() else {
            return Ok(invalid("blueprint_readback"));
        };
        widgets.push(added);
        let expected = VersionedDashboardSnapshot::from_adapter_raw(
            source.connection_id().to_owned(),
            source.server_version().clone(),
            raw,
        )?;
        let Some(expected_fingerprint) = readback.fingerprint(&expected) else {
            return Ok(invalid("blueprint_readback"));
        };
        Ok(Ok(VerifiedDashboardUpdate {
            connection_id: source.connection_id().to_owned(),
            server_version: source.server_version().clone(),
            dashboard_id: source.dashboard_id().to_owned(),
            source_fingerprint: source.fingerprint().to_owned(),
            expected_fingerprint,
            local_fingerprint,
            authority_fingerprint: context.binding_fingerprint,
            item_update_evidence: None,
            blueprint_append: Some(append),
            replacements: Vec::new(),
            replacement_count: 1,
            required_contracts: BTreeSet::from([contract]),
            changed_pages: vec![target.page],
            kinds: BTreeMap::from([(contract, 1)]),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> BlueprintItemBinding {
        BlueprintItemBinding {
            itemid: "7".to_owned(),
            hostid: "6".to_owned(),
            key: "example.blueprint".to_owned(),
            value_type: 3,
            units: "B".to_owned(),
        }
    }

    #[test]
    fn append_refuses_changes_to_the_stored_binding_and_duplicate_keys() {
        let binding = binding();
        let item = ItemDetails {
            itemid: binding.itemid.clone(),
            hostid: binding.hostid.clone(),
            key: binding.key.clone(),
            name: "Synthetic item".to_owned(),
            value_type: binding.value_type,
            units: binding.units.clone(),
            flags: ItemFlags::Plain,
            enabled: Some(true),
            templateid: None,
            discovery: None,
            owner_name: None,
        };
        assert!(matches_binding(std::slice::from_ref(&item), &binding));
        for stale in [
            ItemDetails {
                itemid: "8".into(),
                ..item.clone()
            },
            ItemDetails {
                hostid: "8".into(),
                ..item.clone()
            },
            ItemDetails {
                key: "example.other".into(),
                ..item.clone()
            },
            ItemDetails {
                value_type: 0,
                ..item.clone()
            },
            ItemDetails {
                units: "%".into(),
                ..item.clone()
            },
            ItemDetails {
                flags: ItemFlags::Discovered,
                ..item.clone()
            },
            ItemDetails {
                enabled: None,
                ..item.clone()
            },
            ItemDetails {
                enabled: Some(false),
                ..item.clone()
            },
        ] {
            assert!(!matches_binding(&[stale], &binding));
        }
        assert!(!matches_binding(
            &[
                item.clone(),
                ItemDetails {
                    itemid: "8".into(),
                    ..item
                }
            ],
            &binding
        ));
    }

    #[test]
    fn append_rejects_unknown_fields_source_shapes_and_overlaps() {
        let clock = json!({"x":"0","y":"0","width":"20","height":"3"});
        let widget = json!({
            "type":"item","name":"Metric","view_mode":"0","x":"0","y":"3","width":"21","height":"3",
            "fields":[{"name":"itemid.0","type":"4","value":"7"}],
        });
        let source = [binding()];
        assert_eq!(
            shape(&widget, std::slice::from_ref(&clock), &source),
            Some(WriteContract::AppendBlueprintItem)
        );
        for (pointer, replacement) in [
            ("/x", json!("71")),
            ("/y", json!("2")),
            ("/fields/0/name", json!("itemid._reference")),
            ("/fields/0/type", json!("3")),
            ("/fields/0/value", json!("8")),
            ("/view_mode", json!("1")),
        ] {
            let mut changed = widget.clone();
            *changed
                .pointer_mut(pointer)
                .unwrap_or_else(|| panic!("field")) = replacement;
            assert_eq!(shape(&changed, std::slice::from_ref(&clock), &source), None);
        }
        assert_eq!(shape(&widget, &[clock], &[]), None);
    }

    #[test]
    fn append_refuses_unknown_untouched_widgets_and_shared_dashboards() {
        let base = json!({
            "dashboardid":"10","name":"Synthetic target","private":"1","userid":"11",
            "display_period":"30","auto_start":"1","users":[],"userGroups":[],
            "pages":[
                {"dashboard_pageid":"1","name":"Keep","display_period":"0","widgets":[{
                   "widgetid":"2","type":"clock","name":"Clock","view_mode":"0",
                   "x":"0","y":"0","width":"12","height":"3","fields":[]
                }]},
                {"dashboard_pageid":"2","name":"Target","display_period":"0","widgets":[]}
            ]
        });
        let eligible = |raw| {
            let snapshot = VersionedDashboardSnapshot::from_adapter_raw(
                "synthetic".into(),
                "8.0.0".parse().unwrap_or_else(|_| panic!("version")),
                raw,
            )
            .unwrap_or_else(|_| panic!("snapshot"));
            target_shape(&snapshot, "2").is_some()
        };
        assert!(eligible(base.clone()));
        for (pointer, value) in [
            ("/pages/0/widgets/0/type", json!("unknown")),
            (
                "/pages/0/widgets/0/fields",
                json!([{"name":"other","type":"1","value":"example"}]),
            ),
            ("/pages/0/dashboard_pageid", json!("2")),
            ("/private", json!("0")),
            ("/users", json!([{"userid":"3","permission":"2"}])),
        ] {
            let mut changed = base.clone();
            *changed
                .pointer_mut(pointer)
                .unwrap_or_else(|| panic!("field")) = value;
            assert!(!eligible(changed));
        }
    }

    #[test]
    fn explicit_graph_requires_distinct_contiguous_rows_and_colors() {
        let graph = |count| {
            let mut fields = vec![json!({"name":"ds.0.dataset_type","type":"0","value":"0"})];
            for slot in 0..count {
                fields.extend([
                    json!({"name":format!("ds.0.itemids.{slot}"),"type":"4","value":(slot + 1).to_string()}),
                    json!({"name":format!("ds.0.color.{slot}"),"type":"1","value":"A1B2C3"}),
                ]);
            }
            json!({"type":"svggraph","fields":fields})
        };
        assert_eq!(widget_items(&graph(4)), Some(vec!["1", "2", "3", "4"]));
        assert!(widget_items(&graph(5)).is_none());
        for (pointer, value) in [
            ("/fields/0/value", "1"),
            ("/fields/7/value", "2"),
            ("/fields/7/name", "ds.0.itemids.4"),
            ("/fields/8/name", "ds.1.color.3"),
            ("/fields/8/value", ""),
        ] {
            let mut changed = graph(4);
            *changed
                .pointer_mut(pointer)
                .unwrap_or_else(|| panic!("field")) = json!(value);
            assert!(widget_items(&changed).is_none());
        }
    }

    #[test]
    fn item_background_reset_requires_the_verified_show_mode() {
        let mut item = json!({"type":"item","fields":[
            {"name":"itemid.0","type":"4","value":"7"},
            {"name":"show.0","type":"0","value":"2"},
            {"name":"bg_color","type":"1","value":""}
        ]});
        assert_eq!(widget_items(&item), Some(vec!["7"]));
        item["fields"][1]["value"] = json!("1");
        assert!(widget_items(&item).is_none());
    }
}
