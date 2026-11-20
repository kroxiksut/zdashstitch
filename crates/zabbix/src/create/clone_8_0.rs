// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};

use super::{
    CreateRejection, CreateRejectionCode, CreateScope, ReadWriteGateway, VerifiedDashboardCreate,
    VersionedDashboardSnapshot, WIDGET_KEYS, graph_8_0, prepare, shape, text,
};
use crate::{
    DashboardAccessSelection, ReadMethod, ReadOnlyGateway, dashboard_access::SharingRole,
    is_zabbix_id,
};
use graph_8_0::patterns::{PatternInputs, PatternSpec};

pub(super) const CONTRACT: &str = "clone_dashboard_8_0_0";

pub(super) struct ScopedCloneCreate {
    pub(super) source_fingerprint: String,
    pub(super) require_editable_source: bool,
    local: CheckedContent,
    source: Option<CheckedContent>,
    edited_workcopy: bool,
    attempted: AtomicBool,
}

struct CheckedContent {
    scope: CopyContent,
    evidence: Evidence,
}

#[derive(Default, Eq, PartialEq)]
pub(super) struct CopyContent {
    items: BTreeSet<String>,
    item_value_ids: BTreeSet<String>,
    explicit_sets: Vec<Vec<String>>,
    ordered_explicit: bool,
    patterns: Vec<PatternSpec>,
    default_clock: bool,
}

#[derive(Eq, PartialEq)]
struct Evidence {
    role: SharingRole,
    items: Vec<Value>,
    hosts: Vec<Value>,
    discovered_origins: Vec<Vec<DiscoveredOrigin>>,
    patterns: Vec<PatternInputs>,
}

#[derive(Eq, PartialEq)]
struct DiscoveredOrigin {
    row: Option<usize>,
    root: (String, String),
}

fn number(value: &Value) -> Option<u32> {
    text(Some(value))?.parse().ok()
}

fn reference(value: &str) -> bool {
    value.len() == 5 && value.bytes().all(|byte| byte.is_ascii_uppercase())
}

fn explicit_sets(fields: &[Value]) -> Option<Vec<Vec<String>>> {
    let first = graph_8_0::explicit_dataset_item_ids(fields, 0, 3)?;
    if first.len() >= 2 && fields.len() == 1 + 2 * first.len() {
        return Some(vec![first]);
    }
    let second = graph_8_0::explicit_dataset_item_ids(fields, 1, 2)?;
    (first.len() == 2 && second.len() == 2 && fields.len() == 10).then_some(vec![first, second])
}

// The 8.0.0 ReplaceHost copy probe used the default Clock on the Item value
// page and two linked Graphs on the other page. Do not allow it in other
// copy compositions solely because a default Clock is accepted on the wire.
fn default_clock_copy_shape(pages: &[Value], content: &CopyContent) -> bool {
    let [signals, graphs] = pages else {
        return false;
    };
    let signals = signals["widgets"].as_array().map_or(&[][..], Vec::as_slice);
    let graphs = graphs["widgets"].as_array().map_or(&[][..], Vec::as_slice);
    let ([item, clock], [provider, consumer], [first, second]) =
        (signals, graphs, content.explicit_sets.as_slice())
    else {
        return false;
    };
    let has_field = |widget: &Value, field: &str| {
        widget["fields"]
            .as_array()
            .is_some_and(|fields| fields.iter().any(|row| row["name"] == field))
    };
    item["type"] == "item"
        && clock["type"] == "clock"
        && provider["type"] == "svggraph"
        && consumer["type"] == "svggraph"
        && has_field(provider, "reference")
        && has_field(consumer, "time_period._reference")
        && first.len() == 2
        && first.iter().eq(second.iter().rev())
        && content.item_value_ids.iter().all(|id| first.contains(id))
}

pub(super) fn fresh_ids(
    source: &VersionedDashboardSnapshot,
    copy: &VersionedDashboardSnapshot,
) -> bool {
    fn collect(document: &Value) -> Option<(BTreeSet<&str>, BTreeSet<&str>)> {
        let mut pages = BTreeSet::new();
        let mut widgets = BTreeSet::new();
        for page in document["pages"].as_array()? {
            let page_id = page["dashboard_pageid"]
                .as_str()
                .filter(|id| is_zabbix_id(id))?;
            if !pages.insert(page_id) {
                return None;
            }
            for widget in page["widgets"].as_array()? {
                let widget_id = widget["widgetid"].as_str().filter(|id| is_zabbix_id(id))?;
                if !widgets.insert(widget_id) {
                    return None;
                }
            }
        }
        Some((pages, widgets))
    }
    matches!((collect(source.raw_api_document()), collect(copy.raw_api_document())),
        (Some((source_pages, source_widgets)), Some((copy_pages, copy_widgets)))
            if source.dashboard_id() != copy.dashboard_id()
                && source_pages.is_disjoint(&copy_pages) && source_widgets.is_disjoint(&copy_widgets))
}

