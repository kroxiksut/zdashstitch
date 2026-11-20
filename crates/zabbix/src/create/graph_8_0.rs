// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};

use super::{
    CreateRejection, CreateRejectionCode, CreateRequirements, ReadWriteGateway,
    VerifiedDashboardCreate, WIDGET_KEYS, ZabbixVersion, content_fingerprint,
    is_valid_dashboard_name, shape, text,
};
use crate::{ReadMethod, ReadOnlyGateway, is_zabbix_id};

pub(crate) mod patterns;

pub(super) const CONTRACT: &str = "create_portable_graph_8_0_0";
pub const MAX_PORTABLE_GRAPH_PAGES: usize = 50;

pub(super) const fn exact_version(version: &ZabbixVersion) -> bool {
    version.major == 8 && version.minor == 0 && version.patch == 0
}

#[derive(Eq, PartialEq)]
struct Inputs {
    roleid: String,
    items: Vec<Value>,
    hosts: Vec<Value>,
    discovered_origins: Vec<(String, String)>,
}

#[derive(Clone, Copy)]
enum GraphCreateRole {
    Admin,
    SuperAdmin,
}

impl GraphCreateRole {
    async fn read(self, reader: &ReadOnlyGateway) -> crate::Result<Option<String>> {
        match self {
            Self::Admin => reader.eight_zero_update_role().await,
            Self::SuperAdmin => reader.eight_zero_pattern_create_role().await,
        }
    }
}

pub(super) struct ScopedGraphCreate {
    evidence: Vec<GraphEvidence>,
    widget_count: usize,
    attempted: AtomicBool,
}

struct GraphEvidence {
    sources: GraphSources,
    inputs: GraphInputs,
}

#[derive(Clone, Copy)]
pub(super) enum PortableUpdateSources {
    Ordinary,
    ThreeOrdinary,
    Discovered,
    PatternColor(&'static str),
}

impl ScopedGraphCreate {
    pub(super) fn role_id(&self) -> &str {
        self.evidence[0].inputs.role_id()
    }

    pub(super) fn proves_two_ordinary_rows(&self, fields: &[Value]) -> bool {
        fields.len() == 5
            && graph_item_ids(fields).is_some_and(|ids| {
                ids.len() == 2
                    && self.evidence.iter().any(|evidence| {
                        matches!((&evidence.sources, &evidence.inputs),
                        (GraphSources::Explicit(expected), GraphInputs::Explicit(inputs))
                        if expected == &ids && inputs.discovered_origins.is_empty())
                    })
            })
    }

    pub(super) fn portable_update_sources(
        &self,
        field_count: usize,
    ) -> Option<PortableUpdateSources> {
        if self.widget_count != 1 {
            return None;
        }
        let evidence = self.evidence.first()?;
        match (&evidence.sources, &evidence.inputs) {
            (GraphSources::Explicit(items), GraphInputs::Explicit(inputs))
                if field_count == 7
                    && items.len() == 3
                    && inputs.hosts.len() == 3
                    && inputs.discovered_origins.is_empty() =>
            {
                Some(PortableUpdateSources::ThreeOrdinary)
            }
            (GraphSources::Explicit(items), GraphInputs::Explicit(inputs))
                if field_count == 5 && items.len() == 2 =>
            {
                match inputs.discovered_origins.len() {
                    0 => Some(PortableUpdateSources::Ordinary),
                    2 => Some(PortableUpdateSources::Discovered),
                    _ => None,
                }
            }
            (GraphSources::Patterns(_), GraphInputs::Patterns(_))
                if matches!(field_count, 5 | 6 | 10 | 12) =>
            {
                Some(PortableUpdateSources::PatternColor("ds.0.color"))
            }
            (GraphSources::PairedPatterns(_), GraphInputs::PairedPatterns(_))
                if field_count == 12 =>
            {
                Some(PortableUpdateSources::PatternColor("ds.1.color"))
            }
            (
                GraphSources::Mixed {
                    pattern_dataset, ..
                },
                GraphInputs::Mixed(..),
            ) if field_count == 11 => Some(PortableUpdateSources::PatternColor(
                if *pattern_dataset == 0 {
                    "ds.0.color"
                } else {
                    "ds.1.color"
                },
            )),
            _ => None,
        }
    }

