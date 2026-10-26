// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::BTreeMap;

use zdashstitch_core::{
    VersionRange,
    formats::{
        ProjectRoot, RegistryState, TypedProject, Widget, ZabbixWidgetField, ZabbixWidgetFieldValue,
    },
};
use zdashstitch_zabbix::{WidgetCapabilityRegistry, WidgetFieldType};

use crate::MessageId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompatibilityKnowledge {
    Known,
    Unknown,
    Incompatible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompatibilityNoticeLevel {
    Info,
    Warning,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompatibilityNotice {
    pub code: MessageId,
    pub level: CompatibilityNoticeLevel,
    pub params: BTreeMap<&'static str, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DraftWidgetField {
    pub name: String,
    pub field_type: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DraftWidgetUsage {
    pub widget_type: String,
    pub fields: Vec<DraftWidgetField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DraftCompatibilityReport {
    pub knowledge: CompatibilityKnowledge,
    pub declared: VersionRange,
    pub calculated: Option<VersionRange>,
    pub notices: Vec<CompatibilityNotice>,
}

#[must_use]
pub fn analyze_draft_compatibility(
    declared: &VersionRange,
    widgets: &[DraftWidgetUsage],
    registry: &WidgetCapabilityRegistry,
) -> DraftCompatibilityReport {
    let mut notices = Vec::new();
    if widgets.is_empty() {
        notices.push(notice(
            MessageId::COMPATIBILITY_EMPTY_DRAFT,
            CompatibilityNoticeLevel::Warning,
            [],
        ));
        return DraftCompatibilityReport {
            knowledge: CompatibilityKnowledge::Unknown,
            declared: declared.clone(),
            calculated: None,
            notices,
        };
    }

    let mut calculated: Option<VersionRange> = None;
    let mut has_unknown = false;
    let mut incompatible = false;
    for widget in widgets {
        let Some(capability) = registry.find(&widget.widget_type) else {
            has_unknown = true;
            notices.push(notice(
                MessageId::COMPATIBILITY_UNKNOWN_WIDGET_TYPE,
                CompatibilityNoticeLevel::Warning,
                [("widget_type", widget.widget_type.clone())],
            ));
            continue;
        };
        for field in &widget.fields {
            let field_type = WidgetFieldType::try_from(field.field_type);
            if field_type.is_err()
                || !capability
                    .knows_field(&field.name, field_type.unwrap_or(WidgetFieldType::String))
            {
                has_unknown = true;
                notices.push(notice(
                    MessageId::COMPATIBILITY_UNKNOWN_WIDGET_FIELD,
                    CompatibilityNoticeLevel::Warning,
                    [
                        ("widget_type", widget.widget_type.clone()),
                        ("field", field.name.clone()),
                        ("field_type", field.field_type.to_string()),
                    ],
                ));
            }
        }
        calculated = match calculated {
            Some(current) => match current.intersection(&capability.versions) {
                Some(intersection) => Some(intersection),
                None => {
                    incompatible = true;
                    None
                }
            },
            None if !incompatible => Some(capability.versions.clone()),
            None => None,
        };
    }

    let knowledge = if incompatible {
        notices.push(notice(
            MessageId::COMPATIBILITY_NO_COMMON_RANGE,
            CompatibilityNoticeLevel::Warning,
            [],
        ));
        CompatibilityKnowledge::Incompatible
    } else if has_unknown || calculated.is_none() {
        CompatibilityKnowledge::Unknown
    } else {
        CompatibilityKnowledge::Known
    };

    if knowledge == CompatibilityKnowledge::Known
        && let Some(range) = &calculated
    {
        notices.push(range_notice(
            MessageId::COMPATIBILITY_CALCULATED_RANGE,
            range,
        ));
        if range != declared {
            let mut params = range_params(range);
            params.insert("declared_minimum", declared.minimum.to_string());
            if let Some(maximum) = &declared.maximum_exclusive {
                params.insert("declared_maximum_exclusive", maximum.to_string());
            }
            notices.push(CompatibilityNotice {
                code: MessageId::COMPATIBILITY_DECLARED_MISMATCH,
                level: CompatibilityNoticeLevel::Warning,
                params,
            });
        }
    }

    DraftCompatibilityReport {
        knowledge,
        declared: declared.clone(),
        calculated,
        notices,
    }
}

#[must_use]
pub fn analyze_project_compatibility(
    project: &TypedProject,
    registry: &WidgetCapabilityRegistry,
) -> DraftCompatibilityReport {
    let widgets = match &project.root {
        ProjectRoot::Dashboard(dashboard) => dashboard
            .pages
            .iter()
            .flat_map(|page| page.widgets.iter())
            .map(core_widget_usage)
            .collect::<Vec<_>>(),
        ProjectRoot::Page(page) => page.widgets.iter().map(core_widget_usage).collect(),
    };
    analyze_draft_compatibility(&project.declared_compatibility, &widgets, registry)
}

fn core_widget_usage(widget: &Widget) -> DraftWidgetUsage {
    match widget {
        Widget::Graph { .. } => DraftWidgetUsage {
            widget_type: "svggraph".to_owned(),
            fields: Vec::new(),
        },
        Widget::ItemValue { .. } => DraftWidgetUsage {
            widget_type: "item".to_owned(),
            fields: Vec::new(),
        },
        Widget::Zabbix {
            widget_type,
            fields,
            ..
        } => DraftWidgetUsage {
            widget_type: widget_type.clone(),
            fields: fields
                .iter()
                .map(|field| DraftWidgetField {
                    name: field.name.clone(),
                    field_type: field.field_type,
                })
                .collect(),
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortableWidgetCreateError {
    UnknownWidgetType,
    UnknownWidgetField,
    UnresolvedWidgetField,
    RequiredFieldsMissing,
    Incomplete,
}

impl PortableWidgetCreateError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnknownWidgetType => "portable_widget_unknown_type",
            Self::UnknownWidgetField => "portable_widget_unknown_field",
            Self::UnresolvedWidgetField => "portable_widget_unresolved_field",
            Self::RequiredFieldsMissing => "portable_widget_required_fields_missing",
            Self::Incomplete => "portable_widget_incomplete",
        }
    }
}

/// Decides whether a generic `type: zabbix` widget from a portable draft may be created.
///
/// Zabbix 7.4 accepts many incomplete widgets (for example `gauge` without an item), so
/// this gate is stricter than the server:
/// - every field has a known Zabbix field type (0..=13), is marked `known` and holds a
///   resolved value; widget types with a proven create codec (`create` flag) must also
///   use only registry field names;
/// - `item` needs `itemid.0`; `svggraph` needs at least one `ds.N` data set with hosts and
///   items (or item IDs); `clock` and `problems` may rely on server defaults;
/// - other recognised types need a non-empty field set. The format has no marker proving
///   that fields came complete from a source widget, so a non-empty set is the residual
///   heuristic documented in the format contract.
pub fn validate_portable_widget_for_create(
    widget: &Widget,
    registry: &WidgetCapabilityRegistry,
) -> Result<(), PortableWidgetCreateError> {
    let Widget::Zabbix {
        widget_type,
        registry_state,
        fields,
        ..
    } = widget
    else {
        return Ok(());
    };
    if *registry_state == RegistryState::Unknown {
        return Err(PortableWidgetCreateError::UnknownWidgetType);
    }
    let capability = registry
        .find(widget_type)
        .ok_or(PortableWidgetCreateError::UnknownWidgetType)?;
    for field in fields {
        let field_type = WidgetFieldType::try_from(field.field_type)
            .map_err(|()| PortableWidgetCreateError::UnknownWidgetField)?;
        if field.registry_state == RegistryState::Unknown
            || (capability.flags.create && !capability.knows_field(&field.name, field_type))
        {
            return Err(PortableWidgetCreateError::UnknownWidgetField);
        }
        if matches!(field.value, ZabbixWidgetFieldValue::Unresolved { .. }) {
            return Err(PortableWidgetCreateError::UnresolvedWidgetField);
        }
    }
    match widget_type.as_str() {
        "item" => ensure_required(fields.iter().any(|field| {
            field.name == "itemid.0" && field.field_type == WidgetFieldType::Item as u16
        })),
        "svggraph" => ensure_required(svggraph_has_data_set(fields)),
        "clock" | "problems" => Ok(()),
        _ if fields.is_empty() => Err(PortableWidgetCreateError::Incomplete),
        _ => Ok(()),
    }
}

const fn ensure_required(present: bool) -> Result<(), PortableWidgetCreateError> {
    if present {
        Ok(())
    } else {
        Err(PortableWidgetCreateError::RequiredFieldsMissing)
    }
}

/// True when some data set `ds.N` names hosts and items by pattern, or lists item IDs.
fn svggraph_has_data_set(fields: &[ZabbixWidgetField]) -> bool {
    let mut hosts = Vec::new();
    let mut items = Vec::new();
    for field in fields {
        let parts: Vec<&str> = field.name.split('.').collect();
        let [prefix, set, member, index] = parts.as_slice() else {
            continue;
        };
        if *prefix != "ds" || !is_index(set) || !is_index(index) {
            continue;
        }
        let filled = match &field.value {
            ZabbixWidgetFieldValue::String { value } => !value.trim().is_empty(),
            ZabbixWidgetFieldValue::Metric { .. } => true,
            _ => false,
        };
        if !filled {
            continue;
        }
        match (*member, field.field_type) {
            ("itemids", 4) => return true,
            ("hosts", 1) => hosts.push(*set),
            ("items", 1) => items.push(*set),
            _ => {}
        }
    }
    hosts.iter().any(|set| items.contains(set))
}

fn is_index(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn range_notice(code: MessageId, range: &VersionRange) -> CompatibilityNotice {
    CompatibilityNotice {
        code,
        level: CompatibilityNoticeLevel::Info,
        params: range_params(range),
    }
}

fn range_params(range: &VersionRange) -> BTreeMap<&'static str, String> {
    let mut params = BTreeMap::from([("minimum", range.minimum.to_string())]);
    if let Some(maximum) = &range.maximum_exclusive {
        params.insert("maximum_exclusive", maximum.to_string());
    }
    params
}

fn notice<const N: usize>(
    code: MessageId,
    level: CompatibilityNoticeLevel,
    params: [(&'static str, String); N],
) -> CompatibilityNotice {
    CompatibilityNotice {
        code,
        level,
        params: params.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use zdashstitch_core::{
        Version, VersionRange,
        formats::{
            HostSelection, ProjectRoot, Selection, TypedProject, ZabbixObjectKind, from_yaml_str,
        },
    };

    use super::*;

    fn range(minimum: (u32, u32, u32)) -> VersionRange {
        VersionRange {
            minimum: Version::new(minimum.0, minimum.1, minimum.2),
            maximum_exclusive: None,
        }
    }

    #[test]
    fn empty_draft_is_unknown() {
        let report =
            analyze_draft_compatibility(&range((7, 4, 0)), &[], &WidgetCapabilityRegistry::v7_4());
        assert_eq!(report.knowledge, CompatibilityKnowledge::Unknown);
        assert_eq!(report.calculated, None);
        assert_eq!(report.notices[0].code, MessageId::COMPATIBILITY_EMPTY_DRAFT);
    }

    #[test]
    fn unknown_type_and_unknown_field_fail_closed() {
        let registry = WidgetCapabilityRegistry::v7_4();
        let unknown_type = analyze_draft_compatibility(
            &range((7, 4, 0)),
            &[DraftWidgetUsage {
                widget_type: "vendor.private".to_owned(),
                fields: Vec::new(),
            }],
            &registry,
        );
        assert_eq!(unknown_type.knowledge, CompatibilityKnowledge::Unknown);
        assert_eq!(unknown_type.calculated, None);

        let unknown_field = analyze_draft_compatibility(
            &range((7, 4, 0)),
            &[DraftWidgetUsage {
                widget_type: "item".to_owned(),
                fields: vec![DraftWidgetField {
                    name: "vendor.private".to_owned(),
                    field_type: WidgetFieldType::String as u16,
                }],
            }],
            &registry,
        );
        assert_eq!(unknown_field.knowledge, CompatibilityKnowledge::Unknown);
        assert!(
            unknown_field
                .notices
                .iter()
                .any(|notice| notice.code == MessageId::COMPATIBILITY_UNKNOWN_WIDGET_FIELD)
        );
    }

    #[test]
    fn known_widgets_calculate_7_4_plus_and_warn_on_declared_mismatch() {
        let report = analyze_draft_compatibility(
            &range((7, 4, 16)),
            &[DraftWidgetUsage {
                widget_type: "item".to_owned(),
                fields: vec![DraftWidgetField {
                    name: "itemid.0".to_owned(),
                    field_type: WidgetFieldType::Item as u16,
                }],
            }],
            &WidgetCapabilityRegistry::v7_4(),
        );
        assert_eq!(report.knowledge, CompatibilityKnowledge::Known);
        assert_eq!(
            report.calculated.as_ref().map(|range| &range.minimum),
            Some(&Version::new(7, 4, 0))
        );
        assert!(
            report
                .notices
                .iter()
                .any(|notice| notice.code == MessageId::COMPATIBILITY_DECLARED_MISMATCH)
        );
        let calculated = report
            .notices
            .iter()
            .find(|notice| notice.code == MessageId::COMPATIBILITY_CALCULATED_RANGE)
            .unwrap_or_else(|| panic!("missing calculated range notice"));
        assert_eq!(calculated.params.get("minimum"), Some(&"7.4.0".to_owned()));
        assert!(
            report
                .notices
                .iter()
                .all(|notice| !notice.code.as_str().contains(' '))
        );
    }

    fn global_view_widgets() -> Vec<Widget> {
        let project: TypedProject = from_yaml_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/global-view-v2.yaml"
        )))
        .unwrap_or_else(|error| panic!("global view fixture must parse: {error}"));
        let ProjectRoot::Page(page) = project.root else {
            panic!("fixture must be a page");
        };
        page.widgets
    }

    fn zabbix_widget(widget_type: &str, fields: Vec<ZabbixWidgetField>) -> Widget {
        let mut widget = global_view_widgets().remove(0);
        if let Widget::Zabbix {
            widget_type: kind,
            fields: current,
            ..
        } = &mut widget
        {
            widget_type.clone_into(kind);
            *current = fields;
        }
        widget
    }

    fn field(name: &str, field_type: u16, value: ZabbixWidgetFieldValue) -> ZabbixWidgetField {
        ZabbixWidgetField {
            name: name.to_owned(),
            field_type,
            registry_state: RegistryState::Known,
            value,
        }
    }

    fn text(value: &str) -> ZabbixWidgetFieldValue {
        ZabbixWidgetFieldValue::String {
            value: value.to_owned(),
        }
    }

    fn item_reference() -> ZabbixWidgetFieldValue {
        ZabbixWidgetFieldValue::Metric {
            catalog: "linux".to_owned(),
            metric: "cpu".to_owned(),
            hosts: HostSelection {
                role: "db".to_owned(),
                selection: Selection::One,
                slot_id: None,
            },
        }
    }

    fn check(widget: &Widget) -> Result<(), PortableWidgetCreateError> {
        validate_portable_widget_for_create(widget, &WidgetCapabilityRegistry::v7_4())
    }

    #[test]
    fn portable_creation_blocks_unknown_or_unresolved_fields_before_write_planning() {
        let widgets = global_view_widgets();
        assert_eq!(
            check(&widgets[0]),
            Ok(()),
            "empty clock uses server defaults"
        );
        assert_eq!(
            check(&widgets[1]),
            Err(PortableWidgetCreateError::UnresolvedWidgetField)
        );
        assert_eq!(check(&widgets[2]), Ok(()), "tophosts with complete fields");
        assert_eq!(
            check(&widgets[3]),
            Err(PortableWidgetCreateError::UnknownWidgetField)
        );
        assert_eq!(
            PortableWidgetCreateError::UnresolvedWidgetField.code(),
            "portable_widget_unresolved_field"
        );
    }

    #[test]
    fn portable_creation_requires_item_and_svggraph_data() {
        assert_eq!(
            check(&zabbix_widget("item", Vec::new())),
            Err(PortableWidgetCreateError::RequiredFieldsMissing)
        );
        assert_eq!(
            check(&zabbix_widget(
                "item",
                vec![field("itemid.0", 4, item_reference())]
            )),
            Ok(())
        );
        assert_eq!(
            check(&zabbix_widget(
                "svggraph",
                vec![field("ds.0.hosts.0", 1, text("Linux*"))]
            )),
            Err(PortableWidgetCreateError::RequiredFieldsMissing)
        );
        assert_eq!(
            check(&zabbix_widget(
                "svggraph",
                vec![
                    field("ds.0.hosts.0", 1, text("Linux*")),
                    field("ds.1.items.0", 1, text("CPU*")),
                ]
            )),
            Err(PortableWidgetCreateError::RequiredFieldsMissing),
            "hosts and items must belong to the same data set"
        );
        assert_eq!(
            check(&zabbix_widget(
                "svggraph",
                vec![
                    field("ds.0.hosts.0", 1, text("Linux*")),
                    field("ds.0.items.0", 1, text("CPU*")),
                ]
            )),
            Ok(())
        );
        assert_eq!(
            check(&zabbix_widget(
                "svggraph",
                vec![field("ds.0.itemids.0", 4, item_reference())]
            )),
            Ok(())
        );
        assert_eq!(
            PortableWidgetCreateError::RequiredFieldsMissing.code(),
            "portable_widget_required_fields_missing"
        );
    }

    #[test]
    fn portable_creation_allows_empty_clock_and_problems_only() {
        assert_eq!(check(&zabbix_widget("clock", Vec::new())), Ok(()));
        assert_eq!(check(&zabbix_widget("problems", Vec::new())), Ok(()));
        for widget_type in ["gauge", "tophosts", "honeycomb"] {
            assert_eq!(
                check(&zabbix_widget(widget_type, Vec::new())),
                Err(PortableWidgetCreateError::Incomplete),
                "{widget_type}"
            );
        }
        assert_eq!(
            PortableWidgetCreateError::Incomplete.code(),
            "portable_widget_incomplete"
        );
    }

    #[test]
    fn recognised_types_accept_unregistered_names_with_known_resolved_fields() {
        assert_eq!(
            check(&zabbix_widget(
                "gauge",
                vec![
                    field("itemid.0", 4, item_reference()),
                    field("min", 1, text("0")),
                ]
            )),
            Ok(())
        );
        let mut unknown_state = field("min", 1, text("0"));
        unknown_state.registry_state = RegistryState::Unknown;
        assert_eq!(
            check(&zabbix_widget("gauge", vec![unknown_state])),
            Err(PortableWidgetCreateError::UnknownWidgetField)
        );
        let unknown_type = ZabbixWidgetField {
            name: "vendor.flag".to_owned(),
            field_type: 42,
            registry_state: RegistryState::Unknown,
            value: ZabbixWidgetFieldValue::Unresolved {
                object_kind: ZabbixObjectKind::Unknown,
                display_name: "x".to_owned(),
            },
        };
        assert_eq!(
            check(&zabbix_widget("gauge", vec![unknown_type])),
            Err(PortableWidgetCreateError::UnknownWidgetField)
        );
        assert_eq!(
            check(&zabbix_widget(
                "item",
                vec![
                    field("itemid.0", 4, item_reference()),
                    field("vendor.private", 1, text("x")),
                ]
            )),
            Err(PortableWidgetCreateError::UnknownWidgetField),
            "types with a create codec accept only registry field names"
        );
        assert_eq!(
            check(&zabbix_widget("vendor.private", Vec::new())),
            Err(PortableWidgetCreateError::UnknownWidgetType)
        );
        let mut unknown_widget = zabbix_widget("clock", Vec::new());
        if let Widget::Zabbix { registry_state, .. } = &mut unknown_widget {
            *registry_state = RegistryState::Unknown;
        }
        assert_eq!(
            check(&unknown_widget),
            Err(PortableWidgetCreateError::UnknownWidgetType)
        );
    }
}
