// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    collections::{BTreeMap, HashSet},
    hash::Hash,
};

use super::{
    Cardinality, Dashboard, DashboardKind, FormatError, ItemExpansion, Layout, MetricCatalog,
    PortableProject, ProjectFormat, ProjectRoot, Result, Selection, TypedProject, ValueType,
    Widget, ZabbixWidgetFieldValue,
};

pub trait Validate {
    fn validate(&self) -> Result<()>;
}

pub fn validate_project(
    project: &PortableProject,
    catalogs: &BTreeMap<String, MetricCatalog>,
) -> Result<()> {
    project.validate()?;
    validate_project_content(project, catalogs)
}

fn validate_project_content(
    project: &PortableProject,
    catalogs: &BTreeMap<String, MetricCatalog>,
) -> Result<()> {
    ensure(
        catalogs.len() == project.catalogs.len()
            && project
                .catalogs
                .iter()
                .all(|entry| catalogs.contains_key(&entry.id)),
        "catalog_set",
        "missing or extra catalog",
    )?;

    let mut metrics: BTreeMap<&str, BTreeMap<&str, ValueType>> = BTreeMap::new();
    for reference in &project.catalogs {
        let catalog = catalogs.get(&reference.id).ok_or_else(|| {
            FormatError::validation("catalog_set", format!("missing catalog {}", reference.id))
        })?;
        catalog.validate()?;
        ensure(
            catalog.catalog_id == reference.id,
            "catalog_identity",
            format!("catalog {} has mismatched catalog_id", reference.id),
        )?;
        metrics.insert(
            reference.id.as_str(),
            catalog
                .metrics
                .iter()
                .map(|metric| (metric.id.as_str(), metric.value_type))
                .collect(),
        );
    }

    ensure_unique(
        project.dashboards.iter().map(|entry| entry.id.as_str()),
        "duplicate_dashboard",
        "dashboard id",
    )?;
    for dashboard in &project.dashboards {
        ensure_unique(
            dashboard.pages.iter().map(|entry| entry.id.as_str()),
            "duplicate_page",
            "page id",
        )?;
        for page in &dashboard.pages {
            ensure_unique(
                page.widgets.iter().map(Widget::id),
                "duplicate_widget",
                "widget id",
            )?;
            let mut rectangles = Vec::new();
            for widget in &page.widgets {
                if let Widget::Zabbix { fields, .. } = widget {
                    for field in fields {
                        match &field.value {
                            ZabbixWidgetFieldValue::Host {
                                role,
                                selection,
                                slot_id,
                            } => {
                                validate_host_relationship(
                                    &project.roles,
                                    role,
                                    *selection,
                                    slot_id.as_deref(),
                                )?;
                                ensure_single_reference(&project.roles, role, *selection)?;
                            }
                            ZabbixWidgetFieldValue::Metric {
                                catalog,
                                metric,
                                hosts,
                            } => {
                                validate_host_relationship(
                                    &project.roles,
                                    &hosts.role,
                                    hosts.selection,
                                    hosts.slot_id.as_deref(),
                                )?;
                                ensure_single_reference(
                                    &project.roles,
                                    &hosts.role,
                                    hosts.selection,
                                )?;
                                let value_type = metrics
                                    .get(catalog.as_str())
                                    .and_then(|entries| entries.get(metric.as_str()))
                                    .copied()
                                    .ok_or_else(|| {
                                        FormatError::validation(
                                            "unknown_metric",
                                            format!("unknown metric {catalog}/{metric}"),
                                        )
                                    })?;
                                // Zabbix item fields (type 4) render one item value; binary
                                // items are unsupported there, as for item_value widgets.
                                ensure(
                                    field.field_type != 4 || value_type != ValueType::Binary,
                                    "binary_item_value",
                                    "binary metrics are unsupported in item fields",
                                )?;
                            }
                            _ => {}
                        }
                    }
                }
                for (metric, hosts) in widget.references() {
                    validate_host_relationship(
                        &project.roles,
                        &hosts.role,
                        hosts.selection,
                        hosts.slot_id.as_deref(),
                    )?;
                    let value_type = metrics
                        .get(metric.catalog.as_str())
                        .and_then(|catalog| catalog.get(metric.metric.as_str()))
                        .copied()
                        .ok_or_else(|| {
                            FormatError::validation(
                                "unknown_metric",
                                format!("unknown metric {}/{}", metric.catalog, metric.metric),
                            )
                        })?;
                    match widget {
                        Widget::Graph { .. } => ensure(
                            value_type.is_numeric(),
                            "nonnumeric_graph",
                            "graph series must use numeric metrics",
                        )?,
                        Widget::ItemValue { .. } => ensure(
                            value_type != ValueType::Binary,
                            "binary_item_value",
                            "binary item_value is unsupported by MVP",
                        )?,
                        Widget::Zabbix { .. } => {}
                    }
                }

                if let Widget::ItemValue {
                    expansion, hosts, ..
                } = widget
                {
                    if *expansion == ItemExpansion::Single && hosts.selection == Selection::All {
                        ensure(
                            project.roles[&hosts.role].cardinality == Cardinality::One,
                            "single_requires_one_host",
                            "single value needs exactly one host selector",
                        )?;
                    }
                    if *expansion == ItemExpansion::WidgetPerHost {
                        ensure(
                            hosts.selection == Selection::All,
                            "repeat_requires_all",
                            "widget_per_host requires all selection",
                        )?;
                    }
                }

                if let Layout::Fixed { x, y, w, h } = widget.layout() {
                    let right = u32::from(*x) + u32::from(*w);
                    let bottom = u32::from(*y) + u32::from(*h);
                    ensure(
                        right <= 72 && bottom <= 64,
                        "layout_bounds",
                        "fixed widget is outside 72x64",
                    )?;
                    let current = (u32::from(*x), u32::from(*y), right, bottom);
                    for previous in &rectangles {
                        ensure(
                            !overlaps(current, *previous),
                            "layout_overlap",
                            "fixed widgets overlap",
                        )?;
                    }
                    rectangles.push(current);
                }
            }
        }
    }
    Ok(())
}