    pub(super) async fn recheck(&self, client: &ReadWriteGateway) -> bool {
        for evidence in &self.evidence {
            if !matches!(evidence.sources.read_inputs(client).await, Ok(Some(inputs)) if inputs == evidence.inputs)
            {
                return false;
            }
        }
        true
    }

    pub(super) fn contract(&self) -> &'static str {
        if self.widget_count == 1 {
            self.evidence[0].sources.contract()
        } else {
            "create_portable_graph_composition_8_0_0"
        }
    }

    pub(super) fn reserve_send(&self) -> bool {
        self.attempted
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

#[derive(Eq, PartialEq)]
enum GraphSources {
    Explicit(Vec<String>),
    Patterns(patterns::PatternSpec),
    PairedPatterns([patterns::PatternSpec; 2]),
    Mixed {
        explicit: [String; 2],
        patterns: patterns::PatternSpec,
        pattern_dataset: u32,
    },
}

#[derive(Eq, PartialEq)]
enum GraphInputs {
    Explicit(Inputs),
    Patterns(patterns::PatternInputs),
    PairedPatterns([patterns::PatternInputs; 2]),
    Mixed(Inputs, patterns::PatternInputs),
}

impl GraphInputs {
    fn role_id(&self) -> &str {
        match self {
            Self::Explicit(inputs) | Self::Mixed(inputs, _) => &inputs.roleid,
            Self::Patterns(inputs) | Self::PairedPatterns([inputs, _]) => inputs.role_id(),
        }
    }
}

impl GraphSources {
    fn from_document(document: &Value, readback: bool) -> Option<Self> {
        let fields = graph_fields(document, readback)?;
        let first = uint(field_value(fields, "ds.0.dataset_type", 0)?)?;
        let second = field_value(fields, "ds.1.dataset_type", 0).and_then(uint);
        match (first, second) {
            (0, Some(1)) => Self::mixed(fields, 1, 0),
            (1, Some(0)) => Self::mixed(fields, 0, 1),
            (1, Some(1)) => Self::paired_patterns(fields),
            (0, _) => graph_item_ids(fields).map(Self::Explicit),
            (1, None) => patterns::PatternSpec::from_fields(fields, 0).map(Self::Patterns),
            _ => None,
        }
    }

    fn mixed(fields: &[Value], pattern_dataset: u32, explicit_dataset: usize) -> Option<Self> {
        // One plain three-mask data set and one data set of two explicit rows.
        if fields.len() != 11 {
            return None;
        }
        let patterns = plain_three_host_pattern(fields, pattern_dataset)?;
        let explicit: [String; 2] = explicit_dataset_item_ids(fields, explicit_dataset, 2)?
            .try_into()
            .ok()?;
        (explicit[0] != explicit[1]).then_some(Self::Mixed {
            explicit,
            patterns,
            pattern_dataset,
        })
    }

    fn paired_patterns(fields: &[Value]) -> Option<Self> {
        if fields.len() != 12 {
            return None;
        }
        Some(Self::PairedPatterns([
            plain_three_host_pattern(fields, 0)?,
            plain_three_host_pattern(fields, 1)?,
        ]))
    }

    fn contract(&self) -> &'static str {
        match self {
            Self::Explicit(_) => CONTRACT,
            Self::Patterns(_) => "create_portable_graph_patterns_8_0_0",
            Self::PairedPatterns(_) => "create_portable_graph_paired_patterns_8_0_0",
            Self::Mixed { .. } => "create_portable_graph_mixed_8_0_0",
        }
    }

    async fn read_inputs(&self, client: &ReadWriteGateway) -> crate::Result<Option<GraphInputs>> {
        Ok(match self {
            Self::Explicit(ids) => client
                .graph_create_inputs(ids, GraphCreateRole::Admin)
                .await?
                .map(GraphInputs::Explicit),
            Self::Patterns(spec) => patterns::read_inputs(client.reader(), spec)
                .await?
                .map(GraphInputs::Patterns),
            Self::PairedPatterns([first, second]) => {
                let Some(first) = patterns::read_inputs(client.reader(), first).await? else {
                    return Ok(None);
                };
                let Some(second) = patterns::read_inputs(client.reader(), second).await? else {
                    return Ok(None);
                };
                first
                    .accepts_companion(&second)
                    .then_some(GraphInputs::PairedPatterns([first, second]))
            }
            Self::Mixed {
                explicit, patterns, ..
            } => {
                let Some(explicit) = client
                    .graph_create_inputs(explicit, GraphCreateRole::SuperAdmin)
                    .await?
                else {
                    return Ok(None);
                };
                let Some(patterns) = patterns::read_inputs(client.reader(), patterns).await? else {
                    return Ok(None);
                };
                patterns
                    .accepts_explicit(&explicit)
                    .then_some(GraphInputs::Mixed(explicit, patterns))
            }
        })
    }
}

pub(super) fn graph_readback_supported(document: &Value) -> bool {
    split_graph_documents(document).is_some_and(|widgets| {
        widgets
            .iter()
            .all(|widget| GraphSources::from_document(widget, true).is_some())
    })
}

fn split_graph_documents(document: &Value) -> Option<Vec<Value>> {
    let dashboard = document.as_object()?;
    let pages = dashboard.get("pages")?.as_array()?;
    if !(1..=MAX_PORTABLE_GRAPH_PAGES).contains(&pages.len()) {
        return None;
    }
    let dashboard_without_pages: serde_json::Map<_, _> = dashboard
        .iter()
        .filter(|(key, _)| key.as_str() != "pages")
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let mut single_widgets = Vec::new();
    for page in pages {
        let page = page.as_object()?;
        let widgets = page.get("widgets")?.as_array()?;
        if !(1..=super::MAX_PORTABLE_APPEND_WIDGETS).contains(&widgets.len()) {
            return None;
        }
        let page_without_widgets: serde_json::Map<_, _> = page
            .iter()
            .filter(|(key, _)| key.as_str() != "widgets")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        for (index, widget) in widgets.iter().enumerate() {
            if !super::portable_update::has_room(&widgets[..index], widget)? {
                return None;
            }
            let mut isolated_page = page_without_widgets.clone();
            isolated_page.insert("widgets".into(), json!([widget]));
            let mut isolated = dashboard_without_pages.clone();
            isolated.insert("pages".into(), json!([isolated_page]));
            single_widgets.push(Value::Object(isolated));
        }
    }
    Some(single_widgets)
}

pub(super) fn allowed_keys(value: &Value, required: &[&str], extra: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        required.iter().all(|key| object.contains_key(*key))
            && object
                .keys()
                .all(|key| required.contains(&key.as_str()) || extra.contains(&key.as_str()))
    })
}

