// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reusable widget operations edit only local portable data. Publishability is
//! checked independently by the existing adapters and closed publisher.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use zdashstitch_core::formats::{
    BLUEPRINT_FORMAT, BlueprintField, BlueprintFieldValue, BlueprintMetric, BlueprintObject,
    BlueprintParameters, BlueprintRole, BlueprintSize, Cardinality, HostSelection, ItemExpansion,
    Layout, MetricReference, PROFILE_FORMAT, Page, ProjectRoot, RegistryState, ReusableWidgetType,
    Selection, StyleProfile, StyleSection, TypedPortableBundle, ValueType, Widget, WidgetBlueprint,
    ZabbixObjectKind, ZabbixWidgetField, ZabbixWidgetFieldValue,
};
use zdashstitch_zabbix::{AdapterProfile, WidgetFieldType};

use crate::{
    compatibility::project_evaluation_profile,
    offline_pages::OfflinePageView,
    reusable_library::{ReusableDocument, ReusableEntry},
    workcopy::WorkcopyError,
};

#[derive(Serialize)]
pub struct ReusableActionResult {
    pub preview: ReusablePreview,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<OfflinePageView>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PageReusableState {
    marks: BTreeMap<String, ProfileMark>,
    inserted: BTreeMap<String, u32>,
    history: Option<ReusableHistory>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReusableHistoryAction {
    Switch { redo: bool },
    Detach { widget_id: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileMark {
    profile_id: String,
    profile_version: u32,
    profile_name: String,
    applied_at_ms: u64,
    fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReusableHistory {
    revision: u64,
    redo: bool,
    snapshot: Page,
    marks: BTreeMap<String, ProfileMark>,
    inserted: BTreeMap<String, u32>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ReusableInfo {
    marks: BTreeMap<String, ProfileMark>,
    pub(crate) undo: bool,
    pub(crate) redo: bool,
}

impl ReusableInfo {
    pub(crate) fn is_empty(&self) -> bool {
        self.marks.is_empty() && !self.undo && !self.redo
    }
}

impl PageReusableState {
    pub(crate) fn info(&self, revision: u64) -> ReusableInfo {
        let history = self
            .history
            .as_ref()
            .filter(|history| history.revision == revision);
        ReusableInfo {
            marks: self.marks.clone(),
            undo: history.is_some_and(|item| !item.redo),
            redo: history.is_some_and(|item| item.redo),
        }
    }

    pub(crate) fn record(
        &mut self,
        snapshot: Page,
        revision: u64,
        now_ms: u64,
        asset: &ReusableEntry,
        preview: &ReusablePreview,
    ) {
        let page_id = snapshot.id.clone();
        self.history = Some(ReusableHistory {
            revision,
            redo: false,
            snapshot,
            marks: self.marks.clone(),
            inserted: self.inserted.clone(),
        });
        match preview {
            ReusablePreview::Style { widgets } => {
                for widget in widgets
                    .iter()
                    .filter(|widget| widget.page_id == page_id && !widget.changes.is_empty())
                {
                    let mark = self
                        .marks
                        .entry(widget.widget_id.clone())
                        .or_insert_with(|| ProfileMark {
                            profile_id: asset.asset.id().to_owned(),
                            profile_version: 0,
                            profile_name: asset.asset.name().to_owned(),
                            applied_at_ms: 0,
                            fields: BTreeMap::new(),
                        });
                    if mark.profile_id != asset.asset.id() {
                        mark.profile_id = asset.asset.id().to_owned();
                        mark.fields.clear();
                    }
                    mark.fields.extend(
                        widget
                            .changes
                            .iter()
                            .map(|change| (change.name.clone(), change.after.clone())),
                    );
                    mark.profile_name = asset.asset.name().to_owned();
                    mark.profile_version = asset.asset.version();
                    mark.applied_at_ms = now_ms;
                }
            }
            ReusablePreview::Insert { .. } => {
                let count = self
                    .inserted
                    .entry(asset.asset.id().to_owned())
                    .or_default();
                *count = count.saturating_add(1);
            }
        }
    }

    pub(crate) fn history(
        &mut self,
        page: &mut Page,
        revision: u64,
        redo: bool,
    ) -> Result<(), WorkcopyError> {
        let history = self
            .history
            .as_mut()
            .filter(|history| {
                history.revision == revision
                    && history.redo == redo
                    && page.id == history.snapshot.id
            })
            .ok_or_else(|| WorkcopyError::new("reusable_history_unavailable"))?;
        history.revision = revision
            .checked_add(1)
            .ok_or_else(|| WorkcopyError::new("storage_limit_reached"))?;
        history.redo = !redo;
        std::mem::swap(page, &mut history.snapshot);
        std::mem::swap(&mut self.marks, &mut history.marks);
        std::mem::swap(&mut self.inserted, &mut history.inserted);
        Ok(())
    }

    pub(crate) fn edited(&mut self, page: &Page) {
        self.history = None;
        self.marks
            .retain(|id, _| page.widgets.iter().any(|widget| widget.id() == id));
    }

    pub(crate) fn detach(
        &mut self,
        page: &Page,
        revision: u64,
        widget_id: &str,
    ) -> Result<(), WorkcopyError> {
        if !page.widgets.iter().any(|widget| widget.id() == widget_id)
            || !self.marks.contains_key(widget_id)
        {
            return Err(WorkcopyError::new("reusable_selection_invalid"));
        }
        self.history = Some(ReusableHistory {
            revision: revision
                .checked_add(1)
                .ok_or_else(|| WorkcopyError::new("storage_limit_reached"))?,
            redo: false,
            snapshot: page.clone(),
            marks: self.marks.clone(),
            inserted: self.inserted.clone(),
        });
        self.marks.remove(widget_id);
        Ok(())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.marks.is_empty() && self.inserted.is_empty() && self.history.is_none()
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReusableAction {
    pub asset_id: String,
    pub asset_revision: u64,
    pub expected_revision: u64,
    pub operation: ReusableOperation,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReusableOperation {
    Style {
        widget_ids: Vec<String>,
        mode: StyleMode,
        #[serde(default)]
        entire_dashboard: bool,
    },
    Insert {
        roles: BTreeMap<String, RoleAssignment>,
        metrics: BTreeMap<String, Option<MetricReference>>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StyleMode {
    Replace,
    KeepLocal,
    Merge,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleAssignment {
    pub role: String,
    #[serde(default)]
    pub slots: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FieldChange {
    pub name: String,
    pub before: Option<String>,
    pub after: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct WidgetStylePreview {
    pub page_id: String,
    pub page_name: String,
    pub widget_id: String,
    pub widget_name: String,
    pub state: &'static str,
    pub changes: Vec<FieldChange>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReusablePreview {
    Style { widgets: Vec<WidgetStylePreview> },
    Insert { unresolved: usize },
}

impl ReusablePreview {
    pub(crate) fn changed_pages(&self, insertion_page: &str) -> BTreeSet<String> {
        match self {
            Self::Style { widgets } => widgets
                .iter()
                .filter(|widget| !widget.changes.is_empty())
                .map(|widget| widget.page_id.clone())
                .collect(),
            Self::Insert { .. } => BTreeSet::from([insertion_page.to_owned()]),
        }
    }
    pub fn has_changes(&self) -> bool {
        match self {
            Self::Style { widgets } => widgets.iter().any(|widget| !widget.changes.is_empty()),
            Self::Insert { .. } => true,
        }
    }
}

/// Transform a copy of the bundle, so any unsupported field, invalid selection
/// or project validation failure leaves all of the original data untouched.
pub(crate) fn plan(
    bundle: &TypedPortableBundle,
    page_id: &str,
    asset: &ReusableEntry,
    action: &ReusableAction,
    state: Option<&PageReusableState>,
) -> Result<(TypedPortableBundle, ReusablePreview), WorkcopyError> {
    let profile = check_adapter(bundle, asset, action)?;
    let mut result = bundle.clone();
    let page = page_mut(&mut result.project.root, page_id)?;
    let preview = match (&asset.asset, &action.operation) {
        (
            ReusableDocument::StyleProfile(style),
            ReusableOperation::Style {
                widget_ids,
                mode,
                entire_dashboard: false,
            },
        ) => {
            let widgets = apply_style(page, style, *mode, widget_ids, state)?;
            ReusablePreview::Style { widgets }
        }
        (
            ReusableDocument::WidgetBlueprint(blueprint),
            ReusableOperation::Insert { roles, metrics },
        ) => {
            let (new, unresolved) = instantiate(bundle, profile, blueprint, roles, metrics, page)?;
            page.widgets.push(new);
            ReusablePreview::Insert { unresolved }
        }
        _ => return Err(WorkcopyError::new("reusable_operation_invalid")),
    };
    result
        .validate()
        .map_err(|_| WorkcopyError::new("reusable_draft_invalid"))?;
    Ok((result, preview))
}

fn check_adapter(
    bundle: &TypedPortableBundle,
    asset: &ReusableEntry,
    action: &ReusableAction,
) -> Result<AdapterProfile, WorkcopyError> {
    if asset.revision != action.asset_revision || asset.asset.id() != action.asset_id {
        return Err(WorkcopyError::new("reusable_conflict"));
    }
    let profile = project_evaluation_profile(&bundle.project)
        .ok_or_else(|| WorkcopyError::new("reusable_adapter_unsupported"))?;
    if profile.family() != asset.asset.adapter() {
        return Err(WorkcopyError::new("reusable_adapter_unsupported"));
    }
    let selection = &bundle.project.declared_compatibility.minimum;
    let compatibility = match &asset.asset {
        ReusableDocument::StyleProfile(doc) => doc.declared_compatibility.as_ref(),
        ReusableDocument::WidgetBlueprint(doc) => doc.declared_compatibility.as_ref(),
    };
    if compatibility.is_some_and(|range| !range.contains(selection))
        || (matches!(action.operation, ReusableOperation::Style { .. })
            && profile == AdapterProfile::V8_0
            && selection.patch != 0)
    {
        return Err(WorkcopyError::new("reusable_adapter_unsupported"));
    }

    Ok(profile)
}

pub(crate) fn plan_dashboard_style(
    bundle: &TypedPortableBundle,
    asset: &ReusableEntry,
    action: &ReusableAction,
    states: Option<&BTreeMap<String, PageReusableState>>,
) -> Result<(TypedPortableBundle, ReusablePreview), WorkcopyError> {
    check_adapter(bundle, asset, action)?;
    let (
        ReusableDocument::StyleProfile(style),
        ReusableOperation::Style {
            widget_ids,
            mode,
            entire_dashboard: true,
        },
    ) = (&asset.asset, &action.operation)
    else {
        return Err(WorkcopyError::new("reusable_operation_invalid"));
    };
    if !widget_ids.is_empty() {
        return Err(WorkcopyError::new("reusable_selection_invalid"));
    }
    let mut result = bundle.clone();
    let ProjectRoot::Dashboard(dashboard) = &mut result.project.root else {
        return Err(WorkcopyError::new("reusable_operation_invalid"));
    };
    let mut widgets = Vec::new();
    for page in dashboard
        .pages
        .iter_mut()
        .filter(|page| !page.widgets.is_empty())
    {
        let ids = page
            .widgets
            .iter()
            .map(|widget| widget.id().to_owned())
            .collect::<Vec<_>>();
        let state = states.and_then(|states| states.get(&page.id));
        widgets.extend(apply_style(page, style, *mode, &ids, state)?);
    }
    result
        .validate()
        .map_err(|_| WorkcopyError::new("reusable_draft_invalid"))?;
    Ok((result, ReusablePreview::Style { widgets }))
}

fn page_mut<'a>(root: &'a mut ProjectRoot, id: &str) -> Result<&'a mut Page, WorkcopyError> {
    match root {
        ProjectRoot::Page(page) if page.id == id => Some(page),
        ProjectRoot::Dashboard(dashboard) => dashboard.pages.iter_mut().find(|page| page.id == id),
        _ => None,
    }
    .ok_or_else(|| WorkcopyError::new("offline_page_not_found"))
}

fn apply_style(
    page: &mut Page,
    style: &StyleProfile,
    mode: StyleMode,
    widget_ids: &[String],
    state: Option<&PageReusableState>,
) -> Result<Vec<WidgetStylePreview>, WorkcopyError> {
    let ids: BTreeSet<&str> = widget_ids.iter().map(String::as_str).collect();
    if ids.is_empty()
        || ids.len() != widget_ids.len()
        || !ids
            .iter()
            .all(|id| page.widgets.iter().any(|widget| widget.id() == *id))
    {
        return Err(WorkcopyError::new("reusable_selection_invalid"));
    }
    let mut preview = Vec::with_capacity(ids.len());
    for widget in &mut page.widgets {
        if !ids.contains(widget.id()) {
            continue;
        }
        let id = widget.id().to_owned();
        let widget_name = match widget {
            Widget::Graph { title, .. } | Widget::ItemValue { title, .. } => title.clone(),
            Widget::Zabbix { name, .. } => name.clone(),
        };
        let previous = state
            .and_then(|state| state.marks.get(&id))
            .filter(|mark| mark.profile_id == style.profile_id);
        let (state, changes) = match widget {
            Widget::Graph { series, .. } => {
                let section = style.widget_types.get(&ReusableWidgetType::Svggraph);
                match section.and_then(|section| section.series_palette.as_ref()) {
                    Some(palette) if matches!(style.adapter.as_str(), "7.4" | "8.0") => {
                        let mut changes = Vec::new();
                        for (index, series) in series.iter_mut().enumerate() {
                            let new = format!("#{}", palette[index % palette.len()]);
                            let name = format!("series.{index}.color");
                            if series.color.as_ref() != Some(&new)
                                && may_replace(mode, previous, &name, series.color.as_deref())
                            {
                                changes.push(FieldChange {
                                    name,
                                    before: series.color.replace(new.clone()),
                                    after: new,
                                });
                            }
                        }
                        (
                            if only_legacy_palette(section) {
                                "compatible"
                            } else {
                                "partial"
                            },
                            changes,
                        )
                    }
                    _ => ("incompatible", Vec::new()),
                }
            }
            Widget::Zabbix {
                widget_type,
                registry_state: RegistryState::Known,
                fields,
                ..
            } if widget_type == "svggraph" => {
                let section = style.widget_types.get(&ReusableWidgetType::Svggraph);
                match section.and_then(|section| section.series_palette.as_ref()) {
                    Some(palette) if matches!(style.adapter.as_str(), "7.4" | "8.0") => {
                        let changes =
                            replace_existing_graph_colors(fields, palette, mode, previous);
                        (
                            if changes.is_empty() {
                                "incompatible"
                            } else {
                                "partial"
                            },
                            changes,
                        )
                    }
                    _ => ("incompatible", Vec::new()),
                }
            }
            _ => ("incompatible", Vec::new()),
        };
        preview.push(WidgetStylePreview {
            page_id: page.id.clone(),
            page_name: page.name.clone(),
            widget_id: id,
            widget_name,
            state,
            changes,
        });
    }
    Ok(preview)
}

fn may_replace(
    mode: StyleMode,
    previous: Option<&ProfileMark>,
    field: &str,
    current: Option<&str>,
) -> bool {
    match (mode, previous, current) {
        (StyleMode::Replace, _, _) | (_, _, None) => true,
        (_, Some(mark), Some(current)) => match mark.fields.get(field) {
            Some(from_profile) => from_profile == current,
            None => matches!(mode, StyleMode::Merge),
        },
        _ => false,
    }
}

fn only_legacy_palette(section: Option<&StyleSection>) -> bool {
    section.is_some_and(|section| {
        section.widget_fields.is_empty()
            && section
                .dataset_fields
                .as_ref()
                .is_none_or(BTreeMap::is_empty)
            && section.attributes.is_none()
    })
}

/// Existing, indexed color fields belonging to explicit sources. Pattern
/// forms, unknown fields and absent color rows need their own evidence.
fn replace_existing_graph_colors(
    fields: &mut [ZabbixWidgetField],
    palette: &[String],
    mode: StyleMode,
    previous: Option<&ProfileMark>,
) -> Vec<FieldChange> {
    let source_names: BTreeSet<String> = fields
        .iter()
        .filter(|field| {
            field.registry_state == RegistryState::Known
                && field.field_type == 4
                && matches!(
                    field.value,
                    ZabbixWidgetFieldValue::Metric { .. }
                        | ZabbixWidgetFieldValue::LldInstance { .. }
                )
        })
        .map(|field| field.name.clone())
        .collect();
    let mut changes = Vec::new();
    for field in fields {
        let parts: Vec<_> = field.name.split('.').collect();
        let ["ds", dataset, "color", index] = parts.as_slice() else {
            continue;
        };
        let Ok(position) = index.parse::<usize>() else {
            continue;
        };
        if field.registry_state != RegistryState::Known
            || field.field_type != 1
            || !source_names.contains(&format!("ds.{dataset}.itemids.{index}"))
        {
            continue;
        }
        let ZabbixWidgetFieldValue::String { value } = &mut field.value else {
            continue;
        };
        let new = &palette[position % palette.len()];
        if value != new && may_replace(mode, previous, &field.name, Some(value)) {
            changes.push(FieldChange {
                name: field.name.clone(),
                before: Some(std::mem::replace(value, new.clone())),
                after: new.clone(),
            });
        }
    }
    changes
}

fn instantiate(
    bundle: &TypedPortableBundle,
    adapter: AdapterProfile,
    blueprint: &WidgetBlueprint,
    roles: &BTreeMap<String, RoleAssignment>,
    metrics: &BTreeMap<String, Option<MetricReference>>,
    page: &Page,
) -> Result<(Widget, usize), WorkcopyError> {
    if blueprint
        .attributes
        .as_ref()
        .is_some_and(|a| a.view_mode.is_some() || a.rf_rate.is_some())
    {
        return Err(WorkcopyError::new("reusable_blueprint_shape_unsupported"));
    }
    if roles.len() != blueprint.parameters.roles.len()
        || metrics.len() != blueprint.parameters.metrics.len()
    {
        return Err(WorkcopyError::new("reusable_assignments_invalid"));
    }
    for role in &blueprint.parameters.roles {
        let assigned = roles.get(&role.param).ok_or_else(invalid_assignments)?;
        let target = bundle
            .project
            .roles
            .get(&assigned.role)
            .ok_or_else(invalid_assignments)?;
        if role.cardinality != target.cardinality
            || role.slots.as_ref().map_or(0, Vec::len) != assigned.slots.len()
        {
            return Err(invalid_assignments());
        }
        let mut target_slots = BTreeSet::new();
        for slot in role.slots.iter().flatten() {
            let to = assigned.slots.get(slot).ok_or_else(invalid_assignments)?;
            if !target
                .slots
                .as_ref()
                .is_some_and(|slots| slots.contains(to))
                || !target_slots.insert(to)
            {
                return Err(invalid_assignments());
            }
        }
    }
    for metric in &blueprint.parameters.metrics {
        if let Some(reference) = metrics.get(&metric.param).ok_or_else(invalid_assignments)? {
            let chosen = bundle
                .catalogs
                .get(&reference.catalog)
                .and_then(|catalog| {
                    catalog
                        .metrics
                        .iter()
                        .find(|candidate| candidate.id == reference.metric)
                })
                .ok_or_else(invalid_assignments)?;
            if !chosen.value_type.is_numeric()
                || metric
                    .value_type
                    .is_some_and(|required| required != chosen.value_type)
                || metric
                    .units
                    .as_ref()
                    .is_some_and(|units| units != &chosen.units)
            {
                return Err(invalid_assignments());
            }
        }
    }
    let registry = adapter.widget_capabilities();
    let widget_type = blueprint.widget_type.as_str();
    let capability = registry.find(widget_type).ok_or_else(unsupported_shape)?;
    let mut fields = Vec::with_capacity(blueprint.fields.len());
    let mut unresolved = 0;
    for field in &blueprint.fields {
        let field_type = u16::from(field.field_type);
        let known = WidgetFieldType::try_from(field_type)
            .is_ok_and(|kind| capability.knows_field(&field.name, kind));
        if !known {
            return Err(unsupported_shape());
        }
        let value = match &field.value {
            BlueprintFieldValue::Integer { value } => ZabbixWidgetFieldValue::Integer {
                value: i64::from(*value),
            },
            BlueprintFieldValue::String { value } => ZabbixWidgetFieldValue::String {
                value: value.clone(),
            },
            BlueprintFieldValue::Host {
                role,
                selection,
                slot_id,
            } if field_type == 3 => {
                let hosts = resolve_role(roles, role, *selection, slot_id.as_deref())?;
                ZabbixWidgetFieldValue::Host {
                    role: hosts.role,
                    selection: hosts.selection,
                    slot_id: hosts.slot_id,
                }
            }
            BlueprintFieldValue::Metric {
                metric,
                role,
                selection,
                slot_id,
            } if field_type == 4 => {
                let hosts = resolve_role(roles, role, *selection, slot_id.as_deref())?;
                match metrics
                    .get(metric.strip_prefix('$').ok_or_else(invalid_assignments)?)
                    .ok_or_else(invalid_assignments)?
                {
                    Some(metric) => ZabbixWidgetFieldValue::Metric {
                        catalog: metric.catalog.clone(),
                        metric: metric.metric.clone(),
                        hosts,
                    },
                    None => {
                        unresolved += 1;
                        unbound(ZabbixObjectKind::Item)
                    }
                }
            }
            BlueprintFieldValue::Unbound {
                object: BlueprintObject::Item,
            } if field_type == 4 => {
                unresolved += 1;
                unbound(ZabbixObjectKind::Item)
            }
            BlueprintFieldValue::Unbound {
                object: BlueprintObject::Host,
            } if field_type == 3 => {
                unresolved += 1;
                unbound(ZabbixObjectKind::Host)
            }
            _ => return Err(unsupported_shape()),
        };
        fields.push(ZabbixWidgetField {
            name: field.name.clone(),
            field_type,
            registry_state: RegistryState::Known,
            value,
        });
    }
    let id = (1..=256)
        .map(|n| format!("blueprint-widget-{n}"))
        .find(|id| page.widgets.iter().all(|widget| widget.id() != id))
        .ok_or_else(|| WorkcopyError::new("storage_limit_reached"))?;
    let widget = Widget::Zabbix {
        id,
        widget_type: widget_type.to_owned(),
        name: blueprint.name.clone(),
        registry_state: RegistryState::Known,
        fields,
        layout: Layout::Auto {
            w: blueprint.default_size.width.into(),
            h: blueprint.default_size.height.into(),
        },
    };
    Ok((widget, unresolved))
}

fn resolve_role(
    roles: &BTreeMap<String, RoleAssignment>,
    role: &str,
    selection: Selection,
    slot_id: Option<&str>,
) -> Result<HostSelection, WorkcopyError> {
    let assignment = roles
        .get(role.strip_prefix('$').ok_or_else(invalid_assignments)?)
        .ok_or_else(invalid_assignments)?;
    let slot_id = slot_id
        .map(|name| {
            assignment
                .slots
                .get(name)
                .cloned()
                .ok_or_else(invalid_assignments)
        })
        .transpose()?;
    Ok(HostSelection {
        role: assignment.role.clone(),
        selection,
        slot_id,
    })
}

fn unbound(object_kind: ZabbixObjectKind) -> ZabbixWidgetFieldValue {
    ZabbixWidgetFieldValue::Unresolved {
        object_kind,
        display_name: String::new(),
    }
}

fn invalid_assignments() -> WorkcopyError {
    WorkcopyError::new("reusable_assignments_invalid")
}

fn unsupported_shape() -> WorkcopyError {
    WorkcopyError::new("reusable_blueprint_shape_unsupported")
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionKind {
    StyleProfile,
    WidgetBlueprint,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extraction {
    pub expected_revision: u64,
    pub widget_id: String,
    pub name: String,
    pub kind: ExtractionKind,
}

pub fn extract(
    bundle: &TypedPortableBundle,
    request: &Extraction,
) -> Result<ReusableDocument, WorkcopyError> {
    let adapter = project_evaluation_profile(&bundle.project)
        .ok_or_else(|| WorkcopyError::new("reusable_adapter_unsupported"))?;
    let ProjectRoot::Page(page) = &bundle.project.root else {
        return Err(WorkcopyError::new("offline_page_not_found"));
    };
    let widget = page
        .widgets
        .iter()
        .find(|widget| widget.id() == request.widget_id)
        .ok_or_else(|| WorkcopyError::new("reusable_selection_invalid"))?;
    let doc = match request.kind {
        ExtractionKind::StyleProfile => extract_palette(bundle, widget, adapter, &request.name)?,
        ExtractionKind::WidgetBlueprint => {
            extract_blueprint(bundle, widget, adapter, &request.name)?
        }
    };
    ReusableDocument::validated(doc)
}

const PLACEHOLDER_UUID: &str = "00000000-0000-4000-8000-000000000000";

fn extract_palette(
    bundle: &TypedPortableBundle,
    widget: &Widget,
    adapter: AdapterProfile,
    name: &str,
) -> Result<ReusableDocument, WorkcopyError> {
    if adapter == AdapterProfile::V8_0 && bundle.project.declared_compatibility.minimum.patch != 0 {
        return Err(WorkcopyError::new("reusable_adapter_unsupported"));
    }
    // A legacy Graph has one explicit series order. A generic Graph requires
    // the caller to select a dataset; do not pick one implicitly.
    let Widget::Graph { series, .. } = widget else {
        return Err(WorkcopyError::new("reusable_style_source_unsupported"));
    };
    let palette: Option<Vec<_>> = series
        .iter()
        .map(|series| {
            series
                .color
                .as_ref()?
                .strip_prefix('#')
                .map(str::to_ascii_uppercase)
        })
        .collect();
    let section = StyleSection {
        series_palette: Some(
            palette.ok_or_else(|| WorkcopyError::new("reusable_style_source_unsupported"))?,
        ),
        ..StyleSection::default()
    };
    Ok(ReusableDocument::StyleProfile(StyleProfile {
        format: PROFILE_FORMAT.to_owned(),
        profile_id: PLACEHOLDER_UUID.to_owned(),
        name: name.to_owned(),
        description: None,
        tags: Vec::new(),
        version: 1,
        adapter: adapter.family().to_owned(),
        declared_compatibility: Some(bundle.project.declared_compatibility.clone()),
        widget_types: BTreeMap::from([(ReusableWidgetType::Svggraph, section)]),
    }))
}

#[derive(Default)]
struct ExtractionParameters {
    roles: Vec<BlueprintRole>,
    metrics: Vec<BlueprintMetric>,
    role_names: BTreeMap<String, usize>,
    slot_names: BTreeMap<(usize, String), String>,
}

impl ExtractionParameters {
    fn role(
        &mut self,
        bundle: &TypedPortableBundle,
        hosts: &HostSelection,
    ) -> Result<(String, Selection, Option<String>), WorkcopyError> {
        let role = bundle
            .project
            .roles
            .get(&hosts.role)
            .ok_or_else(unsupported_shape)?;
        let role_index = *self
            .role_names
            .entry(hosts.role.clone())
            .or_insert_with(|| {
                let index = self.roles.len();
                self.roles.push(BlueprintRole {
                    param: format!("role-{:02}", index + 1),
                    cardinality: role.cardinality,
                    slots: None,
                    description: None,
                });
                index
            });
        let definition = self
            .roles
            .get_mut(role_index)
            .ok_or_else(unsupported_shape)?;
        let slot = match (role.cardinality, hosts.selection) {
            (Cardinality::One, Selection::All | Selection::One) => None,
            (Cardinality::Many, Selection::Slot) => {
                let id = hosts.slot_id.as_ref().ok_or_else(unsupported_shape)?;
                let slots = definition.slots.get_or_insert_with(Vec::new);
                let slot = self
                    .slot_names
                    .entry((role_index, id.clone()))
                    .or_insert_with(|| {
                        let alias = format!("slot-{:02}", slots.len() + 1);
                        slots.push(alias.clone());
                        alias
                    });
                Some(slot.clone())
            }
            _ => return Err(unsupported_shape()),
        };
        let selection = if slot.is_some() {
            Selection::Slot
        } else {
            Selection::One
        };
        Ok((format!("${}", definition.param), selection, slot))
    }

    fn metric(
        &mut self,
        bundle: &TypedPortableBundle,
        reference: &MetricReference,
        hosts: &HostSelection,
    ) -> Result<BlueprintFieldValue, WorkcopyError> {
        let (role, selection, slot_id) = self.role(bundle, hosts)?;
        let chosen = bundle
            .catalogs
            .get(&reference.catalog)
            .and_then(|catalog| {
                catalog
                    .metrics
                    .iter()
                    .find(|metric| metric.id == reference.metric)
            })
            .ok_or_else(unsupported_shape)?;
        if matches!(chosen.value_type, ValueType::Binary | ValueType::Json) {
            return Err(unsupported_shape());
        }
        let param = format!("metric-{:02}", self.metrics.len() + 1);
        self.metrics.push(BlueprintMetric {
            param: param.clone(),
            value_type: Some(chosen.value_type),
            units: Some(chosen.units.clone()),
            description: None,
        });
        Ok(BlueprintFieldValue::Metric {
            metric: format!("${}", param),
            role,
            selection,
            slot_id,
        })
    }
}

fn extract_blueprint(
    bundle: &TypedPortableBundle,
    widget: &Widget,
    adapter: AdapterProfile,
    name: &str,
) -> Result<ReusableDocument, WorkcopyError> {
    let mut extracted = ExtractionParameters::default();
    let mut fields = Vec::new();
    let widget_type = match widget {
        Widget::ItemValue {
            metric,
            hosts,
            expansion: ItemExpansion::Single,
            ..
        } => {
            fields.push(BlueprintField {
                name: "itemid.0".to_owned(),
                field_type: 4,
                value: extracted.metric(bundle, metric, hosts)?,
            });
            ReusableWidgetType::Item
        }
        Widget::Graph { series, .. } => {
            fields.push(BlueprintField {
                name: "ds.0.dataset_type".to_owned(),
                field_type: 0,
                value: BlueprintFieldValue::Integer { value: 0 },
            });
            for (index, series) in series.iter().enumerate() {
                fields.push(BlueprintField {
                    name: format!("ds.0.itemids.{index}"),
                    field_type: 4,
                    value: extracted.metric(bundle, &series.metric, &series.hosts)?,
                });
                if let Some(color) = &series.color {
                    fields.push(BlueprintField {
                        name: format!("ds.0.color.{index}"),
                        field_type: 1,
                        value: BlueprintFieldValue::String {
                            value: color.trim_start_matches('#').to_ascii_uppercase(),
                        },
                    });
                }
            }
            ReusableWidgetType::Svggraph
        }
        Widget::Zabbix {
            widget_type,
            registry_state: RegistryState::Known,
            fields: existing,
            ..
        } => {
            let reusable_type = match widget_type.as_str() {
                "item" => ReusableWidgetType::Item,
                "svggraph" => ReusableWidgetType::Svggraph,
                _ => return Err(unsupported_shape()),
            };
            for field in existing {
                if field.registry_state != RegistryState::Known {
                    return Err(unsupported_shape());
                }
                // Widget instance links are allocated outside the reusable document.
                if field.name == "reference" {
                    continue;
                }
                let value = match &field.value {
                    ZabbixWidgetFieldValue::Integer { value } => BlueprintFieldValue::Integer {
                        value: i32::try_from(*value).map_err(|_| unsupported_shape())?,
                    },
                    ZabbixWidgetFieldValue::String { value } => BlueprintFieldValue::String {
                        value: value.clone(),
                    },
                    ZabbixWidgetFieldValue::Metric {
                        catalog,
                        metric,
                        hosts,
                    } => {
                        let reference = MetricReference {
                            catalog: catalog.clone(),
                            metric: metric.clone(),
                        };
                        extracted.metric(bundle, &reference, hosts)?
                    }
                    ZabbixWidgetFieldValue::Host {
                        role,
                        selection,
                        slot_id,
                    } => {
                        let source = HostSelection {
                            role: role.clone(),
                            selection: *selection,
                            slot_id: slot_id.clone(),
                        };
                        let (role, selection, slot_id) = extracted.role(bundle, &source)?;
                        BlueprintFieldValue::Host {
                            role,
                            selection,
                            slot_id,
                        }
                    }
                    ZabbixWidgetFieldValue::Unresolved {
                        object_kind: ZabbixObjectKind::Item,
                        ..
                    } => BlueprintFieldValue::Unbound {
                        object: BlueprintObject::Item,
                    },
                    ZabbixWidgetFieldValue::Unresolved {
                        object_kind: ZabbixObjectKind::Host,
                        ..
                    } => BlueprintFieldValue::Unbound {
                        object: BlueprintObject::Host,
                    },
                    _ => return Err(unsupported_shape()),
                };
                fields.push(BlueprintField {
                    name: field.name.clone(),
                    field_type: u8::try_from(field.field_type).map_err(|_| unsupported_shape())?,
                    value,
                });
            }
            reusable_type
        }
        _ => return Err(unsupported_shape()),
    };
    let (width, height) = match widget.layout() {
        Layout::Fixed { w, h, .. } | Layout::Auto { w, h } => (*w, *h),
    };
    Ok(ReusableDocument::WidgetBlueprint(WidgetBlueprint {
        format: BLUEPRINT_FORMAT.to_owned(),
        blueprint_id: PLACEHOLDER_UUID.to_owned(),
        name: name.to_owned(),
        description: None,
        tags: Vec::new(),
        version: 1,
        adapter: adapter.family().to_owned(),
        declared_compatibility: Some(bundle.project.declared_compatibility.clone()),
        widget_type,
        default_size: BlueprintSize {
            width: u8::try_from(width).map_err(|_| unsupported_shape())?,
            height: u8::try_from(height).map_err(|_| unsupported_shape())?,
        },
        attributes: None,
        parameters: BlueprintParameters {
            roles: extracted.roles,
            metrics: extracted.metrics,
        },
        fields,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        offline_dashboards::OfflineDashboardStore,
        offline_pages::OfflinePageStore,
        reusable_library::{ImportMode, ReusableLibrary, ReusableOrigin},
    };
    use zdashstitch_core::formats::{Role, from_yaml_str, load_typed_project_file};

    fn fixture() -> TypedPortableBundle {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/page-v2.yaml");
        load_typed_project_file(&path).unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn whole_page_palette_is_atomic_and_keeps_geometry_and_sources() {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let root = root.path().to_owned();
        let store = OfflinePageStore::new(root.clone());
        let mut bundle = fixture();
        bundle.project.declared_compatibility = AdapterProfile::V8_0.widget_version_range();
        let first_widget = match &mut bundle.project.root {
            ProjectRoot::Page(page) => {
                let mut second = page.widgets[0].clone();
                if let Widget::Graph { id, .. } = &mut second {
                    *id = "second-graph".to_owned();
                }
                page.widgets.push(second);
                page.widgets[0].clone()
            }
            _ => panic!("fixture page"),
        };
        let page = store
            .save_imported(bundle)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut style: StyleProfile =
            from_yaml_str(include_str!("../../../examples/style-profile.yaml"))
                .unwrap_or_else(|error| panic!("{error}"));
        style.adapter = "8.0".to_owned();
        style.declared_compatibility = None;
        let profile = ReusableLibrary::new(root)
            .import(ReusableDocument::StyleProfile(style), ImportMode::Copy)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut action = ReusableAction {
            asset_id: profile.asset.id().to_owned(),
            asset_revision: profile.revision,
            expected_revision: page.revision,
            operation: ReusableOperation::Style {
                widget_ids: vec!["os-memory".to_owned(), "missing".to_owned()],
                mode: StyleMode::Replace,
                entire_dashboard: false,
            },
        };
        assert!(
            store
                .reusable_action(&page.local_id, &action, true)
                .is_err()
        );
        assert_eq!(store.get(&page.local_id).ok().flatten(), Some(page.clone()));
        action.operation = ReusableOperation::Style {
            widget_ids: vec!["os-memory".to_owned(), "second-graph".to_owned()],
            mode: StyleMode::Replace,
            entire_dashboard: false,
        };
        let preview = store
            .reusable_action(&page.local_id, &action, false)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(preview.page.is_none());
        assert_eq!(store.get(&page.local_id).ok().flatten(), Some(page.clone()));
        let result = store
            .reusable_action(&page.local_id, &action, true)
            .unwrap_or_else(|error| panic!("{error}"));
        let updated = result.page.unwrap_or_else(|| panic!("commit result")).page;
        assert_eq!(updated.revision, page.revision + 1);
        assert_eq!(updated.bundle.project.roles, page.bundle.project.roles);
        assert_eq!(updated.bundle.catalogs, page.bundle.catalogs);
        let ProjectRoot::Page(data) = &updated.bundle.project.root else {
            panic!("page")
        };
        assert_eq!(data.widgets.len(), 2);
        for widget in &data.widgets {
            let Widget::Graph { series, layout, .. } = widget else {
                panic!("graph")
            };
            let Widget::Graph {
                series: original,
                layout: original_layout,
                ..
            } = &first_widget
            else {
                panic!("graph")
            };
            assert_eq!(layout, original_layout);
            assert_eq!(series[0].metric, original[0].metric);
            assert_eq!(series[0].hosts, original[0].hosts);
            assert_eq!(series[0].color.as_deref(), Some("#3156A3"));
        }
        assert!(
            store
                .reusable_action(&page.local_id, &action, true)
                .is_err()
        );
    }

    #[test]
    fn blueprint_extraction_and_insertion_use_explicit_slots() {
        let mut bundle = fixture();
        let role = bundle
            .project
            .roles
            .get_mut("system_nodes")
            .unwrap_or_else(|| panic!("role"));
        role.slots = Some(vec!["synthetic-a".into(), "synthetic-b".into()]);
        let ProjectRoot::Page(page) = &mut bundle.project.root else {
            panic!("page")
        };
        let Widget::Graph { series, .. } = &mut page.widgets[0] else {
            panic!("graph")
        };
        series[0].hosts.selection = Selection::Slot;
        series[0].hosts.slot_id = Some("synthetic-a".to_owned());
        series[0].color = Some("#3156A3".to_owned());
        let asset = extract(
            &bundle,
            &Extraction {
                expected_revision: 1,
                widget_id: "os-memory".to_owned(),
                name: "Synthetic graph".to_owned(),
                kind: ExtractionKind::WidgetBlueprint,
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let data = String::from_utf8(asset.export().unwrap_or_else(|error| panic!("{error}")))
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(!data.contains("synthetic-a"));
        assert!(!data.contains("system_nodes"));
        assert!(!data.contains("memory_used"));
        assert!(data.contains("role-01") && data.contains("metric-01") && data.contains("slot-01"));

        bundle.project.roles.insert(
            "target-role".to_owned(),
            Role {
                cardinality: Cardinality::Many,
                description: None,
                slots: Some(vec!["target-slot".to_owned()]),
            },
        );
        let stored = ReusableEntry {
            asset,
            revision: 1,
            origin: ReusableOrigin::Local,
        };
        let mut action = ReusableAction {
            asset_id: stored.asset.id().to_owned(),
            asset_revision: 1,
            expected_revision: 1,
            operation: ReusableOperation::Insert {
                roles: BTreeMap::from([(
                    "role-01".to_owned(),
                    RoleAssignment {
                        role: "target-role".to_owned(),
                        slots: BTreeMap::new(),
                    },
                )]),
                metrics: BTreeMap::from([(
                    "metric-01".to_owned(),
                    Some(MetricReference {
                        catalog: "linux".to_owned(),
                        metric: "memory_used".to_owned(),
                    }),
                )]),
            },
        };
        assert!(plan(&bundle, "operating-system", &stored, &action, None).is_err());
        let ReusableOperation::Insert { roles, .. } = &mut action.operation else {
            panic!("insert")
        };
        roles
            .get_mut("role-01")
            .unwrap_or_else(|| panic!("role"))
            .slots
            .insert("slot-01".to_owned(), "target-slot".to_owned());
        let (updated, preview) = plan(&bundle, "operating-system", &stored, &action, None)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(matches!(preview, ReusablePreview::Insert { unresolved: 0 }));
        let ProjectRoot::Page(page) = updated.project.root else {
            panic!("page")
        };
        let Widget::Zabbix { fields, .. } = &page.widgets[1] else {
            panic!("inserted graph")
        };
        assert!(fields.iter().any(|field| matches!(
            &field.value,
            ZabbixWidgetFieldValue::Metric { hosts, .. }
            if hosts.role == "target-role" && hosts.slot_id.as_deref() == Some("target-slot")
        )));
    }

    #[test]
    fn dashboard_scope_updates_and_undoes_all_pages_in_one_revision() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let root = directory.path().to_owned();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/dashboard-v2.yaml");
        let mut bundle = load_typed_project_file(&path).unwrap_or_else(|error| panic!("{error}"));
        let ProjectRoot::Dashboard(data) = &mut bundle.project.root else {
            panic!("dashboard")
        };
        let mut sibling = data.pages[0].clone();
        sibling.id = "sibling-page".to_owned();
        data.pages.push(sibling);
        let store = OfflineDashboardStore::new(root.clone());
        let dashboard = store
            .save_imported(bundle)
            .unwrap_or_else(|error| panic!("{error}"));
        let library = ReusableLibrary::new(root);
        let profile = library
            .list()
            .unwrap_or_else(|error| panic!("{error}"))
            .into_iter()
            .find(|entry| entry.asset.id() == "7a2f3c10-5b7e-4c8a-9d21-0f6e4b1a2c3d")
            .unwrap_or_else(|| panic!("profile"));
        let action = ReusableAction {
            asset_id: profile.asset.id().to_owned(),
            asset_revision: profile.revision,
            expected_revision: dashboard.revision,
            operation: ReusableOperation::Style {
                widget_ids: Vec::new(),
                mode: StyleMode::Replace,
                entire_dashboard: true,
            },
        };
        let result = store
            .reusable_action(&dashboard.local_id, "operating-system", &action, true)
            .unwrap_or_else(|error| panic!("{error}"));
        let view = result.page.unwrap_or_else(|| panic!("committed view"));
        assert_eq!(view.page.revision, dashboard.revision + 1);
        assert!(view.reusable.undo);
        let ReusablePreview::Style { widgets } = result.preview else {
            panic!("style")
        };
        assert_eq!(widgets.len(), 2);
        assert!(widgets.iter().all(|widget| !widget.changes.is_empty()));
        assert_ne!(widgets[0].page_id, widgets[1].page_id);
        let restored = store
            .reusable_history(
                &dashboard.local_id,
                "sibling-page",
                view.page.revision,
                &ReusableHistoryAction::Switch { redo: false },
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(restored.reusable.redo);
        let reopened = store
            .get(&dashboard.local_id)
            .ok()
            .flatten()
            .unwrap_or_else(|| panic!("dashboard"));
        assert_eq!(reopened.bundle, dashboard.bundle);
    }
}
