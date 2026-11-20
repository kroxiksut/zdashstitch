// SPDX-License-Identifier: MIT OR Apache-2.0

//! Read-only lookups for the source resolver (P2-02): hosts, items and
//! classic graphs with the evidence fields of the P1-IN-03 status model
//! (`key_`, `value_type`, `units`, `flags`, `templateid`, LLD parent).
//!
//! Every call uses a method of the P1-IN-08 read allowlist (`hostgroup.get`,
//! `host.get`, `item.get`, `graph.get`, `itemprototype.get`,
//! `graphprototype.get`).
//! Prototypes are read only by ID, to find the owner (host or template) of
//! the prototype fields of widget field types 5 and 7 (decision 2026-10-26).
//! Objects hidden by permissions are simply absent from the answers; Zabbix
//! does not tell them apart from missing objects (P1-IN-08).

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::dashboards::is_zabbix_id;
use crate::gateway::ReadMethod;
use crate::{AdapterProfile, ReadOnlyGateway, Result, ZabbixError};

/// IDs or names sent in one lookup call.
pub const LOOKUP_BATCH: usize = 500;
/// Largest accepted per-host item or graph list.
pub const MAX_HOST_OBJECTS: usize = 20_000;
/// Largest page of a host search ([`ReadOnlyGateway::search_hosts`]).
pub const MAX_HOST_SEARCH: usize = 100;
/// Longest host search text, in characters (the Zabbix host name limit).
pub const MAX_HOST_SEARCH_CHARS: usize = 128;
/// Largest page of a host-group search.
pub const MAX_HOST_GROUP_SEARCH: usize = 100;
/// Longest group search text or exact tag component.
pub const MAX_FILTER_CHARS: usize = 255;
pub const MAX_SVGGRAPH_PATTERN_MATCHES: usize = 500;

/// One exact `tag=value` host filter. Several entries are ANDed. Duplicate
/// tag names are refused so Zabbix's AND/OR evaluation cannot weaken AND.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ExactHostTag {
    pub tag: String,
    pub value: String,
}

/// Read-only filters for a target-host picker. A group is optional and
/// singular; choosing more hosts is independent from filtering them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HostSearchFilter {
    pub name: String,
    pub groupid: Option<String>,
    pub tags: Vec<ExactHostTag>,
}

/// A host group visible to the current account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostGroupListing {
    pub groupid: String,
    pub name: String,
}

/// A host as seen by the account. `name` is the visible name, which
/// `svggraph` host patterns are matched against; `host` is the technical
/// name.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostDetails {
    pub hostid: String,
    pub host: String,
    pub name: String,
}

/// A template linked directly to a host (server data).
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateRef {
    pub templateid: String,
    pub name: String,
}

/// A host found by a search, with its host groups and directly linked
/// templates (server data, for choosing a target host).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostListing {
    pub hostid: String,
    pub host: String,
    pub name: String,
    /// Host group names, sorted.
    pub groups: Vec<String>,
    /// Directly linked templates, sorted by name.
    pub templates: Vec<TemplateRef>,
}

/// Zabbix item `flags`: how the item came to exist.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemFlags {
    /// `0`: a plain item (on the host or inherited from a template).
    Plain,
    /// `4`: an instance created by low-level discovery.
    Discovered,
    /// Any other value; `item.get` does not return prototypes or LLD rules.
    Other(u8),
}

impl ItemFlags {
    fn from_number(number: u64) -> Self {
        match number {
            0 => Self::Plain,
            4 => Self::Discovered,
            other => Self::Other(u8::try_from(other).unwrap_or(u8::MAX)),
        }
    }
}

/// Link of a discovered item to its item prototype.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemDiscoveryLink {
    /// Item prototype on the same host.
    pub parent_itemid: String,
    /// Key of the item prototype, with LLD macros.
    pub prototype_key: String,
    /// A fresh discovery status, when the read requested it.
    pub currently_discovered: Option<bool>,
}

/// Item evidence for matching. No values, no history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemDetails {
    pub itemid: String,
    /// Owner host; for a template item, the template.
    pub hostid: String,
    pub key: String,
    pub name: String,
    /// 0 float, 1 character, 2 log, 3 unsigned, 4 text, 5 binary, 6 JSON (8.0).
    pub value_type: u8,
    pub units: String,
    pub flags: ItemFlags,
    /// Whether the item itself is enabled, when the adapter read its status.
    pub enabled: Option<bool>,
    /// Parent template item, `None` for an own or discovered item.
    pub templateid: Option<String>,
    pub discovery: Option<ItemDiscoveryLink>,
    /// Visible name of the owner (host or template), when the read asked for it.
    pub owner_name: Option<String>,
}

/// Account-visible results of a per-host list read. `truncated` means more
/// visible objects matched than the requested limit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostObjects<T> {
    pub objects: Vec<T>,
    pub truncated: bool,
}