fn uint(value: &Value) -> Option<u32> {
    text(Some(value))?.parse().ok()
}

/// A single page with a single Graph. Generated IDs are accepted only when
/// reading the resulting dashboard.
fn graph_fields(document: &Value, readback: bool) -> Option<&[Value]> {
    if !readback
        && !allowed_keys(
            document,
            &["name", "display_period", "auto_start", "pages"],
            &[],
        )
    {
        return None;
    }
    let [page] = document["pages"].as_array()?.as_slice() else {
        return None;
    };
    let [widget] = page["widgets"].as_array()?.as_slice() else {
        return None;
    };
    let mut widget_keys = WIDGET_KEYS.to_vec();
    widget_keys.push("fields");
    if !allowed_keys(
        page,
        &["name", "display_period", "widgets"],
        if readback { &["dashboard_pageid"] } else { &[] },
    ) || !allowed_keys(
        widget,
        &widget_keys,
        if readback { &["widgetid"] } else { &[] },
    ) || page["name"].as_str().is_none()
        || widget["name"].as_str().is_none()
        || widget["type"] != "svggraph"
        || uint(&document["display_period"]) != Some(30)
        || uint(&document["auto_start"]) != Some(1)
        || uint(&page["display_period"]) != Some(0)
        || uint(&widget["view_mode"]) != Some(0)
    {
        return None;
    }
    let (x, y, w, h) = (
        uint(&widget["x"])?,
        uint(&widget["y"])?,
        uint(&widget["width"])?,
        uint(&widget["height"])?,
    );
    if w == 0 || h == 0 || x.checked_add(w)? > 72 || y.checked_add(h)? > 64 {
        return None;
    }
    let fields = widget["fields"].as_array()?;
    if !(5..=32).contains(&fields.len())
        || fields
            .iter()
            .any(|field| !allowed_keys(field, &["name", "type", "value"], &[]))
    {
        return None;
    }
    Some(fields)
}

