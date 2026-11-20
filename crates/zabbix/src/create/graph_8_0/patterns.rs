// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{field_value, uint};
use crate::{HostDetails, ReadMethod, ReadOnlyGateway, SvgGraphItemMatch, SvgGraphItemTagFilter};

// Zabbix 8.0.0, ui/include/defines.inc.php and CSvgGraphHelper::getMetricsPattern.
const RENDERER_LIMIT: usize = 50;

#[derive(Eq, PartialEq)]
pub(crate) struct PatternSpec {
    hosts: Vec<String>,
    item: String,
    tags: SvgGraphItemTagFilter,
    overlays: bool,
}

impl PatternSpec {
    pub(crate) fn from_fields(fields: &[Value], dataset_index: u32) -> Option<Self> {
        let dataset = |name| format!("ds.{dataset_index}.{name}");
        let string = |name: &str| field_value(fields, name, 1)?.as_str();
        let mask = |name: &str| {
            let value = string(name)?;
            (!value.is_empty() && value.len() <= 255 && !value.chars().any(char::is_control))
                .then_some(value)
        };
        let integer = |name: &str| uint(field_value(fields, name, 0)?);
        let mut hosts = vec![mask(&dataset("hosts.0"))?, mask(&dataset("hosts.1"))?];
        if let Some(host) = mask(&dataset("hosts.2")) {
            hosts.push(host);
        }
        let item = mask(&dataset("items.0"))?;
        let color = string(&dataset("color"))?;
        if hosts.iter().copied().collect::<BTreeSet<_>>().len() != hosts.len()
            || color.len() != 6
            || !color.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return None;
        }
        let option_fields = fields.len().checked_sub(3 + hosts.len())?;
        match option_fields {
            0 => {}
            4 if integer(&dataset("item_tags_evaltype")) == Some(0)
                && mask(&dataset("item_tags.0.tag")).is_some()
                && integer(&dataset("item_tags.0.operator")) == Some(4)
                && string(&dataset("item_tags.0.value")) == Some("") => {}
            6 if string("or.0.hosts.0") == Some("*")
                && item.is_ascii()
                && mask("or.0.items.0") == Some(item)
                && integer("or.0.invert_values") == Some(1)
                && integer("show_problems") == Some(1)
                && integer("graph_item_problems") == Some(1)
                && hosts.contains(&mask("problemhosts.0")?) => {}
            _ => return None,
        }
        let tags = SvgGraphItemTagFilter::from_widget_fields(fields, dataset_index).ok()?;
        Some(Self {
            hosts: hosts.into_iter().map(str::to_owned).collect(),
            item: item.into(),
            tags,
            overlays: option_fields == 6,
        })
    }
}

