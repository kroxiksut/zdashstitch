// SPDX-License-Identifier: MIT OR Apache-2.0

//! Explicit changes to a disposable portable copy; never mutates a draft.

use crate::compatibility::project_evaluation_profile;
use crate::portable_scan::{CandidateScan, scan};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use zdashstitch_core::formats::{
    Page, ProjectRoot, RegistryState, TypedPortableBundle, Widget, ZabbixWidgetField,
    ZabbixWidgetFieldValue,
};
use zdashstitch_zabbix::{AdapterProfile, WidgetCapabilityRegistry, WidgetFieldType};

const MAX_CHANGES: usize = 128;

#[derive(Clone, Deserialize, Serialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RedactionTarget {
    DashboardName,
    ProjectId,
    DashboardId,
    PageId {
        page_index: usize,
    },
    WidgetId {
        page_index: usize,
        widget_index: usize,
    },
    PageName {
        page_index: usize,
    },
    WidgetName {
        page_index: usize,
        widget_index: usize,
    },
    WidgetHostMask {
        page_index: usize,
        widget_index: usize,
        field_index: usize,
    },
    RoleAlias {
        role_index: usize,
    },
    SlotAlias {
        role_index: usize,
        slot_index: usize,
    },
    RoleDescription {
        role_index: usize,
    },
    WidgetUnresolvedName {
        page_index: usize,
        widget_index: usize,
        field_index: usize,
    },
    WidgetText {
        page_index: usize,
        widget_index: usize,
        field_index: usize,
    },
    WidgetItemPattern {
        page_index: usize,
        widget_index: usize,
        field_index: usize,
    },
    CatalogTemplateName {
        catalog_index: usize,
    },
    CatalogMetricName {
        catalog_index: usize,
        metric_index: usize,
    },
    CatalogMetricUnits {
        catalog_index: usize,
        metric_index: usize,
    },
    CatalogMetricId {
        catalog_index: usize,
        metric_index: usize,
    },
    CatalogId {
        catalog_index: usize,
    },
    CatalogSelectorAliases {
        catalog_index: usize,
    },
    CatalogClearUuids {
        catalog_index: usize,
    },
    CatalogClearSource {
        catalog_index: usize,
    },
    ClearProvenance,
    RemovePage {
        page_index: usize,
    },
    RemoveWidget {
        page_index: usize,
        widget_index: usize,
    },
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactionChange {
    pub target: RedactionTarget,
    pub replacement: String,
    #[serde(default)]
    pub apply_to_matching: bool,
}

#[derive(Serialize)]
pub struct ChangeSummary {
    pub target: RedactionTarget,
    pub request_index: usize,
    pub before_chars: usize,
    pub after_chars: usize,
    pub before_masked: String,
}

pub struct RedactionPreview {
    pub bundle: TypedPortableBundle,
    pub changes: Vec<ChangeSummary>,
    pub remaining: CandidateScan,
}

#[derive(Debug, thiserror::Error)]
pub enum RedactionError {
    #[error("invalid_redaction")]
    Invalid,
    #[error("protected_redaction_target")]
    Protected,
}

fn page_at(root: &mut ProjectRoot, index: usize) -> Option<&mut Page> {
    match root {
        ProjectRoot::Page(page) if index == 0 => Some(page),
        ProjectRoot::Dashboard(dashboard) => dashboard.pages.get_mut(index),
        _ => None,
    }
}

fn mask(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return "…".into();
    };
    if value.chars().count() < 5 {
        return "…".into();
    }
    let last = chars.next_back().unwrap_or(first);
    format!("{first}…{last}")
}

fn is_svggraph_host_mask(name: &str) -> bool {
    let parts: Vec<_> = name.split('.').collect();
    let number = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    match parts.as_slice() {
        ["ds" | "or", dataset, "hosts", host] => number(dataset) && number(host),
        ["problemhosts", host] => number(host),
        _ => false,
    }
}

fn is_safe_text_field(widget_type: &str, name: &str) -> bool {
    let parts: Vec<_> = name.split('.').collect();
    let numbered = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    match (widget_type, parts.as_slice()) {
        ("item", ["description"])
        | ("problems", ["problem"])
        | ("problems", ["tag_priority"])
        | ("problemsbysv", ["problem"])
        | ("svggraph", ["problem_name"]) => true,
        ("svggraph", ["ds", n, "data_set_label"]) => numbered(n),
        ("tophosts", ["columns", n, "name" | "text"]) => numbered(n),
        ("problems" | "problemsbysv" | "svggraph" | "tophosts", ["tags", n, "tag" | "value"]) => {
            numbered(n)
        }
        _ => false,
    }
}

fn is_item_pattern(widget_type: &str, name: &str) -> bool {
    let parts: Vec<_> = name.split('.').collect();
    let number = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    match (widget_type, parts.as_slice()) {
        ("svggraph", ["ds" | "or", dataset, "items", index]) => number(dataset) && number(index),
        ("tophosts", ["columns", index, "item"]) => number(index),
        _ => false,
    }
}

fn known_field(
    registry: Option<&WidgetCapabilityRegistry>,
    widget_type: &str,
    field: &ZabbixWidgetField,
) -> bool {
    field.registry_state == RegistryState::Known
        && registry
            .and_then(|registry| registry.find(widget_type))
            .is_some_and(|capability| {
                WidgetFieldType::try_from(field.field_type)
                    .is_ok_and(|kind| capability.knows_field(&field.name, kind))
            })
}

fn opaque_widget(
    registry: Option<&WidgetCapabilityRegistry>,
    widget_type: &str,
    state: RegistryState,
    fields: &[ZabbixWidgetField],
) -> bool {
    let Some(capability) = registry.and_then(|registry| registry.find(widget_type)) else {
        return true;
    };
    state != RegistryState::Known
        || capability.fields.is_empty()
        || fields
            .iter()
            .any(|field| !known_field(registry, widget_type, field))
}

fn root_has_opaque(registry: Option<&WidgetCapabilityRegistry>, root: &ProjectRoot) -> bool {
    let pages: Vec<&Page> = match root {
        ProjectRoot::Page(page) => vec![page],
        ProjectRoot::Dashboard(dashboard) => dashboard.pages.iter().collect(),
    };
    pages
        .iter()
        .flat_map(|page| &page.widgets)
        .any(|widget| match widget {
            Widget::Zabbix {
                widget_type,
                registry_state,
                fields,
                ..
            } => opaque_widget(registry, widget_type, *registry_state, fields),
            _ => false,
        })
}

pub(crate) fn can_remove_widgets(
    registry: Option<&WidgetCapabilityRegistry>,
    root: &ProjectRoot,
    removals: &BTreeSet<(usize, usize)>,
) -> bool {
    let pages: Vec<&Page> = match root {
        ProjectRoot::Page(page) => vec![page],
        ProjectRoot::Dashboard(dashboard) => dashboard.pages.iter().collect(),
    };
    if pages.iter().enumerate().all(|(page_index, page)| {
        page.widgets
            .iter()
            .enumerate()
            .all(|(widget_index, _)| removals.contains(&(page_index, widget_index)))
    }) {
        return true;
    }
    let mut removed_providers = BTreeSet::new();
    let mut remaining_consumers = BTreeSet::new();
    let mut providers = BTreeSet::new();
    let mut removed_opaque = false;
    for (page_index, page) in pages.iter().enumerate() {
        for (widget_index, widget) in page.widgets.iter().enumerate() {
            let Widget::Zabbix {
                widget_type,
                registry_state,
                fields,
                ..
            } = widget
            else {
                continue;
            };
            if opaque_widget(registry, widget_type, *registry_state, fields) {
                if removals.contains(&(page_index, widget_index)) {
                    removed_opaque = true;
                    continue;
                }
                return false;
            }
            for field in fields {
                if field.name != "reference" && !field.name.ends_with("._reference") {
                    continue;
                }
                let ZabbixWidgetFieldValue::String { value } = &field.value else {
                    return false;
                };
                if field.field_type != 1 {
                    return false;
                }
                if field.name == "reference" {
                    if value.is_empty() {
                        continue;
                    }
                    if value.len() != 5
                        || !value.bytes().all(|b| b.is_ascii_alphanumeric())
                        || !providers.insert(value.clone())
                    {
                        return false;
                    }
                    if removals.contains(&(page_index, widget_index)) {
                        removed_providers.insert(value.clone());
                    }
                } else if !removals.contains(&(page_index, widget_index)) && !value.is_empty() {
                    let Some((provider, channel)) = value.split_once("._") else {
                        return false;
                    };
                    if provider != "DASHBOARD"
                        && (provider.len() != 5
                            || !provider.bytes().all(|b| b.is_ascii_alphanumeric())
                            || channel.is_empty())
                    {
                        return false;
                    }
                    if provider != "DASHBOARD" {
                        remaining_consumers.insert(provider.to_owned());
                    }
                }
            }
        }
    }
    (!removed_opaque || remaining_consumers.is_empty())
        && removed_providers.is_disjoint(&remaining_consumers)
}