/// Selection fields for one 8.0 data set. Zabbix evaluates these filters
/// against its own tag inventory, including inherited tags.
#[derive(Debug, Default, Eq, PartialEq, Serialize)]
pub struct SvgGraphItemTagFilter {
    evaltype: u64,
    tags: Vec<SvgGraphItemTag>,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
struct SvgGraphItemTag {
    tag: String,
    operator: u64,
    value: String,
}

#[derive(Debug, Default)]
struct SvgGraphItemTagFields {
    tag: Option<String>,
    operator: Option<u64>,
    value: Option<String>,
}

impl SvgGraphItemTagFilter {
    /// Decode the flattened dashboard fields of the addressed 8.0 set.
    pub fn from_widget_fields(fields: &[Value], dataset_index: u32) -> Result<Self> {
        let invalid = || ZabbixError::SnapshotShape("Invalid svggraph item tag fields".to_owned());
        let prefix = format!("ds.{dataset_index}.");
        let mut evaltype = None;
        let mut rows = BTreeMap::<u32, SvgGraphItemTagFields>::new();
        for field in fields {
            let Some(name) = field["name"]
                .as_str()
                .and_then(|name| name.strip_prefix(&prefix))
            else {
                continue;
            };
            let field_type = number(field.get("type"));
            if name == "item_tags_evaltype" {
                let value = number(field.get("value"))
                    .filter(|value| matches!(*value, 0 | 2))
                    .ok_or_else(invalid)?;
                if field_type != Some(0) || evaltype.replace(value).is_some() {
                    return Err(invalid());
                }
                continue;
            }
            let Some(name) = name.strip_prefix("item_tags.") else {
                continue;
            };
            let (index, member) = name.split_once('.').ok_or_else(invalid)?;
            let index: u32 = index.parse().map_err(|_| invalid())?;
            let row = rows.entry(index).or_default();
            let repeated = match (member, field_type) {
                ("tag", Some(1)) => row
                    .tag
                    .replace(string_field(field, "value", "item tag name")?.to_owned())
                    .is_some(),
                ("operator", Some(0)) => row
                    .operator
                    .replace(
                        number(field.get("value"))
                            .filter(|value| *value <= 5)
                            .ok_or_else(invalid)?,
                    )
                    .is_some(),
                ("value", Some(1)) => row
                    .value
                    .replace(string_field(field, "value", "item tag value")?.to_owned())
                    .is_some(),
                _ => return Err(invalid()),
            };
            if repeated {
                return Err(invalid());
            }
        }
        let tags = rows
            .into_values()
            .map(|row| {
                let (Some(tag), Some(operator), Some(value)) = (row.tag, row.operator, row.value)
                else {
                    return Err(invalid());
                };
                // Empty form rows are ignored by Zabbix. A nonempty condition
                // without a name is outside the supported preview.
                if tag.is_empty() && value.is_empty() {
                    return Ok(None);
                }
                if tag.is_empty() {
                    return Err(invalid());
                }
                Ok(Some(SvgGraphItemTag {
                    tag,
                    operator,
                    value,
                }))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        Ok(Self {
            evaltype: evaltype.unwrap_or_default(),
            tags,
        })
    }
}

/// An account-visible item matching a single 8.0 graph pattern.
#[derive(Debug, Eq, PartialEq)]
pub struct SvgGraphItemMatch {
    pub hostid: String,
    pub itemid: String,
    pub name: String,
}

/// A classic graph (`graph.get`), not a prototype.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphDetails {
    pub graphid: String,
    pub name: String,
    /// Parent template graph, `None` for an own or discovered graph.
    pub templateid: Option<String>,
    /// `true` when created by low-level discovery (`flags = 4`).
    pub discovered: bool,
    /// Hosts whose items the graph draws.
    pub hostids: Vec<String>,
}

/// An item prototype (`itemprototype.get`), read only to find its owner and
/// template origin.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemPrototypeDetails {
    pub itemid: String,
    /// Owner host; for a template prototype, the template.
    pub hostid: String,
    /// Key with LLD macros.
    pub key: String,
    pub name: String,
    /// 0 float, 1 character, 2 log, 3 unsigned, 4 text, 5 binary, 6 JSON (8.0).
    pub value_type: u8,
    pub units: String,
    /// Parent template prototype, `None` for an own prototype.
    pub templateid: Option<String>,
    /// The LLD rule the prototype belongs to.
    pub rule_itemid: Option<String>,
    /// Visible name of the owner (host or template).
    pub owner_name: Option<String>,
}

/// A graph prototype (`graphprototype.get`), read only to find its owner and
/// template origin.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphPrototypeDetails {
    pub graphid: String,
    pub name: String,
    /// Parent template graph prototype, `None` for an own prototype.
    pub templateid: Option<String>,
    /// Owners (hosts or templates) of the items the prototype draws.
    pub hostids: Vec<String>,
    /// The LLD rule the prototype belongs to.
    pub rule_itemid: Option<String>,
    /// Visible name of the first owner.
    pub owner_name: Option<String>,
}

const ITEM_OUTPUT: [&str; 8] = [
    "itemid",
    "hostid",
    "key_",
    "name",
    "value_type",
    "units",
    "flags",
    "templateid",
];

const ITEM_PROTOTYPE_OUTPUT: [&str; 7] = [
    "itemid",
    "hostid",
    "key_",
    "name",
    "value_type",
    "units",
    "templateid",
];

fn check_ids(ids: &[String], what: &str) -> Result<()> {
    if ids.iter().any(|id| !is_zabbix_id(id)) {
        return Err(ZabbixError::SnapshotShape(format!(
            "{what} must be a Zabbix ID"
        )));
    }
    Ok(())
}

/// Missing IDs are allowed because the server scopes results to the account.
pub(crate) fn check_returned_ids(
    profile: AdapterProfile,
    rows: &[Value],
    requested: &[String],
    key: &str,
) -> Result<()> {
    if profile == AdapterProfile::V7_4 {
        return Ok(());
    }
    let requested: HashSet<&str> = requested.iter().map(String::as_str).collect();
    let mut seen = HashSet::with_capacity(rows.len());
    for row in rows {
        let id = string_field(row, key, "inventory row")?;
        if !requested.contains(id) || !seen.insert(id) {
            return Err(ZabbixError::SnapshotShape(format!(
                "inventory returned an unexpected or duplicate {key}"
            )));
        }
    }
    Ok(())
}

fn discovery_fields(profile: AdapterProfile) -> (&'static str, &'static str) {
    match profile {
        AdapterProfile::V7_4 => ("selectItemDiscovery", "itemDiscovery"),
        AdapterProfile::V8_0 => ("selectDiscoveryData", "discoveryData"),
    }
}

fn item_discovery_output(profile: AdapterProfile) -> &'static [&'static str] {
    match profile {
        AdapterProfile::V7_4 => &["parent_itemid", "key_"],
        AdapterProfile::V8_0 => &["parent_itemid", "key_", "status"],
    }
}

fn item_output(profile: AdapterProfile) -> Vec<&'static str> {
    let mut fields = ITEM_OUTPUT.to_vec();
    if profile == AdapterProfile::V8_0 {
        fields.push("status");
    }
    fields
}

impl ReadOnlyGateway {
    /// Follow a host prototype to its template prototype, without treating
    /// server-local IDs or names as portable identity. Missing UUIDs fail closed.
    pub async fn portable_prototype_identity(
        &self,
        prototype_id: &str,
    ) -> Result<Option<(String, String)>> {
        self.portable_template_item_identity(prototype_id, ReadMethod::ItemPrototypeGet)
            .await
    }

    /// Follow an inherited ordinary item to the UUIDs of its template and item.
    pub async fn portable_item_identity(&self, item_id: &str) -> Result<Option<(String, String)>> {
        self.portable_template_item_identity(item_id, ReadMethod::ItemGet)
            .await
    }

