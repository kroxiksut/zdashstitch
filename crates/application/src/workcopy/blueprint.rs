// SPDX-License-Identifier: MIT OR Apache-2.0

//! A local-only add-widget operation with explicit source assignments.

use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zdashstitch_core::formats::{
    BlueprintFieldValue, BlueprintMetric, Cardinality, Layout, MetricOrigin, MetricSelector,
    RegistryState, Selection, ValueType, Widget, WidgetBlueprint, ZabbixObjectKind,
    ZabbixWidgetField, ZabbixWidgetFieldValue,
};
use zdashstitch_zabbix::{
    BlueprintItemBinding, ItemFlags, ReadOnlyGateway, ZabbixVersion, is_zabbix_id,
};

use crate::catalog_library::CatalogLibrary;
use crate::reusable_library::{ReusableDocument, ReusableLibrary};

use super::operations::{OP_VERSION_1, Operation};
use super::{
    DraftDetail, DraftPage, DraftWidget, WorkcopyError, WorkcopyService, is_fingerprint, now_ms,
};

pub(crate) const ADD_WIDGET: &str = "add_widget";
const MAX_PARAMS: usize = 64;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AddWidgetEntry {
    pub seq: u64,
    pub op: String,
    pub op_version: u32,
    pub recorded_at_ms: u64,
    pub base_fingerprint: String,
    pub blueprint_id: String,
    pub blueprint_version: u32,
    pub page_id: String,
    pub widget_type: String,
    pub name: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub view_mode: u8,
    pub fields: Vec<ResolvedBlueprintField>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResolvedBlueprintField {
    name: String,
    field_type: u8,
    value: Option<ResolvedValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    binding: Option<BlueprintItemBinding>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum ResolvedValue {
    Integer(i32),
    Text(String),
    Host(String),
    Item(String),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopyBlueprintRequest {
    expected_revision: u64,
    blueprint_id: String,
    blueprint_revision: u64,
    page_id: String,
    rebind_widget: Option<String>,
    roles: BTreeMap<String, RoleChoice>,
    metrics: BTreeMap<String, Option<MetricChoice>>,
    x: u32,
    y: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleChoice {
    host: Option<String>,
    slots: BTreeMap<String, Option<String>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricChoice {
    catalog_id: String,
    version: u32,
    metric_id: String,
}

#[derive(Serialize)]
pub struct CopyBlueprintPreview {
    pub revision: u64,
    pub widget: DraftWidgetPreview,
    pub unresolved: Vec<String>,
    pub fingerprint: String,
}

#[derive(Serialize)]
pub struct DraftWidgetPreview {
    pub id: String,
    pub name: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize)]
pub struct CopyBlueprintState {
    pub revision: u64,
    pub has_blueprint: bool,
    pub publication: Option<CopyBlueprintPublication>,
}

#[derive(Serialize)]
pub struct CopyBlueprintPublication {
    pub base_fingerprint: String,
    pub local_fingerprint: String,
    pub page_name: String,
    pub widget: DraftWidgetPreview,
}

pub(crate) fn only_blueprint(log: &[Operation]) -> Option<&AddWidgetEntry> {
    match log {
        [Operation::AddWidget(entry)] => Some(entry),
        _ => None,
    }
}

impl AddWidgetEntry {
    pub fn well_formed(&self, base: &str) -> bool {
        let mut names = BTreeSet::new();
        self.op == ADD_WIDGET
            && self.op_version == OP_VERSION_1
            && self.base_fingerprint == base
            && is_fingerprint(base)
            && self.blueprint_id.len() == 36
            && self.blueprint_version > 0
            && is_zabbix_id(&self.page_id)
            && matches!(self.widget_type.as_str(), "item" | "svggraph")
            && (1..=255).contains(&self.name.chars().count())
            && !self.name.chars().any(char::is_control)
            && self.view_mode <= 1
            && self.width > 0
            && self.height > 0
            && self.x.saturating_add(self.width) <= 72
            && self.y.saturating_add(self.height) <= 64
            && (1..=128).contains(&self.fields.len())
            && self.fields.iter().all(|field| {
                (1..=128).contains(&field.name.len())
                && names.insert(&field.name)
                && field.binding.as_ref().is_none_or(|binding| {
                    matches!(&field.value, Some(ResolvedValue::Item(id)) if id == &binding.itemid)
                        && is_zabbix_id(&binding.hostid)
                        && !binding.key.is_empty()
                        && binding.key.len() <= 8192
                        && binding.units.len() <= 8192
                        && matches!(binding.value_type, 0 | 3)
                })
                && match (&field.value, field.field_type) {
                    (Some(ResolvedValue::Integer(_)), 0) => true,
                    (Some(ResolvedValue::Text(value)), 1) => value.len() <= 8192,
                    (Some(ResolvedValue::Host(id)), 3) | (Some(ResolvedValue::Item(id)), 4) => {
                        is_zabbix_id(id)
                    }
                    (None, 3 | 4) => true,
                    _ => false,
                }
            })
    }

    pub(crate) fn fingerprint(&self) -> Result<String, WorkcopyError> {
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(self).map_err(|_| corrupt())?)
        ))
    }

    pub(crate) fn publication_input(&self) -> Option<(Value, Vec<BlueprintItemBinding>)> {
        let mut bindings = BTreeMap::new();
        let fields = self
            .fields
            .iter()
            .map(|field| {
                let value = match (&field.value, field.field_type) {
                    (Some(ResolvedValue::Integer(value)), 0) => value.to_string(),
                    (Some(ResolvedValue::Text(value)), 1) => value.clone(),
                    (Some(ResolvedValue::Item(id)), 4) => {
                        let binding = field
                            .binding
                            .as_ref()
                            .filter(|binding| &binding.itemid == id)?;
                        bindings.insert(&field.name, binding.clone());
                        id.clone()
                    }
                    _ => return None,
                };
                Some(json!({"name":field.name,"type":field.field_type.to_string(),"value":value}))
            })
            .collect::<Option<Vec<_>>>()?;
        Some((
            json!({
                "type": self.widget_type, "name": self.name, "view_mode": self.view_mode.to_string(),
                "x": self.x.to_string(), "y": self.y.to_string(),
                "width": self.width.to_string(), "height": self.height.to_string(), "fields": fields
            }),
            bindings.into_values().collect(),
        ))
    }

    fn widget(&self) -> DraftWidget {
        DraftWidget {
            id: format!("local-widget-{}", self.seq),
            name: self.name.clone(),
            widget_type: self.widget_type.clone(),
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            editable: false,
            unbound_sources: self
                .fields
                .iter()
                .filter(|field| field.value.is_none())
                .count() as u32,
            blueprint_id: Some(self.blueprint_id.clone()),
        }
    }

    pub(crate) fn portable_widget(&self) -> Result<Widget, WorkcopyError> {
        if self.view_mode != 0 {
            return Err(WorkcopyError::new("portable_conversion_unsupported_data"));
        }
        let fields = self
            .fields
            .iter()
            .map(|field| {
                let value = match (&field.value, field.field_type) {
                    (Some(ResolvedValue::Integer(value)), 0) => ZabbixWidgetFieldValue::Integer {
                        value: i64::from(*value),
                    },
                    (Some(ResolvedValue::Text(value)), 1) => ZabbixWidgetFieldValue::String {
                        value: value.clone(),
                    },
                    (_, 3) => ZabbixWidgetFieldValue::Unresolved {
                        object_kind: ZabbixObjectKind::Host,
                        display_name: String::new(),
                    },
                    (_, 4) => ZabbixWidgetFieldValue::Unresolved {
                        object_kind: ZabbixObjectKind::Item,
                        display_name: String::new(),
                    },
                    _ => return Err(corrupt()),
                };
                Ok(ZabbixWidgetField {
                    name: field.name.clone(),
                    field_type: u16::from(field.field_type),
                    registry_state: RegistryState::Known,
                    value,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Widget::Zabbix {
            id: format!("blueprint-widget-{}", self.seq),
            widget_type: self.widget_type.clone(),
            name: self.name.clone(),
            registry_state: RegistryState::Known,
            fields,
            layout: Layout::Fixed {
                x: u16::try_from(self.x).map_err(|_| corrupt())?,
                y: u16::try_from(self.y).map_err(|_| corrupt())?,
                w: u16::try_from(self.width).map_err(|_| corrupt())?,
                h: u16::try_from(self.height).map_err(|_| corrupt())?,
            },
        })
    }
}

pub(crate) fn apply_to_view(
    draft: &mut DraftDetail,
    operations: &[Operation],
) -> Result<(), WorkcopyError> {
    for entry in operations.iter().filter_map(|op| match op {
        Operation::AddWidget(entry) => Some(entry),
        _ => None,
    }) {
        let page = draft
            .pages
            .iter_mut()
            .find(|page| page.id == entry.page_id)
            .ok_or_else(corrupt)?;
        let widget = entry.widget();
        check_location(page, &widget).map_err(|_| corrupt())?;
        page.widgets.push(widget);
    }
    Ok(())
}

fn check_location(page: &DraftPage, widget: &DraftWidget) -> Result<(), WorkcopyError> {
    if page.widgets.len() >= 256
        || widget.x.saturating_add(widget.width) > 72
        || widget.y.saturating_add(widget.height) > 64
        || page.widgets.iter().any(|other| {
            widget.x < other.x.saturating_add(other.width)
                && other.x < widget.x.saturating_add(widget.width)
                && widget.y < other.y.saturating_add(other.height)
                && other.y < widget.y.saturating_add(widget.height)
        })
    {
        return Err(WorkcopyError::new("reusable_blueprint_placement"));
    }
    Ok(())
}

impl WorkcopyService {
    pub fn workcopy_blueprint_state(
        &self,
        local_id: &str,
    ) -> Result<CopyBlueprintState, WorkcopyError> {
        let (file, base, log) = self.load_log(local_id)?;
        let publication = only_blueprint(&log)
            .filter(|entry| {
                entry.publication_input().is_some_and(|(widget, bindings)| {
                    zdashstitch_zabbix::can_preview_blueprint_append(
                        &base,
                        &entry.page_id,
                        &widget,
                        &bindings,
                    )
                })
            })
            .map(|entry| -> Result<_, WorkcopyError> {
                let widget = entry.widget();
                Ok(CopyBlueprintPublication {
                    base_fingerprint: base.fingerprint().to_owned(),
                    local_fingerprint: entry.fingerprint()?,
                    page_name: super::view::find_page(base.raw_api_document(), &entry.page_id)
                        .and_then(|(_, page)| page["name"].as_str())
                        .ok_or_else(corrupt)?
                        .to_owned(),
                    widget: DraftWidgetPreview {
                        id: widget.id,
                        name: widget.name,
                        x: widget.x,
                        y: widget.y,
                        width: widget.width,
                        height: widget.height,
                    },
                })
            })
            .transpose()?;
        Ok(CopyBlueprintState {
            revision: file.revision,
            has_blueprint: log
                .iter()
                .any(|operation| matches!(operation, Operation::AddWidget(_))),
            publication,
        })
    }

    pub async fn insert_workcopy_blueprint(
        &self,
        local_id: &str,
        request: &CopyBlueprintRequest,
        reader: Option<&ReadOnlyGateway>,
        approved_fingerprint: Option<&str>,
        allow_unbound: bool,
    ) -> Result<CopyBlueprintPreview, WorkcopyError> {
        let (file, base, mut log) = self.load_log(local_id)?;
        if file.revision != request.expected_revision {
            return Err(WorkcopyError::new("workcopy_revision_conflict"));
        }
        let revision = file.revision;
        let version = base.server_version();
        let family = format!("{}.{}", version.major, version.minor);
        let root = self.store.root_path();
        let asset = ReusableLibrary::new(root.clone()).get(&request.blueprint_id)?;
        if asset.revision != request.blueprint_revision {
            return Err(WorkcopyError::new("reusable_conflict"));
        }
        let ReusableDocument::WidgetBlueprint(blueprint) = asset.asset else {
            return Err(shape());
        };
        check_blueprint(&blueprint, request, &family, version)?;
        let reader = reader.filter(|reader| {
            &reader.capabilities().version == version
                && file.source.server_identity_fingerprint.as_deref()
                    == Some(reader.identity().fingerprint())
        });
        let registry = Self::registry_for_version(version)?;
        let capability = registry
            .find(blueprint.widget_type.as_str())
            .ok_or_else(shape)?;
        let replacement = request
            .rebind_widget
            .as_ref()
            .map(|id| {
                log.iter()
                    .enumerate()
                    .find_map(|(index, operation)| match operation {
                        Operation::AddWidget(entry) if entry.widget().id == *id => {
                            Some((index, entry))
                        }
                        _ => None,
                    })
                    .filter(|(_, entry)| {
                        entry.blueprint_id == blueprint.blueprint_id
                            && entry.blueprint_version == blueprint.version
                            && entry.page_id == request.page_id
                            && entry.x == request.x
                            && entry.y == request.y
                            && entry.width == u32::from(blueprint.default_size.width)
                            && entry.height == u32::from(blueprint.default_size.height)
                    })
                    .ok_or_else(assignments)
            })
            .transpose()?;
        let mut entry = AddWidgetEntry {
            seq: if let Some((_, existing)) = replacement {
                existing.seq
            } else {
                log.last()
                    .map_or(0, Operation::seq)
                    .checked_add(1)
                    .ok_or_else(corrupt)?
            },
            op: ADD_WIDGET.to_owned(),
            op_version: OP_VERSION_1,
            recorded_at_ms: now_ms().max(file.updated_at_ms),
            base_fingerprint: file.base.fingerprint.clone(),
            blueprint_id: blueprint.blueprint_id.clone(),
            blueprint_version: blueprint.version,
            page_id: request.page_id.clone(),
            widget_type: blueprint.widget_type.as_str().to_owned(),
            name: blueprint.name.clone(),
            x: request.x,
            y: request.y,
            width: u32::from(blueprint.default_size.width),
            height: u32::from(blueprint.default_size.height),
            view_mode: blueprint
                .attributes
                .as_ref()
                .and_then(|attrs| attrs.view_mode)
                .unwrap_or(0),
            fields: Vec::new(),
        };
        let widget = entry.widget();
        let mut view = super::view::materialize(&file, base.raw_api_document(), &registry)?;
        apply_to_view(&mut view, &log)?;
        let page = view
            .pages
            .iter()
            .find(|page| page.id == request.page_id)
            .ok_or_else(|| WorkcopyError::new("reusable_selection_invalid"))?;
        if replacement.is_none() {
            check_location(page, &widget)?;
        }
        let catalog = CatalogLibrary::new(root);
        let hosts = verified_hosts(request, reader).await?;
        let mut inventory = BTreeMap::new();
        for field in &blueprint.fields {
            let kind = zdashstitch_zabbix::WidgetFieldType::try_from(u16::from(field.field_type))
                .map_err(|_| shape())?;
            if !capability.knows_field(&field.name, kind) {
                return Err(shape());
            }
            let mut binding = None;
            let value = match &field.value {
                BlueprintFieldValue::Integer { value } => Some(ResolvedValue::Integer(*value)),
                BlueprintFieldValue::String { value } => Some(ResolvedValue::Text(value.clone())),
                BlueprintFieldValue::Unbound { .. } => None,
                BlueprintFieldValue::Host {
                    role,
                    selection,
                    slot_id,
                } if field.field_type == 3 => {
                    selected_host(request, role, *selection, slot_id.as_deref())?
                        .map(ResolvedValue::Host)
                }
                BlueprintFieldValue::Metric {
                    metric,
                    role,
                    selection,
                    slot_id,
                } if field.field_type == 4 => {
                    let host = selected_host(request, role, *selection, slot_id.as_deref())?;
                    let parameter = parameter(metric)?;
                    let choice = request.metrics.get(parameter).ok_or_else(assignments)?;
                    if let (Some(host), Some(choice)) = (host, choice) {
                        let reader = reader.ok_or_else(not_connected)?;
                        let requirement = blueprint
                            .parameters
                            .metrics
                            .iter()
                            .find(|metric| metric.param == parameter)
                            .ok_or_else(assignments)?;
                        let host = hosts.get(&host).ok_or_else(binding_required)?;
                        let items = match inventory.entry(host.clone()) {
                            Entry::Occupied(entry) => entry.into_mut(),
                            Entry::Vacant(entry) => {
                                let result = reader
                                    .portable_host_items(host)
                                    .await
                                    .map_err(connect_error)?;
                                if result.truncated {
                                    return Err(WorkcopyError::new(
                                        "reusable_blueprint_inventory_incomplete",
                                    ));
                                }
                                entry.insert(result.objects)
                            }
                        };
                        let selected = resolve_metric(&catalog, choice, requirement, host, items)?;
                        let id = selected.itemid.clone();
                        binding = Some(selected);
                        Some(ResolvedValue::Item(id))
                    } else {
                        None
                    }
                }
                _ => return Err(shape()),
            };
            entry.fields.push(ResolvedBlueprintField {
                name: field.name.clone(),
                field_type: field.field_type,
                value,
                binding,
            });
        }
        if !entry.well_formed(&file.base.fingerprint) {
            return Err(shape());
        }
        let replace_at = if let Some((index, previous)) = replacement {
            if entry.fields.len() != previous.fields.len()
                || entry.fields.iter().zip(&previous.fields).any(|(new, old)| {
                    new.name != old.name
                        || new.field_type != old.field_type
                        || (matches!(new.field_type, 0 | 1) && new.value != old.value)
                })
                || entry.widget_type != previous.widget_type
                || entry.view_mode != previous.view_mode
            {
                return Err(WorkcopyError::new("reusable_conflict"));
            }
            entry.name.clone_from(&previous.name);
            Some(index)
        } else {
            None
        };
        let unresolved: Vec<_> = entry
            .fields
            .iter()
            .filter(|field| field.value.is_none())
            .map(|field| field.name.clone())
            .collect();
        let widget = entry.widget();
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(
                    local_id,
                    &entry.base_fingerprint,
                    &entry.blueprint_id,
                    request.blueprint_revision,
                    revision,
                    replace_at,
                    &entry.page_id,
                    &entry.widget_type,
                    &entry.name,
                    entry.x,
                    entry.y,
                    entry.width,
                    entry.height,
                    entry.view_mode,
                    &entry.fields
                ))
                .map_err(|_| corrupt())?
            )
        );
        let revision = if let Some(approved) = approved_fingerprint {
            if approved != fingerprint {
                return Err(WorkcopyError::new("reusable_conflict"));
            }
            if !allow_unbound && !unresolved.is_empty() {
                return Err(binding_required());
            }
            if let Some(index) = replace_at {
                log[index] = Operation::AddWidget(entry);
            } else {
                log.push(Operation::AddWidget(entry));
            }
            self.commit_operations(local_id, revision, &log)?.revision
        } else {
            revision
        };
        Ok(CopyBlueprintPreview {
            revision,
            widget: DraftWidgetPreview {
                id: widget.id,
                name: widget.name,
                x: widget.x,
                y: widget.y,
                width: widget.width,
                height: widget.height,
            },
            unresolved,
            fingerprint,
        })
    }

    pub async fn search_workcopy_blueprint_hosts(
        &self,
        local_id: &str,
        reader: &ReadOnlyGateway,
        search: &str,
    ) -> Result<zdashstitch_zabbix::HostObjects<zdashstitch_zabbix::HostListing>, WorkcopyError>
    {
        let file = self.load(local_id)?;
        if file.source.server_version != reader.capabilities().version.to_string()
            || file.source.server_identity_fingerprint.as_deref()
                != Some(reader.identity().fingerprint())
        {
            return Err(not_connected());
        }
        reader
            .search_hosts(search.trim(), 50)
            .await
            .map_err(connect_error)
    }
}

fn check_blueprint(
    blueprint: &WidgetBlueprint,
    request: &CopyBlueprintRequest,
    family: &str,
    version: &ZabbixVersion,
) -> Result<(), WorkcopyError> {
    if blueprint.adapter != family
        || blueprint
            .declared_compatibility
            .as_ref()
            .is_some_and(|range| {
                version
                    .to_string()
                    .parse()
                    .map_or(true, |version| !range.contains(&version))
            })
        || blueprint
            .attributes
            .as_ref()
            .is_some_and(|attrs| attrs.rf_rate.is_some())
        || blueprint.fields.len() > 128
    {
        return Err(shape());
    }
    if request.roles.len() != blueprint.parameters.roles.len()
        || request.metrics.len() != blueprint.parameters.metrics.len()
        || request.roles.len() + request.metrics.len() > MAX_PARAMS
        || blueprint
            .parameters
            .metrics
            .iter()
            .any(|metric| !request.metrics.contains_key(&metric.param))
    {
        return Err(assignments());
    }
    for role in &blueprint.parameters.roles {
        let assigned = request.roles.get(&role.param).ok_or_else(assignments)?;
        let declared = role.slots.as_deref().unwrap_or(&[]);
        if assigned.slots.len() != declared.len()
            || declared
                .iter()
                .any(|slot| !assigned.slots.contains_key(slot))
            || (role.cardinality == Cardinality::Many && assigned.host.is_some())
        {
            return Err(assignments());
        }
    }
    Ok(())
}

async fn verified_hosts(
    request: &CopyBlueprintRequest,
    reader: Option<&ReadOnlyGateway>,
) -> Result<BTreeSet<String>, WorkcopyError> {
    let selected: BTreeSet<_> = request
        .roles
        .values()
        .flat_map(|choice| std::iter::once(&choice.host).chain(choice.slots.values()))
        .filter_map(Option::as_ref)
        .cloned()
        .collect();
    if selected.len() > MAX_PARAMS || !selected.iter().all(|id| is_zabbix_id(id)) {
        return Err(assignments());
    }
    if selected.is_empty() {
        return Ok(BTreeSet::new());
    }
    let reader = reader.ok_or_else(not_connected)?;
    let ids: Vec<_> = selected.iter().cloned().collect();
    let visible: BTreeSet<_> = reader
        .hosts_by_ids(&ids)
        .await
        .map_err(connect_error)?
        .into_iter()
        .map(|host| host.hostid)
        .collect();
    if visible != selected {
        return Err(binding_required());
    }
    Ok(visible)
}

fn selected_host(
    request: &CopyBlueprintRequest,
    role: &str,
    selection: Selection,
    slot: Option<&str>,
) -> Result<Option<String>, WorkcopyError> {
    let role = request
        .roles
        .get(parameter(role)?)
        .ok_or_else(assignments)?;
    match (selection, slot) {
        (Selection::One, None) => Ok(role.host.clone()),
        (Selection::Slot, Some(slot)) => role.slots.get(slot).cloned().ok_or_else(assignments),
        _ => Err(shape()),
    }
}

fn parameter(reference: &str) -> Result<&str, WorkcopyError> {
    reference.strip_prefix('$').ok_or_else(assignments)
}

fn resolve_metric(
    catalog: &CatalogLibrary,
    choice: &MetricChoice,
    requirement: &BlueprintMetric,
    host: &str,
    items: &[zdashstitch_zabbix::ItemDetails],
) -> Result<BlueprintItemBinding, WorkcopyError> {
    let template = catalog
        .get(&choice.catalog_id, choice.version)?
        .ok_or_else(|| WorkcopyError::new("catalog_missing"))?;
    let metric = template
        .catalog
        .metrics
        .iter()
        .find(|metric| metric.id == choice.metric_id)
        .ok_or_else(assignments)?;
    let MetricSelector::Key(key) = &metric.selector else {
        return Err(assignments());
    };
    let value_type = match metric.value_type {
        ValueType::NumericFloat => 0,
        ValueType::NumericUnsigned => 3,
        _ => return Err(assignments()),
    };
    if !matches!(
        metric.origin,
        MetricOrigin::TemplateItem | MetricOrigin::HostItem
    ) || requirement
        .value_type
        .is_some_and(|required| required != metric.value_type)
        || requirement
            .units
            .as_ref()
            .is_some_and(|required| required != &metric.units)
    {
        return Err(assignments());
    }
    let mut candidates = items
        .iter()
        .filter(|item| item.hostid == host && &item.key == key);
    let item = candidates.next().ok_or_else(binding_required)?;
    if candidates.next().is_some()
        || item.value_type != value_type
        || item.units != metric.units
        || item.flags != ItemFlags::Plain
        || item.enabled == Some(false)
    {
        return Err(binding_required());
    }
    Ok(BlueprintItemBinding {
        itemid: item.itemid.clone(),
        hostid: host.to_owned(),
        key: key.clone(),
        value_type,
        units: metric.units.clone(),
    })
}

fn corrupt() -> WorkcopyError {
    WorkcopyError::new("workcopy_corrupt")
}
fn not_connected() -> WorkcopyError {
    WorkcopyError::new("not_connected")
}
fn shape() -> WorkcopyError {
    WorkcopyError::new("reusable_blueprint_shape_unsupported")
}
fn assignments() -> WorkcopyError {
    WorkcopyError::new("reusable_assignments_invalid")
}
fn binding_required() -> WorkcopyError {
    WorkcopyError::new("source_binding_required")
}
fn connect_error(error: zdashstitch_zabbix::ZabbixError) -> WorkcopyError {
    WorkcopyError::from_zabbix(&error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_insert_is_readable_and_needs_explicit_rebinding_to_publish() {
        let base = "f".repeat(64);
        let mut record = json!({
            "seq":1,"op":"add_widget","op_version":1,"recorded_at_ms":1,
            "base_fingerprint":base,
            "blueprint_id":"91c0d2e4-6a1b-4f3c-8e5d-7b9a0c1d2e3f",
            "blueprint_version":1,"page_id":"10","widget_type":"item",
            "name":"Local widget","x":0,"y":0,"width":12,"height":6,"view_mode":0,
            "fields":[{"name":"itemid.0","field_type":4,"value":{"kind":"item","value":"987654321"}}]
        });
        let decode = |raw| -> AddWidgetEntry {
            serde_json::from_value(raw).unwrap_or_else(|_| panic!("valid local record"))
        };
        let legacy = decode(record.clone());
        assert!(legacy.well_formed(&base));
        assert!(legacy.publication_input().is_none());
        assert!(legacy.portable_widget().is_ok());
        record["fields"][0]["binding"] = json!({
            "itemid":"987654321","hostid":"123456789","key":"synthetic.protected.selector",
            "value_type":3,"units":"B"
        });
        let bound = decode(record.clone());
        assert!(bound.well_formed(&base));
        assert!(bound.publication_input().is_some());
        assert_ne!(legacy.fingerprint().ok(), bound.fingerprint().ok());
        let portable = serde_json::to_string(&bound.portable_widget().ok())
            .unwrap_or_else(|_| panic!("serialize portable widget"));
        for private in ["987654321", "123456789", "synthetic.protected.selector"] {
            assert!(!portable.contains(private));
        }
        record["fields"][0]["binding"]["itemid"] = json!("8");
        let mismatched = decode(record);
        assert!(!mismatched.well_formed(&base));
        assert!(mismatched.publication_input().is_none());
    }
}