/// Narrow 8.0.0 wire contract. Every field is accounted for, since Zabbix
/// accepts fields even when a widget does not use or understand them.
pub(super) fn supported_content(document: &Value) -> Option<CopyContent> {
    if !graph_8_0::allowed_keys(
        document,
        &["name", "display_period", "auto_start", "pages"],
        &[
            "dashboardid",
            "private",
            "userid",
            "users",
            "userGroups",
            "uuid",
        ],
    ) || number(&document["display_period"]) != Some(30)
    {
        return None;
    }
    let pages = document["pages"].as_array()?;
    if !(1..=2).contains(&pages.len()) {
        return None;
    }
    let mut content = CopyContent::default();
    let mut all_references = BTreeSet::new();
    for page in pages {
        if !graph_8_0::allowed_keys(
            page,
            &["name", "display_period", "widgets"],
            &["dashboard_pageid"],
        ) || !matches!(number(&page["display_period"]), Some(0 | 60))
        {
            return None;
        }
        let widgets = page["widgets"].as_array()?;
        if !(1..=2).contains(&widgets.len()) {
            return None;
        }
        let mut providers = BTreeSet::new();
        let mut consumers = BTreeSet::new();
        let mut occupied = [0_u128; 64];
        for widget in widgets {
            if !graph_8_0::allowed_keys(widget, &WIDGET_KEYS, &["widgetid", "fields"]) {
                return None;
            }
            let (x, y, width, height) = (
                number(&widget["x"])?,
                number(&widget["y"])?,
                number(&widget["width"])?,
                number(&widget["height"])?,
            );
            if width == 0
                || height == 0
                || x.checked_add(width)? > 72
                || y.checked_add(height)? > 64
                || number(&widget["view_mode"]) != Some(0)
            {
                return None;
            }
            let mask = ((1_u128 << width) - 1) << x;
            for row in &mut occupied[y as usize..(y + height) as usize] {
                if *row & mask != 0 {
                    return None;
                }
                *row |= mask;
            }
            let fields = widget["fields"].as_array()?;
            if fields
                .iter()
                .any(|field| field.as_object().is_none_or(|object| object.len() != 3))
            {
                return None;
            }
            match widget["type"].as_str()? {
                "svggraph" => {
                    let mut dataset = Vec::new();
                    let mut links = 0;
                    for field in fields {
                        match field["name"].as_str()? {
                            "reference" if number(&field["type"]) == Some(1) => {
                                let value =
                                    field["value"].as_str().filter(|value| reference(value))?;
                                if !all_references.insert(value) || !providers.insert(value) {
                                    return None;
                                }
                                links += 1;
                            }
                            "time_period._reference" if number(&field["type"]) == Some(1) => {
                                let value =
                                    field["value"].as_str()?.strip_suffix("._timeperiod")?;
                                if !reference(value) || !consumers.insert(value) {
                                    return None;
                                }
                                links += 1;
                            }
                            name if name.starts_with("ds.0.")
                                || name.starts_with("ds.1.")
                                || matches!(
                                    name,
                                    "or.0.hosts.0"
                                        | "or.0.items.0"
                                        | "or.0.invert_values"
                                        | "show_problems"
                                        | "graph_item_problems"
                                        | "problemhosts.0"
                                ) =>
                            {
                                dataset.push(field.clone())
                            }
                            _ => return None,
                        }
                    }
                    if links > 1 {
                        return None;
                    }
                    if let Some(sets) = explicit_sets(&dataset) {
                        let count = sets.iter().map(Vec::len).sum::<usize>();
                        if sets.iter().flatten().collect::<BTreeSet<_>>().len() != count
                            || (sets.len() == 2 && content.ordered_explicit)
                        {
                            return None;
                        }
                        content.ordered_explicit |= sets.len() == 2;
                        content.items.extend(sets.iter().flatten().cloned());
                        content.explicit_sets.extend(sets);
                    } else {
                        if !content.patterns.is_empty() {
                            return None;
                        }
                        content.patterns = graph_8_0::patterns::clone_specs(&dataset)?;
                    }
                }
                "item" => {
                    let [field] = fields.as_slice() else {
                        return None;
                    };
                    if field["name"] != "itemid.0" || number(&field["type"]) != Some(4) {
                        return None;
                    }
                    let id = text(Some(&field["value"]))?;
                    if !is_zabbix_id(&id) {
                        return None;
                    }
                    content.item_value_ids.insert(id.clone());
                    content.items.insert(id);
                }
                "clock"
                    if matches!(fields.as_slice(), [field] if field["name"] == "clock_type"
                    && number(&field["type"]) == Some(0) && number(&field["value"]) == Some(1)) => {
                }
                "clock" if fields.is_empty() && !content.default_clock => {
                    content.default_clock = true;
                }
                _ => return None,
            }
        }
        if !consumers.is_subset(&providers) {
            return None;
        }
    }
    if content.ordered_explicit
        && (content.explicit_sets.len() != 2
            || content.items.len() != 4
            || !content.item_value_ids.is_empty()
            || !content.patterns.is_empty())
    {
        return None;
    }
    if content.default_clock && !default_clock_copy_shape(pages, &content) {
        return None;
    }
    match number(&document["auto_start"]) {
        Some(1) => {}
        Some(0) if content.default_clock => {}
        _ => return None,
    }
    Some(content)
}

