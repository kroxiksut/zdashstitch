// SPDX-License-Identifier: MIT OR Apache-2.0

//! P4-08: content-free inventory before a Workcopy can become a portable
//! typed project. This is read-only and does not claim that conversion exists.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;
use zdashstitch_core::{
    Version, VersionRange,
    formats::{
        Layout, Page, ProjectKind, ProjectProvenance, ProjectRoot, RegistryState,
        TypedPortableBundle, TypedProject, TypedProjectFormat, Widget, ZabbixWidgetField,
        ZabbixWidgetFieldValue,
    },
};
use zdashstitch_zabbix::{
    DependencyKind, DependencyState, VersionedDashboardSnapshot, WidgetCapabilityRegistry,
    WidgetFieldType,
};

use crate::{
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

impl WorkcopyService {
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
        convert_page(&local, page_id, project_id)
    }
}

fn convert_page(
    snapshot: &VersionedDashboardSnapshot,
    page_id: &str,
    project_id: String,
) -> Result<TypedPortableBundle, WorkcopyError> {
    let inventory = inspect(snapshot, DraftKind::Page, Some(page_id))?;
    if inventory.item_fields
        + inventory.host_fields
        + inventory.graph_fields
        + inventory.host_pattern_fields
        + inventory.item_pattern_fields
        + inventory.unknown_fields
        + inventory.reference_definitions
        + inventory.reference_consumers
        > 0
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
    let registry = WidgetCapabilityRegistry::v7_4();
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
            .filter(|capability| capability.flags.create)
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
            let value = match field_type {
                0 => ZabbixWidgetFieldValue::Integer {
                    value: field["value"]
                        .as_i64()
                        .or_else(|| field["value"].as_str().and_then(|value| value.parse().ok()))
                        .ok_or_else(|| {
                            WorkcopyError::new("portable_conversion_unsupported_data")
                        })?,
                },
                1 => ZabbixWidgetFieldValue::String {
                    value: field["value"]
                        .as_str()
                        .ok_or_else(|| WorkcopyError::new("portable_conversion_unsupported_data"))?
                        .to_owned(),
                },
                _ => return Err(WorkcopyError::new("portable_conversion_binding_required")),
            };
            let known_type = WidgetFieldType::try_from(field_type)
                .map_err(|_| WorkcopyError::new("portable_conversion_binding_required"))?;
            if !capability.knows_field(name, known_type) {
                return Err(WorkcopyError::new("portable_conversion_field_unsupported"));
            }
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
    let page = Page {
        id: "page-1".to_owned(),
        name: raw["name"]
            .as_str()
            .ok_or_else(|| WorkcopyError::new("snapshot_corrupt"))?
            .to_owned(),
        widgets,
    };
    let bundle = TypedPortableBundle {
        project: TypedProject {
            format: TypedProjectFormat::V2,
            project_id,
            kind: ProjectKind::Page,
            declared_compatibility: VersionRange {
                minimum: Version::new(7, 4, 0),
                maximum_exclusive: None,
            },
            provenance: ProjectProvenance::Local,
            catalogs: Vec::new(),
            roles: BTreeMap::new(),
            root: ProjectRoot::Page(page),
        },
        catalogs: BTreeMap::new(),
    };
    bundle
        .validate()
        .map_err(|_| WorkcopyError::new("portable_conversion_unsupported_data"))?;
    Ok(bundle)
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
            DependencyKind::Unknown => result.unknown_fields += 1,
            DependencyKind::WidgetReferenceDefinition => result.reference_definitions += 1,
            DependencyKind::WidgetReference => result.reference_consumers += 1,
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use zdashstitch_zabbix::ZabbixVersion;

    use super::*;

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
        assert_eq!(bundle.project.provenance, ProjectProvenance::Local);
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
}
