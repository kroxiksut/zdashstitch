// SPDX-License-Identifier: MIT OR Apache-2.0

//! GUI contract for adding one explicit series to a two-row SVG data set.

use serde::{Deserialize, Serialize};
use zdashstitch_zabbix::{HostListing, ReadOnlyGateway, VersionedDashboardSnapshot, is_zabbix_id};

use crate::clone_mapping::{
    CardinalityRequest, CloneCardinalityTransform, SeriesBindingKind, SourceSeries,
    SvgDatasetCardinality, plan_cardinality,
};
use crate::resolver::{
    Evidence, FieldAddress, SourceFacts, SourceKind, collect_target_facts, resolve_replace_host,
};

use super::{
    Applicability, CloneSetupRequest, ReplaceHostRequest, ResolveScope, SourceHostDecision,
    SourceHostMapping, SourceModel, TargetSlot, TargetSlotId, WorkcopyError, fingerprint,
    target_resolution,
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CloneGrowthRequest {
    sample: FieldAddress,
    target_hostid: String,
}

#[derive(Clone, Serialize)]
pub struct CloneSeriesOption {
    field: FieldAddress,
    hostid: String,
    widget_name: String,
    dataset_index: u32,
    position: usize,
    source: SeriesLabel,
}

#[derive(Clone, Serialize)]
struct SeriesLabel {
    host: String,
    item: String,
    key: String,
}

#[derive(Serialize)]
pub struct CloneGrowthPreview {
    pub fingerprint: String,
    before: Vec<SeriesLabel>,
    after: Vec<SeriesLabel>,
}

impl CloneGrowthPreview {
    pub(super) fn bind_to(
        &mut self,
        request: &CloneSetupRequest,
        local: &str,
    ) -> Result<(), WorkcopyError> {
        self.fingerprint = fingerprint(&(
            request.revision,
            &request.source_fingerprint,
            &request.name,
            &request.hosts,
            request.keep_patterns,
            &request.pattern_choices,
            &request.growth,
            &request.lld_choices,
            local,
            &self.before,
            &self.after,
        ))?;
        Ok(())
    }
}

fn invalid() -> WorkcopyError {
    WorkcopyError::new("clone_growth_selection_invalid")
}

pub(super) fn samples(
    snapshot: &VersionedDashboardSnapshot,
    model: &SourceModel,
    facts: &SourceFacts,
) -> Vec<CloneSeriesOption> {
    let mut result = Vec::new();
    for widget in model
        .widgets
        .iter()
        .filter(|widget| widget.widget_type == "svggraph")
    {
        let fields = snapshot.raw_api_document()["pages"][widget.widget.page_index]["widgets"]
            [widget.widget.widget_index]["fields"]
            .as_array();
        let Some(fields) = fields else { continue };
        let names: Vec<_> = fields
            .iter()
            .filter_map(|field| field["name"].as_str())
            .collect();
        for dataset in &widget.datasets {
            if dataset.itemids.len() != 2
                || dataset.item_references != 0
                || !dataset.host_patterns.is_empty()
                || !dataset.item_patterns.is_empty()
            {
                continue;
            }
            let index = dataset.index;
            let prefix = format!("ds.{index}.itemids.");
            let third_color = format!("ds.{index}.color.2");
            if names
                .iter()
                .filter(|name| name.starts_with(&prefix))
                .count()
                != 2
                || names.contains(&third_color.as_str())
            {
                continue;
            }
            let rows: Vec<_> = (0..2)
                .filter_map(|position| {
                    let name = format!("{prefix}{position}");
                    let field = widget.sources.iter().find(|source| {
                        source.field.field_name == name
                            && source.applicability == Applicability::Editable
                    })?;
                    let SourceKind::Item { itemid } = &field.kind else {
                        return None;
                    };
                    let item = facts.items.get(itemid)?;
                    let host = facts.hosts.get(&item.hostid)?;
                    Some(CloneSeriesOption {
                        field: field.field.clone(),
                        hostid: host.hostid.clone(),
                        widget_name: widget.name.clone(),
                        dataset_index: index,
                        position,
                        source: SeriesLabel {
                            host: host.name.clone(),
                            item: item.name.clone(),
                            key: item.key.clone(),
                        },
                    })
                })
                .collect();
            if rows.len() == 2 && rows[0].hostid != rows[1].hostid {
                result.extend(rows);
            }
        }
    }
    result
}

pub(super) struct Growth {
    pub transform: CloneCardinalityTransform,
    pub preview: CloneGrowthPreview,
}

pub(super) struct GrowthSelection {
    sample: CloneSeriesOption,
    rows: Vec<CloneSeriesOption>,
}

impl GrowthSelection {
    pub(super) async fn add_slot(
        input: &CloneGrowthRequest,
        options: &[CloneSeriesOption],
        model: &SourceModel,
        facts: &SourceFacts,
        reader: &ReadOnlyGateway,
        mappings: &mut [SourceHostMapping],
        selected: &mut Vec<HostListing>,
    ) -> Result<Self, WorkcopyError> {
        let sample = options
            .iter()
            .find(|option| option.field == input.sample)
            .ok_or_else(invalid)?;
        if !is_zabbix_id(&input.target_hostid)
            || input.target_hostid == sample.hostid
            || selected
                .iter()
                .any(|host| host.hostid == input.target_hostid)
        {
            return Err(invalid());
        }
        let mapping = mappings
            .iter_mut()
            .find(|mapping| mapping.source_host.hostid == sample.hostid)
            .ok_or_else(invalid)?;
        if mapping
            .references
            .iter()
            .any(|row| matches!(row.widget_type.as_str(), "item" | "graph"))
        {
            return Err(WorkcopyError::new("clone_growth_single_host_widget"));
        }
        let [reference] = mapping.references.as_mut_slice() else {
            return Err(WorkcopyError::new("clone_growth_other_references"));
        };
        let request = ReplaceHostRequest {
            source_hostid: sample.hostid.clone(),
            target_hostid: input.target_hostid.clone(),
            scope: ResolveScope::Dashboard,
        };
        let target = collect_target_facts(
            reader,
            model,
            &request.source_hostid,
            &request.target_hostid,
        )
        .await
        .map_err(|error| WorkcopyError::from_resolver(&error))?;
        let resolved = resolve_replace_host(model, &request, facts, &target)
            .map_err(|error| WorkcopyError::from_resolver(&error))?;
        let host = resolved
            .target_host
            .ok_or_else(|| WorkcopyError::new("clone_target_not_visible"))?;
        let extra = TargetSlot {
            slot_id: TargetSlotId(format!("extra-{}", sample.hostid)),
            host,
        };
        let mut row = resolved
            .rows
            .into_iter()
            .find(|row| row.field == sample.field)
            .ok_or_else(invalid)?;
        selected.push(HostListing {
            hostid: extra.host.hostid.clone(),
            host: extra.host.host.clone(),
            name: extra.host.name.clone(),
            groups: vec![],
            templates: vec![],
        });
        let SourceHostDecision::TargetSlots(targets) = &mut mapping.decision else {
            return Err(invalid());
        };
        targets.push(extra.clone());
        reference
            .targets
            .push(target_resolution(extra.slot_id, &mut row));
        Ok(Self {
            sample: sample.clone(),
            rows: options
                .iter()
                .filter(|option| {
                    option.field.widget == sample.field.widget
                        && option.dataset_index == sample.dataset_index
                })
                .cloned()
                .collect(),
        })
    }

    pub(super) fn plan(&self, mappings: &[SourceHostMapping]) -> Result<Growth, WorkcopyError> {
        let sample = &self.sample;
        let mut source_series = Vec::new();
        let mut slots = Vec::new();
        let mut resolutions = Vec::new();
        let mut before = Vec::new();
        let mut extra = None;
        for row in &self.rows {
            let mapping = mappings
                .iter()
                .find(|mapping| mapping.source_host.hostid == row.hostid)
                .ok_or_else(invalid)?;
            let SourceHostDecision::TargetSlots(targets) = &mapping.decision else {
                return Err(invalid());
            };
            let slot = targets.first().ok_or_else(invalid)?;
            let reference = mapping
                .references
                .iter()
                .find(|reference| reference.field == row.field)
                .ok_or_else(invalid)?;
            source_series.push(SourceSeries {
                slot_id: slot.slot_id.clone(),
                field: row.field.clone(),
            });
            slots.push(slot.clone());
            resolutions.push(reference.targets.first().ok_or_else(invalid)?.clone());
            before.push(row.source.clone());
            if row.field == sample.field {
                extra = targets.get(1).zip(reference.targets.get(1));
            }
        }
        let (extra_slot, extra_resolution) = extra.ok_or_else(invalid)?;
        slots.push(extra_slot.clone());
        resolutions.push(extra_resolution.clone());
        let request = CardinalityRequest {
            widget_type: "svggraph".into(),
            source_slot_ids: source_series
                .iter()
                .map(|series| series.slot_id.clone())
                .collect(),
            target_slots: slots,
            datasets: vec![SvgDatasetCardinality {
                dataset_index: sample.dataset_index,
                binding: SeriesBindingKind::ExplicitItem,
                source_series,
                template_slot_id: Some(TargetSlotId(format!("host-{}", sample.hostid))),
                target_resolutions: resolutions,
                // Scope of this GUI path. It adds exactly one row to an existing pair.
                max_series: 3,
            }],
        };
        let plan = plan_cardinality(&request);
        if !plan.is_ready() {
            return Err(WorkcopyError::new("clone_growth_metric_incompatible"));
        }
        let after = request
            .target_slots
            .iter()
            .zip(&request.datasets[0].target_resolutions)
            .map(|(slot, resolution)| {
                let Some(Evidence::Item(item)) = &resolution.evidence else {
                    return Err(WorkcopyError::new("clone_growth_metric_incompatible"));
                };
                Ok(SeriesLabel {
                    host: slot.host.name.clone(),
                    item: item.name.clone(),
                    key: item.key.clone(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Growth {
            transform: CloneCardinalityTransform {
                widget: sample.field.widget.clone(),
                plan,
                added_item_colors: Default::default(),
            },
            preview: CloneGrowthPreview {
                fingerprint: String::new(),
                before,
                after,
            },
        })
    }
}