fn field_value<'a>(fields: &'a [Value], name: &str, kind: u32) -> Option<&'a Value> {
    let mut matching = fields.iter().filter(|field| field["name"] == name);
    let found = matching.next()?;
    (matching.next().is_none() && uint(&found["type"]) == Some(kind)).then_some(&found["value"])
}

fn in_dataset(field: &Value, prefix: &str) -> bool {
    field["name"]
        .as_str()
        .is_some_and(|name| name.starts_with(prefix))
}

fn plain_three_host_pattern(fields: &[Value], dataset: u32) -> Option<patterns::PatternSpec> {
    let prefix = format!("ds.{dataset}.");
    let rows: Vec<_> = fields
        .iter()
        .filter(|field| in_dataset(field, &prefix))
        .cloned()
        .collect();
    if rows.len() != 6 {
        return None;
    }
    patterns::PatternSpec::from_fields(&rows, dataset)
}

pub(super) fn explicit_dataset_item_ids(
    fields: &[Value],
    dataset: usize,
    max: usize,
) -> Option<Vec<String>> {
    let prefix = format!("ds.{dataset}.");
    let count = fields
        .iter()
        .filter(|field| in_dataset(field, &prefix))
        .count();
    if count < 3
        || count % 2 != 1
        || count / 2 > max
        || uint(field_value(fields, &format!("{prefix}dataset_type"), 0)?) != Some(0)
    {
        return None;
    }
    let mut ids = Vec::with_capacity(count / 2);
    for slot in 0..count / 2 {
        let color = field_value(fields, &format!("{prefix}color.{slot}"), 1)?.as_str()?;
        let id = text(Some(field_value(
            fields,
            &format!("{prefix}itemids.{slot}"),
            4,
        )?))?;
        if color.len() != 6
            || !color.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !is_zabbix_id(&id)
        {
            return None;
        }
        ids.push(id);
    }
    Some(ids)
}

fn graph_item_ids(fields: &[Value]) -> Option<Vec<String>> {
    let datasets = if field_value(fields, "ds.1.dataset_type", 0).is_some() {
        2
    } else {
        1
    };
    let max = if datasets == 1 { 6 } else { 3 };
    let mut ids = Vec::new();
    for dataset in 0..datasets {
        ids.extend(explicit_dataset_item_ids(fields, dataset, max)?);
    }
    (fields.len() == datasets + 2 * ids.len()
        && (2..=6).contains(&ids.len())
        && ids.iter().collect::<BTreeSet<_>>().len() == ids.len())
    .then_some(ids)
}

pub(super) async fn monitored_hosts(
    reader: &ReadOnlyGateway,
    mut hostids: Vec<&str>,
) -> crate::Result<Option<Vec<Value>>> {
    hostids.sort_unstable();
    if hostids.windows(2).any(|window| window[0] == window[1]) {
        return Ok(None);
    }
    let mut hosts = reader
        .read_inventory_rows(
            ReadMethod::HostGet,
            json!({"hostids":hostids,"output":["hostid","status"]}),
        )
        .await?;
    hosts.sort_by(|a, b| a["hostid"].as_str().cmp(&b["hostid"].as_str()));
    if hosts.len() != hostids.len()
        || hosts
            .iter()
            .zip(hostids)
            .any(|(host, id)| host["hostid"] != id || uint(&host["status"]) != Some(0))
    {
        return Ok(None);
    }
    Ok(Some(hosts))
}

pub(crate) async fn current_lld_origin(
    reader: &ReadOnlyGateway,
    item: &Value,
) -> crate::Result<Option<(String, String)>> {
    let discovery = &item["discoveryData"];
    let Some(parent) = discovery["parent_itemid"]
        .as_str()
        .filter(|id| is_zabbix_id(id))
    else {
        return Ok(None);
    };
    if uint(&discovery["status"]) != Some(0) || discovery["key_"].as_str().is_none_or(str::is_empty)
    {
        return Ok(None);
    }
    let prototypes = reader.item_prototypes_by_ids(&[parent.to_owned()]).await?;
    if !matches!(prototypes.as_slice(), [prototype]
        if prototype.itemid == parent && prototype.templateid.is_some()
            && item["hostid"] == prototype.hostid.as_str()
            && discovery["key_"] == prototype.key.as_str()
            && item["units"] == prototype.units.as_str()
            && uint(&item["value_type"]) == Some(u32::from(prototype.value_type)))
    {
        return Ok(None);
    }
    reader.portable_prototype_identity(parent).await
}