fn matching_targets(source: &TypedPortableBundle, expected: &str) -> Vec<RedactionTarget> {
    let mut found = Vec::new();
    let pages: Vec<&Page> = match &source.project.root {
        ProjectRoot::Page(page) => vec![page],
        ProjectRoot::Dashboard(dashboard) => {
            if dashboard.name == expected {
                found.push(RedactionTarget::DashboardName);
            }
            dashboard.pages.iter().collect()
        }
    };
    for (page_index, page) in pages.iter().enumerate() {
        if page.name == expected {
            found.push(RedactionTarget::PageName { page_index });
        }
        for (widget_index, widget) in page.widgets.iter().enumerate() {
            match widget {
                Widget::Graph { title, .. } | Widget::ItemValue { title, .. }
                    if title == expected =>
                {
                    found.push(RedactionTarget::WidgetName {
                        page_index,
                        widget_index,
                    })
                }
                Widget::Zabbix {
                    name,
                    registry_state: RegistryState::Known,
                    widget_type,
                    fields,
                    ..
                } => {
                    if name == expected {
                        found.push(RedactionTarget::WidgetName {
                            page_index,
                            widget_index,
                        });
                    }
                    if widget_type == "svggraph" {
                        for (field_index, field) in fields.iter().enumerate() {
                            if field.registry_state == RegistryState::Known
                                && field.field_type == 1
                                && is_svggraph_host_mask(&field.name)
                                && matches!(&field.value, ZabbixWidgetFieldValue::String { value } | ZabbixWidgetFieldValue::DynamicPattern { value, .. } if value == expected)
                            {
                                found.push(RedactionTarget::WidgetHostMask {
                                    page_index,
                                    widget_index,
                                    field_index,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    found
}

fn original_at<'a>(
    source: &'a TypedPortableBundle,
    registry: Option<&WidgetCapabilityRegistry>,
    target: &RedactionTarget,
) -> Result<&'a str, RedactionError> {
    let page = |index| match &source.project.root {
        ProjectRoot::Page(page) if index == 0 => Some(page),
        ProjectRoot::Dashboard(dashboard) => dashboard.pages.get(index),
        _ => None,
    };
    match target {
        RedactionTarget::ProjectId => Ok(&source.project.project_id),
        RedactionTarget::DashboardId => match &source.project.root {
            ProjectRoot::Dashboard(dashboard) => Ok(&dashboard.id),
            _ => Err(RedactionError::Invalid),
        },
        RedactionTarget::PageId { page_index } => page(*page_index)
            .map(|p| p.id.as_str())
            .ok_or(RedactionError::Invalid),
        RedactionTarget::WidgetId {
            page_index,
            widget_index,
        } => page(*page_index)
            .and_then(|p| p.widgets.get(*widget_index))
            .map(Widget::id)
            .ok_or(RedactionError::Invalid),
        RedactionTarget::DashboardName => match &source.project.root {
            ProjectRoot::Dashboard(dashboard) => Ok(&dashboard.name),
            _ => Err(RedactionError::Invalid),
        },
        RedactionTarget::PageName { page_index } => page(*page_index)
            .map(|page| page.name.as_str())
            .ok_or(RedactionError::Invalid),
        RedactionTarget::WidgetName {
            page_index,
            widget_index,
        } => match page(*page_index)
            .and_then(|page| page.widgets.get(*widget_index))
            .ok_or(RedactionError::Invalid)?
        {
            Widget::Graph { title, .. } | Widget::ItemValue { title, .. } => Ok(title),
            Widget::Zabbix {
                name,
                widget_type,
                registry_state: RegistryState::Known,
                fields,
                ..
            } if !opaque_widget(registry, widget_type, RegistryState::Known, fields) => Ok(name),
            Widget::Zabbix { .. } => Err(RedactionError::Protected),
        },
        RedactionTarget::WidgetHostMask {
            page_index,
            widget_index,
            field_index,
        } => {
            let widget = page(*page_index)
                .and_then(|page| page.widgets.get(*widget_index))
                .ok_or(RedactionError::Invalid)?;
            let Widget::Zabbix {
                widget_type,
                registry_state: RegistryState::Known,
                fields,
                ..
            } = widget
            else {
                return Err(RedactionError::Protected);
            };
            if widget_type != "svggraph" {
                return Err(RedactionError::Protected);
            }
            let field = fields.get(*field_index).ok_or(RedactionError::Invalid)?;
            if !known_field(registry, widget_type, field)
                || field.field_type != 1
                || !is_svggraph_host_mask(&field.name)
            {
                return Err(RedactionError::Protected);
            }
            match &field.value {
                ZabbixWidgetFieldValue::String { value }
                | ZabbixWidgetFieldValue::DynamicPattern { value, .. } => Ok(value),
                _ => Err(RedactionError::Protected),
            }
        }
        RedactionTarget::RoleAlias { role_index } => source
            .project
            .roles
            .keys()
            .nth(*role_index)
            .map(String::as_str)
            .ok_or(RedactionError::Invalid),
        RedactionTarget::SlotAlias {
            role_index,
            slot_index,
        } => source
            .project
            .roles
            .values()
            .nth(*role_index)
            .and_then(|role| role.slots.as_ref())
            .and_then(|slots| slots.get(*slot_index))
            .map(String::as_str)
            .ok_or(RedactionError::Invalid),
        RedactionTarget::RoleDescription { role_index } => source
            .project
            .roles
            .values()
            .nth(*role_index)
            .and_then(|r| r.description.as_deref())
            .ok_or(RedactionError::Invalid),
        RedactionTarget::CatalogTemplateName { catalog_index } => source
            .catalogs
            .values()
            .nth(*catalog_index)
            .and_then(|c| c.source.template_name.as_deref())
            .ok_or(RedactionError::Invalid),
        RedactionTarget::CatalogMetricName {
            catalog_index,
            metric_index,
        } => source
            .catalogs
            .values()
            .nth(*catalog_index)
            .and_then(|c| c.metrics.get(*metric_index))
            .map(|m| m.name.as_str())
            .ok_or(RedactionError::Invalid),
        RedactionTarget::CatalogMetricUnits {
            catalog_index,
            metric_index,
        } => source
            .catalogs
            .values()
            .nth(*catalog_index)
            .and_then(|catalog| catalog.metrics.get(*metric_index))
            .map(|metric| metric.units.as_str())
            .ok_or(RedactionError::Invalid),
        RedactionTarget::CatalogMetricId {
            catalog_index,
            metric_index,
        } => source
            .catalogs
            .values()
            .nth(*catalog_index)
            .and_then(|c| c.metrics.get(*metric_index))
            .map(|m| m.id.as_str())
            .ok_or(RedactionError::Invalid),
        RedactionTarget::CatalogId { catalog_index } => source
            .catalogs
            .keys()
            .nth(*catalog_index)
            .map(String::as_str)
            .ok_or(RedactionError::Invalid),
        RedactionTarget::CatalogSelectorAliases { catalog_index }
        | RedactionTarget::CatalogClearUuids { catalog_index }
        | RedactionTarget::CatalogClearSource { catalog_index } => source
            .catalogs
            .values()
            .nth(*catalog_index)
            .map(|c| c.catalog_id.as_str())
            .ok_or(RedactionError::Invalid),
        RedactionTarget::ClearProvenance => Ok("provenance"),
        RedactionTarget::RemovePage { page_index } => match &source.project.root {
            ProjectRoot::Dashboard(dashboard) => dashboard
                .pages
                .get(*page_index)
                .map(|page| page.name.as_str())
                .ok_or(RedactionError::Invalid),
            _ => Err(RedactionError::Invalid),
        },
        RedactionTarget::WidgetUnresolvedName {
            page_index,
            widget_index,
            field_index,
        } => {
            let Widget::Zabbix {
                widget_type,
                registry_state: RegistryState::Known,
                fields,
                ..
            } = page(*page_index)
                .and_then(|p| p.widgets.get(*widget_index))
                .ok_or(RedactionError::Invalid)?
            else {
                return Err(RedactionError::Protected);
            };
            let field = fields.get(*field_index).ok_or(RedactionError::Invalid)?;
            if !known_field(registry, widget_type, field) {
                return Err(RedactionError::Protected);
            }
            match &field.value {
                ZabbixWidgetFieldValue::Unresolved { display_name, .. } => Ok(display_name),
                _ => Err(RedactionError::Protected),
            }
        }
        RedactionTarget::WidgetText {
            page_index,
            widget_index,
            field_index,
        } => {
            let Widget::Zabbix {
                widget_type,
                registry_state: RegistryState::Known,
                fields,
                ..
            } = page(*page_index)
                .and_then(|p| p.widgets.get(*widget_index))
                .ok_or(RedactionError::Invalid)?
            else {
                return Err(RedactionError::Protected);
            };
            let field = fields.get(*field_index).ok_or(RedactionError::Invalid)?;
            if !known_field(registry, widget_type, field)
                || field.field_type != 1
                || !is_safe_text_field(widget_type, &field.name)
            {
                return Err(RedactionError::Protected);
            }
            match &field.value {
                ZabbixWidgetFieldValue::String { value }
                | ZabbixWidgetFieldValue::DynamicPattern { value, .. } => Ok(value),
                _ => Err(RedactionError::Protected),
            }
        }
        RedactionTarget::WidgetItemPattern {
            page_index,
            widget_index,
            field_index,
        } => {
            let Widget::Zabbix {
                widget_type,
                registry_state: RegistryState::Known,
                fields,
                ..
            } = page(*page_index)
                .and_then(|p| p.widgets.get(*widget_index))
                .ok_or(RedactionError::Invalid)?
            else {
                return Err(RedactionError::Protected);
            };
            let field = fields.get(*field_index).ok_or(RedactionError::Invalid)?;
            if !known_field(registry, widget_type, field)
                || field.field_type != 1
                || !is_item_pattern(widget_type, &field.name)
            {
                return Err(RedactionError::Protected);
            }
            match &field.value {
                ZabbixWidgetFieldValue::String { value }
                | ZabbixWidgetFieldValue::DynamicPattern { value, .. } => Ok(value),
                _ => Err(RedactionError::Protected),
            }
        }
        RedactionTarget::RemoveWidget {
            page_index,
            widget_index,
        } => {
            let widget = page(*page_index)
                .and_then(|p| p.widgets.get(*widget_index))
                .ok_or(RedactionError::Invalid)?;
            match widget {
                Widget::Graph { title, .. } | Widget::ItemValue { title, .. } => Ok(title),
                Widget::Zabbix { name, .. } => Ok(name),
            }
        }
    }
}

fn replace_role_or_slot(
    bundle: &mut TypedPortableBundle,
    registry: Option<&WidgetCapabilityRegistry>,
    current_roles: &mut [String],
    target: &RedactionTarget,
    replacement: &str,
) -> Result<String, RedactionError> {
    let (role_index, slot_index) = match target {
        RedactionTarget::RoleAlias { role_index } => (*role_index, None),
        RedactionTarget::SlotAlias {
            role_index,
            slot_index,
        } => (*role_index, Some(*slot_index)),
        _ => return Err(RedactionError::Invalid),
    };
    let role = current_roles
        .get(role_index)
        .ok_or(RedactionError::Invalid)?
        .clone();
    let old = if let Some(index) = slot_index {
        let old = bundle
            .project
            .roles
            .get(&role)
            .and_then(|r| r.slots.as_ref())
            .and_then(|s| s.get(index))
            .ok_or(RedactionError::Invalid)?
            .clone();
        if bundle
            .project
            .roles
            .values()
            .flat_map(|r| r.slots.iter().flatten())
            .any(|s| s == replacement)
        {
            return Err(RedactionError::Invalid);
        }
        old
    } else {
        if bundle.project.roles.contains_key(replacement) {
            return Err(RedactionError::Invalid);
        }
        role.clone()
    };
    let pages: Vec<&mut Page> = match &mut bundle.project.root {
        ProjectRoot::Page(page) => vec![page],
        ProjectRoot::Dashboard(dashboard) => dashboard.pages.iter_mut().collect(),
    };
    for page in pages {
        for widget in &mut page.widgets {
            let update = |host_role: &mut String, slot: &mut Option<String>| {
                if *host_role == role {
                    if slot_index.is_some() {
                        if slot.as_deref() == Some(old.as_str()) {
                            *slot = Some(replacement.to_owned());
                        }
                    } else {
                        *host_role = replacement.to_owned();
                    }
                }
            };
            match widget {
                Widget::Graph { series, .. } => {
                    for entry in series {
                        update(&mut entry.hosts.role, &mut entry.hosts.slot_id);
                    }
                }
                Widget::ItemValue { hosts, .. } => update(&mut hosts.role, &mut hosts.slot_id),
                Widget::Zabbix {
                    widget_type,
                    registry_state,
                    fields,
                    ..
                } => {
                    if opaque_widget(registry, widget_type, *registry_state, fields) {
                        return Err(RedactionError::Protected);
                    }
                    for field in fields {
                        match &mut field.value {
                            ZabbixWidgetFieldValue::Host { role, slot_id, .. } => {
                                update(role, slot_id)
                            }
                            ZabbixWidgetFieldValue::Metric { hosts, .. }
                            | ZabbixWidgetFieldValue::LldInstance { hosts, .. } => {
                                update(&mut hosts.role, &mut hosts.slot_id)
                            }
                            ZabbixWidgetFieldValue::DynamicPattern {
                                role: pattern_role, ..
                            } if slot_index.is_none() && *pattern_role == role => {
                                *pattern_role = replacement.to_owned();
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    if let Some(index) = slot_index {
        let slots = bundle
            .project
            .roles
            .get_mut(&role)
            .and_then(|r| r.slots.as_mut())
            .ok_or(RedactionError::Invalid)?;
        slots[index] = replacement.to_owned();
    } else {
        let entry = bundle
            .project
            .roles
            .remove(&role)
            .ok_or(RedactionError::Invalid)?;
        bundle.project.roles.insert(replacement.to_owned(), entry);
        current_roles[role_index] = replacement.to_owned();
    }
    Ok(old)
}

/// Apply only explicitly chosen human-readable names to an independent copy.
/// The caller binds the request to a draft revision before calling this function.
pub fn preview(
    source: &TypedPortableBundle,
    requested: &[RedactionChange],
) -> Result<RedactionPreview, RedactionError> {
    if requested.is_empty() || requested.len() > MAX_CHANGES {
        return Err(RedactionError::Invalid);
    }
    source.validate().map_err(|_| RedactionError::Invalid)?;
    let project_registry =
        project_evaluation_profile(&source.project).map(AdapterProfile::widget_capabilities);
    let registry = project_registry.as_ref();
    if requested.iter().any(|change| change.apply_to_matching) {
        let mut expanded = Vec::new();
        let mut request_indices = Vec::new();
        for (request_index, change) in requested.iter().enumerate() {
            if change.apply_to_matching
                && matches!(
                    change.target,
                    RedactionTarget::RoleAlias { .. }
                        | RedactionTarget::SlotAlias { .. }
                        | RedactionTarget::RoleDescription { .. }
                        | RedactionTarget::WidgetUnresolvedName { .. }
                        | RedactionTarget::WidgetText { .. }
                        | RedactionTarget::WidgetItemPattern { .. }
                        | RedactionTarget::CatalogTemplateName { .. }
                        | RedactionTarget::CatalogMetricName { .. }
                        | RedactionTarget::CatalogMetricUnits { .. }
                        | RedactionTarget::CatalogMetricId { .. }
                        | RedactionTarget::CatalogId { .. }
                        | RedactionTarget::CatalogSelectorAliases { .. }
                        | RedactionTarget::CatalogClearUuids { .. }
                        | RedactionTarget::CatalogClearSource { .. }
                        | RedactionTarget::ClearProvenance
                        | RedactionTarget::ProjectId
                        | RedactionTarget::DashboardId
                        | RedactionTarget::PageId { .. }
                        | RedactionTarget::WidgetId { .. }
                        | RedactionTarget::RemovePage { .. }
                        | RedactionTarget::RemoveWidget { .. }
                )
            {
                return Err(RedactionError::Protected);
            }
            let original = original_at(source, registry, &change.target)?;
            let targets = if change.apply_to_matching {
                matching_targets(source, original)
            } else {
                vec![change.target.clone()]
            };
            for target in targets {
                if expanded.len() >= MAX_CHANGES {
                    return Err(RedactionError::Invalid);
                }
                expanded.push(RedactionChange {
                    target,
                    replacement: change.replacement.clone(),
                    apply_to_matching: false,
                });
                request_indices.push(request_index);
            }
        }
        let mut result = preview(source, &expanded)?;
        for (summary, request_index) in result.changes.iter_mut().zip(request_indices) {
            summary.request_index = request_index;
        }
        return Ok(result);
    }
    let mut bundle = source.clone();
    let mut current_roles: Vec<String> = source.project.roles.keys().cloned().collect();
    let mut seen = BTreeSet::new();
    let mut removals = BTreeSet::new();
    let mut removed_pages = BTreeSet::new();
    let mut catalog_renames = Vec::<(String, String)>::new();
    let mut changes = Vec::with_capacity(requested.len());
    let has_opaque = root_has_opaque(registry, &source.project.root);
    for (request_index, change) in requested.iter().enumerate() {
        if !seen.insert(change.target.clone())
            || (change.replacement.trim().is_empty()
                && !matches!(
                    change.target,
                    RedactionTarget::RemoveWidget { .. }
                        | RedactionTarget::RemovePage { .. }
                        | RedactionTarget::CatalogSelectorAliases { .. }
                        | RedactionTarget::CatalogClearUuids { .. }
                        | RedactionTarget::CatalogClearSource { .. }
                        | RedactionTarget::CatalogMetricUnits { .. }
                        | RedactionTarget::ClearProvenance
                ))
            || change.replacement.chars().count() > 200
            || change.replacement.chars().any(char::is_control)
        {
            return Err(RedactionError::Invalid);
        }
        if matches!(change.target, RedactionTarget::CatalogMetricUnits { .. })
            && !change.replacement.is_empty()
            && change.replacement.trim().is_empty()
        {
            return Err(RedactionError::Invalid);
        }
        if has_opaque
            && matches!(
                change.target,
                RedactionTarget::ProjectId
                    | RedactionTarget::DashboardId
                    | RedactionTarget::PageId { .. }
                    | RedactionTarget::WidgetId { .. }
                    | RedactionTarget::CatalogMetricId { .. }
                    | RedactionTarget::CatalogId { .. }
            )
        {
            return Err(RedactionError::Protected);
        }
        if let RedactionTarget::RemovePage { page_index } = change.target {
            if !change.replacement.is_empty() || change.apply_to_matching {
                return Err(RedactionError::Invalid);
            }
            let ProjectRoot::Dashboard(dashboard) = &bundle.project.root else {
                return Err(RedactionError::Invalid);
            };
            let page = dashboard
                .pages
                .get(page_index)
                .ok_or(RedactionError::Invalid)?;
            for widget_index in 0..page.widgets.len() {
                removals.insert((page_index, widget_index));
            }
            removed_pages.insert(page_index);
            changes.push(ChangeSummary {
                target: change.target.clone(),
                request_index,
                before_chars: page.name.chars().count(),
                after_chars: 0,
                before_masked: mask(&page.name),
            });
            continue;
        }
        if let RedactionTarget::CatalogId { catalog_index } = change.target {
            let old = bundle
                .catalogs
                .keys()
                .nth(catalog_index)
                .ok_or(RedactionError::Invalid)?
                .clone();
            if bundle.catalogs.contains_key(&change.replacement)
                || catalog_renames
                    .iter()
                    .any(|(_, new)| *new == change.replacement)
            {
                return Err(RedactionError::Invalid);
            }
            changes.push(ChangeSummary {
                target: change.target.clone(),
                request_index,
                before_chars: old.chars().count(),
                after_chars: change.replacement.chars().count(),
                before_masked: mask(&old),
            });
            catalog_renames.push((old, change.replacement.clone()));
            continue;
        }
        if let RedactionTarget::CatalogMetricId {
            catalog_index,
            metric_index,
        } = change.target
        {
            let catalog = bundle
                .catalogs
                .values_mut()
                .nth(catalog_index)
                .ok_or(RedactionError::Invalid)?;
            if catalog
                .metrics
                .iter()
                .any(|metric| metric.id == change.replacement)
            {
                return Err(RedactionError::Invalid);
            }
            let catalog_id = catalog.catalog_id.clone();
            let metric = catalog
                .metrics
                .get_mut(metric_index)
                .ok_or(RedactionError::Invalid)?;
            let old = std::mem::replace(&mut metric.id, change.replacement.clone());
            let pages: Vec<&mut Page> = match &mut bundle.project.root {
                ProjectRoot::Page(page) => vec![page],
                ProjectRoot::Dashboard(dashboard) => dashboard.pages.iter_mut().collect(),
            };
            for page in pages {
                for widget in &mut page.widgets {
                    match widget {
                        Widget::Graph { series, .. } => {
                            for row in series {
                                if row.metric.catalog == catalog_id && row.metric.metric == old {
                                    row.metric.metric.clone_from(&change.replacement);
                                }
                            }
                        }
                        Widget::ItemValue { metric, .. } => {
                            if metric.catalog == catalog_id && metric.metric == old {
                                metric.metric.clone_from(&change.replacement);
                            }
                        }
                        Widget::Zabbix { fields, .. } => {
                            for field in fields {
                                if let ZabbixWidgetFieldValue::Metric {
                                    catalog, metric, ..
                                }
                                | ZabbixWidgetFieldValue::LldInstance {
                                    catalog, metric, ..
                                } = &mut field.value
                                    && *catalog == catalog_id
                                    && *metric == old
                                {
                                    metric.clone_from(&change.replacement);
                                }
                            }
                        }
                    }
                }
            }
            changes.push(ChangeSummary {
                target: change.target.clone(),
                request_index,
                before_chars: old.chars().count(),
                after_chars: change.replacement.chars().count(),
                before_masked: mask(&old),
            });
            continue;
        }
        if matches!(
            change.target,
            RedactionTarget::CatalogSelectorAliases { .. }
                | RedactionTarget::CatalogClearUuids { .. }
                | RedactionTarget::CatalogClearSource { .. }
                | RedactionTarget::ClearProvenance
        ) {
            if !change.replacement.is_empty() || change.apply_to_matching {
                return Err(RedactionError::Invalid);
            }
            let before = original_at(source, registry, &change.target)?;
            match change.target {
                RedactionTarget::CatalogSelectorAliases { catalog_index } => {
                    let catalog = bundle
                        .catalogs
                        .values_mut()
                        .nth(catalog_index)
                        .ok_or(RedactionError::Invalid)?;
                    for (index, metric) in catalog.metrics.iter_mut().enumerate() {
                        metric.selector = zdashstitch_core::formats::MetricSelector::Alias(
                            format!("metric_{}", index + 1),
                        );
                    }
                    catalog.export_profile =
                        zdashstitch_core::formats::ExportProfile::RedactedAliases;
                }
                RedactionTarget::CatalogClearUuids { catalog_index } => {
                    let catalog = bundle
                        .catalogs
                        .values_mut()
                        .nth(catalog_index)
                        .ok_or(RedactionError::Invalid)?;
                    catalog.source.template_uuid = None;
                    for metric in &mut catalog.metrics {
                        metric.uuid = None;
                    }
                }
                RedactionTarget::CatalogClearSource { catalog_index } => {
                    let catalog = bundle
                        .catalogs
                        .values_mut()
                        .nth(catalog_index)
                        .ok_or(RedactionError::Invalid)?;
                    catalog.source.kind = zdashstitch_core::formats::SourceKind::Manual;
                    catalog.source.export_format_version = None;
                    catalog.source.source_server_version = None;
                    catalog.source.template_name = None;
                    catalog.source.template_uuid = None;
                }
                RedactionTarget::ClearProvenance => {
                    bundle.project.provenance = zdashstitch_core::formats::ProjectProvenance::Local
                }
                _ => return Err(RedactionError::Invalid),
            }
            changes.push(ChangeSummary {
                target: change.target.clone(),
                request_index,
                before_chars: before.chars().count(),
                after_chars: 0,
                before_masked: mask(before),
            });
            continue;
        }
        if let RedactionTarget::RemoveWidget {
            page_index,
            widget_index,
        } = change.target
        {
            if !change.replacement.is_empty() || change.apply_to_matching {
                return Err(RedactionError::Invalid);
            }
            let before = original_at(source, registry, &change.target)?;
            removals.insert((page_index, widget_index));
            changes.push(ChangeSummary {
                target: change.target.clone(),
                request_index,
                before_chars: before.chars().count(),
                after_chars: 0,
                before_masked: mask(before),
            });
            continue;
        }
        if matches!(
            change.target,
            RedactionTarget::RoleAlias { .. } | RedactionTarget::SlotAlias { .. }
        ) {
            let before = replace_role_or_slot(
                &mut bundle,
                registry,
                &mut current_roles,
                &change.target,
                &change.replacement,
            )?;
            changes.push(ChangeSummary {
                target: change.target.clone(),
                request_index,
                before_chars: before.chars().count(),
                after_chars: change.replacement.chars().count(),
                before_masked: mask(&before),
            });
            continue;
        }
        let value = match &change.target {
            RedactionTarget::ProjectId => &mut bundle.project.project_id,
            RedactionTarget::DashboardId => match &mut bundle.project.root {
                ProjectRoot::Dashboard(dashboard) => &mut dashboard.id,
                _ => return Err(RedactionError::Invalid),
            },
            RedactionTarget::PageId { page_index } => {
                &mut page_at(&mut bundle.project.root, *page_index)
                    .ok_or(RedactionError::Invalid)?
                    .id
            }
            RedactionTarget::WidgetId {
                page_index,
                widget_index,
            } => {
                let widget = page_at(&mut bundle.project.root, *page_index)
                    .and_then(|page| page.widgets.get_mut(*widget_index))
                    .ok_or(RedactionError::Invalid)?;
                match widget {
                    Widget::Graph { id, .. }
                    | Widget::ItemValue { id, .. }
                    | Widget::Zabbix { id, .. } => id,
                }
            }
            RedactionTarget::DashboardName => match &mut bundle.project.root {
                ProjectRoot::Dashboard(dashboard) => &mut dashboard.name,
                _ => return Err(RedactionError::Invalid),
            },
            RedactionTarget::PageName { page_index } => {
                &mut page_at(&mut bundle.project.root, *page_index)
                    .ok_or(RedactionError::Invalid)?
                    .name
            }
            RedactionTarget::WidgetName {
                page_index,
                widget_index,
            } => {
                let widget = page_at(&mut bundle.project.root, *page_index)
                    .and_then(|page| page.widgets.get_mut(*widget_index))
                    .ok_or(RedactionError::Invalid)?;
                match widget {
                    Widget::Graph { title, .. } | Widget::ItemValue { title, .. } => title,
                    Widget::Zabbix {
                        name,
                        widget_type,
                        registry_state: RegistryState::Known,
                        fields,
                        ..
                    } if !opaque_widget(registry, widget_type, RegistryState::Known, fields) => {
                        name
                    }
                    Widget::Zabbix { .. } => return Err(RedactionError::Protected),
                }
            }
            RedactionTarget::WidgetHostMask {
                page_index,
                widget_index,
                field_index,
            } => {
                let widget = page_at(&mut bundle.project.root, *page_index)
                    .and_then(|page| page.widgets.get_mut(*widget_index))
                    .ok_or(RedactionError::Invalid)?;
                let Widget::Zabbix {
                    widget_type,
                    registry_state: RegistryState::Known,
                    fields,
                    ..
                } = widget
                else {
                    return Err(RedactionError::Protected);
                };
                if widget_type != "svggraph" {
                    return Err(RedactionError::Protected);
                }
                let field = fields
                    .get_mut(*field_index)
                    .ok_or(RedactionError::Invalid)?;
                if !known_field(registry, widget_type, field)
                    || field.field_type != 1
                    || !is_svggraph_host_mask(&field.name)
                {
                    return Err(RedactionError::Protected);
                }
                match &mut field.value {
                    ZabbixWidgetFieldValue::String { value }
                    | ZabbixWidgetFieldValue::DynamicPattern { value, .. } => value,
                    _ => return Err(RedactionError::Protected),
                }
            }
            RedactionTarget::RoleDescription { role_index } => {
                let role = current_roles
                    .get(*role_index)
                    .ok_or(RedactionError::Invalid)?;
                bundle
                    .project
                    .roles
                    .get_mut(role)
                    .and_then(|r| r.description.as_mut())
                    .ok_or(RedactionError::Invalid)?
            }
            RedactionTarget::CatalogTemplateName { catalog_index } => bundle
                .catalogs
                .values_mut()
                .nth(*catalog_index)
                .and_then(|c| c.source.template_name.as_mut())
                .ok_or(RedactionError::Invalid)?,
            RedactionTarget::CatalogMetricName {
                catalog_index,
                metric_index,
            } => {
                &mut bundle
                    .catalogs
                    .values_mut()
                    .nth(*catalog_index)
                    .and_then(|c| c.metrics.get_mut(*metric_index))
                    .ok_or(RedactionError::Invalid)?
                    .name
            }
            RedactionTarget::CatalogMetricUnits {
                catalog_index,
                metric_index,
            } => {
                &mut bundle
                    .catalogs
                    .values_mut()
                    .nth(*catalog_index)
                    .and_then(|catalog| catalog.metrics.get_mut(*metric_index))
                    .ok_or(RedactionError::Invalid)?
                    .units
            }
            RedactionTarget::WidgetUnresolvedName {
                page_index,
                widget_index,
                field_index,
            } => {
                let widget = page_at(&mut bundle.project.root, *page_index)
                    .and_then(|p| p.widgets.get_mut(*widget_index))
                    .ok_or(RedactionError::Invalid)?;
                let Widget::Zabbix {
                    widget_type,
                    registry_state: RegistryState::Known,
                    fields,
                    ..
                } = widget
                else {
                    return Err(RedactionError::Protected);
                };
                let field = fields
                    .get_mut(*field_index)
                    .ok_or(RedactionError::Invalid)?;
                if !known_field(registry, widget_type, field) {
                    return Err(RedactionError::Protected);
                }
                let ZabbixWidgetFieldValue::Unresolved { display_name, .. } = &mut field.value
                else {
                    return Err(RedactionError::Protected);
                };
                display_name
            }
            RedactionTarget::WidgetText {
                page_index,
                widget_index,
                field_index,
            } => {
                let widget = page_at(&mut bundle.project.root, *page_index)
                    .and_then(|p| p.widgets.get_mut(*widget_index))
                    .ok_or(RedactionError::Invalid)?;
                let Widget::Zabbix {
                    widget_type,
                    registry_state: RegistryState::Known,
                    fields,
                    ..
                } = widget
                else {
                    return Err(RedactionError::Protected);
                };
                let field = fields
                    .get_mut(*field_index)
                    .ok_or(RedactionError::Invalid)?;
                if !known_field(registry, widget_type, field)
                    || field.field_type != 1
                    || !is_safe_text_field(widget_type, &field.name)
                {
                    return Err(RedactionError::Protected);
                }
                match &mut field.value {
                    ZabbixWidgetFieldValue::String { value }
                    | ZabbixWidgetFieldValue::DynamicPattern { value, .. } => value,
                    _ => return Err(RedactionError::Protected),
                }
            }
            RedactionTarget::WidgetItemPattern {
                page_index,
                widget_index,
                field_index,
            } => {
                let widget = page_at(&mut bundle.project.root, *page_index)
                    .and_then(|p| p.widgets.get_mut(*widget_index))
                    .ok_or(RedactionError::Invalid)?;
                let Widget::Zabbix {
                    widget_type,
                    registry_state: RegistryState::Known,
                    fields,
                    ..
                } = widget
                else {
                    return Err(RedactionError::Protected);
                };
                let field = fields
                    .get_mut(*field_index)
                    .ok_or(RedactionError::Invalid)?;
                if !known_field(registry, widget_type, field)
                    || field.field_type != 1
                    || !is_item_pattern(widget_type, &field.name)
                {
                    return Err(RedactionError::Protected);
                }
                match &mut field.value {
                    ZabbixWidgetFieldValue::String { value }
                    | ZabbixWidgetFieldValue::DynamicPattern { value, .. } => value,
                    _ => return Err(RedactionError::Protected),
                }
            }
            RedactionTarget::RemoveWidget { .. } => return Err(RedactionError::Invalid),
            RedactionTarget::RemovePage { .. } => return Err(RedactionError::Invalid),
            RedactionTarget::CatalogMetricId { .. } => return Err(RedactionError::Invalid),
            RedactionTarget::CatalogId { .. } => return Err(RedactionError::Invalid),
            RedactionTarget::CatalogSelectorAliases { .. }
            | RedactionTarget::CatalogClearUuids { .. }
            | RedactionTarget::CatalogClearSource { .. }
            | RedactionTarget::ClearProvenance => return Err(RedactionError::Invalid),
            RedactionTarget::RoleAlias { .. } | RedactionTarget::SlotAlias { .. } => {
                return Err(RedactionError::Invalid);
            }
        };
        if *value == change.replacement {
            return Err(RedactionError::Invalid);
        }
        changes.push(ChangeSummary {
            target: change.target.clone(),
            request_index,
            before_chars: value.chars().count(),
            after_chars: change.replacement.chars().count(),
            before_masked: mask(value),
        });
        value.clone_from(&change.replacement);
    }
    for (old, new) in catalog_renames {
        let mut catalog = bundle
            .catalogs
            .remove(&old)
            .ok_or(RedactionError::Invalid)?;
        catalog.catalog_id.clone_from(&new);
        bundle.catalogs.insert(new.clone(), catalog);
        let reference = bundle
            .project
            .catalogs
            .iter_mut()
            .find(|entry| entry.id == old)
            .ok_or(RedactionError::Invalid)?;
        reference.id.clone_from(&new);
        reference.file = format!("metrics/{new}.yaml");
        let pages: Vec<&mut Page> = match &mut bundle.project.root {
            ProjectRoot::Page(page) => vec![page],
            ProjectRoot::Dashboard(dashboard) => dashboard.pages.iter_mut().collect(),
        };
        for page in pages {
            for widget in &mut page.widgets {
                match widget {
                    Widget::Graph { series, .. } => {
                        for row in series {
                            if row.metric.catalog == old {
                                row.metric.catalog.clone_from(&new);
                            }
                        }
                    }
                    Widget::ItemValue { metric, .. } => {
                        if metric.catalog == old {
                            metric.catalog.clone_from(&new);
                        }
                    }
                    Widget::Zabbix { fields, .. } => {
                        for field in fields {
                            if let ZabbixWidgetFieldValue::Metric { catalog, .. }
                            | ZabbixWidgetFieldValue::LldInstance { catalog, .. } =
                                &mut field.value
                                && *catalog == old
                            {
                                catalog.clone_from(&new);
                            }
                        }
                    }
                }
            }
        }
    }
    if (!removals.is_empty() || !removed_pages.is_empty())
        && !can_remove_widgets(registry, &bundle.project.root, &removals)
    {
        return Err(RedactionError::Protected);
    }
    for &(page_index, widget_index) in removals.iter().rev() {
        if removed_pages.contains(&page_index) {
            continue;
        }
        if requested.iter().any(|change| match change.target {
            RedactionTarget::WidgetName {
                page_index: p,
                widget_index: w,
            }
            | RedactionTarget::WidgetHostMask {
                page_index: p,
                widget_index: w,
                ..
            }
            | RedactionTarget::WidgetUnresolvedName {
                page_index: p,
                widget_index: w,
                ..
            }
            | RedactionTarget::WidgetText {
                page_index: p,
                widget_index: w,
                ..
            }
            | RedactionTarget::WidgetItemPattern {
                page_index: p,
                widget_index: w,
                ..
            } => p == page_index && w == widget_index,
            RedactionTarget::WidgetId {
                page_index: p,
                widget_index: w,
            } => p == page_index && w == widget_index,
            _ => false,
        }) {
            return Err(RedactionError::Invalid);
        }
        let page = page_at(&mut bundle.project.root, page_index).ok_or(RedactionError::Invalid)?;
        if widget_index >= page.widgets.len() {
            return Err(RedactionError::Invalid);
        }
        page.widgets.remove(widget_index);
    }
    for page_index in removed_pages.iter().rev().copied() {
        if requested.iter().any(|change| match change.target {
            RedactionTarget::PageName { page_index: p }
            | RedactionTarget::PageId { page_index: p }
            | RedactionTarget::RemoveWidget { page_index: p, .. }
            | RedactionTarget::WidgetName { page_index: p, .. }
            | RedactionTarget::WidgetId { page_index: p, .. }
            | RedactionTarget::WidgetText { page_index: p, .. }
            | RedactionTarget::WidgetItemPattern { page_index: p, .. }
            | RedactionTarget::WidgetHostMask { page_index: p, .. }
            | RedactionTarget::WidgetUnresolvedName { page_index: p, .. } => p == page_index,
            _ => false,
        }) {
            return Err(RedactionError::Invalid);
        }
        let ProjectRoot::Dashboard(dashboard) = &mut bundle.project.root else {
            return Err(RedactionError::Invalid);
        };
        if page_index >= dashboard.pages.len() {
            return Err(RedactionError::Invalid);
        }
        dashboard.pages.remove(page_index);
    }
    bundle.validate().map_err(|_| RedactionError::Invalid)?;
    let remaining = scan(&bundle).map_err(|_| RedactionError::Invalid)?;
    Ok(RedactionPreview {
        bundle,
        changes,
        remaining,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use std::path::Path;
    use zdashstitch_core::formats::load_typed_project_file;

    #[test]
    fn removal_changes_only_copy_and_rejects_reference_uncertainty() {
        use zdashstitch_core::formats::ZabbixWidgetField;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let mut source =
            load_typed_project_file(&root.join("examples/clock-page-v2.yaml")).expect("fixture");
        let original = source.clone();
        let remove = || RedactionChange {
            target: RedactionTarget::RemoveWidget {
                page_index: 0,
                widget_index: 0,
            },
            replacement: String::new(),
            apply_to_matching: false,
        };
        let result = preview(&source, &[remove()]).expect("remove");
        assert_eq!(source, original);
        let ProjectRoot::Page(page) = result.bundle.project.root else {
            panic!("page")
        };
        assert!(page.widgets.is_empty());
        let ProjectRoot::Page(page) = &mut source.project.root else {
            panic!("page")
        };
        let Widget::Zabbix { fields, .. } = &mut page.widgets[0] else {
            panic!("widget")
        };
        fields.push(ZabbixWidgetField {
            name: "reference".into(),
            field_type: 1,
            registry_state: RegistryState::Known,
            value: ZabbixWidgetFieldValue::String {
                value: "REF01".into(),
            },
        });
        let mut consumer = page.widgets[0].clone();
        let Widget::Zabbix {
            id, fields, layout, ..
        } = &mut consumer
        else {
            panic!("widget")
        };
        *id = "consumer".into();
        *layout = zdashstitch_core::formats::Layout::Fixed {
            x: 18,
            y: 0,
            w: 18,
            h: 4,
        };
        fields.clear();
        fields.push(ZabbixWidgetField {
            name: "time_period._reference".into(),
            field_type: 1,
            registry_state: RegistryState::Known,
            value: ZabbixWidgetFieldValue::String {
                value: "REF01._timeperiod".into(),
            },
        });
        page.widgets.push(consumer);
        assert!(matches!(
            preview(&source, &[remove()]),
            Err(RedactionError::Protected)
        ));
        let remove_consumer = RedactionChange {
            target: RedactionTarget::RemoveWidget {
                page_index: 0,
                widget_index: 1,
            },
            replacement: String::new(),
            apply_to_matching: false,
        };
        assert!(preview(&source, &[remove(), remove_consumer]).is_ok());
    }

    #[test]
    fn role_description_can_be_redacted_without_changing_role() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let mut source =
            load_typed_project_file(&root.join("examples/page-v2.yaml")).expect("fixture");
        source
            .project
            .roles
            .get_mut("system_nodes")
            .expect("role")
            .description = Some("private description".into());
        let result = preview(
            &source,
            &[RedactionChange {
                target: RedactionTarget::RoleDescription { role_index: 0 },
                replacement: "public description".into(),
                apply_to_matching: false,
            }],
        )
        .expect("preview");
        assert_eq!(
            result.bundle.project.roles["system_nodes"]
                .description
                .as_deref(),
            Some("public description")
        );
        assert_eq!(
            source.project.roles["system_nodes"].description.as_deref(),
            Some("private description")
        );
    }

    #[test]
    fn text_allowlist_excludes_keys_and_references() {
        assert!(is_svggraph_host_mask("or.1.hosts.0"));
        assert!(is_svggraph_host_mask("problemhosts.0"));
        assert!(!is_svggraph_host_mask("or.1.items.0"));
        assert!(is_safe_text_field("item", "description"));
        assert!(is_safe_text_field("svggraph", "ds.1.data_set_label"));
        assert!(is_safe_text_field("tophosts", "columns.0.text"));
        assert!(!is_safe_text_field("svggraph", "ds.1.items.0"));
        assert!(!is_safe_text_field("item", "reference"));
        assert!(!is_safe_text_field("tophosts", "columns.0.item"));
        assert!(!is_safe_text_field("svggraph", "ds.x.data_set_label"));
    }

    #[test]
    fn catalog_names_change_without_touching_metric_selectors() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let source = load_typed_project_file(&root.join("examples/page-v2.yaml")).expect("fixture");
        let before = source.catalogs.values().next().expect("catalog").metrics[0]
            .selector
            .clone();
        let result = preview(
            &source,
            &[RedactionChange {
                target: RedactionTarget::CatalogMetricName {
                    catalog_index: 0,
                    metric_index: 0,
                },
                replacement: "Public metric".into(),
                apply_to_matching: false,
            }],
        )
        .expect("preview");
        let metric = &result
            .bundle
            .catalogs
            .values()
            .next()
            .expect("catalog")
            .metrics[0];
        assert_eq!(metric.name, "Public metric");
        assert_eq!(metric.selector, before);
        assert_ne!(
            source.catalogs.values().next().expect("catalog").metrics[0].name,
            metric.name
        );
    }

    #[test]
    fn catalog_aliases_and_uuid_removal_affect_only_the_copy() {
        use zdashstitch_core::formats::{ExportProfile, MetricSelector, ProjectProvenance};
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let mut source =
            load_typed_project_file(&root.join("examples/page-v2.yaml")).expect("fixture");
        let catalog = source.catalogs.values_mut().next().expect("catalog");
        catalog.source.template_uuid = Some("0123456789abcdef0123456789abcdef".into());
        catalog.metrics[0].uuid = Some("abcdef0123456789abcdef0123456789".into());
        let original = source.clone();
        let action = |target| RedactionChange {
            target,
            replacement: String::new(),
            apply_to_matching: false,
        };
        let result = preview(
            &source,
            &[
                action(RedactionTarget::CatalogSelectorAliases { catalog_index: 0 }),
                action(RedactionTarget::CatalogClearUuids { catalog_index: 0 }),
                action(RedactionTarget::ClearProvenance),
            ],
        )
        .expect("redacted copy");
        assert_eq!(source, original);
        let catalog = result.bundle.catalogs.values().next().expect("catalog");
        assert_eq!(catalog.export_profile, ExportProfile::RedactedAliases);
        assert!(catalog.source.template_uuid.is_none());
        assert!(catalog.metrics.iter().enumerate().all(|(index, m)| m.uuid.is_none() && matches!(&m.selector, MetricSelector::Alias(alias) if alias == &format!("metric_{}", index + 1))));
        assert!(matches!(
            result.bundle.project.provenance,
            ProjectProvenance::Local
        ));
        assert_eq!(result.bundle.project.root, source.project.root);
        let bytes =
            zdashstitch_core::formats::build_typed_package_bytes(&result.bundle).expect("package");
        let reopened = zdashstitch_core::formats::read_typed_package_bytes(&bytes).expect("reopen");
        assert_eq!(reopened, result.bundle);
    }

    #[test]
    fn catalog_source_can_be_removed_without_changing_metrics() {
        use zdashstitch_core::formats::SourceKind;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let source = load_typed_project_file(&root.join("examples/page-v2.yaml")).expect("fixture");
        let original = source.clone();
        let result = preview(
            &source,
            &[RedactionChange {
                target: RedactionTarget::CatalogClearSource { catalog_index: 0 },
                replacement: String::new(),
                apply_to_matching: false,
            }],
        )
        .expect("clear source");
        assert_eq!(source, original);
        let old = source.catalogs.values().next().expect("catalog");
        let new = result.bundle.catalogs.values().next().expect("catalog");
        assert_eq!(new.metrics, old.metrics);
        assert_eq!(new.source.kind, SourceKind::Manual);
        assert!(
            new.source.template_name.is_none()
                && new.source.template_uuid.is_none()
                && new.source.export_format_version.is_none()
                && new.source.source_server_version.is_none()
        );
        let bytes =
            zdashstitch_core::formats::build_typed_package_bytes(&result.bundle).expect("package");
        assert_eq!(
            zdashstitch_core::formats::read_typed_package_bytes(&bytes).expect("reopen"),
            result.bundle
        );
    }

    #[test]
    fn catalog_units_can_be_cleared_without_changing_metric_identity() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let source = load_typed_project_file(&root.join("examples/page-v2.yaml")).expect("fixture");
        let index = source
            .catalogs
            .values()
            .next()
            .expect("catalog")
            .metrics
            .iter()
            .position(|metric| !metric.units.is_empty())
            .expect("units");
        let result = preview(
            &source,
            &[RedactionChange {
                target: RedactionTarget::CatalogMetricUnits {
                    catalog_index: 0,
                    metric_index: index,
                },
                replacement: String::new(),
                apply_to_matching: false,
            }],
        )
        .expect("clear units");
        let old = &source.catalogs.values().next().expect("catalog").metrics[index];
        let new = &result
            .bundle
            .catalogs
            .values()
            .next()
            .expect("catalog")
            .metrics[index];
        assert!(new.units.is_empty());
        assert_eq!(new.id, old.id);
        assert_eq!(new.selector, old.selector);
        assert!(!old.units.is_empty());
    }

    #[test]
    fn role_and_slot_aliases_update_references_only_in_copy() {
        use zdashstitch_core::formats::Selection;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let mut source =
            load_typed_project_file(&root.join("examples/page-v2.yaml")).expect("fixture");
        source
            .project
            .roles
            .get_mut("system_nodes")
            .expect("role")
            .slots = Some(vec!["private_slot".into()]);
        let ProjectRoot::Page(page) = &mut source.project.root else {
            panic!("page")
        };
        let Widget::Graph { series, .. } = &mut page.widgets[0] else {
            panic!("graph")
        };
        series[0].hosts.selection = Selection::Slot;
        series[0].hosts.slot_id = Some("private_slot".into());
        let original = source.clone();
        let result = preview(
            &source,
            &[
                RedactionChange {
                    target: RedactionTarget::RoleAlias { role_index: 0 },
                    replacement: "public_role".into(),
                    apply_to_matching: false,
                },
                RedactionChange {
                    target: RedactionTarget::SlotAlias {
                        role_index: 0,
                        slot_index: 0,
                    },
                    replacement: "public_slot".into(),
                    apply_to_matching: false,
                },
            ],
        )
        .expect("aliases");
        assert_eq!(source, original);
        assert!(!result.bundle.project.roles.contains_key("system_nodes"));
        assert_eq!(
            result.bundle.project.roles["public_role"]
                .slots
                .as_ref()
                .expect("slots"),
            &["public_slot"]
        );
        let ProjectRoot::Page(page) = &result.bundle.project.root else {
            panic!("page")
        };
        let Widget::Graph { series, .. } = &page.widgets[0] else {
            panic!("graph")
        };
        assert_eq!(series[0].hosts.role, "public_role");
        assert_eq!(series[0].hosts.slot_id.as_deref(), Some("public_slot"));
        assert_eq!(result.bundle.catalogs, source.catalogs);
    }

    #[test]
    fn dynamic_pattern_roles_and_values_are_redacted_together() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let source = load_typed_project_file(
            &root.join("fixtures/formats/v2/positive/dynamic-pattern.yaml"),
        )
        .expect("fixture");
        let original = source.clone();
        let changes = [
            RedactionChange {
                target: RedactionTarget::RoleAlias { role_index: 0 },
                replacement: "public-role".into(),
                apply_to_matching: false,
            },
            RedactionChange {
                target: RedactionTarget::WidgetHostMask {
                    page_index: 0,
                    widget_index: 0,
                    field_index: 0,
                },
                replacement: "public-*".into(),
                apply_to_matching: false,
            },
            RedactionChange {
                target: RedactionTarget::WidgetItemPattern {
                    page_index: 0,
                    widget_index: 0,
                    field_index: 1,
                },
                replacement: "Memory*".into(),
                apply_to_matching: false,
            },
        ];
        let result = preview(&source, &changes).expect("redaction");
        assert_eq!(source, original);
        let ProjectRoot::Page(page) = &result.bundle.project.root else {
            panic!("page")
        };
        let Widget::Zabbix { fields, .. } = &page.widgets[0] else {
            panic!("widget")
        };
        assert!(
            matches!(&fields[0].value, ZabbixWidgetFieldValue::DynamicPattern {value,role} if value == "public-*" && role == "public-role")
        );
        assert!(
            matches!(&fields[1].value, ZabbixWidgetFieldValue::DynamicPattern {value,role} if value == "Memory*" && role == "public-role")
        );
        assert!(result.bundle.validate().is_ok());
    }

    #[test]
    fn aliases_refuse_unknown_fields_and_collisions() {
        use zdashstitch_core::formats::ZabbixWidgetField;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let mut source =
            load_typed_project_file(&root.join("examples/page-v2.yaml")).expect("fixture");
        let rename = || RedactionChange {
            target: RedactionTarget::RoleAlias { role_index: 0 },
            replacement: "alias".into(),
            apply_to_matching: false,
        };
        let same = RedactionChange {
            target: RedactionTarget::RoleAlias { role_index: 0 },
            replacement: "system_nodes".into(),
            apply_to_matching: false,
        };
        assert!(matches!(
            preview(&source, &[same]),
            Err(RedactionError::Invalid)
        ));
        let ProjectRoot::Page(page) = &mut source.project.root else {
            panic!("page")
        };
        page.widgets.push(Widget::Zabbix {
            id: "opaque".into(),
            widget_type: "clock".into(),
            name: "Clock".into(),
            registry_state: RegistryState::Known,
            fields: vec![ZabbixWidgetField {
                name: "opaque".into(),
                field_type: 1,
                registry_state: RegistryState::Unknown,
                value: ZabbixWidgetFieldValue::String {
                    value: "private".into(),
                },
            }],
            layout: zdashstitch_core::formats::Layout::Auto { w: 18, h: 4 },
        });
        let original = source.clone();
        assert!(matches!(
            preview(&source, &[rename()]),
            Err(RedactionError::Protected)
        ));
        assert_eq!(source, original);
    }

    #[test]
    fn changes_only_copy_and_rescans_without_disclosing_values() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let source =
            load_typed_project_file(&root.join("examples/clock-page-v2.yaml")).expect("fixture");
        let original = source.clone();
        let changes = [RedactionChange {
            target: RedactionTarget::PageName { page_index: 0 },
            replacement: "site-1".into(),
            apply_to_matching: false,
        }];
        let result = preview(&source, &changes).expect("preview");
        assert_eq!(source, original);
        let ProjectRoot::Page(page) = &result.bundle.project.root else {
            panic!("page");
        };
        assert_eq!(page.name, "site-1");
        let ProjectRoot::Page(original_page) = &source.project.root else {
            panic!("page");
        };
        assert_eq!(page.widgets, original_page.widgets);
        assert_eq!(page.id, original_page.id);
        let summary = serde_json::to_string(&result.changes).expect("summary");
        assert!(!summary.contains("site-1"));
        assert!(!summary.contains("Часы"));
        assert_eq!(result.bundle.catalogs, source.catalogs);
        assert_eq!(result.bundle.project.roles, source.project.roles);
    }

    #[test]
    fn invalid_duplicates_and_unknown_widgets_are_refused() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let source =
            load_typed_project_file(&root.join("fixtures/formats/v2/positive/unknown-widget.yaml"))
                .expect("fixture");
        let removed = preview(
            &source,
            &[RedactionChange {
                target: RedactionTarget::RemoveWidget {
                    page_index: 0,
                    widget_index: 0,
                },
                replacement: String::new(),
                apply_to_matching: false,
            }],
        )
        .expect("last unknown widget may be excluded");
        assert!(
            matches!(&removed.bundle.project.root, ProjectRoot::Page(page) if page.widgets.is_empty())
        );
        let request = [RedactionChange {
            target: RedactionTarget::WidgetName {
                page_index: 0,
                widget_index: 0,
            },
            replacement: "alias".into(),
            apply_to_matching: false,
        }];
        assert!(matches!(
            preview(&source, &request),
            Err(RedactionError::Protected)
        ));
        let duplicate = [
            RedactionChange {
                target: RedactionTarget::PageName { page_index: 0 },
                replacement: "one".into(),
                apply_to_matching: false,
            },
            RedactionChange {
                target: RedactionTarget::PageName { page_index: 0 },
                replacement: "two".into(),
                apply_to_matching: false,
            },
        ];
        assert!(matches!(
            preview(&source, &duplicate),
            Err(RedactionError::Invalid)
        ));
        let missing = [RedactionChange {
            target: RedactionTarget::PageName {
                page_index: usize::MAX,
            },
            replacement: "alias".into(),
            apply_to_matching: false,
        }];
        assert!(matches!(
            preview(&source, &missing),
            Err(RedactionError::Invalid)
        ));
        let controls = [RedactionChange {
            target: RedactionTarget::PageName { page_index: 0 },
            replacement: "alias\nname".into(),
            apply_to_matching: false,
        }];
        assert!(matches!(
            preview(&source, &controls),
            Err(RedactionError::Invalid)
        ));
    }

    #[test]
    fn opaque_widget_can_be_removed_only_when_remaining_widgets_are_independent() {
        use zdashstitch_core::formats::ZabbixWidgetField;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let mut source =
            load_typed_project_file(&root.join("fixtures/formats/v2/positive/unknown-widget.yaml"))
                .expect("fixture");
        let clock =
            load_typed_project_file(&root.join("examples/clock-page-v2.yaml")).expect("clock");
        let ProjectRoot::Page(clock_page) = clock.project.root else {
            panic!("clock")
        };
        let ProjectRoot::Page(page) = &mut source.project.root else {
            panic!("page")
        };
        page.widgets.push(clock_page.widgets[0].clone());
        let before = source.clone();
        let remove = || RedactionChange {
            target: RedactionTarget::RemoveWidget {
                page_index: 0,
                widget_index: 0,
            },
            replacement: String::new(),
            apply_to_matching: false,
        };
        let result = preview(&source, &[remove()]).expect("independent widget stays");
        assert_eq!(source, before);
        assert!(
            matches!(&result.bundle.project.root, ProjectRoot::Page(page) if page.widgets.len() == 1)
        );
        let ProjectRoot::Page(page) = &mut source.project.root else {
            panic!("page")
        };
        let Widget::Zabbix { fields, .. } = &mut page.widgets[1] else {
            panic!("widget")
        };
        fields.push(ZabbixWidgetField {
            name: "time_period._reference".into(),
            field_type: 1,
            registry_state: RegistryState::Known,
            value: ZabbixWidgetFieldValue::String {
                value: "ABC01._timeperiod".into(),
            },
        });
        assert!(matches!(
            preview(&source, &[remove()]),
            Err(RedactionError::Protected)
        ));
    }

    #[test]
    fn excluding_page_with_opaque_widget_preserves_other_pages() {
        use zdashstitch_core::formats::{Layout, ZabbixWidgetField};
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let mut source =
            load_typed_project_file(&root.join("examples/dashboard-v2.yaml")).expect("dashboard");
        let opaque =
            load_typed_project_file(&root.join("fixtures/formats/v2/positive/unknown-widget.yaml"))
                .expect("opaque");
        let ProjectRoot::Page(unknown_page) = opaque.project.root else {
            panic!("page")
        };
        let ProjectRoot::Dashboard(dashboard) = &mut source.project.root else {
            panic!("dashboard")
        };
        dashboard.pages.push(unknown_page);
        let before = source.clone();
        let remove = RedactionChange {
            target: RedactionTarget::RemovePage { page_index: 1 },
            replacement: String::new(),
            apply_to_matching: false,
        };
        let result = preview(&source, &[remove]).expect("safe exclusion");
        assert_eq!(source, before);
        let (ProjectRoot::Dashboard(after), ProjectRoot::Dashboard(before_dashboard)) =
            (&result.bundle.project.root, &before.project.root)
        else {
            panic!("dashboard")
        };
        assert_eq!(after.pages, before_dashboard.pages[..1]);
        let remove_both = [0, 1].map(|page_index| RedactionChange {
            target: RedactionTarget::RemovePage { page_index },
            replacement: String::new(),
            apply_to_matching: false,
        });
        assert!(matches!(
            preview(&source, &remove_both),
            Err(RedactionError::Invalid)
        ));
        let bytes =
            zdashstitch_core::formats::build_typed_package_bytes(&result.bundle).expect("package");
        assert_eq!(
            zdashstitch_core::formats::read_typed_package_bytes(&bytes).expect("reopen"),
            result.bundle
        );
        let ProjectRoot::Dashboard(dashboard) = &mut source.project.root else {
            panic!("dashboard")
        };
        dashboard.pages[0].widgets.push(Widget::Zabbix {
            id: "consumer".into(),
            widget_type: "item".into(),
            name: "consumer".into(),
            registry_state: RegistryState::Known,
            fields: vec![ZabbixWidgetField {
                name: "time_period._reference".into(),
                field_type: 1,
                registry_state: RegistryState::Known,
                value: ZabbixWidgetFieldValue::String {
                    value: "ABC01._timeperiod".into(),
                },
            }],
            layout: Layout::Auto { w: 12, h: 4 },
        });
        let remove = RedactionChange {
            target: RedactionTarget::RemovePage { page_index: 1 },
            replacement: String::new(),
            apply_to_matching: false,
        };
        assert!(matches!(
            preview(&source, &[remove]),
            Err(RedactionError::Protected)
        ));
    }

    #[test]
    fn local_ids_are_replaced_only_when_no_opaque_data_exists() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let source = load_typed_project_file(&root.join("examples/page-v2.yaml")).expect("fixture");
        let before = source.clone();
        let changes = [
            RedactionChange {
                target: RedactionTarget::ProjectId,
                replacement: "public-project".into(),
                apply_to_matching: false,
            },
            RedactionChange {
                target: RedactionTarget::PageId { page_index: 0 },
                replacement: "public-page".into(),
                apply_to_matching: false,
            },
            RedactionChange {
                target: RedactionTarget::WidgetId {
                    page_index: 0,
                    widget_index: 0,
                },
                replacement: "public-widget".into(),
                apply_to_matching: false,
            },
        ];
        let result = preview(&source, &changes).expect("safe aliases");
        assert_eq!(source, before);
        assert_eq!(result.bundle.project.project_id, "public-project");
        let ProjectRoot::Page(page) = &result.bundle.project.root else {
            panic!("page")
        };
        assert_eq!(page.id, "public-page");
        assert_eq!(page.widgets[0].id(), "public-widget");
        let bytes =
            zdashstitch_core::formats::build_typed_package_bytes(&result.bundle).expect("package");
        assert_eq!(
            zdashstitch_core::formats::read_typed_package_bytes(&bytes).expect("reopen"),
            result.bundle
        );
        let mut opaque =
            load_typed_project_file(&root.join("fixtures/formats/v2/positive/unknown-widget.yaml"))
                .expect("opaque");
        assert!(matches!(
            preview(&opaque, &changes[..1]),
            Err(RedactionError::Protected)
        ));
        let ProjectRoot::Page(page) = &mut opaque.project.root else {
            panic!("page")
        };
        let Widget::Zabbix {
            registry_state,
            fields,
            ..
        } = &mut page.widgets[0]
        else {
            panic!("widget")
        };
        *registry_state = RegistryState::Known;
        fields.truncate(1);
        for field in fields {
            field.registry_state = RegistryState::Known;
        }
        assert!(scan(&opaque).expect("scan").unknown_objects > 0);
        assert!(matches!(
            preview(&opaque, &changes[..1]),
            Err(RedactionError::Protected)
        ));
    }

    #[test]
    fn metric_id_alias_updates_typed_references_and_round_trips() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let source = load_typed_project_file(&root.join("examples/page-v2.yaml")).expect("fixture");
        let ProjectRoot::Page(original_page) = &source.project.root else {
            panic!("page")
        };
        let Widget::Graph { series, .. } = &original_page.widgets[0] else {
            panic!("graph")
        };
        let catalog_id = &series[0].metric.catalog;
        let metric_id = &series[0].metric.metric;
        let catalog_index = source
            .catalogs
            .keys()
            .position(|key| key == catalog_id)
            .expect("catalog");
        let metric_index = source.catalogs[catalog_id]
            .metrics
            .iter()
            .position(|metric| &metric.id == metric_id)
            .expect("metric");
        let change = RedactionChange {
            target: RedactionTarget::CatalogMetricId {
                catalog_index,
                metric_index,
            },
            replacement: "public-metric".into(),
            apply_to_matching: false,
        };
        let before = source.clone();
        let result = preview(&source, &[change]).expect("alias");
        assert_eq!(source, before);
        let ProjectRoot::Page(page) = &result.bundle.project.root else {
            panic!("page")
        };
        let Widget::Graph { series, .. } = &page.widgets[0] else {
            panic!("graph")
        };
        assert_eq!(series[0].metric.metric, "public-metric");
        assert_eq!(
            result.bundle.catalogs[catalog_id].metrics[metric_index].id,
            "public-metric"
        );
        let bytes =
            zdashstitch_core::formats::build_typed_package_bytes(&result.bundle).expect("package");
        assert_eq!(
            zdashstitch_core::formats::read_typed_package_bytes(&bytes).expect("reopen"),
            result.bundle
        );
    }

    #[test]
    fn catalog_and_metric_ids_change_without_breaking_references() {
        use zdashstitch_core::formats::{ItemExpansion, Layout, Selection, ZabbixWidgetField};
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let mut source =
            load_typed_project_file(&root.join("examples/page-v2.yaml")).expect("fixture");
        let ProjectRoot::Page(page) = &source.project.root else {
            panic!("page")
        };
        let Widget::Graph { series, .. } = &page.widgets[0] else {
            panic!("graph")
        };
        let old_catalog = &series[0].metric.catalog;
        let catalog_index = source
            .catalogs
            .keys()
            .position(|key| key == old_catalog)
            .expect("catalog");
        let metric_index = source.catalogs[old_catalog]
            .metrics
            .iter()
            .position(|metric| metric.id == series[0].metric.metric)
            .expect("metric");
        let metric_ref = series[0].metric.clone();
        let hosts = series[0].hosts.clone();
        let mut single_host = hosts.clone();
        single_host.selection = Selection::Slot;
        single_host.slot_id = Some("test-slot".into());
        source
            .project
            .roles
            .get_mut(&single_host.role)
            .expect("role")
            .slots = Some(vec!["test-slot".into()]);
        let ProjectRoot::Page(page) = &mut source.project.root else {
            panic!("page")
        };
        page.widgets.push(Widget::ItemValue {
            id: "item".into(),
            title: "Item".into(),
            expansion: ItemExpansion::WidgetPerHost,
            metric: metric_ref.clone(),
            hosts,
            layout: Layout::Auto { w: 12, h: 4 },
        });
        page.widgets.push(Widget::Zabbix {
            id: "zabbix-item".into(),
            widget_type: "item".into(),
            name: "Item".into(),
            registry_state: RegistryState::Known,
            fields: vec![ZabbixWidgetField {
                name: "itemid.0".into(),
                field_type: 4,
                registry_state: RegistryState::Known,
                value: ZabbixWidgetFieldValue::Metric {
                    catalog: metric_ref.catalog,
                    metric: metric_ref.metric,
                    hosts: single_host,
                },
            }],
            layout: Layout::Auto { w: 12, h: 4 },
        });
        let before = source.clone();
        source.validate().expect("source is valid");
        let request = [
            RedactionChange {
                target: RedactionTarget::CatalogId { catalog_index },
                replacement: "public-catalog".into(),
                apply_to_matching: false,
            },
            RedactionChange {
                target: RedactionTarget::CatalogMetricId {
                    catalog_index,
                    metric_index,
                },
                replacement: "public-metric".into(),
                apply_to_matching: false,
            },
        ];
        let result = preview(&source, &request).expect("aliases");
        assert_eq!(source, before);
        let ProjectRoot::Page(page) = &result.bundle.project.root else {
            panic!("page")
        };
        let Widget::Graph { series, .. } = &page.widgets[0] else {
            panic!("graph")
        };
        assert_eq!(series[0].metric.catalog, "public-catalog");
        assert_eq!(series[0].metric.metric, "public-metric");
        let Widget::ItemValue { metric, .. } = &page.widgets[1] else {
            panic!("item")
        };
        assert_eq!(metric.catalog, "public-catalog");
        assert_eq!(metric.metric, "public-metric");
        let Widget::Zabbix { fields, .. } = &page.widgets[2] else {
            panic!("zabbix")
        };
        assert!(
            matches!(&fields[0].value, ZabbixWidgetFieldValue::Metric { catalog, metric, .. } if catalog == "public-catalog" && metric == "public-metric")
        );
        assert_eq!(
            result.bundle.project.catalogs[0].file,
            "metrics/public-catalog.yaml"
        );
        let bytes =
            zdashstitch_core::formats::build_typed_package_bytes(&result.bundle).expect("package");
        assert_eq!(
            zdashstitch_core::formats::read_typed_package_bytes(&bytes).expect("reopen"),
            result.bundle
        );
    }

    #[test]
    fn dashboard_and_multiple_names_change_only_in_copy() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let source =
            zdashstitch_core::formats::read_package(&root.join("examples/db-pilot-package.zip"))
                .expect("legacy")
                .migrate(&zdashstitch_core::formats::LegacyRootSelection::Dashboard {
                    dashboard_id: "db-pilot".into(),
                })
                .expect("migrate");
        let original = source.clone();
        let request = [
            RedactionChange {
                target: RedactionTarget::DashboardName,
                replacement: "site-a".into(),
                apply_to_matching: false,
            },
            RedactionChange {
                target: RedactionTarget::PageName { page_index: 0 },
                replacement: "site-a".into(),
                apply_to_matching: false,
            },
            RedactionChange {
                target: RedactionTarget::WidgetName {
                    page_index: 0,
                    widget_index: 0,
                },
                replacement: "site-a".into(),
                apply_to_matching: false,
            },
        ];
        let result = preview(&source, &request).expect("preview");
        assert_eq!(source, original);
        assert_eq!(result.bundle.catalogs, source.catalogs);
        assert_eq!(result.bundle.project.roles, source.project.roles);
        let (ProjectRoot::Dashboard(output), ProjectRoot::Dashboard(before)) =
            (&result.bundle.project.root, &source.project.root)
        else {
            panic!("dashboard")
        };
        assert_eq!(output.name, "site-a");
        assert_eq!(output.pages[0].name, "site-a");
        assert_eq!(output.pages[1..], before.pages[1..]);
        let (
            Widget::Graph { title, series, .. },
            Widget::Graph {
                series: old_series, ..
            },
        ) = (&output.pages[0].widgets[0], &before.pages[0].widgets[0])
        else {
            panic!("graph")
        };
        assert_eq!(title, "site-a");
        assert_eq!(series, old_series);
    }

    #[test]
    fn only_known_svggraph_host_mask_fields_can_be_replaced() {
        use zdashstitch_core::formats::ZabbixWidgetField;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let mut source =
            load_typed_project_file(&root.join("examples/clock-page-v2.yaml")).expect("fixture");
        let ProjectRoot::Page(page) = &mut source.project.root else {
            panic!("page")
        };
        let make_field = |name: &str, state| ZabbixWidgetField {
            name: name.into(),
            field_type: 1,
            registry_state: state,
            value: ZabbixWidgetFieldValue::String {
                value: "internal-*".into(),
            },
        };
        page.widgets[0] = Widget::Zabbix {
            id: "graph".into(),
            widget_type: "svggraph".into(),
            name: "Graph".into(),
            registry_state: RegistryState::Known,
            fields: vec![
                make_field("ds.0.hosts.0", RegistryState::Known),
                make_field("ds.0.items.0", RegistryState::Known),
                make_field("reference", RegistryState::Known),
                make_field("ds.0.hosts.1", RegistryState::Unknown),
            ],
            layout: zdashstitch_core::formats::Layout::Auto { w: 18, h: 4 },
        };
        page.name = "internal-*".into();
        let original = source.clone();
        let change = |field_index| RedactionChange {
            target: RedactionTarget::WidgetHostMask {
                page_index: 0,
                widget_index: 0,
                field_index,
            },
            replacement: "host-*".into(),
            apply_to_matching: false,
        };
        let result = preview(&source, &[change(0)]).expect("known host mask");
        assert_eq!(source, original);
        let ProjectRoot::Page(page) = result.bundle.project.root else {
            panic!("page")
        };
        let Widget::Zabbix { fields, .. } = &page.widgets[0] else {
            panic!("widget")
        };
        assert!(
            matches!(&fields[0].value, ZabbixWidgetFieldValue::String { value } if value == "host-*")
        );
        assert!(
            matches!(&fields[1].value, ZabbixWidgetFieldValue::String { value } if value == "internal-*")
        );
        for index in [1, 2, 3] {
            assert!(matches!(
                preview(&source, &[change(index)]),
                Err(RedactionError::Protected)
            ));
        }
        let item_change = |field_index| RedactionChange {
            target: RedactionTarget::WidgetItemPattern {
                page_index: 0,
                widget_index: 0,
                field_index,
            },
            replacement: "public.*".into(),
            apply_to_matching: false,
        };
        let item_copy = preview(&source, &[item_change(1)]).expect("explicit item pattern");
        let ProjectRoot::Page(page) = &item_copy.bundle.project.root else {
            panic!("page")
        };
        let Widget::Zabbix { fields, .. } = &page.widgets[0] else {
            panic!("widget")
        };
        assert!(
            matches!(&fields[1].value, ZabbixWidgetFieldValue::String { value } if value == "public.*")
        );
        assert!(
            matches!(&fields[2].value, ZabbixWidgetFieldValue::String { value } if value == "internal-*")
        );
        assert_eq!(source, original);
        assert!(matches!(
            preview(&source, &[item_change(2)]),
            Err(RedactionError::Protected)
        ));
        let mut grouped_item = item_change(1);
        grouped_item.apply_to_matching = true;
        assert!(matches!(
            preview(&source, &[grouped_item]),
            Err(RedactionError::Protected)
        ));
        let mut matching = change(0);
        matching.apply_to_matching = true;
        let result = preview(&source, &[matching]).expect("exact matching");
        assert_eq!(result.changes.len(), 2);
        assert!(result.changes.iter().all(|entry| entry.request_index == 0));
        let ProjectRoot::Page(page) = result.bundle.project.root else {
            panic!("page")
        };
        assert_eq!(page.name, "host-*");
        let Widget::Zabbix { fields, .. } = &page.widgets[0] else {
            panic!("widget")
        };
        assert!(
            matches!(&fields[0].value, ZabbixWidgetFieldValue::String { value } if value == "host-*")
        );
        assert!(
            matches!(&fields[1].value, ZabbixWidgetFieldValue::String { value } if value == "internal-*")
        );
        assert!(
            matches!(&fields[3].value, ZabbixWidgetFieldValue::String { value } if value == "internal-*")
        );
        let mut overlap = change(0);
        overlap.apply_to_matching = true;
        let explicit = RedactionChange {
            target: RedactionTarget::PageName { page_index: 0 },
            replacement: "other".into(),
            apply_to_matching: false,
        };
        assert!(matches!(
            preview(&source, &[overlap, explicit]),
            Err(RedactionError::Invalid)
        ));
    }

    #[test]
    fn matching_does_not_silently_truncate_at_limit() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("workspace");
        let mut source =
            load_typed_project_file(&root.join("examples/clock-page-v2.yaml")).expect("fixture");
        let ProjectRoot::Page(page) = &mut source.project.root else {
            panic!("page")
        };
        page.name = "same-name".into();
        let prototype = page.widgets[0].clone();
        page.widgets = (0..128)
            .map(|index| {
                let mut widget = prototype.clone();
                let Widget::Zabbix { id, name, .. } = &mut widget else {
                    panic!("widget")
                };
                *id = format!("widget-{index}");
                *name = "same-name".into();
                widget
            })
            .collect();
        let original = source.clone();
        let request = [RedactionChange {
            target: RedactionTarget::PageName { page_index: 0 },
            replacement: "alias".into(),
            apply_to_matching: true,
        }];
        assert!(matches!(
            preview(&source, &request),
            Err(RedactionError::Invalid)
        ));
        assert_eq!(source, original);
    }
}
