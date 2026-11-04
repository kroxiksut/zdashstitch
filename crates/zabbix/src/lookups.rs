// SPDX-License-Identifier: MIT OR Apache-2.0

//! Read-only lookups for the source resolver (P2-02): hosts, items and
//! classic graphs with the evidence fields of the P1-IN-03 status model
//! (`key_`, `value_type`, `units`, `flags`, `templateid`, LLD parent).
//!
//! Every call uses a method of the P1-IN-08 read allowlist (`host.get`,
//! `item.get`, `graph.get`, `itemprototype.get`, `graphprototype.get`).
//! Prototypes are read only by ID, to find the owner (host or template) of
//! the prototype fields of widget field types 5 and 7 (decision 2026-10-26).
//! Objects hidden by permissions are simply absent from the answers; Zabbix
//! does not tell them apart from missing objects (P1-IN-08).

use serde_json::{Value, json};

use crate::dashboards::is_zabbix_id;
use crate::gateway::ReadMethod;
use crate::{ReadOnlyGateway, Result, ZabbixError};

/// IDs or names sent in one lookup call.
pub const LOOKUP_BATCH: usize = 500;
/// Largest accepted per-host item or graph list.
pub const MAX_HOST_OBJECTS: usize = 20_000;
/// Largest page of a host search ([`ReadOnlyGateway::search_hosts`]).
pub const MAX_HOST_SEARCH: usize = 100;
/// Longest host search text, in characters (the Zabbix host name limit).
pub const MAX_HOST_SEARCH_CHARS: usize = 128;

/// A host as seen by the account. `name` is the visible name, which
/// `svggraph` host patterns are matched against; `host` is the technical
/// name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostDetails {
    pub hostid: String,
    pub host: String,
    pub name: String,
}

/// A template linked directly to a host (server data).
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct TemplateRef {
    pub templateid: String,
    pub name: String,
}

/// A host found by a search, with its host groups and directly linked
/// templates (server data, for choosing a target host).
#[derive(Clone, Debug, Eq, PartialEq)]
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

/// Link of a discovered item to its item prototype (`itemDiscovery`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemDiscoveryLink {
    /// Item prototype on the same host.
    pub parent_itemid: String,
    /// Key of the item prototype, with LLD macros.
    pub prototype_key: String,
}

/// Item evidence for matching. No values, no history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemDetails {
    pub itemid: String,
    /// Owner host; for a template item, the template.
    pub hostid: String,
    pub key: String,
    pub name: String,
    /// 0 float, 1 character, 2 log, 3 unsigned, 4 text, 5 binary.
    pub value_type: u8,
    pub units: String,
    pub flags: ItemFlags,
    /// Parent template item, `None` for an own or discovered item.
    pub templateid: Option<String>,
    pub discovery: Option<ItemDiscoveryLink>,
    /// Visible name of the owner (host or template), when the read asked for it.
    pub owner_name: Option<String>,
}

/// Result of a per-host list read; `truncated` means more objects exist than
/// [`MAX_HOST_OBJECTS`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostObjects<T> {
    pub objects: Vec<T>,
    pub truncated: bool,
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
    /// 0 float, 1 character, 2 log, 3 unsigned, 4 text, 5 binary.
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

