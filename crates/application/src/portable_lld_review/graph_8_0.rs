// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::BTreeSet;

use serde_json::{Value, json};
use zdashstitch_core::formats::{
    Layout, ProjectRoot, Widget, ZabbixWidgetField, ZabbixWidgetFieldValue,
};

use super::{MaterializedPortableDashboard, VerifiedPortableLldEvidence, sources::widget_sources};
use crate::workcopy::{WorkcopyError, WorkcopyService};

impl WorkcopyService {
    /// Assemble the independently tested 8.0.0 portable Graph shape in
    /// memory. The resulting data does not carry permission to publish.
    pub(crate) fn materialize_portable_graph_8_0(
        &self,
        evidence: &VerifiedPortableLldEvidence,
    ) -> Result<MaterializedPortableDashboard, WorkcopyError> {
        let grouped = self.materialize_graph_groups(evidence)?;
        let mut result = grouped
            .first()
            .and_then(|page| page.first())
            .ok_or_else(unsupported)?
            .document
            .clone();
        let mut pages = Vec::with_capacity(grouped.len());
        for page in grouped {
            let mut merged = page.first().ok_or_else(unsupported)?.document["pages"][0].clone();
            merged["widgets"] = json!(
                page.into_iter()
                    .map(|graph| graph.document["pages"][0]["widgets"][0].clone())
                    .collect::<Vec<_>>()
            );
            pages.push(merged);
        }
        result["pages"] = json!(pages);
        Ok(MaterializedPortableDashboard { document: result })
    }

    pub(crate) fn materialize_portable_graphs_8_0(
        &self,
        evidence: &VerifiedPortableLldEvidence,
    ) -> Result<Vec<MaterializedPortableDashboard>, WorkcopyError> {
        Ok(self
            .materialize_graph_groups(evidence)?
            .into_iter()
            .flatten()
            .collect())
    }

    fn materialize_graph_groups(
        &self,
        evidence: &VerifiedPortableLldEvidence,
    ) -> Result<Vec<Vec<MaterializedPortableDashboard>>, WorkcopyError> {
        if evidence.version.to_string() != "8.0.0" {
            return Err(unsupported());
        }
        let (revision, bundle) = self.load_portable_source_draft(&evidence.local_id)?;
        if revision != evidence.revision
            || super::bundle_fingerprint(&bundle)? != evidence.draft_fingerprint
        {
            return Err(WorkcopyError::new("workcopy_revision_conflict"));
        }
        let (name, pages) = match &bundle.project.root {
            ProjectRoot::Dashboard(dashboard) => (&dashboard.name, dashboard.pages.as_slice()),
            ProjectRoot::Page(page) => (&page.name, std::slice::from_ref(page)),
        };
        let groups = match &evidence.scope {
            Some(scope) => vec![(
                scope.page_index,
                scope.widgets.iter().copied().collect::<Vec<_>>(),
            )],
            None if (1..=zdashstitch_zabbix::MAX_PORTABLE_GRAPH_PAGES).contains(&pages.len()) => {
                pages
                    .iter()
                    .enumerate()
                    .map(|(index, page)| (index, (0..page.widgets.len()).collect()))
                    .collect()
            }
            None => return Err(unsupported()),
        };
        groups.into_iter().map(|(page_index,widgets)| {
        if !(1..=zdashstitch_zabbix::MAX_PORTABLE_APPEND_WIDGETS).contains(&widgets.len()) { return Err(unsupported()); }
        let page = pages.get(page_index).ok_or_else(unsupported)?;
        widgets.into_iter().map(|widget_index| {
        let widget = page.widgets.get(widget_index).ok_or_else(unsupported)?;
        let (x, y, width, height) = match *widget.layout() {
            Layout::Fixed { x, y, w, h } => (x, y, w, h),
            Layout::Auto { w, h } => (0, 0, w, h),
        };
        let (title, fields) = match widget {
            Widget::Graph { title, .. } if !evidence.patterns.iter()
                .any(|saved| (saved.review.page_index,saved.review.widget_index) == (page_index,widget_index)) => {
                (title, explicit_fields(&bundle, widget, evidence, (page_index,widget_index))?)
            }
            Widget::Zabbix {
                widget_type,
                name,
                fields,
                ..
            } if widget_type == "svggraph" => (name, portable_fields(fields, evidence, (page_index,widget_index))?),
            _ => return Err(unsupported()),
        };
        Ok(MaterializedPortableDashboard {
            document: json!({
                "name":name,"display_period":"30","auto_start":"1",
                "pages":[{"name":page.name,"display_period":"0","widgets":[{
                    "type":"svggraph","name":title,"x":x.to_string(),"y":y.to_string(),
                    "width":width.to_string(),"height":height.to_string(),"view_mode":"0","fields":fields,
                }]}],
            }),
        })
        }).collect()
        }).collect()
    }
}

fn unsupported() -> WorkcopyError {
    WorkcopyError::new("portable_materialization_unsupported")
}

