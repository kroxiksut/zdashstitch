// SPDX-License-Identifier: MIT OR Apache-2.0

//! Read-only reads for copying a dashboard from the server (P2-05): a bounded
//! dashboard list, one dashboard as a [`VersionedDashboardSnapshot`] and the
//! metadata of referenced items (decision I-4). The Zabbix 7.4 request and
//! response shapes stay in this adapter; the application only sees typed
//! results. Every call goes through the P1-IN-08 read allowlist.

use serde_json::{Value, json};

use crate::gateway::ReadMethod;
use crate::{ReadOnlyGateway, Result, VersionedDashboardSnapshot, ZabbixError};

/// Largest number of dashboards one list read returns.
pub const MAX_DASHBOARD_LIST_LIMIT: usize = 1_000;
/// Longest accepted name search, in Unicode scalar values.
pub const MAX_DASHBOARD_SEARCH_CHARS: usize = 255;
/// Item IDs sent in one `item.get` call.
pub const ITEM_METADATA_BATCH: usize = 500;
/// Longest accepted Zabbix object ID, in decimal digits (64-bit unsigned).
const MAX_ID_DIGITS: usize = 20;

/// One dashboard of a list read. Names are server data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DashboardListing {
    pub dashboardid: String,
    pub name: String,
    /// Owner `userid`; compare with [`crate::ServerIdentity::account_userid`].
    pub owner_userid: String,
    /// `private = 1` in Zabbix: shared only with the listed users and groups.
    pub private: bool,
    pub pages: Vec<DashboardPageListing>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DashboardPageListing {
    pub dashboard_pageid: String,
    pub name: String,
    pub widgets: Vec<ListedWidget>,
}

/// Widget type and field signature, enough to ask the capability registry
/// whether ZDashstitch can edit it. Field values are not kept.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListedWidget {
    pub widget_type: String,
    pub fields: Vec<ListedField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListedField {
    pub name: String,
    /// `ZBX_WIDGET_FIELD_TYPE_*`; `None` when the server sent no number.
    pub field_type: Option<u16>,
}

/// Result of a list read. Zabbix has no offset paging, so `truncated` says
/// that more dashboards matched than `limit` (ordered by `dashboardid`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DashboardListPage {
    pub dashboards: Vec<DashboardListing>,
    pub truncated: bool,
}

/// Item properties kept locally with a copy from the server (I-4).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemMetadata {
    pub itemid: String,
    /// Zabbix `value_type`: 0 float, 1 character, 2 log, 3 unsigned, 4 text, 5 binary.
    pub value_type: u8,
    pub units: String,
    pub value_map: Option<ValueMap>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueMap {
    pub valuemapid: String,
    pub name: String,
    pub mappings: Vec<ValueMapping>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueMapping {
    /// Zabbix mapping `type` (0 equals, 1 ≥, 2 ≤, 3 range, 4 regexp, 5 default).
    pub mapping_type: u8,
    pub value: String,
    pub newvalue: String,
}

/// Whether `value` is a Zabbix object ID: 1–20 ASCII digits.
#[must_use]
pub fn is_zabbix_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_ID_DIGITS && value.bytes().all(|b| b.is_ascii_digit())
}

pub(crate) fn snapshot_read_params() -> Value {
    json!({
        "output": "extend",
        "selectPages": "extend",
        "selectUsers": "extend",
        "selectUserGroups": "extend",
        "sortfield": "dashboardid"
    })
}

impl ReadOnlyGateway {
    /// Lists visible dashboards, optionally filtered by a name substring. An
    /// empty result does not mean the server has no dashboards: Zabbix
    /// returns less data instead of an error when permissions hide objects
    /// (P1-IN-08).
    pub async fn list_dashboards(
        &self,
        search: Option<&str>,
        limit: usize,
    ) -> Result<DashboardListPage> {
        if !(1..=MAX_DASHBOARD_LIST_LIMIT).contains(&limit) {
            return Err(ZabbixError::SnapshotShape(
                "dashboard list limit must be 1..=1000".to_owned(),
            ));
        }
        let mut params = json!({
            "output": ["dashboardid", "name", "userid", "private"],
            "selectPages": "extend",
            "sortfield": "dashboardid",
            "limit": limit + 1
        });
        if let Some(search) = search {
            if search.chars().count() > MAX_DASHBOARD_SEARCH_CHARS
                || search.chars().any(char::is_control)
            {
                return Err(ZabbixError::SnapshotShape(
                    "dashboard search is too long or has control characters".to_owned(),
                ));
            }
            if !search.is_empty() {
                params["search"] = json!({ "name": search });
            }
        }
        let raw: Vec<Value> = self
            .session
            .transport
            .read(ReadMethod::DashboardGet, params, &self.session.token)
            .await?;
        let truncated = raw.len() > limit;
        let dashboards = raw
            .iter()
            .take(limit)
            .map(parse_listing)
            .collect::<Result<Vec<_>>>()?;
        Ok(DashboardListPage {
            dashboards,
            truncated,
        })
    }