async fn evidence(
    reader: &ReadOnlyGateway,
    content: &CopyContent,
    edited_workcopy: bool,
) -> crate::Result<Option<Evidence>> {
    if edited_workcopy && (!content.patterns.is_empty() || content.ordered_explicit) {
        return Ok(None);
    }
    let Some(role) = reader.eight_zero_sharing_role().await? else {
        return Ok(None);
    };
    if content.default_clock && (!edited_workcopy || role.super_admin) {
        return Ok(None);
    }
    let ids = &content.items;
    let mut items = if ids.is_empty() {
        Vec::new()
    } else {
        reader.read_inventory_rows(ReadMethod::ItemGet, json!({
            "itemids":ids,"output":["itemid","hostid","key_","value_type","units","status","flags"],
            "selectDiscoveryData":["parent_itemid","key_","status"],
        })).await?
    };
    items.sort_by(|left, right| left["itemid"].as_str().cmp(&right["itemid"].as_str()));
    if items.len() != ids.len()
        || items.iter().zip(ids).any(|(item, id)| {
            item["itemid"] != id.as_str()
                || item["key_"].as_str().is_none_or(str::is_empty)
                || (edited_workcopy && !item["units"].is_string())
                || number(&item["value_type"]) != Some(3)
                || number(&item["status"]) != Some(0)
                || !matches!(number(&item["flags"]), Some(0 | 4))
        })
    {
        return Ok(None);
    }
    let Some(hostids) = items
        .iter()
        .map(|item| item["hostid"].as_str().filter(|id| is_zabbix_id(id)))
        .collect::<Option<BTreeSet<_>>>()
    else {
        return Ok(None);
    };
    if !edited_workcopy && hostids.len() != items.len() {
        return Ok(None);
    }
    let mut ordinary_key = None;
    let mut origins_by_id = BTreeMap::new();
    for item in &items {
        if number(&item["flags"]) == Some(4) {
            let Some(id) = item["itemid"].as_str() else {
                return Ok(None);
            };
            if edited_workcopy || origins_by_id.len() == 3 || content.item_value_ids.contains(id) {
                return Ok(None);
            }
            let Some(origin) = graph_8_0::current_lld_origin(reader, item).await? else {
                return Ok(None);
            };
            origins_by_id.insert(id, origin);
        } else if !edited_workcopy
            && ordinary_key
                .replace(&item["key_"])
                .is_some_and(|key| key != &item["key_"])
        {
            return Ok(None);
        }
    }
    let hosts = if hostids.is_empty() {
        Vec::new()
    } else {
        let Some(hosts) = graph_8_0::monitored_hosts(reader, hostids.into_iter().collect()).await?
        else {
            return Ok(None);
        };
        hosts
    };
    let discovered_origins: Vec<Vec<_>> = content
        .explicit_sets
        .iter()
        .map(|items| {
            items
                .iter()
                .enumerate()
                .filter_map(|(row, id)| {
                    origins_by_id
                        .get(id.as_str())
                        .cloned()
                        .map(|root| DiscoveredOrigin {
                            row: content.ordered_explicit.then_some(row),
                            root,
                        })
                })
                .collect()
        })
        .collect();
    let origin_count = origins_by_id.len();
    let distinct_count = origins_by_id.values().collect::<BTreeSet<_>>().len();
    let proven_origins = match (origin_count, content.ordered_explicit) {
        (0 | 1, false) => true,
        (2, false) => {
            distinct_count == 2
                && content.explicit_sets.iter().any(|set| {
                    set.len() == 2 && set.iter().all(|id| origins_by_id.contains_key(id.as_str()))
                })
        }
        (3, false) => {
            distinct_count == 3
                && content.patterns.is_empty()
                && content.item_value_ids.is_empty()
                && matches!(content.explicit_sets.as_slice(), [set] if set.len() == 3)
        }
        (3, true) => {
            distinct_count == 3
                && matches!(discovered_origins.as_slice(),
            [first, second] if matches!((first.len(), second.len()), (2, 1) | (1, 2)))
        }
        _ => false,
    };
    if !proven_origins {
        return Ok(None);
    }
    let mut patterns = Vec::new();
    for spec in &content.patterns {
        let Some(proof) = graph_8_0::patterns::read_clone_inputs(reader, spec).await? else {
            return Ok(None);
        };
        patterns.push(proof);
    }
    if let [first, second] = patterns.as_slice()
        && !first.accepts_clone_companion(second)
    {
        return Ok(None);
    }
    if reader.eight_zero_sharing_role().await?.as_ref() != Some(&role) {
        return Ok(None);
    }
    Ok(Some(Evidence {
        role,
        items,
        hosts,
        discovered_origins,
        patterns,
    }))
}