/// Only the Clone layouts that were independently read back: one plain,
/// tagged, or override dataset, or two disjoint plain datasets. All use two
/// host selectors per dataset.
pub(in crate::create) fn clone_specs(fields: &[Value]) -> Option<Vec<PatternSpec>> {
    let pattern_type = |index| {
        uint(field_value(fields, &format!("ds.{index}.dataset_type"), 0)?).filter(|kind| *kind == 1)
    };
    pattern_type(0)?;
    if pattern_type(1).is_some() {
        if fields.len() != 10 {
            return None;
        }
        let (first, second): (Vec<_>, Vec<_>) = fields.iter().cloned().partition(|field| {
            field["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("ds.0."))
        });
        if first.len() != 5 || second.len() != 5 {
            return None;
        }
        return Some(vec![
            PatternSpec::from_fields(&first, 0)?,
            PatternSpec::from_fields(&second, 1)?,
        ]);
    }
    let spec = PatternSpec::from_fields(fields, 0)?;
    (spec.hosts.len() == 2).then_some(vec![spec])
}

#[derive(Eq, PartialEq)]
pub(crate) struct PatternInputs {
    roleid: String,
    visible_hosts: Vec<HostDetails>,
    hosts: Vec<Value>,
    host_mask_hits: Vec<HostDetails>,
    visible_items: Vec<SvgGraphItemMatch>,
    renderer_items: Vec<Value>,
}

impl PatternInputs {
    /// Both reads must agree on the complete visible inventory. Only the
    /// first selected host and its one rendered item may change.
    pub(crate) fn replacement_outside_matches(
        &self,
        after: &Self,
        before_pairs: [(&str, &str); 2],
        after_pairs: [(&str, &str); 2],
    ) -> Option<usize> {
        let renders = |proof: &Self, pairs: [(&str, &str); 2]| {
            proof
                .host_mask_hits
                .iter()
                .map(|host| host.hostid.as_str())
                .eq(pairs.map(|(id, _)| id))
                && proof.renderer_items.len() == 2
                && pairs.into_iter().all(|(hostid, itemid)| {
                    proof
                        .renderer_items
                        .iter()
                        .any(|row| row["hostid"] == hostid && row["itemid"] == itemid)
                })
        };
        (self.roleid == after.roleid
            && self.visible_hosts == after.visible_hosts
            && self.visible_items == after.visible_items
            && self
                .visible_items
                .windows(2)
                .all(|rows| rows[0].itemid != rows[1].itemid)
            && renders(self, before_pairs)
            && renders(after, after_pairs))
        .then(|| {
            after
                .visible_items
                .iter()
                .filter(|item| after_pairs.iter().all(|(hostid, _)| item.hostid != *hostid))
                .count()
        })
    }

    pub(crate) fn role_id(&self) -> &str {
        &self.roleid
    }

    pub(super) fn accepts_explicit(&self, explicit: &super::Inputs) -> bool {
        self.roleid == explicit.roleid
            && explicit.discovered_origins.is_empty()
            && explicit.hosts.iter().all(|host| {
                self.host_mask_hits
                    .iter()
                    .any(|hit| host["hostid"] == hit.hostid.as_str())
            })
            && explicit.items.iter().all(|item| {
                self.renderer_items
                    .iter()
                    .all(|rendered| item["itemid"] != rendered["itemid"])
            })
    }

    pub(super) fn accepts_companion(&self, other: &Self) -> bool {
        self.roleid == other.roleid
            && self.visible_hosts == other.visible_hosts
            && self.hosts == other.hosts
            && self.renderer_items.len() + other.renderer_items.len() <= RENDERER_LIMIT
            && self.renderer_items.iter().all(|item| {
                other
                    .renderer_items
                    .iter()
                    .all(|other| item["itemid"] != other["itemid"])
            })
    }

    pub(in crate::create) fn accepts_clone_companion(&self, other: &Self) -> bool {
        self.visible_items.len() + other.visible_items.len()
            <= crate::lookups::MAX_SVGGRAPH_PATTERN_MATCHES
            && self.accepts_companion(other)
    }
}

/// `or` uses a case-insensitive anchored preg_match after `preg_quote`,
/// turning only `*` into `.*`. Stay in the observed ASCII case.
fn ascii_glob(pattern: &str, name: &str) -> bool {
    if !name.is_ascii() || name.chars().any(char::is_control) {
        return false;
    }
    let pattern = pattern.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    let mut parts = pattern.split('*');
    let Some(mut name) = name.strip_prefix(parts.next().unwrap_or_default()) else {
        return false;
    };
    let rest: Vec<_> = parts.collect();
    let Some((tail, interior)) = rest.split_last() else {
        return name.is_empty();
    };
    for literal in interior {
        let Some(index) = name.find(literal) else {
            return false;
        };
        name = &name[index + literal.len()..];
    }
    name.ends_with(tail)
}

pub(in crate::create) async fn read_inputs(
    reader: &ReadOnlyGateway,
    spec: &PatternSpec,
) -> crate::Result<Option<PatternInputs>> {
    read_inputs_in_scope(reader, spec, false).await
}

pub(crate) async fn read_clone_inputs(
    reader: &ReadOnlyGateway,
    spec: &PatternSpec,
) -> crate::Result<Option<PatternInputs>> {
    read_inputs_in_scope(reader, spec, true).await
}

async fn read_inputs_in_scope(
    reader: &ReadOnlyGateway,
    spec: &PatternSpec,
    clone: bool,
) -> crate::Result<Option<PatternInputs>> {
    let Some(roleid) = reader.eight_zero_pattern_create_role().await? else {
        return Ok(None);
    };
    let visible = if clone {
        reader.clone_svggraph_host_matches("*").await?
    } else {
        reader.portable_svggraph_host_matches("*").await?
    };
    let mut visible_hosts = visible.objects;
    visible_hosts.sort_by(|a, b| a.hostid.cmp(&b.hostid));
    if visible.truncated
        || visible_hosts.len() < spec.hosts.len()
        || visible_hosts
            .windows(2)
            .any(|rows| rows[0].hostid == rows[1].hostid)
    {
        return Ok(None);
    }
    let mut host_mask_hits = Vec::with_capacity(spec.hosts.len());
    for pattern in &spec.hosts {
        let matching = reader.portable_svggraph_host_matches(pattern).await?;
        let [host] = matching.objects.as_slice() else {
            return Ok(None);
        };
        if matching.truncated || !visible_hosts.contains(host) || host_mask_hits.contains(host) {
            return Ok(None);
        }
        host_mask_hits.push(host.clone());
    }
    let hostids: Vec<_> = host_mask_hits
        .iter()
        .map(|host| host.hostid.clone())
        .collect();
    let all_ids: Vec<_> = visible_hosts
        .iter()
        .map(|host| host.hostid.clone())
        .collect();
    let Some(hosts) =
        super::monitored_hosts(reader, hostids.iter().map(String::as_str).collect()).await?
    else {
        return Ok(None);
    };
    let visible = if clone {
        reader
            .clone_svggraph_item_matches(&all_ids, &spec.item, &spec.tags)
            .await?
    } else {
        reader
            .portable_svggraph_item_matches(&all_ids, &spec.item, &spec.tags)
            .await?
    };
    let selected = reader
        .portable_svggraph_item_matches(&hostids, &spec.item, &spec.tags)
        .await?;
    let mut visible_items = visible.objects;
    let mut selected_items = selected.objects;
    visible_items.sort_by(|a, b| a.itemid.cmp(&b.itemid));
    selected_items.sort_by(|a, b| a.itemid.cmp(&b.itemid));
    let selected_host_ids: BTreeSet<_> = selected_items
        .iter()
        .map(|item| item.hostid.as_str())
        .collect();
    if visible.truncated
        || selected.truncated
        || selected_items.len() > RENDERER_LIMIT
        || selected_host_ids.len() != spec.hosts.len()
        || !visible_items
            .iter()
            .filter(|item| hostids.contains(&item.hostid))
            .eq(&selected_items)
        || (spec.overlays
            && selected_items
                .iter()
                .any(|item| !ascii_glob(&spec.item, &item.name)))
    {
        return Ok(None);
    }
    let itemids: Vec<_> = selected_items
        .iter()
        .map(|item| item.itemid.as_str())
        .collect();
    let mut renderer_items = reader.read_inventory_rows(ReadMethod::ItemGet,json!({
        "itemids":itemids,"webitems":true,"output":["itemid","hostid","value_type","status","flags","name_resolved"],
    })).await?;
    renderer_items.sort_by(|a, b| a["itemid"].as_str().cmp(&b["itemid"].as_str()));
    if renderer_items.len() != selected_items.len()
        || renderer_items
            .iter()
            .zip(&selected_items)
            .any(|(row, item)| {
                row["itemid"] != item.itemid.as_str()
                    || row["hostid"] != item.hostid.as_str()
                    || row["name_resolved"] != item.name.as_str()
                    || !matches!(uint(&row["value_type"]), Some(0 | 3))
                    || uint(&row["status"]) != Some(0)
                    || uint(&row["flags"]) != Some(0)
            })
        || reader.eight_zero_pattern_create_role().await?.as_deref() != Some(&roleid)
    {
        return Ok(None);
    }
    Ok(Some(PatternInputs {
        roleid,
        visible_hosts,
        hosts,
        host_mask_hits,
        visible_items,
        renderer_items,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_and_three_host_patterns_accept_only_the_proven_options() {
        let field =
            |name: &str, kind: &str, value: &str| json!({"name":name,"type":kind,"value":value});
        let mut base = vec![
            field("ds.0.dataset_type", "0", "1"),
            field("ds.0.items.0", "1", "*ping*"),
            field("ds.0.color", "1", "FF0000"),
            field("ds.0.hosts.0", "1", "First*"),
            field("ds.0.hosts.1", "1", "Second*"),
        ];
        let mut overlays = [
            field("or.0.hosts.0", "1", "*"),
            field("or.0.items.0", "1", "*ping*"),
            field("or.0.invert_values", "0", "1"),
            field("show_problems", "0", "1"),
            field("graph_item_problems", "0", "1"),
            field("problemhosts.0", "1", "Second*"),
        ];
        let tags = [
            field("ds.0.item_tags_evaltype", "0", "0"),
            field("ds.0.item_tags.0.tag", "1", "component"),
            field("ds.0.item_tags.0.operator", "0", "4"),
            field("ds.0.item_tags.0.value", "1", ""),
        ];
        for host_count in 2..=3 {
            if host_count == 3 {
                base.push(field("ds.0.hosts.2", "1", "Third*"));
                overlays[5]["value"] = json!("Third*");
            }
            let mut tagged = [base.as_slice(), &tags].concat();
            let spec = PatternSpec::from_fields(&tagged, 0);
            assert!(matches!(spec, Some(spec) if spec.hosts.len() == host_count && !spec.overlays));
            let mut overlaid = [base.as_slice(), &overlays].concat();
            let spec = PatternSpec::from_fields(&overlaid, 0);
            assert!(matches!(spec, Some(spec) if spec.hosts.len() == host_count && spec.overlays));
            let combined = [tagged.as_slice(), &overlays].concat();
            assert!(PatternSpec::from_fields(&combined, 0).is_none());
            tagged[host_count + 5]["value"] = json!("1");
            assert!(PatternSpec::from_fields(&tagged, 0).is_none());
            overlaid[host_count + 8]["value"] = json!("Outside*");
            assert!(PatternSpec::from_fields(&overlaid, 0).is_none());
        }
    }

    #[test]
    fn mixed_inputs_require_the_same_role_and_disjoint_rows_on_masked_hosts() {
        let plan = PatternInputs {
            roleid: "8".into(),
            visible_hosts: Vec::new(),
            hosts: Vec::new(),
            host_mask_hits: ["11", "12", "13"]
                .map(|id| HostDetails {
                    hostid: id.into(),
                    host: format!("probe-{id}"),
                    name: format!("Probe {id}"),
                })
                .into(),
            visible_items: Vec::new(),
            renderer_items: vec![json!({"itemid":"22"})],
        };
        let mut explicit = super::super::Inputs {
            roleid: "8".into(),
            items: vec![json!({"itemid":"23"}), json!({"itemid":"24"})],
            hosts: vec![json!({"hostid":"11"}), json!({"hostid":"12"})],
            discovered_origins: Vec::new(),
        };
        assert!(plan.accepts_explicit(&explicit));
        explicit.roleid = "9".into();
        assert!(!plan.accepts_explicit(&explicit));
        explicit.roleid = "8".into();
        explicit.items[1]["itemid"] = json!("22");
        assert!(!plan.accepts_explicit(&explicit));
        explicit.items[1]["itemid"] = json!("24");
        explicit.hosts[1]["hostid"] = json!("14");
        assert!(!plan.accepts_explicit(&explicit));
    }

    #[test]
    fn companion_patterns_share_role_and_scope_and_renderer_budget() {
        let evidence = |first, last| PatternInputs {
            roleid: "8".into(),
            visible_hosts: Vec::new(),
            hosts: vec![json!({"hostid":"11","status":"0"})],
            host_mask_hits: Vec::new(),
            visible_items: Vec::new(),
            renderer_items: (first..last).map(|id| json!({"itemid":id})).collect(),
        };
        let mut first = evidence(1, 26);
        let mut second = evidence(26, 51);
        assert!(first.accepts_companion(&second));
        assert!(!first.accepts_companion(&evidence(25, 50)));
        assert!(!first.accepts_companion(&evidence(26, 52)));
        second.roleid = "9".into();
        assert!(!first.accepts_companion(&second));
        second.roleid = "8".into();
        second.hosts[0]["hostid"] = json!("12");
        assert!(!first.accepts_companion(&second));
        second.hosts[0]["hostid"] = json!("11");
        let hit = |id: usize| SvgGraphItemMatch {
            hostid: "11".into(),
            itemid: id.to_string(),
            name: id.to_string(),
        };
        first.visible_items.extend((1..=250).map(hit));
        second.visible_items.extend((251..=500).map(hit));
        assert!(first.accepts_clone_companion(&second));
        second.visible_items.push(hit(501));
        assert!(!first.accepts_clone_companion(&second));
        assert!(first.accepts_companion(&second));
    }
}
