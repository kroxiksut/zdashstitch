// SPDX-License-Identifier: MIT OR Apache-2.0

//! P4-08: content-free inventory and source-profile conversion of Workcopies.
//! Server-bound fields require explicit evidence for the selected snapshot.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;
use zdashstitch_core::formats::{
    Dashboard, DashboardKind, Layout, Page, ProjectKind, ProjectProvenance, ProjectRoot,
    RegistryState, TypedPortableBundle, TypedProject, TypedProjectFormat, Widget,
    ZabbixWidgetField, ZabbixWidgetFieldValue,
};
use zdashstitch_zabbix::{
    AdapterProfile, AdapterRegistry, DependencyKind, DependencyState, VersionedDashboardSnapshot,
    WidgetFieldType,
};

use crate::{
    portable_bindings::SnapshotBindings,
    replace_host::materialize_local_operations,
    workcopy::{DraftKind, WorkcopyError, WorkcopyService},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PortableConversionInventory {
    pub kind: DraftKind,
    pub pages: usize,
    pub widgets: usize,
    /// The snapshot analyzer cannot classify these widgets for conversion.
    pub unclassified_widgets: usize,
    pub item_fields: usize,
    pub host_fields: usize,
    pub graph_fields: usize,
    pub host_pattern_fields: usize,
    pub item_pattern_fields: usize,
    pub unknown_fields: usize,
    pub reference_definitions: usize,
    pub reference_consumers: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PortableConversionPreview {
    pub inventory: PortableConversionInventory,
    pub ready: bool,
    /// Stable safe code; never a raw name, ID or server value.
    pub blocker: Option<&'static str>,
    pub blockers: Vec<&'static str>,
}

impl WorkcopyService {
    /// Preview one page of a dashboard copy using its current Local state.
    pub fn preview_dashboard_page_conversion(
        &self,
        local_id: &str,
        page_id: &str,
    ) -> Result<PortableConversionPreview, WorkcopyError> {
        let (file, base, operations) = self.load_log(local_id)?;
        if file.kind != DraftKind::Dashboard {
            return Err(WorkcopyError::new("portable_conversion_scope_unsupported"));
        }
        let local = materialize_local_operations(base, &operations)?;
        ensure_selected_dashboard_page(&local, page_id)?;
        preview_page(&local, page_id)
    }

    /// Convert a selected page without reading Zabbix or changing the copy.
    pub fn convert_dependency_free_dashboard_page(
        &self,
        local_id: &str,
        page_id: &str,
    ) -> Result<TypedPortableBundle, WorkcopyError> {
        let (file, base, operations) = self.load_log(local_id)?;
        if file.kind != DraftKind::Dashboard {
            return Err(WorkcopyError::new("portable_conversion_scope_unsupported"));
        }
        let local = materialize_local_operations(base, &operations)?;
        ensure_selected_dashboard_page(&local, page_id)?;
        let mut random = [0u8; 16];
        getrandom::fill(&mut random)
            .map_err(|_| WorkcopyError::new("random_source_unavailable"))?;
        let project_id = format!(
            "pr-{}",
            random
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        convert_page(&local, page_id, project_id)
    }

    /// Checks the current Local state without creating a project or writing.
    pub fn preview_portable_conversion(
        &self,
        local_id: &str,
    ) -> Result<PortableConversionPreview, WorkcopyError> {
        let (file, base, operations) = self.load_log(local_id)?;
        let local = materialize_local_operations(base, &operations)?;
        let page_id = match file.kind {
            DraftKind::Dashboard => None,
            DraftKind::Page => Some(
                file.source
                    .page
                    .as_ref()
                    .ok_or_else(|| WorkcopyError::new("workcopy_corrupt"))?
                    .dashboard_pageid
                    .as_str(),
            ),
        };
        let inventory = inspect(&local, file.kind, page_id)?;
        let result = match page_id {
            Some(page_id) => convert_page(&local, page_id, "pr-preview".to_owned()),
            None => convert_dashboard(&local, "pr-preview".to_owned()),
        };
        let result = result.and_then(|mut bundle| apply_workcopy_name(&mut bundle, &file.name));
        let mut blockers = Vec::new();
        if inventory.item_fields + inventory.item_pattern_fields > 0 {
            blockers.push("portable_conversion_item_binding_required");
        }
        if inventory.host_fields + inventory.host_pattern_fields > 0 {
            blockers.push("portable_conversion_host_binding_required");
        }
        if inventory.graph_fields > 0 {
            blockers.push("portable_conversion_graph_binding_required");
        }
        // The dependency analyzer classifies some registry-known widgets
        // (for example `clock`) as unclassified because they have no binding
        // codec. The converter registry remains authoritative for whether a
        // widget is portable; only unknown fields are an independent blocker.
        if inventory.unknown_fields > 0 {
            blockers.push("portable_conversion_unknown_data");
        }
        if inventory.reference_definitions + inventory.reference_consumers > 0 && result.is_err() {
            blockers.push("portable_conversion_reference_required");
        }
        let blocker = result.err().map(|error| error.code());
        if let Some(code) = blocker
            && !blockers.contains(&code)
        {
            blockers.push(code);
        }
        Ok(PortableConversionPreview {
            inventory,
            ready: blocker.is_none(),
            blocker,
            blockers,
        })
    }
    /// Counts the selected Workcopy scope in Local = Base + operations, with
    /// no server read and no raw values, IDs or names in the result.
    pub fn portable_conversion_inventory(
        &self,
        local_id: &str,
    ) -> Result<PortableConversionInventory, WorkcopyError> {
        let (file, base, operations) = self.load_log(local_id)?;
        let local = materialize_local_operations(base, &operations)?;
        let page_id = match file.kind {
            DraftKind::Dashboard => None,
            DraftKind::Page => Some(
                file.source
                    .page
                    .as_ref()
                    .ok_or_else(|| WorkcopyError::new("workcopy_corrupt"))?
                    .dashboard_pageid
                    .as_str(),
            ),
        };
        inspect(&local, file.kind, page_id)
    }

    /// Builds an in-memory page project only when every selected value can be
    /// represented without a server binding. The source Workcopy is untouched.
    pub fn convert_dependency_free_page(
        &self,
        local_id: &str,
    ) -> Result<TypedPortableBundle, WorkcopyError> {
        let (file, base, operations) = self.load_log(local_id)?;
        if file.kind != DraftKind::Page {
            return Err(WorkcopyError::new("portable_conversion_scope_unsupported"));
        }
        let local = materialize_local_operations(base, &operations)?;
        let page_id = &file
            .source
            .page
            .as_ref()
            .ok_or_else(|| WorkcopyError::new("workcopy_corrupt"))?
            .dashboard_pageid;
        let mut random = [0u8; 16];
        getrandom::fill(&mut random)
            .map_err(|_| WorkcopyError::new("random_source_unavailable"))?;
        let project_id = format!(
            "pr-{}",
            random
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        let mut bundle = convert_page(&local, page_id, project_id)?;
        apply_workcopy_name(&mut bundle, &file.name)?;
        Ok(bundle)
    }

    /// Converts a dependency-free dashboard without changing its source.
    pub fn convert_dependency_free_dashboard(
        &self,
        local_id: &str,
    ) -> Result<TypedPortableBundle, WorkcopyError> {
        let (file, base, operations) = self.load_log(local_id)?;
        if file.kind != DraftKind::Dashboard {
            return Err(WorkcopyError::new("portable_conversion_scope_unsupported"));
        }
        let local = materialize_local_operations(base, &operations)?;
        let mut random = [0u8; 16];
        getrandom::fill(&mut random)
            .map_err(|_| WorkcopyError::new("random_source_unavailable"))?;
        let project_id = format!(
            "pr-{}",
            random
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        let mut bundle = convert_dashboard(&local, project_id)?;
        apply_workcopy_name(&mut bundle, &file.name)?;
        Ok(bundle)
    }
}

fn ensure_selected_dashboard_page(
    local: &VersionedDashboardSnapshot,
    page_id: &str,
) -> Result<(), WorkcopyError> {
    let pages = local.raw_api_document()["pages"]
        .as_array()
        .ok_or_else(|| WorkcopyError::new("snapshot_corrupt"))?;
    let count = pages
        .iter()
        .filter(|page| page["dashboard_pageid"].as_str() == Some(page_id))
        .count();
    match count {
        1 => Ok(()),
        0 => Err(WorkcopyError::new("import_page_not_found")),
        _ => Err(WorkcopyError::new("workcopy_corrupt")),
    }
}

fn preview_page(
    local: &VersionedDashboardSnapshot,
    page_id: &str,
) -> Result<PortableConversionPreview, WorkcopyError> {
    let inventory = inspect(local, DraftKind::Page, Some(page_id))?;
    let result = convert_page(local, page_id, "pr-preview".to_owned());
    let mut blockers = Vec::new();
    if inventory.item_fields + inventory.item_pattern_fields > 0 {
        blockers.push("portable_conversion_item_binding_required");
    }
    if inventory.host_fields + inventory.host_pattern_fields > 0 {
        blockers.push("portable_conversion_host_binding_required");
    }
    if inventory.graph_fields > 0 {
        blockers.push("portable_conversion_graph_binding_required");
    }
    if inventory.unknown_fields > 0 {
        blockers.push("portable_conversion_unknown_data");
    }
    if inventory.reference_definitions + inventory.reference_consumers > 0 && result.is_err() {
        blockers.push("portable_conversion_reference_required");
    }
    let blocker = result.err().map(|error| error.code());
    if let Some(code) = blocker
        && !blockers.contains(&code)
    {
        blockers.push(code);
    }
    Ok(PortableConversionPreview {
        inventory,
        ready: blocker.is_none(),
        blocker,
        blockers,
    })
}

pub(crate) fn apply_workcopy_name(
    bundle: &mut TypedPortableBundle,
    name: &str,
) -> Result<(), WorkcopyError> {
    match &mut bundle.project.root {
        ProjectRoot::Dashboard(dashboard) => dashboard.name = name.to_owned(),
        ProjectRoot::Page(page) => page.name = name.to_owned(),
    }
    bundle
        .validate()
        .map_err(|_| WorkcopyError::new("portable_conversion_unsupported_data"))
}

fn convert_dashboard(
    snapshot: &VersionedDashboardSnapshot,
    project_id: String,
) -> Result<TypedPortableBundle, WorkcopyError> {
    convert_dashboard_bound(snapshot, project_id, None)
}

pub(crate) fn convert_dashboard_bound(
    snapshot: &VersionedDashboardSnapshot,
    project_id: String,
    evidence: Option<SnapshotBindings<'_>>,
) -> Result<TypedPortableBundle, WorkcopyError> {
    let profile = conversion_profile(snapshot)?;
    let bindings = evidence
        .map(|evidence| evidence.get(snapshot))
        .transpose()?;
    let raw = snapshot.raw_api_document();
    ensure_only_keys(
        raw,
        &[
            "dashboardid",
            "name",
            "pages",
            "userid",
            "private",
            "display_period",
            "auto_start",
            "users",
            "userGroups",
        ],
    )?;
    // O-031 permits omitting the validated target-specific access metadata
    // from the portable project. The local source snapshot still keeps it.
    let omit_access = snapshot.dashboard_access_state().is_some();
    let server_defaults = [("display_period", "30"), ("auto_start", "1")];
    for (key, expected) in server_defaults
        .into_iter()
        .chain((!omit_access).then_some(("private", "1")))
    {
        if raw.get(key).is_some_and(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .or_else(|| value.as_u64().map(|n| n.to_string()))
                .as_deref()
                != Some(expected)
        }) {
            return Err(WorkcopyError::new("portable_conversion_unsupported_data"));
        }
    }
    for key in ["users", "userGroups"].into_iter().filter(|_| !omit_access) {
        if raw
            .get(key)
            .is_some_and(|value| !value.as_array().is_some_and(Vec::is_empty))
        {
            return Err(WorkcopyError::new("portable_conversion_unsupported_data"));
        }
    }
    let raw_pages = raw["pages"]
        .as_array()
        .ok_or_else(|| WorkcopyError::new("snapshot_corrupt"))?;
    let mut ordered = raw_pages
        .iter()
        .map(|page| {
            page.get("sortorder")
                .map(number_u16)
                .transpose()
                .map(|sortorder| (sortorder, page))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if raw_pages.len() > 1 && ordered.iter().any(|(sortorder, _)| sortorder.is_none()) {
        return Err(WorkcopyError::new("portable_conversion_unsupported_data"));
    }
    ordered.sort_by_key(|(sortorder, _)| *sortorder);
    if ordered.windows(2).any(|pair| pair[0].0 == pair[1].0) && ordered.len() > 1 {
        return Err(WorkcopyError::new("portable_conversion_unsupported_data"));
    }
    let mut pages = Vec::with_capacity(ordered.len());
    let mut used_references = BTreeSet::new();
    for (index, (_, raw_page)) in ordered.into_iter().enumerate() {
        let page_id = raw_page["dashboard_pageid"]
            .as_str()
            .ok_or_else(|| WorkcopyError::new("snapshot_corrupt"))?;
        let page_bundle = convert_page_bound(snapshot, page_id, project_id.clone(), evidence)?;
        let ProjectRoot::Page(mut page) = page_bundle.project.root else {
            return Err(WorkcopyError::new("workcopy_corrupt"));
        };
        page.id = format!("page-{}", index + 1);
        crate::offline_dashboards::rewrite_page_references(&mut page, &mut used_references)
            .map_err(|_| WorkcopyError::new("portable_conversion_reference_required"))?;
        pages.push(page);
    }
    let bundle = TypedPortableBundle {
        project: TypedProject {
            format: TypedProjectFormat::V2,
            project_id: project_id.clone(),
            kind: ProjectKind::Dashboard,
            declared_compatibility: profile.widget_version_range(),
            provenance: ProjectProvenance::ServerSnapshot,
            catalogs: bindings.map_or_else(Vec::new, |b| b.references.clone()),
            roles: bindings.map_or_else(BTreeMap::new, |b| b.roles.clone()),
            root: ProjectRoot::Dashboard(Dashboard {
                id: project_id,
                name: raw["name"]
                    .as_str()
                    .ok_or_else(|| WorkcopyError::new("snapshot_corrupt"))?
                    .to_owned(),
                kind: DashboardKind::Global,
                pages,
            }),
        },
        catalogs: bindings.map_or_else(BTreeMap::new, |b| b.catalogs.clone()),
    };
    bundle
        .validate()
        .map_err(|_| WorkcopyError::new("portable_conversion_unsupported_data"))?;
    Ok(bundle)
}

fn convert_page(
    snapshot: &VersionedDashboardSnapshot,
    page_id: &str,
    project_id: String,
) -> Result<TypedPortableBundle, WorkcopyError> {
    convert_page_bound(snapshot, page_id, project_id, None)
}

pub(crate) fn convert_page_bound(
    snapshot: &VersionedDashboardSnapshot,
    page_id: &str,
    project_id: String,
    evidence: Option<SnapshotBindings<'_>>,
) -> Result<TypedPortableBundle, WorkcopyError> {
    let profile = conversion_profile(snapshot)?;
    let bindings = evidence
        .map(|evidence| evidence.get(snapshot))
        .transpose()?;
    let inventory = inspect(snapshot, DraftKind::Page, Some(page_id))?;
    if profile == AdapterProfile::V8_0 && inventory.unclassified_widgets > 0 {
        return Err(WorkcopyError::new("portable_conversion_widget_unsupported"));
    }
    if profile == AdapterProfile::V8_0 && inventory.unknown_fields > 0 {
        return Err(WorkcopyError::new("portable_conversion_field_unsupported"));
    }
    if (bindings.is_none() && inventory.item_fields > 0)
        || (bindings.is_none() && inventory.host_fields > 0)
        || (bindings.is_none() && inventory.graph_fields > 0)
        || (bindings.is_none() && inventory.host_pattern_fields + inventory.item_pattern_fields > 0)
        || inventory.unknown_fields > 0
    {
        return Err(WorkcopyError::new("portable_conversion_binding_required"));
    }
    let pages = snapshot.raw_api_document()["pages"]
        .as_array()
        .ok_or_else(|| WorkcopyError::new("snapshot_corrupt"))?;
    let raw = pages
        .iter()
        .find(|page| page["dashboard_pageid"].as_str() == Some(page_id))
        .ok_or_else(|| WorkcopyError::new("workcopy_corrupt"))?;
    let page_index = pages
        .iter()
        .position(|page| page["dashboard_pageid"].as_str() == Some(page_id))
        .ok_or_else(|| WorkcopyError::new("workcopy_corrupt"))?;
    ensure_only_keys(
        raw,
        &[
            "dashboard_pageid",
            "name",
            "display_period",
            "sortorder",
            "widgets",
        ],
    )?;
    if raw
        .get("display_period")
        .is_some_and(|value| !is_zero(value))
    {
        return Err(WorkcopyError::new("portable_conversion_unsupported_data"));
    }
    let raw_widgets = raw["widgets"]
        .as_array()
        .ok_or_else(|| WorkcopyError::new("snapshot_corrupt"))?;
    let registry = profile.widget_capabilities();
    let mut widgets = Vec::with_capacity(raw_widgets.len());
    for (index, raw_widget) in raw_widgets.iter().enumerate() {
        ensure_only_keys(
            raw_widget,
            &[
                "widgetid",
                "type",
                "name",
                "x",
                "y",
                "width",
                "height",
                "view_mode",
                "fields",
            ],
        )?;
        if raw_widget
            .get("view_mode")
            .is_some_and(|value| !is_zero(value))
        {
            return Err(WorkcopyError::new("portable_conversion_unsupported_data"));
        }
        let widget_type = raw_widget["type"]
            .as_str()
            .ok_or_else(|| WorkcopyError::new("snapshot_corrupt"))?;
        let capability = registry
            .find(widget_type)
            .filter(|capability| match profile {
                AdapterProfile::V7_4 => {
                    capability.flags.create || (widget_type == "graph" && capability.flags.read)
                }
                // The 8.0 read registry describes the raw fields accepted by
                // local conversion. Its server capability flags stay intact.
                AdapterProfile::V8_0 => capability.flags.read,
            })
            .ok_or_else(|| WorkcopyError::new("portable_conversion_widget_unsupported"))?;
        let raw_fields = raw_widget["fields"]
            .as_array()
            .ok_or_else(|| WorkcopyError::new("snapshot_corrupt"))?;
        let mut fields = Vec::with_capacity(raw_fields.len());
        for field in raw_fields {
            ensure_only_keys(field, &["name", "type", "value"])?;
            let name = field["name"]
                .as_str()
                .ok_or_else(|| WorkcopyError::new("snapshot_corrupt"))?;
            let field_type = number_u16(&field["type"])?;
            let known_type = WidgetFieldType::try_from(field_type)
                .map_err(|_| WorkcopyError::new("portable_conversion_field_unsupported"))?;
            if !capability.knows_field(name, known_type) {
                return Err(WorkcopyError::new("portable_conversion_field_unsupported"));
            }
            let value = match field_type {
                0 => ZabbixWidgetFieldValue::Integer {
                    value: field["value"]
                        .as_i64()
                        .or_else(|| field["value"].as_str().and_then(|value| value.parse().ok()))
                        .ok_or_else(|| {
                            WorkcopyError::new("portable_conversion_unsupported_data")
                        })?,
                },
                1 if widget_type == "svggraph"
                    && crate::compatibility::is_svggraph_pattern_field(name) =>
                {
                    bindings
                        .and_then(|binding| {
                            binding.patterns.get(&(page_index, index, name.to_owned()))
                        })
                        .cloned()
                        .ok_or_else(|| WorkcopyError::new("portable_conversion_binding_required"))?
                }
                1 => ZabbixWidgetFieldValue::String {
                    value: field["value"]
                        .as_str()
                        .ok_or_else(|| WorkcopyError::new("portable_conversion_unsupported_data"))?
                        .to_owned(),
                },
                3 => bindings
                    .and_then(|binding| {
                        field["value"].as_str().and_then(|id| binding.hosts.get(id))
                    })
                    .cloned()
                    .ok_or_else(|| WorkcopyError::new("portable_conversion_binding_required"))?,
                4 => bindings
                    .and_then(|binding| {
                        field["value"]
                            .as_str()
                            .and_then(|id| binding.fields.get(id))
                    })
                    .cloned()
                    .ok_or_else(|| WorkcopyError::new("portable_conversion_binding_required"))?,
                6 | 7 => bindings
                    .and_then(|binding| {
                        field["value"]
                            .as_str()
                            .and_then(|id| binding.graphs.get(&(field_type, id.to_owned())))
                    })
                    .cloned()
                    .ok_or_else(|| WorkcopyError::new("portable_conversion_binding_required"))?,
                _ => return Err(WorkcopyError::new("portable_conversion_binding_required")),
            };
            fields.push(ZabbixWidgetField {
                name: name.to_owned(),
                field_type,
                registry_state: RegistryState::Known,
                value,
            });
        }
        widgets.push(Widget::Zabbix {
            id: format!("widget-{}", index + 1),
            widget_type: widget_type.to_owned(),
            name: raw_widget["name"]
                .as_str()
                .ok_or_else(|| WorkcopyError::new("snapshot_corrupt"))?
                .to_owned(),
            registry_state: RegistryState::Known,
            fields,
            layout: Layout::Fixed {
                x: number_u16(&raw_widget["x"])?,
                y: number_u16(&raw_widget["y"])?,
                w: number_u16(&raw_widget["width"])?,
                h: number_u16(&raw_widget["height"])?,
            },
        });
    }
    let mut page = Page {
        id: "page-1".to_owned(),
        name: raw["name"]
            .as_str()
            .ok_or_else(|| WorkcopyError::new("snapshot_corrupt"))?
            .to_owned(),
        widgets,
    };
    if page.widgets.iter().any(|widget| match widget {
        Widget::Zabbix { fields, .. } => fields.iter().any(|field| field.name.ends_with("._reference") && matches!(&field.value, ZabbixWidgetFieldValue::String { value } if value == "DASHBOARD" || value.starts_with("DASHBOARD._"))),
        _ => false,
    }) {
        return Err(WorkcopyError::new("portable_conversion_reference_required"));
    }
    crate::offline_dashboards::rewrite_page_references(&mut page, &mut BTreeSet::new())
        .map_err(|_| WorkcopyError::new("portable_conversion_reference_required"))?;
    let bundle = TypedPortableBundle {
        project: TypedProject {
            format: TypedProjectFormat::V2,
            project_id,
            kind: ProjectKind::Page,
            declared_compatibility: profile.widget_version_range(),
            provenance: ProjectProvenance::ServerSnapshot,
            catalogs: bindings.map_or_else(Vec::new, |b| b.references.clone()),
            roles: bindings.map_or_else(BTreeMap::new, |b| b.roles.clone()),
            root: ProjectRoot::Page(page),
        },
        catalogs: bindings.map_or_else(BTreeMap::new, |b| b.catalogs.clone()),
    };
    bundle
        .validate()
        .map_err(|_| WorkcopyError::new("portable_conversion_unsupported_data"))?;
    Ok(bundle)
}

fn conversion_profile(
    snapshot: &VersionedDashboardSnapshot,
) -> Result<AdapterProfile, WorkcopyError> {
    if snapshot.raw_api_document().get("templateid").is_some() {
        return Err(WorkcopyError::new("portable_conversion_scope_unsupported"));
    }
    AdapterRegistry::resolve_profile(snapshot.server_version())
        .ok()
        .filter(|profile| profile.adapter_id() == snapshot.adapter_id())
        .ok_or_else(|| WorkcopyError::new("portable_conversion_widget_unsupported"))
}

fn ensure_only_keys(value: &Value, allowed: &[&str]) -> Result<(), WorkcopyError> {
    let object = value
        .as_object()
        .ok_or_else(|| WorkcopyError::new("snapshot_corrupt"))?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(WorkcopyError::new("portable_conversion_unsupported_data"));
    }
    Ok(())
}

fn number_u16(value: &Value) -> Result<u16, WorkcopyError> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| WorkcopyError::new("portable_conversion_unsupported_data"))
}

fn is_zero(value: &Value) -> bool {
    value.as_u64() == Some(0) || value.as_str() == Some("0")
}

fn inspect(
    snapshot: &VersionedDashboardSnapshot,
    kind: DraftKind,
    page_id: Option<&str>,
) -> Result<PortableConversionInventory, WorkcopyError> {
    if !matches!(
        (kind, page_id),
        (DraftKind::Dashboard, None) | (DraftKind::Page, Some(_))
    ) {
        return Err(WorkcopyError::new("workcopy_corrupt"));
    }
    let analysis = snapshot
        .analyze()
        .map_err(|error| WorkcopyError::from_zabbix(&error))?;
    let selected = if let Some(page_id) = page_id {
        let pages = snapshot.raw_api_document()["pages"]
            .as_array()
            .ok_or_else(|| WorkcopyError::new("snapshot_corrupt"))?;
        let mut matches = pages.iter().enumerate().filter_map(|(index, page)| {
            (page["dashboard_pageid"].as_str() == Some(page_id)).then_some(index)
        });
        let index = matches
            .next()
            .ok_or_else(|| WorkcopyError::new("workcopy_corrupt"))?;
        if matches.next().is_some() {
            return Err(WorkcopyError::new("workcopy_corrupt"));
        }
        Some(index)
    } else {
        None
    };
    let in_scope = |index| selected.is_none_or(|selected| selected == index);
    let mut result = PortableConversionInventory {
        kind,
        pages: selected.map_or(analysis.page_count, |_| 1),
        widgets: analysis
            .widgets
            .iter()
            .filter(|widget| in_scope(widget.page_index))
            .count(),
        unclassified_widgets: analysis
            .widgets
            .iter()
            .filter(|widget| {
                in_scope(widget.page_index) && widget.state == DependencyState::Unknown
            })
            .count(),
        item_fields: 0,
        host_fields: 0,
        graph_fields: 0,
        host_pattern_fields: 0,
        item_pattern_fields: 0,
        unknown_fields: 0,
        reference_definitions: 0,
        reference_consumers: 0,
    };
    for dependency in analysis
        .dependencies
        .iter()
        .filter(|dependency| in_scope(dependency.page_index))
    {
        match dependency.kind {
            DependencyKind::Item => result.item_fields += 1,
            DependencyKind::Host => result.host_fields += 1,
            DependencyKind::Graph => result.graph_fields += 1,
            DependencyKind::HostPattern => result.host_pattern_fields += 1,
            DependencyKind::ItemPattern => result.item_pattern_fields += 1,
            DependencyKind::ItemPrototype
            | DependencyKind::GraphPrototype
            | DependencyKind::Unknown => {
                result.unknown_fields += 1;
            }
            DependencyKind::WidgetReferenceDefinition => result.reference_definitions += 1,
            DependencyKind::WidgetReference => result.reference_consumers += 1,
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use zdashstitch_zabbix::{WidgetCapabilityRegistry, ZabbixVersion};

    use super::*;

    #[test]
    fn eight_zero_conversion_carries_its_profile_and_fields() {
        let raw = json!({"dashboardid":"700", "name":"Dashboard", "pages":[
            {"dashboard_pageid":"701", "name":"Page", "widgets":[{
                "widgetid":"7011", "type":"svggraph", "name":"Graph",
                "x":"0", "y":"0", "width":"12", "height":"4", "fields":[
                    {"name":"ds.0.item_tags_evaltype","type":0,"value":"0"},
                    {"name":"ds.0.item_tags.0.tag","type":1,"value":"team"},
                    {"name":"ds.0.item_tags.0.operator","type":0,"value":"0"},
                    {"name":"ds.0.item_tags.0.value","type":1,"value":"synthetic"},
                    {"name":"ds.0.invert_values","type":0,"value":"1"},
                    {"name":"show_hostnames","type":0,"value":"1"}
                ]
            }]}
        ]});
        let snapshot = VersionedDashboardSnapshot::from_adapter_raw(
            "test-connection".to_owned(),
            "8.0.2"
                .parse()
                .unwrap_or_else(|error| panic!("version: {error}")),
            raw.clone(),
        )
        .unwrap_or_else(|error| panic!("snapshot: {error}"));
        let page = convert_page(&snapshot, "701", "portable-page".into())
            .unwrap_or_else(|error| panic!("page: {error}"));
        let dashboard = convert_dashboard(&snapshot, "portable-dashboard".into())
            .unwrap_or_else(|error| panic!("dashboard: {error}"));
        assert_eq!(page.project.kind, ProjectKind::Page);
        assert_eq!(dashboard.project.kind, ProjectKind::Dashboard);
        for bundle in [page, dashboard] {
            assert_eq!(
                bundle.project.declared_compatibility,
                AdapterProfile::V8_0.widget_version_range()
            );
            assert_eq!(bundle.project.provenance, ProjectProvenance::ServerSnapshot);
            let bytes = zdashstitch_core::formats::build_typed_package_bytes(&bundle)
                .unwrap_or_else(|error| panic!("package: {error}"));
            assert_eq!(
                zdashstitch_core::formats::read_typed_package_bytes(&bytes)
                    .unwrap_or_else(|error| panic!("reopen: {error}")),
                bundle
            );
            let page = match &bundle.project.root {
                ProjectRoot::Page(page) => page,
                ProjectRoot::Dashboard(dashboard) => &dashboard.pages[0],
            };
            let Widget::Zabbix { fields, .. } = &page.widgets[0] else {
                panic!("generic widget")
            };
            assert_eq!(fields.len(), 6);
            assert_eq!(
                fields[3].value,
                ZabbixWidgetFieldValue::String {
                    value: "synthetic".into()
                }
            );
            assert_eq!(
                fields[4].value,
                ZabbixWidgetFieldValue::Integer { value: 1 }
            );
            assert!(
                crate::compatibility::validate_portable_widget_for_create(
                    &page.widgets[0],
                    &WidgetCapabilityRegistry::v8_0(),
                )
                .is_err()
            );
            let wire = serde_json::to_string(&bundle).unwrap_or_default();
            assert!(!wire.contains("7011") && !wire.contains("test-connection"));
        }
        assert_eq!(snapshot.raw_api_document(), &raw);
        let mut unknown = raw.clone();
        unknown["pages"][0]["widgets"][0]["fields"][0]["name"] = json!("future.unknown");
        let unknown = VersionedDashboardSnapshot::from_adapter_raw(
            "test-connection".into(),
            snapshot.server_version().clone(),
            unknown,
        )
        .unwrap_or_else(|error| panic!("snapshot: {error}"));
        assert_eq!(
            convert_page(&unknown, "701", "portable".to_owned())
                .err()
                .map(|error| error.code()),
            Some("portable_conversion_field_unsupported")
        );
        let mut template = raw.clone();
        template["templateid"] = json!("123");
        let template = VersionedDashboardSnapshot::from_adapter_raw(
            "test-connection".into(),
            snapshot.server_version().clone(),
            template,
        )
        .unwrap_or_else(|error| panic!("snapshot: {error}"));
        assert_eq!(
            convert_page(&template, "701", "portable".into())
                .err()
                .map(|error| error.code()),
            Some("portable_conversion_scope_unsupported")
        );
        let seven = VersionedDashboardSnapshot::from_adapter_raw(
            "test-connection".into(),
            "7.4.16"
                .parse()
                .unwrap_or_else(|error| panic!("version: {error}")),
            raw,
        )
        .unwrap_or_else(|error| panic!("snapshot: {error}"));
        assert!(convert_page(&seven, "701", "portable".into()).is_err());
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn portable_pages_keep_proven_local_links_and_reject_orphans() {
        let version: ZabbixVersion = "7.4.16"
            .parse()
            .unwrap_or_else(|error| panic!("version: {error}"));
        let page = |page_id: &str, order: &str, provider: &str| {
            json!({
                "dashboard_pageid":page_id, "name":"Page", "sortorder":order, "widgets":[
                    {"widgetid":format!("{page_id}1"),"type":"clock","name":"Clock","x":"0","y":"0","width":"12","height":"4","fields":[{"name":"reference","type":1,"value":"ABCDE"}]},
                    {"widgetid":format!("{page_id}2"),"type":"item","name":"Item","x":"12","y":"0","width":"12","height":"4","fields":[{"name":"time_period._reference","type":1,"value":format!("{provider}._timeperiod")}]}
                ]
            })
        };
        let raw = json!({"dashboardid":"700", "name":"Dashboard", "pages":[page("701", "0", "ABCDE"),page("702", "1", "ABCDE")]});
        let snapshot = VersionedDashboardSnapshot::from_adapter_raw(
            "test-connection".into(),
            version.clone(),
            raw.clone(),
        )
        .expect("snapshot");
        let converted = convert_dashboard(&snapshot, "portable".into()).expect("valid links");
        let ProjectRoot::Dashboard(dashboard) = &converted.project.root else {
            panic!("dashboard")
        };
        let link = |page: &Page, widget: usize| match &page.widgets[widget] {
            Widget::Zabbix { fields, .. } => match &fields[0].value {
                ZabbixWidgetFieldValue::String { value } => value.clone(),
                _ => panic!("string"),
            },
            _ => panic!("widget"),
        };
        assert_eq!(link(&dashboard.pages[0], 0), "ABCDE");
        assert_eq!(link(&dashboard.pages[0], 1), "ABCDE._timeperiod");
        assert_ne!(link(&dashboard.pages[1], 0), "ABCDE");
        assert_eq!(
            link(&dashboard.pages[1], 1),
            format!("{}._timeperiod", link(&dashboard.pages[1], 0))
        );
        assert_eq!(snapshot.raw_api_document(), &raw);
        let bytes =
            zdashstitch_core::formats::build_typed_package_bytes(&converted).expect("package");
        assert_eq!(
            zdashstitch_core::formats::read_typed_package_bytes(&bytes).expect("reopen"),
            converted
        );
        let bad = VersionedDashboardSnapshot::from_adapter_raw(
            "test-connection".into(),
            version.clone(),
            json!({"dashboardid":"700","name":"D","pages":[page("701","0","XXXXX")]}),
        )
        .expect("snapshot");
        assert_eq!(
            convert_page(&bad, "701", "portable".into())
                .err()
                .map(|error| error.code()),
            Some("portable_conversion_reference_required")
        );
        let outside = VersionedDashboardSnapshot::from_adapter_raw(
            "test-connection".into(),
            version,
            json!({"dashboardid":"700","name":"D","pages":[page("701","0","DASHBOARD")]}),
        )
        .expect("snapshot");
        assert_eq!(
            convert_page(&outside, "701", "portable".into())
                .err()
                .map(|error| error.code()),
            Some("portable_conversion_reference_required")
        );
    }

    #[test]
    fn selected_page_inventory_excludes_siblings_and_server_values() {
        let version: ZabbixVersion = "7.4.16"
            .parse()
            .unwrap_or_else(|error| panic!("version: {error}"));
        let snapshot = VersionedDashboardSnapshot::from_adapter_raw(
            "conn-synthetic".to_owned(),
            version,
            json!({
                "dashboardid": "700", "name": "private-source-name",
                "pages": [
                    {"dashboard_pageid": "701", "name": "First", "widgets": [{
                        "widgetid": "900", "type": "item", "name": "Private item",
                        "fields": [
                            {"name": "itemid.0", "type": 4, "value": "30001"},
                            {"name": "reference", "type": 1, "value": "ABCDE"}
                        ]
                    }]},
                    {"dashboard_pageid": "702", "name": "Second", "widgets": [{
                        "widgetid": "901", "type": "vendor.unknown", "name": "Private vendor",
                        "fields": [{"name": "hostid", "type": 3, "value": "40001"}]
                    }]}
                ]
            }),
        )
        .unwrap_or_else(|error| panic!("snapshot: {error}"));
        let page = inspect(&snapshot, DraftKind::Page, Some("701"))
            .unwrap_or_else(|error| panic!("page: {error}"));
        assert_eq!(page.pages, 1);
        assert_eq!(page.widgets, 1);
        assert_eq!(page.item_fields, 1);
        assert_eq!(page.reference_definitions, 1);
        assert_eq!(page.host_fields, 0);
        assert_eq!(page.unclassified_widgets, 0);
        let dashboard = inspect(&snapshot, DraftKind::Dashboard, None)
            .unwrap_or_else(|error| panic!("dashboard: {error}"));
        assert_eq!(dashboard.pages, 2);
        assert_eq!(dashboard.widgets, 2);
        assert_eq!(dashboard.host_fields, 1);
        assert_eq!(dashboard.unclassified_widgets, 1);
        let serialized =
            serde_json::to_string(&dashboard).unwrap_or_else(|error| panic!("serialize: {error}"));
        for private in ["30001", "40001", "private-source-name", "Private vendor"] {
            assert!(!serialized.contains(private));
        }
        let missing = inspect(&snapshot, DraftKind::Page, Some("999"));
        assert_eq!(
            missing.err().map(|error| error.code()),
            Some("workcopy_corrupt")
        );
        let mismatched_scope = inspect(&snapshot, DraftKind::Page, None);
        assert_eq!(
            mismatched_scope.err().map(|error| error.code()),
            Some("workcopy_corrupt")
        );
    }

    fn page_snapshot(fields: Value) -> VersionedDashboardSnapshot {
        let version: ZabbixVersion = "7.4.16"
            .parse()
            .unwrap_or_else(|error| panic!("version: {error}"));
        VersionedDashboardSnapshot::from_adapter_raw(
            "conn-synthetic".to_owned(),
            version,
            json!({"dashboardid": "700", "name": "Source dashboard", "pages": [
                {"dashboard_pageid": "701", "name": "Clock page", "sortorder": "0", "widgets": [{
                    "widgetid": "900", "type": "clock", "name": "Clock", "x": "0", "y": "0",
                    "width": "12", "height": "4", "fields": fields
                }]}
            ]}),
        )
        .unwrap_or_else(|error| panic!("snapshot: {error}"))
    }

    #[test]
    fn dependency_free_page_becomes_valid_v2_without_server_ids() {
        let snapshot = page_snapshot(json!([{"name": "time_type", "type": 0, "value": "0"}]));
        let bundle = convert_page(&snapshot, "701", "pr-synthetic".to_owned())
            .unwrap_or_else(|error| panic!("convert: {error}"));
        assert_eq!(bundle.project.kind, ProjectKind::Page);
        assert_eq!(bundle.project.provenance, ProjectProvenance::ServerSnapshot);
        let encoded =
            serde_json::to_string(&bundle).unwrap_or_else(|error| panic!("encode: {error}"));
        for server_id in ["700", "701", "900"] {
            assert!(!encoded.contains(server_id));
        }
        let package = zdashstitch_core::formats::build_typed_package_bytes(&bundle)
            .unwrap_or_else(|error| panic!("package: {error}"));
        let reopened = zdashstitch_core::formats::read_typed_package_bytes(&package)
            .unwrap_or_else(|error| panic!("reopen: {error}"));
        assert_eq!(reopened, bundle);
    }

    #[test]
    fn page_conversion_blocks_server_binding_and_unknown_field() {
        let item = page_snapshot(json!([{"name": "itemid.0", "type": 4, "value": "30001"}]));
        assert_eq!(
            convert_page(&item, "701", "pr-synthetic".to_owned())
                .err()
                .map(|error| error.code()),
            Some("portable_conversion_binding_required")
        );
        let unknown =
            page_snapshot(json!([{"name": "vendor_private", "type": 1, "value": "secret"}]));
        assert_eq!(
            convert_page(&unknown, "701", "pr-synthetic".to_owned())
                .err()
                .map(|error| error.code()),
            Some("portable_conversion_field_unsupported")
        );
    }

    #[test]
    fn dependency_free_dashboard_keeps_page_order_and_round_trips() {
        let version: ZabbixVersion = "7.4.16"
            .parse()
            .unwrap_or_else(|error| panic!("version: {error}"));
        let snapshot = VersionedDashboardSnapshot::from_adapter_raw(
            "conn-synthetic".to_owned(),
            version,
            json!({"dashboardid": "700", "name": "Source dashboard", "pages": [
                {"dashboard_pageid": "702", "name": "Second", "sortorder": "1", "widgets": []},
                {"dashboard_pageid": "701", "name": "First", "sortorder": "0", "widgets": []}
            ]}),
        )
        .unwrap_or_else(|error| panic!("snapshot: {error}"));
        let bundle = convert_dashboard(&snapshot, "pr-synthetic".to_owned())
            .unwrap_or_else(|error| panic!("convert: {error}"));
        assert_eq!(bundle.project.kind, ProjectKind::Dashboard);
        let ProjectRoot::Dashboard(dashboard) = &bundle.project.root else {
            panic!("dashboard root expected");
        };
        assert_eq!(
            dashboard
                .pages
                .iter()
                .map(|page| page.name.as_str())
                .collect::<Vec<_>>(),
            vec!["First", "Second"]
        );
        let package = zdashstitch_core::formats::build_typed_package_bytes(&bundle)
            .unwrap_or_else(|error| panic!("package: {error}"));
        assert_eq!(
            zdashstitch_core::formats::read_typed_package_bytes(&package)
                .unwrap_or_else(|error| panic!("reopen: {error}")),
            bundle
        );
        let encoded =
            serde_json::to_string(&bundle).unwrap_or_else(|error| panic!("encode: {error}"));
        for server_id in ["700", "701", "702"] {
            assert!(!encoded.contains(server_id));
        }
    }

    #[test]
    fn svggraph_patterns_cannot_silently_become_source_bound_strings() {
        let version: ZabbixVersion = "7.4.16".parse().unwrap_or_else(|e| panic!("{e}"));
        let snapshot = VersionedDashboardSnapshot::from_adapter_raw("test".into(), version, json!({"dashboardid":"700","name":"D","pages":[{"dashboard_pageid":"701","name":"P","widgets":[{
            "widgetid":"900", "type":"svggraph", "name":"CPU", "x":"0", "y":"0", "width":"12", "height":"4",
            "fields":[{"name":"ds.0.hosts.0","type":"1","value":"source-*"},{"name":"ds.0.items.0","type":"1","value":"CPU*"}]
        }]}]})).unwrap_or_else(|e| panic!("{e}"));
        let inventory =
            inspect(&snapshot, DraftKind::Page, Some("701")).unwrap_or_else(|e| panic!("{e}"));
        assert!(inventory.host_pattern_fields > 0 && inventory.item_pattern_fields > 0);
        assert_eq!(
            convert_page(&snapshot, "701", "portable".into())
                .err()
                .map(|e| e.code()),
            Some("portable_conversion_binding_required")
        );
        let registry = WidgetCapabilityRegistry::v7_4();
        let model =
            crate::resolver::source_model(&snapshot, &registry).unwrap_or_else(|e| panic!("{e}"));
        let mut bindings = crate::portable_bindings::validate_choices(
            &BTreeSet::new(),
            &crate::resolver::SourceFacts::default(),
            &BTreeMap::new(),
            &[],
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let choices: Vec<_> = ["ds.0.hosts.0", "ds.0.items.0"]
            .into_iter()
            .map(
                |field_name| crate::portable_bindings::PortablePatternChoice {
                    page_index: 0,
                    widget_index: 0,
                    field_name: field_name.into(),
                    role: "hosts".into(),
                },
            )
            .collect();
        assert!(
            crate::portable_bindings::add_pattern_choices(&mut bindings, &model, &choices[..1])
                .is_err()
        );
        crate::portable_bindings::add_pattern_choices(&mut bindings, &model, &choices)
            .unwrap_or_else(|e| panic!("{e}"));
        let bundle = convert_page_bound(
            &snapshot,
            "701",
            "portable".into(),
            Some(bindings.for_snapshot(&snapshot)),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let ProjectRoot::Page(page) = &bundle.project.root else {
            panic!("page")
        };
        assert_eq!(
            crate::compatibility::validate_portable_widget_for_create(&page.widgets[0], &registry),
            Err(crate::compatibility::PortableWidgetCreateError::PatternVerificationRequired)
        );
        let bytes = zdashstitch_core::formats::build_typed_package_bytes(&bundle)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            zdashstitch_core::formats::read_typed_package_bytes(&bytes)
                .unwrap_or_else(|e| panic!("{e}")),
            bundle
        );
    }

    #[test]
    fn dashboard_server_defaults_are_accepted_but_settings_and_sharing_are_not_dropped() {
        let version: ZabbixVersion = "7.4.16".parse().unwrap_or_else(|e| panic!("{e}"));
        let defaults = json!({"dashboardid":"700","name":"D","userid":"42","private":"1","display_period":"30","auto_start":"1","users":[],"userGroups":[],"pages":[{"dashboard_pageid":"701","name":"Page","widgets":[]}]});
        let snapshot = |raw| {
            VersionedDashboardSnapshot::from_adapter_raw(
                "conn-synthetic".into(),
                version.clone(),
                raw,
            )
            .unwrap_or_else(|e| panic!("{e}"))
        };
        assert!(convert_dashboard(&snapshot(defaults.clone()), "portable".into()).is_ok());
        for (key, value) in [
            ("private", json!("0")),
            ("display_period", json!("120")),
            ("auto_start", json!("0")),
            ("users", json!([{"userid":"3"}])),
            ("userGroups", json!([{"usrgrpid":"3"}])),
        ] {
            let mut raw = defaults.clone();
            raw[key] = value;
            assert_eq!(
                convert_dashboard(&snapshot(raw), "portable".into())
                    .err()
                    .map(|e| e.code()),
                Some("portable_conversion_unsupported_data")
            );
        }
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn eight_zero_portable_dashboard_excludes_server_access() {
        let version = "8.0.0".parse().expect("8.0.0 version");
        let raw = json!({
            "dashboardid":"700","name":"D","userid":"4242","private":"0",
            "users":[{"userid":"5345","permission":"2"}],
            "userGroups":[{"usrgrpid":"6767","permission":"3"}],
            "pages":[{"dashboard_pageid":"701","name":"Page","widgets":[]}]
        });
        let snapshot =
            VersionedDashboardSnapshot::from_adapter_raw("conn-synthetic".into(), version, raw)
                .expect("synthetic dashboard");
        let bundle = convert_dashboard(&snapshot, "portable".into()).expect("portable dashboard");
        let portable = serde_json::to_string(&bundle).expect("serialized bundle");
        let yaml = String::from_utf8(
            zdashstitch_core::formats::build_typed_yaml_bytes(&bundle).expect("portable YAML"),
        )
        .expect("UTF-8 YAML");
        let zip =
            zdashstitch_core::formats::build_typed_package_bytes(&bundle).expect("portable ZIP");
        let reopened =
            zdashstitch_core::formats::read_typed_package_bytes(&zip).expect("read portable ZIP");
        assert_eq!(reopened, bundle);
        for private_field in ["userid", "usrgrpid", "userGroups", "4242", "5345", "6767"] {
            assert!(!portable.contains(private_field));
            assert!(!yaml.contains(private_field));
        }
        assert!(snapshot.dashboard_access_state().is_some_and(|access| {
            access.owner == "4242"
                && !access.private
                && access.users.len() == 1
                && access.groups.len() == 1
        }));
    }

    #[test]
    fn dashboard_conversion_refuses_ambiguous_page_order() {
        let version: ZabbixVersion = "7.4.16"
            .parse()
            .unwrap_or_else(|error| panic!("version: {error}"));
        for second in [
            json!({"dashboard_pageid": "702", "name": "Second", "widgets": []}),
            json!({"dashboard_pageid": "702", "name": "Second", "sortorder": "0", "widgets": []}),
        ] {
            let snapshot = VersionedDashboardSnapshot::from_adapter_raw(
                "conn-synthetic".to_owned(),
                version.clone(),
                json!({"dashboardid": "700", "name": "Source", "pages": [
                    {"dashboard_pageid": "701", "name": "First", "sortorder": "0", "widgets": []},
                    second
                ]}),
            )
            .unwrap_or_else(|error| panic!("snapshot: {error}"));
            assert_eq!(
                convert_dashboard(&snapshot, "pr-synthetic".to_owned())
                    .err()
                    .map(|error| error.code()),
                Some("portable_conversion_unsupported_data")
            );
        }
    }

    #[test]
    fn workcopy_name_wins_over_source_name_for_both_root_kinds() {
        let snapshot = page_snapshot(json!([]));
        let mut page = convert_page(&snapshot, "701", "pr-page".to_owned())
            .unwrap_or_else(|error| panic!("page: {error}"));
        apply_workcopy_name(&mut page, "Local page")
            .unwrap_or_else(|error| panic!("rename: {error}"));
        assert!(matches!(&page.project.root, ProjectRoot::Page(root) if root.name == "Local page"));
        let mut dashboard = convert_dashboard(&snapshot, "pr-dashboard".to_owned())
            .unwrap_or_else(|error| panic!("dashboard: {error}"));
        apply_workcopy_name(&mut dashboard, "Local dashboard")
            .unwrap_or_else(|error| panic!("rename: {error}"));
        assert!(matches!(&dashboard.project.root,
            ProjectRoot::Dashboard(root) if root.name == "Local dashboard"));
        for bundle in [&page, &dashboard] {
            let package = zdashstitch_core::formats::build_typed_package_bytes(bundle)
                .unwrap_or_else(|error| panic!("package: {error}"));
            assert_eq!(
                zdashstitch_core::formats::read_typed_package_bytes(&package)
                    .unwrap_or_else(|error| panic!("reopen: {error}")),
                *bundle
            );
        }
        assert_eq!(
            apply_workcopy_name(&mut page, "")
                .err()
                .map(|error| error.code()),
            Some("portable_conversion_unsupported_data")
        );
    }
}