impl ScopedCloneCreate {
    pub(super) async fn recheck(&self, reader: &ReadOnlyGateway) -> bool {
        for checked in std::iter::once(&self.local).chain(self.source.as_ref()) {
            if !matches!(evidence(reader, &checked.scope, self.edited_workcopy).await, Ok(Some(current)) if current == checked.evidence)
            {
                return false;
            }
        }
        true
    }

    pub(super) fn reserve_send(&self) -> bool {
        !self.attempted.swap(true, Ordering::AcqRel)
    }

    pub(super) fn visible_only(&self) -> bool {
        !self.local.evidence.role.super_admin
    }
}

impl ReadOnlyGateway {
    /// Permission for the separately scoped Clone path, with exact patch,
    /// account identity and effective current dashboard rules checked live.
    pub async fn eight_zero_clone_permission(&self) -> crate::Result<bool> {
        Ok(self.eight_zero_sharing_role().await?.is_some())
    }
}

impl ReadWriteGateway {
    pub async fn verify_eight_zero_clone_create(
        &self,
        source: &VersionedDashboardSnapshot,
        local: &VersionedDashboardSnapshot,
        name: &str,
        copy_access: bool,
    ) -> Result<VerifiedDashboardCreate, CreateRejection> {
        self.verify_eight_zero_snapshot_copy(
            source,
            local,
            name,
            if copy_access {
                CopyAccess::Recipients
            } else {
                CopyAccess::Private
            },
            &[],
        )
        .await
    }

    /// Save a verified whole snapshot. The restore choice derives access
    /// exclusively from the source snapshot, with owner writes restricted to
    /// Super Admin and retaining effective editing permission.
    pub async fn verify_eight_zero_save_as_copy(
        &self,
        source: &VersionedDashboardSnapshot,
        local: &VersionedDashboardSnapshot,
        name: &str,
        restore_access: bool,
    ) -> Result<VerifiedDashboardCreate, CreateRejection> {
        let replacements =
            workcopy_replacements(source.raw_api_document(), local.raw_api_document())
                .ok_or_else(|| shape("widget"))?;
        self.verify_eight_zero_snapshot_copy(
            source,
            local,
            name,
            if restore_access {
                CopyAccess::Restore
            } else {
                CopyAccess::Private
            },
            &replacements,
        )
        .await
    }