    /// Reads one dashboard with pages, widgets, users and user groups.
    /// `None` means the dashboard is missing or not visible to this account;
    /// Zabbix does not distinguish the two (P1-IN-08).
    pub async fn dashboard_snapshot(
        &self,
        connection_id: &str,
        dashboardid: &str,
    ) -> Result<Option<VersionedDashboardSnapshot>> {
        if !is_zabbix_id(dashboardid) {
            return Err(ZabbixError::SnapshotShape(
                "dashboardid must be a Zabbix ID".to_owned(),
            ));
        }
        if connection_id.is_empty() || connection_id.len() > 128 {
            return Err(ZabbixError::SnapshotShape(
                "connection_id must contain 1..=128 bytes".to_owned(),
            ));
        }
        let mut params = snapshot_read_params();
        params["dashboardids"] = json!([dashboardid]);
        let mut raw: Vec<Value> = self
            .session
            .transport
            .read(ReadMethod::DashboardGet, params, &self.session.token)
            .await?;
        let Some(document) = raw.pop() else {
            return Ok(None);
        };
        if !raw.is_empty()
            || document.get("dashboardid").and_then(Value::as_str) != Some(dashboardid)
        {
            return Err(ZabbixError::SnapshotShape(
                "dashboard.get returned another dashboard".to_owned(),
            ));
        }
        VersionedDashboardSnapshot::from_adapter_raw(
            connection_id.to_owned(),
            self.session.capabilities.version.clone(),
            document,
        )
        .map(Some)
    }

    /// Reads value type, units and value map of the given items in batches
    /// of [`ITEM_METADATA_BATCH`]. Items the account cannot see are simply
    /// absent from the result. Never reads item values.
    pub async fn item_metadata(&self, itemids: &[String]) -> Result<Vec<ItemMetadata>> {
        if itemids.iter().any(|itemid| !is_zabbix_id(itemid)) {
            return Err(ZabbixError::SnapshotShape(
                "itemid must be a Zabbix ID".to_owned(),
            ));
        }
        let mut metadata = Vec::with_capacity(itemids.len());
        for batch in itemids.chunks(ITEM_METADATA_BATCH) {
            let raw: Vec<Value> = self
                .session
                .transport
                .read(
                    ReadMethod::ItemGet,
                    json!({
                        "output": ["itemid", "value_type", "units", "valuemapid"],
                        "selectValueMap": ["valuemapid", "name", "mappings"],
                        "itemids": batch,
                        "webitems": true
                    }),
                    &self.session.token,
                )
                .await?;
            for item in &raw {
                metadata.push(parse_item_metadata(item)?);
            }
        }
        Ok(metadata)
    }
}

fn string_field<'a>(value: &'a Value, key: &str, context: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ZabbixError::SnapshotShape(format!("{context} has no string {key}")))
}