pub fn validate_typed_project(
    project: &TypedProject,
    catalogs: &BTreeMap<String, MetricCatalog>,
) -> Result<()> {
    project.validate()?;
    let dashboards = match &project.root {
        ProjectRoot::Dashboard(dashboard) => vec![dashboard.clone()],
        ProjectRoot::Page(page) => vec![Dashboard {
            id: "page-root".to_owned(),
            name: "Page root".to_owned(),
            kind: DashboardKind::Global,
            pages: vec![page.clone()],
        }],
    };
    let compatibility_view = PortableProject {
        format: ProjectFormat::V1,
        project_id: project.project_id.clone(),
        catalogs: project.catalogs.clone(),
        roles: project.roles.clone(),
        dashboards,
    };
    validate_project_content(&compatibility_view, catalogs)
}

fn validate_host_relationship(
    roles: &BTreeMap<String, super::Role>,
    role_name: &str,
    selection: Selection,
    slot_id: Option<&str>,
) -> Result<()> {
    let role = roles.get(role_name).ok_or_else(|| {
        FormatError::validation("unknown_role", format!("unknown role {role_name}"))
    })?;
    if selection == Selection::One {
        ensure(
            role.cardinality == Cardinality::One,
            "one_requires_singleton",
            "one selector requires one cardinality; use a named slot for many",
        )?;
    }
    if selection == Selection::Slot {
        let slot = slot_id.ok_or_else(|| {
            FormatError::validation("slot_required", "slot selection requires slot_id")
        })?;
        ensure(
            role.slots
                .as_ref()
                .is_some_and(|slots| slots.iter().any(|item| item == slot)),
            "unknown_role_slot",
            format!("unknown role slot {slot}"),
        )?;
    }
    Ok(())
}

/// A field entry of a generic `type: zabbix` widget holds exactly one Zabbix object,
/// so `selection: all` is only accepted when the role is statically a single host.
fn ensure_single_reference(
    roles: &BTreeMap<String, super::Role>,
    role_name: &str,
    selection: Selection,
) -> Result<()> {
    if selection == Selection::All {
        ensure(
            roles
                .get(role_name)
                .is_some_and(|role| role.cardinality == Cardinality::One),
            "widget_single_reference_multi",
            "a widget field references one object; select one host or a named slot",
        )?;
    }
    Ok(())
}

pub(crate) fn ensure(
    condition: bool,
    code: &'static str,
    message: impl Into<String>,
) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(FormatError::validation(code, message))
    }
}