    async fn verify_eight_zero_snapshot_copy(
        &self,
        source: &VersionedDashboardSnapshot,
        local: &VersionedDashboardSnapshot,
        name: &str,
        access: CopyAccess,
        replacements: &[ItemReplacement<'_>],
    ) -> Result<VerifiedDashboardCreate, CreateRejection> {
        let unverified = || {
            CreateRejection::new(CreateRejectionCode::ContractUnverified).with("contract", CONTRACT)
        };
        let context = self.publication_context();
        let version = &context.capabilities().version;
        if !graph_8_0::exact_version(version)
            || version != local.server_version()
            || version != source.server_version()
            || !context.is_live_authenticated()
            || source.dashboard_id() != local.dashboard_id()
            || source.connection_id() != local.connection_id()
        {
            return Err(unverified());
        }
        let scope = supported_content(local.raw_api_document()).ok_or_else(|| shape("widget"))?;
        if scope.default_clock {
            let [first, second, third] = replacements else {
                return Err(unverified());
            };
            let raw = source.raw_api_document();
            if access != CopyAccess::Private
                || raw["userid"].as_str() != Some(self.reader().identity().account_userid())
                || number(&raw["private"]) != Some(1)
                || !raw["users"].as_array().is_some_and(Vec::is_empty)
                || !raw["userGroups"].as_array().is_some_and(Vec::is_empty)
                || [second, third]
                    .into_iter()
                    .any(|change| change.old != first.old || change.new != first.new)
            {
                return Err(unverified());
            }
        }
        let edited_workcopy = !replacements.is_empty();
        let proof = evidence(self.reader(), &scope, edited_workcopy)
            .await
            .ok()
            .flatten()
            .ok_or_else(unverified)?;
        let original = if edited_workcopy
            || proof.discovered_origins.iter().any(|set| !set.is_empty())
            || !proof.patterns.is_empty()
        {
            let source_scope =
                supported_content(source.raw_api_document()).ok_or_else(unverified)?;
            if !edited_workcopy && source_scope == scope {
                None
            } else {
                let source_proof = evidence(self.reader(), &source_scope, edited_workcopy)
                    .await
                    .ok()
                    .flatten()
                    .ok_or_else(unverified)?;
                if source_proof.role != proof.role
                    || source_scope.ordered_explicit != scope.ordered_explicit
                    || source_scope.default_clock != scope.default_clock
                    || source_proof.discovered_origins != proof.discovered_origins
                    || source_proof.patterns.len() != proof.patterns.len()
                    || (edited_workcopy
                        && !matching_replacements(&source_proof, &proof, replacements))
                {
                    return Err(unverified());
                }
                Some(CheckedContent {
                    scope: source_scope,
                    evidence: source_proof,
                })
            }
        } else {
            None
        };
        let prepared = prepare(local, CreateScope::Dashboard, name)?;
        let require_editable_source = access == CopyAccess::Restore && proof.role.super_admin;
        if require_editable_source
            && !matches!(
                self.reader()
                    .dashboard_is_editable(source.dashboard_id())
                    .await,
                Ok(true)
            )
        {
            return Err(unverified());
        }
        let mut create = VerifiedDashboardCreate {
            portable: false,
            scoped_graph: None,
            scoped_clone: Some(ScopedCloneCreate {
                source_fingerprint: source.fingerprint().to_owned(),
                require_editable_source,
                local: CheckedContent {
                    scope,
                    evidence: proof,
                },
                source: original,
                edited_workcopy,
                attempted: AtomicBool::new(false),
            }),
            access: None,
            connection_id: local.connection_id().to_owned(),
            source_dashboardid: local.dashboard_id().to_owned(),
            server_version: version.clone(),
            authority_fingerprint: context.binding_fingerprint().to_owned(),
            local_fingerprint: local.fingerprint().to_owned(),
            name: name.to_owned(),
            create_params: prepared.params,
            content_fingerprint: prepared.content_fingerprint,
            requirements: prepared.requirements,
        };
        create.requirements.contracts.clear();
        create.requirements.unknown_widgets.clear();
        if access != CopyAccess::Private {
            let original = source.dashboard_access_state().ok_or_else(unverified)?;
            let selection = DashboardAccessSelection {
                private: Some(original.private),
                owner: require_editable_source.then_some(original.owner),
                users: Some(original.users),
                groups: Some(original.groups),
            };
            create = self
                .bind_eight_zero_create_access(create, &selection)
                .await?;
        }
        Ok(create)
    }
}

struct ItemReplacement<'a> {
    old: &'a str,
    new: &'a str,
}