fn explicit_fields(
    bundle: &zdashstitch_core::formats::TypedPortableBundle,
    graph: &Widget,
    evidence: &VerifiedPortableLldEvidence,
    address: (usize, usize),
) -> Result<Vec<Value>, WorkcopyError> {
    let sources = widget_sources(&bundle.project.roles, graph)?;
    const COLORS: [&str; 6] = ["FF0000", "0040FF", "00B85C", "BF00FF", "FF8000", "A94614"];
    let selections: Vec<_> = evidence
        .selections
        .iter()
        .filter(|selection| (selection.page_index, selection.widget_index) == address)
        .collect();
    if !(2..=COLORS.len()).contains(&sources.len()) || selections.len() != sources.len() {
        return Err(unsupported());
    }
    let mut hosts = BTreeSet::new();
    let mut items = BTreeSet::new();
    let mut fields = vec![json!({"type":"0","name":"ds.0.dataset_type","value":"0"})];
    for (slot, (source, selection)) in sources.into_iter().zip(selections).enumerate() {
        if selection.field_index != source.field_index {
            return Err(unsupported());
        }
        let reference = source.metric.ok_or_else(unsupported)?;
        let metric = bundle
            .catalogs
            .get(reference.catalog)
            .and_then(|catalog| {
                catalog
                    .metrics
                    .iter()
                    .find(|metric| metric.id == reference.metric)
            })
            .ok_or_else(unsupported)?;
        if !metric.value_type.is_numeric() {
            return Err(unsupported());
        }
        let id = selection.itemid.as_deref().ok_or_else(unsupported)?;
        hosts.insert(&selection.hostid);
        if !items.insert(id) {
            return Err(unsupported());
        }
        let color = source
            .color
            .and_then(|color| color.strip_prefix('#'))
            .unwrap_or(COLORS[slot]);
        fields.push(json!({"type":"4","name":format!("ds.0.itemids.{slot}"),"value":id}));
        fields.push(json!({"type":"1","name":format!("ds.0.color.{slot}"),"value":color.to_ascii_uppercase()}));
    }
    if !(2..=3).contains(&hosts.len()) {
        return Err(unsupported());
    }
    Ok(fields)
}

fn portable_fields(
    fields: &[ZabbixWidgetField],
    evidence: &VerifiedPortableLldEvidence,
    address: (usize, usize),
) -> Result<Vec<Value>, WorkcopyError> {
    let pattern_count = evidence
        .patterns
        .iter()
        .filter(|saved| (saved.review.page_index, saved.review.widget_index) == address)
        .count();
    let selection_count = evidence
        .selections
        .iter()
        .filter(|selection| (selection.page_index, selection.widget_index) == address)
        .count();
    if (pattern_count != 0 && evidence.visibility_role.is_none())
        || (selection_count == 0 && pattern_count == 0)
    {
        return Err(unsupported());
    }
    let mut used_patterns = 0;
    let mut used_items = 0;
    let mut dataset_host_masks = [0; 2];
    let mut pattern_scope = None;
    let mut rows = Vec::with_capacity(fields.len());
    for (field_index, field) in fields.iter().enumerate() {
        let value = match &field.value {
            ZabbixWidgetFieldValue::Integer { value } => value.to_string(),
            ZabbixWidgetFieldValue::String { value } => value.clone(),
            ZabbixWidgetFieldValue::DynamicPattern { value, .. } => {
                let saved = evidence
                    .patterns
                    .iter()
                    .find(|saved| {
                        (saved.review.page_index, saved.review.widget_index) == address
                            && saved.review.field_index == field_index
                    })
                    .ok_or_else(unsupported)?;
                let targets: BTreeSet<_> = saved
                    .review
                    .targets
                    .iter()
                    .map(|slot| slot.host.hostid.as_str())
                    .collect();
                if !(2..=3).contains(&targets.len())
                    || pattern_scope.get_or_insert_with(|| targets.clone()) != &targets
                {
                    return Err(unsupported());
                }
                if let Some(dataset) = ["ds.0.hosts.", "ds.1.hosts."]
                    .into_iter()
                    .position(|prefix| field.name.starts_with(prefix))
                {
                    if !evidence.selected_single_host_patterns.contains(&(
                        address.0,
                        address.1,
                        field_index,
                    )) {
                        return Err(unsupported());
                    }
                    dataset_host_masks[dataset] += 1;
                }
                used_patterns += 1;
                value.clone()
            }
            ZabbixWidgetFieldValue::Metric { .. } | ZabbixWidgetFieldValue::LldInstance { .. } => {
                let selection = evidence
                    .selections
                    .iter()
                    .find(|selection| {
                        (selection.page_index, selection.widget_index) == address
                            && selection.field_index == field_index
                    })
                    .ok_or_else(unsupported)?;
                used_items += 1;
                selection.itemid.clone().ok_or_else(unsupported)?
            }
            _ => return Err(unsupported()),
        };
        rows.push(json!({"name":field.name,"type":field.field_type.to_string(),"value":value}));
    }
    if used_patterns != pattern_count
        || used_items != selection_count
        || pattern_scope.is_some_and(|scope| {
            dataset_host_masks == [0, 0]
                || dataset_host_masks
                    .iter()
                    .any(|&count| count != 0 && count != scope.len())
        })
    {
        return Err(unsupported());
    }
    Ok(rows)
}