pub(crate) fn ensure_slug(value: &str, label: &str) -> Result<()> {
    let mut chars = value.chars();
    let first = chars.next();
    ensure(
        (2..=64).contains(&value.len())
            && first.is_some_and(|value| value.is_ascii_lowercase())
            && chars.all(|value| {
                value.is_ascii_lowercase() || value.is_ascii_digit() || value == '_' || value == '-'
            }),
        "invalid_slug",
        format!("{label} must match ^[a-z][a-z0-9_-]{{1,63}}$"),
    )
}

pub(crate) fn ensure_len(
    value: &str,
    minimum: usize,
    maximum: usize,
    code: &'static str,
    label: &str,
) -> Result<()> {
    let length = value.chars().count();
    ensure(
        (minimum..=maximum).contains(&length),
        code,
        format!("{label} length must be {minimum}..={maximum}"),
    )
}

pub(crate) fn ensure_unique<T>(
    values: impl IntoIterator<Item = T>,
    code: &'static str,
    label: &str,
) -> Result<()>
where
    T: Eq + Hash,
{
    let mut found = HashSet::new();
    for value in values {
        ensure(found.insert(value), code, format!("duplicate {label}"))?;
    }
    Ok(())
}

pub(crate) fn ensure_decimal_id(value: &str, label: &str) -> Result<()> {
    ensure(
        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()),
        "invalid_zabbix_id",
        format!("{label} must be a decimal string"),
    )
}

pub(crate) fn validate_version(value: &str, label: &str) -> Result<()> {
    let bytes = value.as_bytes();
    let mut cursor = 0;

    let major_start = cursor;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    let valid_major = cursor > major_start && bytes[major_start] != b'0';
    let has_separator = bytes.get(cursor) == Some(&b'.');
    cursor += usize::from(has_separator);

    let minor_start = cursor;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    let valid_minor = cursor > minor_start;

    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        let patch_start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        if cursor == patch_start {
            return Err(FormatError::validation(
                "invalid_version",
                format!("{label} is not a supported version string"),
            ));
        }
    }

    let suffix = &bytes[cursor..];
    ensure(
        valid_major
            && has_separator
            && valid_minor
            && (suffix.is_empty()
                || (suffix[0].is_ascii_alphabetic()
                    && suffix.iter().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
                    }))),
        "invalid_version",
        format!("{label} is not a supported version string"),
    )
}

pub(crate) fn validate_hex(
    value: &str,
    length: usize,
    lowercase_only: bool,
    label: &str,
) -> Result<()> {
    ensure(
        value.len() == length
            && value.bytes().all(|byte| {
                byte.is_ascii_digit()
                    || (b'a'..=b'f').contains(&byte)
                    || (!lowercase_only && (b'A'..=b'F').contains(&byte))
            }),
        "invalid_hex",
        format!("{label} must be {length} hexadecimal characters"),
    )
}

pub(crate) fn validate_color(value: &str) -> Result<()> {
    ensure(
        value.len() == 7
            && value.starts_with('#')
            && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid_color",
        "color must match #RRGGBB",
    )
}

pub(crate) fn validate_layout(layout: &Layout) -> Result<()> {
    let (w, h) = match layout {
        Layout::Auto { w, h } | Layout::Fixed { w, h, .. } => (*w, *h),
    };
    ensure(
        (1..=72).contains(&w) && (1..=64).contains(&h),
        "layout_size",
        "layout width/height are outside schema limits",
    )?;
    if let Layout::Fixed { x, y, .. } = layout {
        ensure(
            *x <= 71 && *y <= 63,
            "layout_origin",
            "fixed layout origin is outside schema limits",
        )?;
    }
    Ok(())
}

fn overlaps(left: (u32, u32, u32, u32), right: (u32, u32, u32, u32)) -> bool {
    left.0 < right.2 && right.0 < left.2 && left.1 < right.3 && right.1 < left.3
}

#[cfg(test)]
mod tests {
    use super::validate_version;

    #[test]
    fn version_matches_the_v1_schema_pattern() {
        for valid in ["5.0", "7.4.3", "8.0.0alpha1", "10.12beta-1"] {
            assert!(validate_version(valid, "version").is_ok(), "{valid}");
        }
        for invalid in [
            "7",
            "",
            "v7.4",
            "7.x",
            "0.0",
            "7.4.3.1",
            "7.4;drop",
            "7.4.3_extra",
        ] {
            assert!(validate_version(invalid, "version").is_err(), "{invalid}");
        }
    }
}