// Reconstruct the whole document with only the proven field changes.
fn workcopy_replacements<'a>(
    base: &'a Value,
    local: &'a Value,
) -> Option<Vec<ItemReplacement<'a>>> {
    if base == local {
        return Some(Vec::new());
    }
    let mut expected = base.clone();
    let mut replacements = Vec::new();
    for (page_index, page) in base["pages"].as_array()?.iter().enumerate() {
        for (widget_index, widget) in page["widgets"].as_array()?.iter().enumerate() {
            let mut edited = false;
            for (field_index, field) in widget["fields"].as_array()?.iter().enumerate() {
                let editable = matches!(
                    (widget["type"].as_str(), field["name"].as_str()),
                    (Some("item"), Some("itemid.0"))
                        | (Some("svggraph"), Some("ds.0.itemids.0" | "ds.0.itemids.1"))
                );
                if !editable || number(&field["type"]) != Some(4) {
                    continue;
                }
                let value = &local["pages"][page_index]["widgets"][widget_index]["fields"]
                    [field_index]["value"];
                if value == &field["value"] {
                    continue;
                }
                if edited || replacements.len() == 4 {
                    return None;
                }
                let id =
                    |value: &'a Value| value.as_str().filter(|id| *id != "0" && is_zabbix_id(id));
                replacements.push(ItemReplacement {
                    old: id(&field["value"])?,
                    new: id(value)?,
                });
                expected["pages"][page_index]["widgets"][widget_index]["fields"][field_index]["value"] =
                    value.clone();
                edited = true;
            }
        }
    }
    (!replacements.is_empty() && expected == *local).then_some(replacements)
}

fn matching_replacements(
    base: &Evidence,
    local: &Evidence,
    replacements: &[ItemReplacement<'_>],
) -> bool {
    let mut host_pair = None;
    replacements.iter().all(|replacement| {
        let old = base
            .items
            .iter()
            .find(|item| item["itemid"] == replacement.old);
        let new = local
            .items
            .iter()
            .find(|item| item["itemid"] == replacement.new);
        let (Some(old), Some(new)) = (old, new) else {
            return false;
        };
        let pair = (&old["hostid"], &new["hostid"]);
        pair.0 != pair.1
            && host_pair.get_or_insert(pair) == &pair
            && old["key_"] == new["key_"]
            && old["units"] == new["units"]
    })
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum CopyAccess {
    Private,
    Recipients,
    Restore,
}

#[cfg(test)]
mod tests {
    use super::workcopy_replacements;
    use serde_json::json;

    #[test]
    fn a_workcopy_edit_must_preserve_access_and_the_rest_of_the_document() {
        let base = json!({"private":"1","pages":[
            {"widgets":[{"type":"item","fields":[{"name":"itemid.0","type":"4","value":"30"}]}]},
            {"widgets":[{"type":"item","fields":[{"name":"itemid.0","type":"4","value":"40"}]}]},
        ]});
        let mut local = base.clone();
        assert_eq!(
            workcopy_replacements(&base, &local).map(|changes| changes.len()),
            Some(0)
        );
        local["pages"][0]["widgets"][0]["fields"][0]["value"] = json!("31");
        assert_eq!(
            workcopy_replacements(&base, &local).map(|changes| changes.len()),
            Some(1)
        );

        let mut extra_edit = local.clone();
        extra_edit["pages"][1]["widgets"][0]["fields"][0]["value"] = json!("41");
        assert_eq!(
            workcopy_replacements(&base, &extra_edit).map(|changes| changes.len()),
            Some(2)
        );
        let mut missing_page = local.clone();
        missing_page["pages"]
            .as_array_mut()
            .unwrap_or_else(|| panic!("pages"))
            .pop();
        assert!(workcopy_replacements(&base, &missing_page).is_none());
        local["private"] = json!("0");
        assert!(workcopy_replacements(&base, &local).is_none());
    }

    #[test]
    fn graph_replacements_must_keep_the_link_and_change_at_most_one_row_per_widget() {
        let base = json!({"pages":[{"widgets":[{"type":"svggraph","fields":[
            {"name":"ds.0.itemids.0","type":"4","value":"30"},
            {"name":"ds.0.itemids.1","type":"4","value":"40"},
            {"name":"time_period._reference","type":"1","value":"ABCDE._timeperiod"},
        ]}]}]});
        let mut local = base.clone();
        let fields = &mut local["pages"][0]["widgets"][0]["fields"];
        fields[1]["value"] = json!("41");
        assert!(matches!(workcopy_replacements(&base, &local).as_deref(),
            Some([change]) if change.old == "40" && change.new == "41"));
        let mut broken = local.clone();
        broken["pages"][0]["widgets"][0]["fields"][2]["value"] = json!("VWXYZ._timeperiod");
        assert!(workcopy_replacements(&base, &broken).is_none());
        local["pages"][0]["widgets"][0]["fields"][0]["value"] = json!("31");
        assert!(workcopy_replacements(&base, &local).is_none());
    }
}