impl ReadWriteGateway {
    async fn graph_create_inputs(
        &self,
        itemids: &[String],
        role: GraphCreateRole,
    ) -> crate::Result<Option<Inputs>> {
        let reader = self.reader();
        let Some(roleid) = role.read(reader).await? else {
            return Ok(None);
        };
        let mut expected = itemids.to_vec();
        expected.sort();
        let mut items = reader.read_inventory_rows(ReadMethod::ItemGet, json!({
            "itemids":itemids,"output":["itemid","hostid","key_","value_type","units","status","flags"],
            "selectDiscoveryData":["parent_itemid","key_","status"],
        })).await?;
        items.sort_by(|a, b| a["itemid"].as_str().cmp(&b["itemid"].as_str()));
        if items.len() != itemids.len()
            || items.iter().zip(&expected).any(|(item, id)| {
                item["itemid"] != id.as_str()
                    || item["key_"].as_str().is_none_or(str::is_empty)
                    || !matches!(uint(&item["value_type"]), Some(0 | 3))
                    || uint(&item["status"]) != Some(0)
                    || !matches!(uint(&item["flags"]), Some(0 | 4))
            })
        {
            return Ok(None);
        }
        let mut discovered_origins = Vec::new();
        for item in items.iter().filter(|item| uint(&item["flags"]) == Some(4)) {
            if discovered_origins.len() == 2 {
                return Ok(None);
            }
            let Some(origin) = current_lld_origin(reader, item).await? else {
                return Ok(None);
            };
            discovered_origins.push(origin);
        }
        let hostids = items
            .iter()
            .map(|item| item["hostid"].as_str().filter(|id| is_zabbix_id(id)))
            .collect::<Option<BTreeSet<_>>>();
        let Some(hostids) = hostids else {
            return Ok(None);
        };
        if !(2..=3).contains(&hostids.len()) {
            return Ok(None);
        }
        let Some(hosts) = monitored_hosts(reader, hostids.into_iter().collect()).await? else {
            return Ok(None);
        };
        if role.read(reader).await?.as_deref() != Some(&roleid) {
            return Ok(None);
        }
        Ok(Some(Inputs {
            roleid,
            items,
            hosts,
            discovered_origins,
        }))
    }