impl ReadOnlyGateway {
    /// Hosts by ID (`host.get`). Hidden and missing hosts are absent.
    pub async fn hosts_by_ids(&self, hostids: &[String]) -> Result<Vec<HostDetails>> {
        check_ids(hostids, "hostid")?;
        let mut hosts = Vec::with_capacity(hostids.len());
        for batch in hostids.chunks(LOOKUP_BATCH) {
            let raw: Vec<Value> = self
                .read_typed(
                    ReadMethod::HostGet,
                    json!({"output": ["hostid", "host", "name"], "hostids": batch}),
                )
                .await?;
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
                .read_typed(
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
        if limit == 0 || limit > MAX_HOST_SEARCH {
            return Err(ZabbixError::SnapshotShape(
                "host search limit is out of range".to_owned(),
            ));
        }
        if search.chars().count() > MAX_HOST_SEARCH_CHARS || search.chars().any(char::is_control) {
            return Err(ZabbixError::SnapshotShape(
                "host search text is too long or has control characters".to_owned(),
            ));
        }
        let mut params = json!({
            "output": ["hostid", "host", "name"],
            "selectHostGroups": ["name"],
            "selectParentTemplates": ["templateid", "name"],
            "sortfield": "name",
            "limit": limit + 1
        });
        if !search.is_empty() {
            params["search"] = json!({"host": search, "name": search});
            params["searchByAny"] = json!(true);
        }
        let raw: Vec<Value> = self.read_typed(ReadMethod::HostGet, params).await?;
        let truncated = raw.len() > limit;
        let objects = raw
            .iter()
            .take(limit)
            .map(parse_host_listing)
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
                .read_typed(
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
        let mut items = Vec::with_capacity(itemids.len());
        for batch in itemids.chunks(LOOKUP_BATCH) {
            let raw: Vec<Value> = self
                .read_typed(
                    ReadMethod::ItemGet,
                    json!({
                        "output": ITEM_OUTPUT,
                        "selectItemDiscovery": ["parent_itemid", "key_"],
                        "selectHosts": ["hostid", "name"],
                        "itemids": batch,
                        "webitems": true
                    }),
                )
                .await?;
            for item in &raw {
                items.push(parse_item(item)?);
            }
        }
        Ok(items)
    }

    /// All items of one host (plain and discovered) with matching evidence.
    pub async fn host_items(&self, hostid: &str) -> Result<HostObjects<ItemDetails>> {
        check_ids(&[hostid.to_owned()], "hostid")?;
        let raw: Vec<Value> = self
            .read_typed(
                ReadMethod::ItemGet,
                json!({
                    "output": ITEM_OUTPUT,
                    "selectItemDiscovery": ["parent_itemid", "key_"],
                    "hostids": [hostid],
                    "webitems": true,
                    "sortfield": "itemid",
                    "limit": MAX_HOST_OBJECTS + 1
                }),
            )
            .await?;
        let truncated = raw.len() > MAX_HOST_OBJECTS;
        let objects = raw
            .iter()
            .take(MAX_HOST_OBJECTS)
            .map(parse_item)
            .collect::<Result<_>>()?;
        Ok(HostObjects { objects, truncated })
    }

    /// Classic graphs by ID (`graph.get`). Prototypes are not returned.
    pub async fn graphs_by_ids(&self, graphids: &[String]) -> Result<Vec<GraphDetails>> {
        check_ids(graphids, "graphid")?;
        let mut graphs = Vec::with_capacity(graphids.len());
        for batch in graphids.chunks(LOOKUP_BATCH) {
            let raw: Vec<Value> = self
                .read_typed(
                    ReadMethod::GraphGet,
                    json!({
                        "output": ["graphid", "name", "templateid", "flags"],
                        "selectHosts": ["hostid"],
                        "graphids": batch
                    }),
                )
                .await?;
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
            .read_typed(
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
        let mut prototypes = Vec::with_capacity(itemids.len());
        for batch in itemids.chunks(LOOKUP_BATCH) {
            let raw: Vec<Value> = self
                .read_typed(
                    ReadMethod::ItemPrototypeGet,
                    json!({
                        "output": ITEM_PROTOTYPE_OUTPUT,
                        "selectHosts": ["hostid", "name"],
                        "selectDiscoveryRule": ["itemid"],
                        "itemids": batch
                    }),
                )
                .await?;
            for prototype in &raw {
                prototypes.push(parse_item_prototype(prototype)?);
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
        let mut prototypes = Vec::with_capacity(graphids.len());
        for batch in graphids.chunks(LOOKUP_BATCH) {
            let raw: Vec<Value> = self
                .read_typed(
                    ReadMethod::GraphPrototypeGet,
                    json!({
                        "output": ["graphid", "name", "templateid"],
                        "selectHosts": ["hostid", "name"],
                        "selectDiscoveryRule": ["itemid"],
                        "graphids": batch
                    }),
                )
                .await?;
            for prototype in &raw {
                prototypes.push(parse_graph_prototype(prototype)?);
            }
        }
        Ok(prototypes)
    }

    async fn read_typed(&self, method: ReadMethod, params: Value) -> Result<Vec<Value>> {
        self.session
            .transport
            .read(method, params, &self.session.token)
            .await
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

fn parse_item(item: &Value) -> Result<ItemDetails> {
    let value_type = number(item.get("value_type"))
        .and_then(|number| u8::try_from(number).ok())
        .filter(|number| *number <= 5)
        .ok_or_else(|| ZabbixError::SnapshotShape("item has no valid value_type".to_owned()))?;
    let flags = number(item.get("flags"))
        .map(ItemFlags::from_number)
        .ok_or_else(|| ZabbixError::SnapshotShape("item has no numeric flags".to_owned()))?;
    let discovery = match item.get("itemDiscovery") {
        Some(link @ Value::Object(_)) if link.get("parent_itemid").is_some() => {
            Some(ItemDiscoveryLink {
                parent_itemid: id_field(link, "parent_itemid", "item discovery")?,
                prototype_key: link
                    .get("key_")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
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

fn parse_item_prototype(prototype: &Value) -> Result<ItemPrototypeDetails> {
    let value_type = number(prototype.get("value_type"))
        .and_then(|number| u8::try_from(number).ok())
        .filter(|number| *number <= 5)
        .ok_or_else(|| {
            ZabbixError::SnapshotShape("item prototype has no valid value_type".to_owned())
        })?;
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
        ItemFlags, parse_graph, parse_graph_prototype, parse_host, parse_host_listing, parse_item,
        parse_item_prototype,
    };

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