    async fn portable_template_item_identity(
        &self,
        item_id: &str,
        method: ReadMethod,
    ) -> Result<Option<(String, String)>> {
        check_ids(&[item_id.to_owned()], "itemid")?;
        let mut current = item_id.to_owned();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..16 {
            if !seen.insert(current.clone()) {
                return Ok(None);
            }
            let rows = self
                .read_inventory_rows(
                    method,
                    json!({
                        "output": ["itemid", "hostid", "templateid", "uuid"], "itemids": [current]
                    }),
                )
                .await?;
            if rows.len() != 1 {
                return Ok(None);
            }
            let row = &rows[0];
            if id_field(row, "itemid", "item prototype")? != current {
                return Ok(None);
            }
            let parent = string_field(row, "templateid", "item prototype")?;
            if parent != "0" {
                if !is_zabbix_id(parent) {
                    return Ok(None);
                }
                current = parent.to_owned();
                continue;
            }
            let Some(uuid) = row
                .get("uuid")
                .and_then(Value::as_str)
                .filter(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            else {
                return Ok(None);
            };
            let owner = id_field(row, "hostid", "item prototype")?;
            let templates = self
                .read_inventory_rows(
                    ReadMethod::TemplateGet,
                    json!({"output": ["templateid", "uuid"], "templateids": [owner]}),
                )
                .await?;
            if templates.len() != 1 {
                return Ok(None);
            }
            if id_field(&templates[0], "templateid", "template")? != owner {
                return Ok(None);
            }
            let Some(template_uuid) = templates[0]
                .get("uuid")
                .and_then(Value::as_str)
                .filter(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            else {
                return Ok(None);
            };
            return Ok(Some((
                template_uuid.to_ascii_lowercase(),
                uuid.to_ascii_lowercase(),
            )));
        }
        Ok(None)
    }
    /// Hosts by ID (`host.get`). Hidden and missing hosts are absent.
    pub async fn hosts_by_ids(&self, hostids: &[String]) -> Result<Vec<HostDetails>> {
        check_ids(hostids, "hostid")?;
        let profile = self.adapter_profile()?;
        let mut hosts = Vec::with_capacity(hostids.len());
        for batch in hostids.chunks(LOOKUP_BATCH) {
            let raw: Vec<Value> = self
                .read_inventory_rows(
                    ReadMethod::HostGet,
                    json!({"output": ["hostid", "host", "name"], "hostids": batch}),
                )
                .await?;
            check_returned_ids(profile, &raw, batch, "hostid")?;
            for host in &raw {
                hosts.push(parse_host(host)?);
            }
        }
        Ok(hosts)
    }

    /// Hosts whose visible name matches one of `names` the way the 7.4
    /// frontend matches `svggraph` host patterns: case-insensitive, `*` is
    /// the only wildcard, a name without `*` matches the whole name. The
    /// caller passes names without `*`; masks are rejected here so that one
    /// lookup cannot list every host.
    pub async fn hosts_by_visible_names(&self, names: &[String]) -> Result<Vec<HostDetails>> {
        if names
            .iter()
            .any(|name| name.is_empty() || name.contains('*') || name.chars().count() > 255)
        {
            return Err(ZabbixError::SnapshotShape(
                "host name lookup takes 1..=255 characters without '*'".to_owned(),
            ));
        }
        let mut hosts = Vec::new();
        for batch in names.chunks(LOOKUP_BATCH) {
            let raw: Vec<Value> = self
                .read_inventory_rows(
                    ReadMethod::HostGet,
                    json!({
                        "output": ["hostid", "host", "name"],
                        "search": {"name": batch},
                        "searchByAny": true,
                        "searchWildcardsEnabled": true,
                        "limit": MAX_HOST_OBJECTS
                    }),
                )
                .await?;
            for host in &raw {
                hosts.push(parse_host(host)?);
            }
        }
        Ok(hosts)
    }

    /// Hosts whose visible or technical name contains `search`
    /// (case-insensitive substring, `*` is an ordinary character; empty
    /// means every visible host), sorted by visible name, with groups and
    /// linked templates (`host.get`). At most `limit` hosts; `truncated`
    /// tells that more matched. Hidden hosts are simply absent.
    pub async fn search_hosts(
        &self,
        search: &str,
        limit: usize,
    ) -> Result<HostObjects<HostListing>> {
        self.search_hosts_filtered(
            &HostSearchFilter {
                name: search.to_owned(),
                ..HostSearchFilter::default()
            },
            limit,
        )
        .await
    }

    /// Hosts matching an optional group, exact tag pairs (AND) and a name
    /// substring. Filters only discover candidates; callers persist an
    /// explicit host list, never the filter.
    pub async fn search_hosts_filtered(
        &self,
        filter: &HostSearchFilter,
        limit: usize,
    ) -> Result<HostObjects<HostListing>> {
        let params = host_search_params(filter, limit)?;
        let raw: Vec<Value> = self
            .read_inventory_rows(ReadMethod::HostGet, params)
            .await?;
        let truncated = raw.len() > limit;
        let objects = raw
            .iter()
            .take(limit)
            .map(parse_host_listing)
            .collect::<Result<_>>()?;
        Ok(HostObjects { objects, truncated })
    }

    /// Visible host groups whose name contains `search`, sorted by name.
    /// An empty result means absent or not visible; Zabbix does not expose
    /// which case applies.
    pub async fn search_host_groups(
        &self,
        search: &str,
        limit: usize,
    ) -> Result<HostObjects<HostGroupListing>> {
        let params = host_group_search_params(search, limit)?;
        let raw: Vec<Value> = self
            .read_inventory_rows(ReadMethod::HostgroupGet, params)
            .await?;
        let truncated = raw.len() > limit;
        let objects = raw
            .iter()
            .take(limit)
            .map(parse_host_group)
            .collect::<Result<_>>()?;
        Ok(HostObjects { objects, truncated })
    }

    /// Hosts by ID with groups and linked templates (`host.get`). Hidden and
    /// missing hosts are absent.
    pub async fn host_listings(&self, hostids: &[String]) -> Result<Vec<HostListing>> {
        check_ids(hostids, "hostid")?;
        let mut hosts = Vec::with_capacity(hostids.len());
        for batch in hostids.chunks(LOOKUP_BATCH) {
            let raw: Vec<Value> = self
                .read_inventory_rows(
                    ReadMethod::HostGet,
                    json!({
                        "output": ["hostid", "host", "name"],
                        "selectHostGroups": ["name"],
                        "selectParentTemplates": ["templateid", "name"],
                        "hostids": batch
                    }),
                )
                .await?;
            for host in &raw {
                hosts.push(parse_host_listing(host)?);
            }
        }
        Ok(hosts)
    }

    /// Items by ID with matching evidence and the owner's visible name.
    pub async fn items_by_ids(&self, itemids: &[String]) -> Result<Vec<ItemDetails>> {
        check_ids(itemids, "itemid")?;
        let profile = self.adapter_profile()?;
        let (select_discovery, _) = discovery_fields(profile);
        let mut items = Vec::with_capacity(itemids.len());
        for batch in itemids.chunks(LOOKUP_BATCH) {
            let raw: Vec<Value> = self
                .read_inventory_rows(
                    ReadMethod::ItemGet,
                    json!({
                        "output": item_output(profile),
                        (select_discovery): item_discovery_output(profile),
                        "selectHosts": ["hostid", "name"],
                        "itemids": batch,
                        "webitems": true
                    }),
                )
                .await?;
            check_returned_ids(profile, &raw, batch, "itemid")?;
            for item in &raw {
                items.push(parse_item(item, profile)?);
            }
        }
        Ok(items)
    }

    /// All items of one host (plain and discovered) with matching evidence.
    pub async fn host_items(&self, hostid: &str) -> Result<HostObjects<ItemDetails>> {
        self.host_items_with_count(hostid, false).await
    }

    /// Complete item inventory of a visible 8.0 target host for portable
    /// source review. Compare the bounded rows with a separate count.
    pub async fn portable_host_items(&self, hostid: &str) -> Result<HostObjects<ItemDetails>> {
        self.host_items_with_count(hostid, self.adapter_profile()? == AdapterProfile::V8_0)
            .await
    }

    async fn host_items_with_count(
        &self,
        hostid: &str,
        verify_count: bool,
    ) -> Result<HostObjects<ItemDetails>> {
        check_ids(&[hostid.to_owned()], "hostid")?;
        let profile = self.adapter_profile()?;
        let (select_discovery, _) = discovery_fields(profile);
        let raw = self
            .bounded_inventory_rows(
                ReadMethod::ItemGet,
                json!({
                    "output": item_output(profile),
                    (select_discovery): item_discovery_output(profile),
                    "hostids": [hostid],
                    "webitems": true,
                    "sortfield": "itemid",
                    "limit": MAX_HOST_OBJECTS + 1
                }),
                MAX_HOST_OBJECTS,
                verify_count,
            )
            .await?;
        let truncated = raw.len() > MAX_HOST_OBJECTS;
        if verify_count
            && raw.iter().take(MAX_HOST_OBJECTS).any(|item| {
                item.get("units").and_then(Value::as_str).is_none()
                    || item.get("templateid").and_then(Value::as_str).is_none()
            })
        {
            return Err(ZabbixError::SnapshotShape(
                "Incomplete portable item inventory".to_owned(),
            ));
        }
        let objects = raw
            .iter()
            .take(MAX_HOST_OBJECTS)
            .map(|item| parse_item(item, profile))
            .collect::<Result<Vec<_>>>()?;
        if profile == AdapterProfile::V8_0 && objects.iter().any(|item| item.hostid != hostid) {
            return Err(ZabbixError::SnapshotShape(
                "item.get returned an item from another host".to_owned(),
            ));
        }
        Ok(HostObjects { objects, truncated })
    }

    /// Ask 8.0 to apply its Graph host mask rules to account-visible hosts.
    pub async fn svggraph_host_matches(&self, pattern: &str) -> Result<HostObjects<HostDetails>> {
        self.svggraph_host_matches_with_count(pattern, MAX_SVGGRAPH_PATTERN_MATCHES, false)
            .await
    }

    /// Review every matching host, checking for an incomplete 8.0 response.
    pub async fn portable_svggraph_host_matches(
        &self,
        pattern: &str,
    ) -> Result<HostObjects<HostDetails>> {
        let limit = if pattern == "*" {
            MAX_HOST_SEARCH
        } else {
            MAX_SVGGRAPH_PATTERN_MATCHES
        };
        self.svggraph_host_matches_with_count(pattern, limit, true)
            .await
    }

    /// Exact Clone inventory. Keep the complete role and list/count proof
    /// while allowing a bounded review beyond the 100-host selector page.
    pub async fn clone_svggraph_host_matches(
        &self,
        pattern: &str,
    ) -> Result<HostObjects<HostDetails>> {
        let role = self.require_complete_clone_inventory().await?;
        let hosts = self
            .svggraph_host_matches_with_count(pattern, MAX_SVGGRAPH_PATTERN_MATCHES, true)
            .await?;
        if self.require_complete_clone_inventory().await? != role {
            return Err(ZabbixError::SnapshotShape(
                "Clone inventory role changed".to_owned(),
            ));
        }
        Ok(hosts)
    }

    /// Read the full visible Clone match set without expanding any hostids
    /// request beyond the regular API bound. The aggregate stays bounded.
    pub async fn clone_svggraph_item_matches(
        &self,
        hostids: &[String],
        pattern: &str,
        filter: &SvgGraphItemTagFilter,
    ) -> Result<HostObjects<SvgGraphItemMatch>> {
        let role = self.require_complete_clone_inventory().await?;
        if hostids.is_empty()
            || hostids.len() > MAX_SVGGRAPH_PATTERN_MATCHES
            || hostids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != hostids.len()
        {
            return Err(ZabbixError::SnapshotShape(
                "Invalid complete Clone host inventory".to_owned(),
            ));
        }
        let mut objects = Vec::new();
        for batch in hostids.chunks(MAX_HOST_SEARCH) {
            let mut rows = self
                .portable_svggraph_item_matches(batch, pattern, filter)
                .await?;
            if rows.truncated || rows.objects.len() > MAX_SVGGRAPH_PATTERN_MATCHES - objects.len() {
                return Ok(HostObjects {
                    objects: Vec::new(),
                    truncated: true,
                });
            }
            objects.append(&mut rows.objects);
        }
        if self.require_complete_clone_inventory().await? != role {
            return Err(ZabbixError::SnapshotShape(
                "Clone inventory role changed".to_owned(),
            ));
        }
        Ok(HostObjects {
            objects,
            truncated: false,
        })
    }

    async fn require_complete_clone_inventory(&self) -> Result<String> {
        self.portable_pattern_inventory_role()
            .await?
            .ok_or_else(|| {
                ZabbixError::SnapshotShape(
                    "Complete Clone pattern inventory is unavailable".to_owned(),
                )
            })
    }

    async fn svggraph_host_matches_with_count(
        &self,
        pattern: &str,
        limit: usize,
        verify_count: bool,
    ) -> Result<HostObjects<HostDetails>> {
        self.require_svggraph_pattern(pattern)?;
        let search = (pattern != "*").then(|| json!({ "name": [pattern] }));
        let raw = self
            .bounded_inventory_rows(
                ReadMethod::HostGet,
                json!({
                    "output": ["hostid", "host", "name"],
                    "search": search,
                    "searchWildcardsEnabled": true,
                    "searchByAny": true,
                    "sortfield": "hostid",
                    "limit": limit + 1
                }),
                limit,
                verify_count,
            )
            .await?;
        let truncated = raw.len() > limit;
        let objects = raw
            .iter()
            .take(limit)
            .map(parse_host)
            .collect::<Result<Vec<_>>>()?;
        Ok(HostObjects { objects, truncated })
    }

    /// Ask 8.0 to match resolved names and inherited item tags, as in a
    /// global Graph data set. The caller supplies the visible host scope.
    pub async fn svggraph_item_matches(
        &self,
        hostids: &[String],
        pattern: &str,
        filter: &SvgGraphItemTagFilter,
    ) -> Result<HostObjects<SvgGraphItemMatch>> {
        self.svggraph_item_matches_with_count(hostids, pattern, filter, false)
            .await
    }

    /// Review every matching numeric item, checking for an incomplete result.
    pub async fn portable_svggraph_item_matches(
        &self,
        hostids: &[String],
        pattern: &str,
        filter: &SvgGraphItemTagFilter,
    ) -> Result<HostObjects<SvgGraphItemMatch>> {
        self.svggraph_item_matches_with_count(hostids, pattern, filter, true)
            .await
    }

    async fn svggraph_item_matches_with_count(
        &self,
        hostids: &[String],
        pattern: &str,
        filter: &SvgGraphItemTagFilter,
        verify_count: bool,
    ) -> Result<HostObjects<SvgGraphItemMatch>> {
        self.require_svggraph_pattern(pattern)?;
        check_ids(hostids, "hostid")?;
        if hostids.is_empty() || hostids.len() > MAX_HOST_SEARCH {
            return Err(ZabbixError::SnapshotShape(
                "Invalid svggraph host scope".to_owned(),
            ));
        }
        let search = (pattern != "*").then(|| json!({ "name_resolved": [pattern] }));
        let raw = self
            .bounded_inventory_rows(
                ReadMethod::ItemGet,
                json!({
                    "output": ["itemid", "hostid", "name_resolved", "value_type"],
                    "hostids": hostids,
                    "search": search,
                    "searchWildcardsEnabled": true,
                    "searchByAny": true,
                    "filter": { "value_type": [0, 3] },
                    "webitems": true,
                    "inheritedTags": true,
                    "evaltype": filter.evaltype,
                    "tags": (!filter.tags.is_empty()).then_some(&filter.tags),
                    "sortfield": "itemid",
                    "limit": MAX_SVGGRAPH_PATTERN_MATCHES + 1
                }),
                MAX_SVGGRAPH_PATTERN_MATCHES,
                verify_count,
            )
            .await?;
        let truncated = raw.len() > MAX_SVGGRAPH_PATTERN_MATCHES;
        let mut seen = HashSet::new();
        let objects = raw
            .iter()
            .take(MAX_SVGGRAPH_PATTERN_MATCHES)
            .map(|item| {
                let hostid = id_field(item, "hostid", "svggraph item")?;
                let itemid = id_field(item, "itemid", "svggraph item")?;
                let name = string_field(item, "name_resolved", "svggraph item")?;
                if !hostids.contains(&hostid)
                    || !seen.insert(itemid.clone())
                    || !matches!(number(item.get("value_type")), Some(0 | 3))
                    || name.is_empty()
                {
                    return Err(ZabbixError::SnapshotShape(
                        "Invalid svggraph item match".to_owned(),
                    ));
                }
                Ok(SvgGraphItemMatch {
                    hostid,
                    itemid,
                    name: name.to_owned(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(HostObjects { objects, truncated })
    }

    async fn bounded_inventory_rows(
        &self,
        method: ReadMethod,
        mut params: Value,
        limit: usize,
        verify_count: bool,
    ) -> Result<Vec<Value>> {
        let rows = self.read_inventory_rows(method, params.clone()).await?;
        if verify_count && rows.len() <= limit {
            let query = params.as_object_mut().ok_or_else(|| {
                ZabbixError::SnapshotShape("Invalid inventory count parameters".to_owned())
            })?;
            for field in ["output", "limit", "sortfield", "selectDiscoveryData"] {
                query.remove(field);
            }
            query.insert("countOutput".to_owned(), json!(true));
            let count: Value = self
                .session
                .transport
                .read(method, params, &self.session.token)
                .await?;
            if number(Some(&count)) != u64::try_from(rows.len()).ok() {
                return Err(ZabbixError::SnapshotShape(
                    "Inventory count does not match returned rows".to_owned(),
                ));
            }
        }
        Ok(rows)
    }

    fn require_svggraph_pattern(&self, pattern: &str) -> Result<()> {
        if self.adapter_profile()? != AdapterProfile::V8_0 {
            return Err(ZabbixError::UnsupportedVersion(
                self.capabilities().version.to_string(),
            ));
        }
        if pattern.is_empty() || !valid_filter_text(pattern, MAX_FILTER_CHARS) {
            return Err(ZabbixError::SnapshotShape(
                "Invalid svggraph pattern".to_owned(),
            ));
        }
        Ok(())
    }

    /// Classic graphs by ID (`graph.get`). Prototypes are not returned.
    pub async fn graphs_by_ids(&self, graphids: &[String]) -> Result<Vec<GraphDetails>> {
        check_ids(graphids, "graphid")?;
        let profile = self.adapter_profile()?;
        let mut graphs = Vec::with_capacity(graphids.len());
        for batch in graphids.chunks(LOOKUP_BATCH) {
            let raw: Vec<Value> = self
                .read_inventory_rows(
                    ReadMethod::GraphGet,
                    json!({
                        "output": ["graphid", "name", "templateid", "flags"],
                        "selectHosts": ["hostid"],
                        "graphids": batch
                    }),
                )
                .await?;
            check_returned_ids(profile, &raw, batch, "graphid")?;
            for graph in &raw {
                graphs.push(parse_graph(graph)?);
            }
        }
        Ok(graphs)
    }

    /// All classic graphs drawing items of one host.
    pub async fn host_graphs(&self, hostid: &str) -> Result<HostObjects<GraphDetails>> {
        check_ids(&[hostid.to_owned()], "hostid")?;
        let raw: Vec<Value> = self
            .read_inventory_rows(
                ReadMethod::GraphGet,
                json!({
                    "output": ["graphid", "name", "templateid", "flags"],
                    "selectHosts": ["hostid"],
                    "hostids": [hostid],
                    "sortfield": "graphid",
                    "limit": MAX_HOST_OBJECTS + 1
                }),
            )
            .await?;
        let truncated = raw.len() > MAX_HOST_OBJECTS;
        let objects = raw
            .iter()
            .take(MAX_HOST_OBJECTS)
            .map(parse_graph)
            .collect::<Result<_>>()?;
        Ok(HostObjects { objects, truncated })
    }

    /// Item prototypes by ID (`itemprototype.get`). Hidden and missing
    /// prototypes are absent.
    pub async fn item_prototypes_by_ids(
        &self,
        itemids: &[String],
    ) -> Result<Vec<ItemPrototypeDetails>> {
        check_ids(itemids, "itemid")?;
        let profile = self.adapter_profile()?;
        let mut prototypes = Vec::with_capacity(itemids.len());
        for batch in itemids.chunks(LOOKUP_BATCH) {
            let raw: Vec<Value> = self
                .read_inventory_rows(
                    ReadMethod::ItemPrototypeGet,
                    json!({
                        "output": ITEM_PROTOTYPE_OUTPUT,
                        "selectHosts": ["hostid", "name"],
                        "selectDiscoveryRule": ["itemid"],
                        "itemids": batch
                    }),
                )
                .await?;
            check_returned_ids(profile, &raw, batch, "itemid")?;
            for prototype in &raw {
                prototypes.push(parse_item_prototype(prototype, profile)?);
            }
        }
        Ok(prototypes)
    }

    /// Graph prototypes by ID (`graphprototype.get`). Hidden and missing
    /// prototypes are absent.
    pub async fn graph_prototypes_by_ids(
        &self,
        graphids: &[String],
    ) -> Result<Vec<GraphPrototypeDetails>> {
        check_ids(graphids, "graphid")?;
        let profile = self.adapter_profile()?;
        let mut prototypes = Vec::with_capacity(graphids.len());
        for batch in graphids.chunks(LOOKUP_BATCH) {
            let raw: Vec<Value> = self
                .read_inventory_rows(
                    ReadMethod::GraphPrototypeGet,
                    json!({
                        "output": ["graphid", "name", "templateid"],
                        "selectHosts": ["hostid", "name"],
                        "selectDiscoveryRule": ["itemid"],
                        "graphids": batch
                    }),
                )
                .await?;
            check_returned_ids(profile, &raw, batch, "graphid")?;
            for prototype in &raw {
                prototypes.push(parse_graph_prototype(prototype)?);
            }
        }
        Ok(prototypes)
    }
}

fn string_field<'a>(value: &'a Value, key: &str, context: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ZabbixError::SnapshotShape(format!("{context} has no string {key}")))
}

fn id_field(value: &Value, key: &str, context: &str) -> Result<String> {
    let id = string_field(value, key, context)?;
    if !is_zabbix_id(id) {
        return Err(ZabbixError::SnapshotShape(format!(
            "{context} has an invalid {key}"
        )));
    }
    Ok(id.to_owned())
}

fn number(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

/// `templateid` of `0` means «not inherited».
fn parent_id(value: &Value, key: &str, context: &str) -> Result<Option<String>> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => {
            let id = id_field(value, key, context)?;
            Ok((id != "0").then_some(id))
        }
    }
}

fn parse_host(host: &Value) -> Result<HostDetails> {
    Ok(HostDetails {
        hostid: id_field(host, "hostid", "host")?,
        host: string_field(host, "host", "host")?.to_owned(),
        name: string_field(host, "name", "host")?.to_owned(),
    })
}

fn valid_filter_text(text: &str, max: usize) -> bool {
    text.chars().count() <= max && !text.chars().any(char::is_control)
}

fn host_search_params(filter: &HostSearchFilter, limit: usize) -> Result<Value> {
    if limit == 0 || limit > MAX_HOST_SEARCH {
        return Err(ZabbixError::SnapshotShape(
            "host search limit is out of range".to_owned(),
        ));
    }
    if !valid_filter_text(&filter.name, MAX_HOST_SEARCH_CHARS) {
        return Err(ZabbixError::SnapshotShape(
            "host search text is too long or has control characters".to_owned(),
        ));
    }
    if filter
        .groupid
        .as_deref()
        .is_some_and(|groupid| !is_zabbix_id(groupid))
    {
        return Err(ZabbixError::SnapshotShape(
            "host search groupid is invalid".to_owned(),
        ));
    }
    let mut tag_names = std::collections::BTreeSet::new();
    for tag in &filter.tags {
        if tag.tag.is_empty()
            || !valid_filter_text(&tag.tag, MAX_FILTER_CHARS)
            || !valid_filter_text(&tag.value, MAX_FILTER_CHARS)
            || !tag_names.insert(tag.tag.as_str())
        {
            return Err(ZabbixError::SnapshotShape(
                "host search tags must have unique non-empty names and valid exact values"
                    .to_owned(),
            ));
        }
    }

    let mut params = json!({
        "output": ["hostid", "host", "name"],
        "selectHostGroups": ["name"],
        "selectParentTemplates": ["templateid", "name"],
        "sortfield": "name",
        "limit": limit + 1
    });
    if !filter.name.is_empty() {
        params["search"] = json!({"host": filter.name, "name": filter.name});
        params["searchByAny"] = json!(true);
    }
    if let Some(groupid) = &filter.groupid {
        params["groupids"] = json!([groupid]);
    }
    if !filter.tags.is_empty() {
        params["evaltype"] = json!(0);
        params["tags"] = Value::Array(
            filter
                .tags
                .iter()
                .map(|tag| json!({"tag": tag.tag, "value": tag.value, "operator": 1}))
                .collect(),
        );
    }
    Ok(params)
}

fn host_group_search_params(search: &str, limit: usize) -> Result<Value> {
    if limit == 0 || limit > MAX_HOST_GROUP_SEARCH {
        return Err(ZabbixError::SnapshotShape(
            "host group search limit is out of range".to_owned(),
        ));
    }
    if !valid_filter_text(search, MAX_FILTER_CHARS) {
        return Err(ZabbixError::SnapshotShape(
            "host group search text is too long or has control characters".to_owned(),
        ));
    }
    let mut params = json!({
        "output": ["groupid", "name"],
        "sortfield": "name",
        "limit": limit + 1
    });
    if !search.is_empty() {
        params["search"] = json!({"name": search});
    }
    Ok(params)
}

fn parse_host_group(group: &Value) -> Result<HostGroupListing> {
    Ok(HostGroupListing {
        groupid: id_field(group, "groupid", "host group")?,
        name: string_field(group, "name", "host group")?.to_owned(),
    })
}

fn parse_host_listing(host: &Value) -> Result<HostListing> {
    let details = parse_host(host)?;
    let mut groups = host
        .get("hostgroups")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|group| string_field(group, "name", "host group").map(str::to_owned))
        .collect::<Result<Vec<_>>>()?;
    groups.sort();
    groups.dedup();
    let mut templates = host
        .get("parentTemplates")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|template| {
            Ok(TemplateRef {
                templateid: id_field(template, "templateid", "template")?,
                name: string_field(template, "name", "template")?.to_owned(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    templates.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.templateid.cmp(&right.templateid))
    });
    templates.dedup();
    Ok(HostListing {
        hostid: details.hostid,
        host: details.host,
        name: details.name,
        groups,
        templates,
    })
}

fn parse_item(item: &Value, profile: AdapterProfile) -> Result<ItemDetails> {
    let value_type = profile.item_value_type(item.get("value_type"), "item")?;
    let flags = number(item.get("flags"))
        .map(ItemFlags::from_number)
        .ok_or_else(|| ZabbixError::SnapshotShape("item has no numeric flags".to_owned()))?;
    let (_, discovery_key) = discovery_fields(profile);
    let discovery = match item.get(discovery_key) {
        Some(link @ Value::Object(_))
            if link.get("parent_itemid").is_some()
                && (profile == AdapterProfile::V7_4 || flags == ItemFlags::Discovered) =>
        {
            Some(ItemDiscoveryLink {
                parent_itemid: id_field(link, "parent_itemid", "item discovery")?,
                prototype_key: link
                    .get("key_")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                currently_discovered: number(link.get("status"))
                    .filter(|status| matches!(status, 0 | 1))
                    .map(|status| status == 0),
            })
        }
        _ => None,
    };
    let owner_name = item
        .get("hosts")
        .and_then(Value::as_array)
        .and_then(|hosts| hosts.first())
        .and_then(|host| host.get("name"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(ItemDetails {
        itemid: id_field(item, "itemid", "item")?,
        hostid: id_field(item, "hostid", "item")?,
        key: string_field(item, "key_", "item")?.to_owned(),
        name: string_field(item, "name", "item")?.to_owned(),
        value_type,
        units: item
            .get("units")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        flags,
        enabled: match profile {
            AdapterProfile::V8_0 => number(item.get("status"))
                .filter(|status| *status <= 1)
                .map(|status| status == 0),
            AdapterProfile::V7_4 => None,
        },
        templateid: parent_id(item, "templateid", "item")?,
        discovery,
        owner_name,
    })
}

fn parse_graph(graph: &Value) -> Result<GraphDetails> {
    let hostids = graph
        .get("hosts")
        .and_then(Value::as_array)
        .map(|hosts| {
            hosts
                .iter()
                .map(|host| id_field(host, "hostid", "graph host"))
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(GraphDetails {
        graphid: id_field(graph, "graphid", "graph")?,
        name: string_field(graph, "name", "graph")?.to_owned(),
        templateid: parent_id(graph, "templateid", "graph")?,
        discovered: number(graph.get("flags")) == Some(4),
        hostids,
    })
}

/// `selectDiscoveryRule` answers an object; an empty array means none.
fn rule_itemid(value: &Value, context: &str) -> Result<Option<String>> {
    match value.get("discoveryRule") {
        Some(rule @ Value::Object(_)) => Ok(Some(id_field(rule, "itemid", context)?)),
        _ => Ok(None),
    }
}

fn first_host_name(value: &Value) -> Option<String> {
    value
        .get("hosts")
        .and_then(Value::as_array)
        .and_then(|hosts| hosts.first())
        .and_then(|host| host.get("name"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn parse_item_prototype(
    prototype: &Value,
    profile: AdapterProfile,
) -> Result<ItemPrototypeDetails> {
    let value_type = profile.item_value_type(prototype.get("value_type"), "item prototype")?;
    Ok(ItemPrototypeDetails {
        itemid: id_field(prototype, "itemid", "item prototype")?,
        hostid: id_field(prototype, "hostid", "item prototype")?,
        key: string_field(prototype, "key_", "item prototype")?.to_owned(),
        name: string_field(prototype, "name", "item prototype")?.to_owned(),
        value_type,
        units: prototype
            .get("units")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        templateid: parent_id(prototype, "templateid", "item prototype")?,
        rule_itemid: rule_itemid(prototype, "item prototype rule")?,
        owner_name: first_host_name(prototype),
    })
}

fn parse_graph_prototype(prototype: &Value) -> Result<GraphPrototypeDetails> {
    let hostids = prototype
        .get("hosts")
        .and_then(Value::as_array)
        .map(|hosts| {
            hosts
                .iter()
                .map(|host| id_field(host, "hostid", "graph prototype host"))
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(GraphPrototypeDetails {
        graphid: id_field(prototype, "graphid", "graph prototype")?,
        name: string_field(prototype, "name", "graph prototype")?.to_owned(),
        templateid: parent_id(prototype, "templateid", "graph prototype")?,
        hostids,
        rule_itemid: rule_itemid(prototype, "graph prototype rule")?,
        owner_name: first_host_name(prototype),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        AdapterProfile, ExactHostTag, HostSearchFilter, ItemDetails, ItemFlags,
        ItemPrototypeDetails, SvgGraphItemTagFilter, host_group_search_params, host_search_params,
        parse_graph, parse_graph_prototype, parse_host, parse_host_group, parse_host_listing,
    };

    fn parse_item(item: &serde_json::Value) -> crate::Result<ItemDetails> {
        super::parse_item(item, AdapterProfile::V7_4)
    }

    fn parse_item_prototype(item: &serde_json::Value) -> crate::Result<ItemPrototypeDetails> {
        super::parse_item_prototype(item, AdapterProfile::V7_4)
    }

    #[test]
    fn graph_item_tag_decoder_uses_dataset_and_rejects_invalid_shape() {
        let valid = vec![
            json!({"type":"0","name":"ds.0.item_tags_evaltype","value":"2"}),
            json!({"type":"1","name":"ds.0.item_tags.0.tag","value":"Component"}),
            json!({"type":0,"name":"ds.0.item_tags.0.operator","value":5}),
            json!({"type":1,"name":"ds.0.item_tags.0.value","value":""}),
            json!({"type":1,"name":"ds.1.item_tags.0.tag","value":"other"}),
        ];
        let filter = SvgGraphItemTagFilter::from_widget_fields(&valid, 0)
            .unwrap_or_else(|error| panic!("item tags: {error}"));
        assert_eq!(
            json!(filter),
            json!({"evaltype":2,"tags":[
                {"tag":"Component","operator":5,"value":""}
            ]})
        );
        let defaults = SvgGraphItemTagFilter::from_widget_fields(&valid, 2)
            .unwrap_or_else(|error| panic!("item tags: {error}"));
        assert_eq!(json!(defaults), json!({"evaltype":0,"tags":[]}));
        assert!(SvgGraphItemTagFilter::from_widget_fields(&valid, 1).is_err());
        let mut duplicate = valid.clone();
        duplicate.push(valid[3].clone());
        assert!(SvgGraphItemTagFilter::from_widget_fields(&duplicate, 0).is_err());
        let mut invalid = valid;
        invalid[2]["value"] = json!(6);
        assert!(SvgGraphItemTagFilter::from_widget_fields(&invalid, 0).is_err());
    }

    #[test]
    fn target_filters_use_allowlisted_exact_and_semantics() {
        let params = host_search_params(
            &HostSearchFilter {
                name: "pg-b".to_owned(),
                groupid: Some("81".to_owned()),
                tags: vec![
                    ExactHostTag {
                        tag: "role".to_owned(),
                        value: "pgsql".to_owned(),
                    },
                    ExactHostTag {
                        tag: "site".to_owned(),
                        value: "pilot".to_owned(),
                    },
                ],
            },
            20,
        )
        .unwrap_or_else(|error| panic!("host params: {error}"));
        assert_eq!(params["groupids"], json!(["81"]));
        assert_eq!(params["search"], json!({"host": "pg-b", "name": "pg-b"}));
        assert_eq!(params["evaltype"], 0);
        assert_eq!(params["tags"][0]["operator"], 1);
        assert_eq!(params["tags"][1]["tag"], "site");
        assert_eq!(params["limit"], 21);

        let duplicate = HostSearchFilter {
            tags: vec![
                ExactHostTag {
                    tag: "role".to_owned(),
                    value: "pgsql".to_owned(),
                },
                ExactHostTag {
                    tag: "role".to_owned(),
                    value: "shared".to_owned(),
                },
            ],
            ..HostSearchFilter::default()
        };
        assert!(host_search_params(&duplicate, 20).is_err());
    }

    #[test]
    fn host_group_search_is_bounded_and_parsed() {
        let params = host_group_search_params("Pilot", 25)
            .unwrap_or_else(|error| panic!("group params: {error}"));
        assert_eq!(params["search"], json!({"name": "Pilot"}));
        assert_eq!(params["limit"], 26);
        let group = parse_host_group(&json!({"groupid": "81", "name": "Pilot/DB"}))
            .unwrap_or_else(|error| panic!("group: {error}"));
        assert_eq!(group.groupid, "81");
        assert_eq!(group.name, "Pilot/DB");
        assert!(parse_host_group(&json!({"groupid": "bad", "name": "x"})).is_err());
    }

    #[test]
    fn item_prototype_keeps_owner_template_origin_and_rule() {
        let own = parse_item_prototype(&json!({
            "itemid": "501", "hostid": "800", "key_": "pgsql.db.size[{#DBNAME}]",
            "name": "DB size", "value_type": "3", "units": "B", "templateid": "0",
            "hosts": [{"hostid": "800", "name": "Synthetic template"}],
            "discoveryRule": {"itemid": "400"}
        }))
        .unwrap_or_else(|error| panic!("own prototype: {error}"));
        assert_eq!(own.hostid, "800");
        assert_eq!(own.templateid, None);
        assert_eq!(own.rule_itemid.as_deref(), Some("400"));
        assert_eq!(own.owner_name.as_deref(), Some("Synthetic template"));

        let inherited = parse_item_prototype(&json!({
            "itemid": "601", "hostid": "11", "key_": "pgsql.db.size[{#DBNAME}]",
            "name": "DB size", "value_type": "3", "templateid": "501",
            "discoveryRule": []
        }))
        .unwrap_or_else(|error| panic!("inherited prototype: {error}"));
        assert_eq!(inherited.templateid.as_deref(), Some("501"));
        assert_eq!(inherited.rule_itemid, None);
        assert_eq!(inherited.owner_name, None);
        assert!(
            parse_item_prototype(&json!({
                "itemid": "1", "hostid": "x", "key_": "k", "name": "n", "value_type": "0"
            }))
            .is_err()
        );
        assert!(
            parse_item_prototype(&json!({
                "itemid": "1", "hostid": "2", "key_": "k", "name": "n", "value_type": "9"
            }))
            .is_err()
        );
    }

    #[test]
    fn graph_prototype_keeps_owners_and_template_origin() {
        let prototype = parse_graph_prototype(&json!({
            "graphid": "701", "name": "DB graph", "templateid": "650",
            "hosts": [{"hostid": "11", "name": "synthetic-a"}],
            "discoveryRule": {"itemid": "400"}
        }))
        .unwrap_or_else(|error| panic!("graph prototype: {error}"));
        assert_eq!(prototype.hostids, vec!["11".to_owned()]);
        assert_eq!(prototype.templateid.as_deref(), Some("650"));
        assert_eq!(prototype.rule_itemid.as_deref(), Some("400"));
        assert_eq!(prototype.owner_name.as_deref(), Some("synthetic-a"));
        assert!(parse_graph_prototype(&json!({"graphid": "1"})).is_err());
        assert!(
            parse_graph_prototype(&json!({
                "graphid": "1", "name": "n", "hosts": [{"hostid": "x"}]
            }))
            .is_err()
        );
    }

    #[test]
    fn host_listing_keeps_groups_and_templates_sorted() {
        let host = parse_host_listing(&json!({
            "hostid": "11", "host": "synthetic-b", "name": "Synthetic B",
            "hostgroups": [{"name": "Pilot/DB"}, {"name": "Linux"}],
            "parentTemplates": [
                {"templateid": "902", "name": "Synthetic PostgreSQL"},
                {"templateid": "901", "name": "Synthetic Linux"}
            ]
        }))
        .unwrap_or_else(|error| panic!("host listing: {error}"));
        assert_eq!(host.groups, ["Linux", "Pilot/DB"]);
        assert_eq!(
            host.templates
                .iter()
                .map(|template| template.templateid.as_str())
                .collect::<Vec<_>>(),
            ["901", "902"]
        );
        let bare = parse_host_listing(&json!({"hostid": "12", "host": "h", "name": "h"}))
            .unwrap_or_else(|error| panic!("bare host: {error}"));
        assert!(bare.groups.is_empty() && bare.templates.is_empty());
        assert!(
            parse_host_listing(&json!({
                "hostid": "12", "host": "h", "name": "h",
                "parentTemplates": [{"templateid": "x", "name": "t"}]
            }))
            .is_err()
        );
    }

    #[test]
    fn item_evidence_keeps_key_type_units_origin_and_lld_parent() {
        let plain = parse_item(&json!({
            "itemid": "101", "hostid": "11", "key_": "system.cpu.util", "name": "CPU utilization",
            "value_type": "0", "units": "%", "flags": "0", "templateid": "901",
            "itemDiscovery": [], "hosts": [{"hostid": "11", "name": "synthetic-a"}]
        }))
        .unwrap_or_else(|error| panic!("plain item: {error}"));
        assert_eq!(plain.flags, ItemFlags::Plain);
        assert_eq!(plain.templateid.as_deref(), Some("901"));
        assert_eq!(plain.discovery, None);
        assert_eq!(plain.owner_name.as_deref(), Some("synthetic-a"));

        let discovered = parse_item(&json!({
            "itemid": "102", "hostid": "11", "key_": "vfs.fs.size[/data,used]", "name": "FS used",
            "value_type": "3", "units": "B", "flags": "4", "templateid": "0",
            "itemDiscovery": {"parent_itemid": "555", "key_": "vfs.fs.size[{#FSNAME},used]"}
        }))
        .unwrap_or_else(|error| panic!("discovered item: {error}"));
        assert_eq!(discovered.flags, ItemFlags::Discovered);
        assert_eq!(discovered.templateid, None);
        assert_eq!(
            discovered.discovery.map(|link| link.prototype_key),
            Some("vfs.fs.size[{#FSNAME},used]".to_owned())
        );
    }

    #[test]
    fn malformed_lookup_answers_are_rejected() {
        assert!(
            parse_item(
                &json!({"itemid": "1", "hostid": "x", "key_": "k", "name": "n",
                               "value_type": "0", "flags": "0"})
            )
            .is_err()
        );
        assert!(
            parse_item(
                &json!({"itemid": "1", "hostid": "2", "key_": "k", "name": "n",
                               "value_type": "9", "flags": "0"})
            )
            .is_err()
        );
        assert!(parse_host(&json!({"hostid": "1", "host": "h"})).is_err());
        assert!(parse_graph(&json!({"graphid": "1"})).is_err());
    }

    #[test]
    fn graph_evidence_keeps_name_origin_and_hosts() {
        let graph = parse_graph(&json!({
            "graphid": "77", "name": "CPU load", "templateid": "700", "flags": "0",
            "hosts": [{"hostid": "11"}]
        }))
        .unwrap_or_else(|error| panic!("graph: {error}"));
        assert_eq!(graph.templateid.as_deref(), Some("700"));
        assert!(!graph.discovered);
        assert_eq!(graph.hostids, vec!["11".to_owned()]);
    }
}