fn small_number(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn parse_listing(dashboard: &Value) -> Result<DashboardListing> {
    let pages = dashboard
        .get("pages")
        .and_then(Value::as_array)
        .ok_or_else(|| ZabbixError::SnapshotShape("dashboard has no array pages".to_owned()))?;
    Ok(DashboardListing {
        dashboardid: string_field(dashboard, "dashboardid", "dashboard")?.to_owned(),
        name: string_field(dashboard, "name", "dashboard")?.to_owned(),
        owner_userid: string_field(dashboard, "userid", "dashboard")?.to_owned(),
        private: small_number(dashboard.get("private")) != Some(0),
        pages: pages
            .iter()
            .map(parse_page_listing)
            .collect::<Result<_>>()?,
    })
}

fn parse_page_listing(page: &Value) -> Result<DashboardPageListing> {
    let widgets = page
        .get("widgets")
        .and_then(Value::as_array)
        .ok_or_else(|| ZabbixError::SnapshotShape("dashboard page has no array widgets".into()))?;
    Ok(DashboardPageListing {
        dashboard_pageid: string_field(page, "dashboard_pageid", "dashboard page")?.to_owned(),
        name: page
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        widgets: widgets
            .iter()
            .map(|widget| {
                let fields = widget
                    .get("fields")
                    .and_then(Value::as_array)
                    .map(|fields| {
                        fields
                            .iter()
                            .map(|field| ListedField {
                                name: field
                                    .get("name")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_owned(),
                                field_type: small_number(field.get("type"))
                                    .and_then(|number| u16::try_from(number).ok()),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(ListedWidget {
                    widget_type: string_field(widget, "type", "dashboard widget")?.to_owned(),
                    fields,
                })
            })
            .collect::<Result<_>>()?,
    })
}

fn parse_item_metadata(item: &Value) -> Result<ItemMetadata> {
    let itemid = string_field(item, "itemid", "item")?;
    if !is_zabbix_id(itemid) {
        return Err(ZabbixError::SnapshotShape(
            "item.get returned an invalid itemid".to_owned(),
        ));
    }
    let value_type = small_number(item.get("value_type"))
        .and_then(|number| u8::try_from(number).ok())
        .filter(|number| *number <= 5)
        .ok_or_else(|| ZabbixError::SnapshotShape("item has no valid value_type".to_owned()))?;
    let value_map = match item.get("valuemap") {
        Some(map @ Value::Object(_)) if map.get("name").is_some() => Some(parse_value_map(map)?),
        _ => None,
    };
    Ok(ItemMetadata {
        itemid: itemid.to_owned(),
        value_type,
        units: item
            .get("units")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        value_map,
    })
}

fn parse_value_map(map: &Value) -> Result<ValueMap> {
    let mappings = map
        .get("mappings")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    Ok(ValueMap {
        valuemapid: map
            .get("valuemapid")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        name: string_field(map, "name", "value map")?.to_owned(),
        mappings: mappings
            .iter()
            .map(|mapping| {
                Ok(ValueMapping {
                    mapping_type: small_number(mapping.get("type"))
                        .and_then(|number| u8::try_from(number).ok())
                        .unwrap_or(0),
                    value: mapping
                        .get("value")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    newvalue: string_field(mapping, "newvalue", "value mapping")?.to_owned(),
                })
            })
            .collect::<Result<_>>()?,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{is_zabbix_id, parse_item_metadata, parse_listing};

    #[test]
    fn zabbix_ids_are_bounded_digit_strings() {
        assert!(is_zabbix_id("1"));
        assert!(is_zabbix_id("18446744073709551615"));
        for invalid in ["", "-1", "1a", "../1", "123456789012345678901", " 1"] {
            assert!(!is_zabbix_id(invalid), "{invalid:?}");
        }
    }

    #[test]
    fn listing_keeps_types_and_field_signatures_only() {
        let listing = parse_listing(&json!({
            "dashboardid": "7", "name": "Synthetic", "userid": "3", "private": "0",
            "pages": [{"dashboard_pageid": "70", "name": "", "widgets": [
                {"type": "item", "fields": [{"type": "4", "name": "itemid", "value": "1"}]}
            ]}]
        }))
        .unwrap_or_else(|error| panic!("listing: {error}"));
        assert!(!listing.private);
        assert_eq!(listing.pages[0].widgets[0].fields[0].field_type, Some(4));
        assert_eq!(listing.pages[0].widgets[0].fields[0].name, "itemid");
    }

    #[test]
    fn item_metadata_accepts_missing_value_map_and_rejects_bad_type() {
        let item = parse_item_metadata(&json!({
            "itemid": "5", "value_type": "3", "units": "B", "valuemapid": "0", "valuemap": []
        }))
        .unwrap_or_else(|error| panic!("item: {error}"));
        assert_eq!(item.value_map, None);
        assert_eq!(item.value_type, 3);
        assert!(parse_item_metadata(&json!({"itemid": "5", "value_type": "9"})).is_err());
        let mapped = parse_item_metadata(&json!({
            "itemid": "6", "value_type": "3", "units": "",
            "valuemap": {"valuemapid": "2", "name": "Service state",
                         "mappings": [{"type": "0", "value": "1", "newvalue": "Up"}]}
        }))
        .unwrap_or_else(|error| panic!("mapped: {error}"));
        assert_eq!(
            mapped.value_map.map(|map| map.mappings[0].newvalue.clone()),
            Some("Up".to_owned())
        );
    }
}