    /// Issue the 8.0.0 adapter token only for the independently verified
    /// Graph shape, live dashboard role and distinct monitored hosts.
    pub async fn verify_eight_zero_portable_graph_create(
        &self,
        document: &Value,
        connection_id: &str,
        local_fingerprint: &str,
        name: &str,
    ) -> std::result::Result<VerifiedDashboardCreate, CreateRejection> {
        let unverified = || {
            CreateRejection::new(CreateRejectionCode::ContractUnverified).with("contract", CONTRACT)
        };
        let context = self.publication_context();
        let version = &context.capabilities().version;
        if !exact_version(version) || !context.is_live_authenticated() {
            return Err(unverified());
        }
        if !is_valid_dashboard_name(name) {
            return Err(CreateRejection::new(CreateRejectionCode::NameInvalid));
        }
        let widgets = split_graph_documents(document).ok_or_else(|| shape("widget"))?;
        let widget_count = widgets.len();
        let mut evidence: Vec<GraphEvidence> = Vec::new();
        for widget in widgets {
            let sources =
                GraphSources::from_document(&widget, false).ok_or_else(|| shape("widget"))?;
            if evidence.iter().any(|verified| verified.sources == sources) {
                continue;
            }
            let inputs = sources
                .read_inputs(self)
                .await
                .ok()
                .flatten()
                .ok_or_else(|| {
                    CreateRejection::new(CreateRejectionCode::ContractUnverified)
                        .with("contract", sources.contract())
                })?;
            if evidence
                .first()
                .is_some_and(|verified| verified.inputs.role_id() != inputs.role_id())
            {
                return Err(unverified());
            }
            evidence.push(GraphEvidence { sources, inputs });
        }
        let mut params = document.clone();
        params["name"] = json!(name);
        params["private"] = json!("1");
        let fingerprint = content_fingerprint(&params).ok_or_else(|| shape("dashboard"))?;
        let page_count = params["pages"]
            .as_array()
            .ok_or_else(|| shape("dashboard"))?
            .len();
        Ok(VerifiedDashboardCreate {
            portable: true,
            access: None,
            scoped_graph: Some(ScopedGraphCreate {
                evidence,
                widget_count,
                attempted: AtomicBool::new(false),
            }),
            scoped_clone: None,
            connection_id: connection_id.to_owned(),
            source_dashboardid: String::new(),
            server_version: version.clone(),
            authority_fingerprint: context.binding_fingerprint().to_owned(),
            local_fingerprint: local_fingerprint.to_owned(),
            name: name.to_owned(),
            create_params: params,
            content_fingerprint: fingerprint,
            requirements: CreateRequirements {
                contracts: Default::default(),
                unknown_widgets: Vec::new(),
                page_count,
                widget_count,
                reference_definitions: 0,
                reference_consumers: 0,
                links_outside_page: Vec::new(),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_composition_uses_the_verified_page_and_widget_bounds() {
        let widgets: Vec<_> = (0..256)
            .map(|widget| {
                json!({
                    "x": (widget % 18) * 4, "y": (widget / 18) * 4, "width": 4, "height": 4,
                })
            })
            .collect();
        let mut document = json!({"pages":[{"widgets":widgets}]});
        assert_eq!(
            split_graph_documents(&document).map(|graphs| graphs.len()),
            Some(256)
        );
        document["pages"][0]["widgets"]
            .as_array_mut()
            .unwrap_or_else(|| panic!("widgets"))
            .push(json!({"x":64,"y":60,"width":4,"height":4}));
        assert!(split_graph_documents(&document).is_none());
        let page = json!({"widgets":[widgets[0]]});
        document["pages"] = json!(vec![page.clone(); 50]);
        assert_eq!(
            split_graph_documents(&document).map(|graphs| graphs.len()),
            Some(50)
        );
        document["pages"]
            .as_array_mut()
            .unwrap_or_else(|| panic!("pages"))
            .push(page);
        assert!(split_graph_documents(&document).is_none());
    }

    #[test]
    fn portable_graph_shape_rejects_unknown_fields_references_and_duplicates() {
        let graph = json!({"name":"Graph","display_period":"30","auto_start":"1",
            "pages":[{"name":"Page","display_period":"0","widgets":[{
                "type":"svggraph","name":"Graph","x":"0","y":"0","width":"18","height":"4","view_mode":"0",
                "fields":[
                    {"name":"ds.0.dataset_type","type":"0","value":"0"},
                    {"name":"ds.0.itemids.0","type":"4","value":"11"},
                    {"name":"ds.0.color.0","type":"1","value":"FF0000"},
                    {"name":"ds.0.itemids.1","type":"4","value":"12"},
                    {"name":"ds.0.color.1","type":"1","value":"0040FF"},
                ]
            }]}],
        });
        assert!(
            matches!(GraphSources::from_document(&graph, false), Some(GraphSources::Explicit(ids)) if ids == ["11", "12"])
        );
        let mut unknown = graph.clone();
        unknown["pages"][0]["widgets"][0]["fields"][4]["name"] = json!("vendor.unknown");
        assert!(GraphSources::from_document(&unknown, false).is_none());
        let mut reference = graph.clone();
        reference["pages"][0]["widgets"][0]["fields"][1]["name"] =
            json!("ds.0.itemids.0._reference");
        assert!(GraphSources::from_document(&reference, false).is_none());
        let mut duplicate = graph;
        duplicate["pages"][0]["widgets"][0]["fields"][3]["value"] = json!("11");
        assert!(GraphSources::from_document(&duplicate, false).is_none());
    }

    #[test]
    fn explicit_datasets_require_contiguous_rows_and_globally_distinct_items() {
        let fields = json!([
            {"name":"ds.0.dataset_type","type":"0","value":"0"},
            {"name":"ds.0.itemids.0","type":"4","value":"11"},
            {"name":"ds.0.color.0","type":"1","value":"FF0000"},
            {"name":"ds.1.dataset_type","type":"0","value":"0"},
            {"name":"ds.1.itemids.0","type":"4","value":"12"},
            {"name":"ds.1.color.0","type":"1","value":"0040FF"},
            {"name":"ds.1.itemids.1","type":"4","value":"13"},
            {"name":"ds.1.color.1","type":"1","value":"0040FF"},
        ]);
        let fields = fields.as_array().unwrap_or_else(|| unreachable!());
        assert_eq!(
            graph_item_ids(fields),
            Some(vec!["11".into(), "12".into(), "13".into()])
        );
        let mut duplicate = fields.clone();
        duplicate[4]["value"] = json!("11");
        assert!(graph_item_ids(&duplicate).is_none());
        let mut gap = fields.clone();
        gap[6]["name"] = json!("ds.1.itemids.2");
        assert!(graph_item_ids(&gap).is_none());
        let mut mixed = fields.clone();
        mixed[3]["value"] = json!("1");
        assert!(graph_item_ids(&mixed).is_none());
    }

    #[test]
    fn mixed_datasets_require_three_plain_masks_and_two_distinct_explicit_rows() {
        let fields = json!([
            {"name":"ds.0.dataset_type","type":"0","value":"1"},
            {"name":"ds.0.items.0","type":"1","value":"*ping*"},
            {"name":"ds.0.color","type":"1","value":"FF0000"},
            {"name":"ds.0.hosts.0","type":"1","value":"First*"},
            {"name":"ds.0.hosts.1","type":"1","value":"Second*"},
            {"name":"ds.0.hosts.2","type":"1","value":"Third*"},
            {"name":"ds.1.dataset_type","type":"0","value":"0"},
            {"name":"ds.1.itemids.0","type":"4","value":"11"},
            {"name":"ds.1.color.0","type":"1","value":"0040FF"},
            {"name":"ds.1.itemids.1","type":"4","value":"12"},
            {"name":"ds.1.color.1","type":"1","value":"00B85C"},
        ]);
        let fields = fields.as_array().unwrap_or_else(|| unreachable!());
        assert!(
            matches!(GraphSources::mixed(fields, 0, 1), Some(GraphSources::Mixed { explicit, .. }) if explicit == ["11", "12"])
        );
        let mut duplicate = fields.clone();
        duplicate[9]["value"] = json!("11");
        assert!(GraphSources::mixed(&duplicate, 0, 1).is_none());
        let mut with_tag = fields.clone();
        with_tag[5] = json!({"name":"ds.0.item_tags_evaltype","type":"0","value":"0"});
        assert!(GraphSources::mixed(&with_tag, 0, 1).is_none());
        let mut reversed = fields.clone();
        for field in &mut reversed {
            let name = field["name"].as_str().unwrap_or_else(|| unreachable!());
            field["name"] = if let Some(suffix) = name.strip_prefix("ds.0.") {
                json!(format!("ds.1.{suffix}"))
            } else {
                json!(name.replacen("ds.1.", "ds.0.", 1))
            };
        }
        assert!(
            matches!(GraphSources::mixed(&reversed, 1, 0), Some(GraphSources::Mixed { explicit, .. }) if explicit == ["11", "12"])
        );
        assert!(GraphSources::mixed(&reversed, 0, 1).is_none());
        assert!(graph_item_ids(&reversed).is_none());
    }

    #[test]
    fn paired_patterns_require_three_plain_masks_in_each_dataset() {
        let mut fields = Vec::new();
        for (dataset, item) in [(0, "*ping*"), (1, "*uptime*")] {
            fields.extend([
                json!({"name":format!("ds.{dataset}.dataset_type"),"type":"0","value":"1"}),
                json!({"name":format!("ds.{dataset}.items.0"),"type":"1","value":item}),
                json!({"name":format!("ds.{dataset}.color"),"type":"1","value":"FF0000"}),
            ]);
            for (index, host) in ["First*", "Second*", "Third*"].into_iter().enumerate() {
                fields.push(
                    json!({"name":format!("ds.{dataset}.hosts.{index}"),"type":"1","value":host}),
                );
            }
        }
        assert!(matches!(
            GraphSources::paired_patterns(&fields),
            Some(GraphSources::PairedPatterns(_))
        ));
        fields[11]["name"] = json!("ds.1.item_tags.0.tag");
        assert!(GraphSources::paired_patterns(&fields).is_none());
        fields.pop();
        assert!(GraphSources::paired_patterns(&fields).is_none());
    }
}
