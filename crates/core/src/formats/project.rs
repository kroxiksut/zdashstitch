// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{FormatError, Result, Validate};
use crate::formats::validation::{
    ensure, ensure_len, ensure_slug, ensure_unique, validate_color, validate_layout,
};
use crate::{Version, VersionRange};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ProjectFormat {
    #[serde(rename = "zdashstitch.project/v1")]
    V1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TypedProjectFormat {
    #[serde(rename = "zdashstitch.project/v2")]
    V2,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    Dashboard,
    Page,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields,
    from = "ProjectProvenanceInput"
)]
pub enum ProjectProvenance {
    Local,
    MigratedV1 {
        legacy_format: ProjectFormat,
        source_project_id: String,
        source_dashboard_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_page_id: Option<String>,
    },
    ServerSnapshot,
}

/// Deserialization form of [`ProjectProvenance`]. Unit variants of an
/// internally tagged enum ignore `deny_unknown_fields` (serde-rs/serde#2294),
/// so `local` and `server_snapshot` wrap an empty struct that rejects extra
/// fields. Serialization keeps using the derived `kind: local` form.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ProjectProvenanceInput {
    Local(NoFields),
    MigratedV1 {
        legacy_format: ProjectFormat,
        source_project_id: String,
        source_dashboard_id: String,
        #[serde(default)]
        source_page_id: Option<String>,
    },
    ServerSnapshot(NoFields),
}

/// Payload of a tagged unit variant: accepts no fields besides the tag.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NoFields {}

impl From<ProjectProvenanceInput> for ProjectProvenance {
    fn from(input: ProjectProvenanceInput) -> Self {
        match input {
            ProjectProvenanceInput::Local(NoFields {}) => Self::Local,
            ProjectProvenanceInput::MigratedV1 {
                legacy_format,
                source_project_id,
                source_dashboard_id,
                source_page_id,
            } => Self::MigratedV1 {
                legacy_format,
                source_project_id,
                source_dashboard_id,
                source_page_id,
            },
            ProjectProvenanceInput::ServerSnapshot(NoFields {}) => Self::ServerSnapshot,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ProjectRoot {
    Dashboard(Dashboard),
    Page(Page),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TypedProject {
    pub format: TypedProjectFormat,
    pub project_id: String,
    pub kind: ProjectKind,
    pub declared_compatibility: VersionRange,
    pub provenance: ProjectProvenance,
    pub catalogs: Vec<CatalogReference>,
    pub roles: BTreeMap<String, Role>,
    pub root: ProjectRoot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LegacyRootSelection {
    Dashboard {
        dashboard_id: String,
    },
    Page {
        dashboard_id: String,
        page_id: String,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortableProject {
    pub format: ProjectFormat,
    pub project_id: String,
    pub catalogs: Vec<CatalogReference>,
    pub roles: BTreeMap<String, Role>,
    pub dashboards: Vec<Dashboard>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogReference {
    pub id: String,
    pub file: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub cardinality: Cardinality,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cardinality {
    One,
    Many,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Dashboard {
    pub id: String,
    pub name: String,
    pub kind: DashboardKind,
    pub pages: Vec<Page>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DashboardKind {
    #[serde(rename = "global")]
    Global,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub id: String,
    pub name: String,
    pub widgets: Vec<Widget>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Widget {
    Graph {
        id: String,
        title: String,
        expansion: GraphExpansion,
        series: Vec<GraphSeries>,
        layout: Layout,
    },
    ItemValue {
        id: String,
        title: String,
        expansion: ItemExpansion,
        metric: MetricReference,
        hosts: HostSelection,
        layout: Layout,
    },
    Zabbix {
        id: String,
        widget_type: String,
        name: String,
        registry_state: RegistryState,
        fields: Vec<ZabbixWidgetField>,
        layout: Layout,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistryState {
    Known,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ZabbixWidgetField {
    pub name: String,
    pub field_type: u16,
    pub registry_state: RegistryState,
    pub value: ZabbixWidgetFieldValue,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ZabbixWidgetFieldValue {
    Integer {
        value: i64,
    },
    String {
        value: String,
    },
    Host {
        role: String,
        selection: Selection,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        slot_id: Option<String>,
    },
    Metric {
        catalog: String,
        metric: String,
        hosts: HostSelection,
    },
    Unresolved {
        object_kind: ZabbixObjectKind,
        display_name: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ZabbixObjectKind {
    HostGroup,
    Host,
    Item,
    ItemPrototype,
    Graph,
    GraphPrototype,
    Map,
    Service,
    Sla,
    User,
    Action,
    MediaType,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum GraphExpansion {
    #[serde(rename = "series_per_host")]
    SeriesPerHost,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemExpansion {
    Single,
    WidgetPerHost,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphSeries {
    pub metric: MetricReference,
    pub hosts: HostSelection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MetricReference {
    pub catalog: String,
    pub metric: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostSelection {
    pub role: String,
    pub selection: Selection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Selection {
    All,
    One,
    Slot,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Layout {
    Auto { w: u16, h: u16 },
    Fixed { x: u16, y: u16, w: u16, h: u16 },
}

impl Widget {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Graph { id, .. } | Self::ItemValue { id, .. } | Self::Zabbix { id, .. } => id,
        }
    }

    #[must_use]
    pub const fn layout(&self) -> &Layout {
        match self {
            Self::Graph { layout, .. }
            | Self::ItemValue { layout, .. }
            | Self::Zabbix { layout, .. } => layout,
        }
    }

    pub(crate) fn references(&self) -> Vec<(&MetricReference, &HostSelection)> {
        match self {
            Self::Graph { series, .. } => series
                .iter()
                .map(|entry| (&entry.metric, &entry.hosts))
                .collect(),
            Self::ItemValue { metric, hosts, .. } => vec![(metric, hosts)],
            Self::Zabbix { .. } => Vec::new(),
        }
    }
}

impl Validate for PortableProject {
    fn validate(&self) -> Result<()> {
        ensure_slug(&self.project_id, "project_id")?;
        validate_catalogs_and_roles(&self.catalogs, &self.roles, false)?;
        ensure(
            (1..=16).contains(&self.dashboards.len()),
            "dashboard_count",
            "project dashboards must contain 1..=16 entries",
        )?;
        for dashboard in &self.dashboards {
            dashboard.validate_with_page_limit(PROJECT_V1_MAX_PAGES)?;
            ensure(
                dashboard.pages.iter().all(|page| {
                    page.widgets
                        .iter()
                        .all(|widget| !matches!(widget, Widget::Zabbix { .. }))
                }),
                "legacy_widget_type",
                "project/v1 cannot contain the project/v2 generic Zabbix widget",
            )?;
        }
        Ok(())
    }
}

impl Validate for TypedProject {
    fn validate(&self) -> Result<()> {
        ensure_slug(&self.project_id, "project_id")?;
        self.declared_compatibility.validate()?;
        validate_catalogs_and_roles(&self.catalogs, &self.roles, true)?;
        match (self.kind, &self.root) {
            (ProjectKind::Dashboard, ProjectRoot::Dashboard(dashboard)) => {
                dashboard.validate_with_page_limit(PROJECT_V2_MAX_PAGES)?
            }
            (ProjectKind::Page, ProjectRoot::Page(page)) => page.validate()?,
            _ => {
                return Err(FormatError::validation(
                    "project_kind_root",
                    "project kind does not match root object",
                ));
            }
        }
        match (&self.kind, &self.provenance) {
            (
                ProjectKind::Dashboard,
                ProjectProvenance::MigratedV1 {
                    source_page_id: Some(_),
                    ..
                },
            ) => {
                return Err(FormatError::validation(
                    "dashboard_source_page",
                    "dashboard migration provenance cannot contain source_page_id",
                ));
            }
            (
                ProjectKind::Page,
                ProjectProvenance::MigratedV1 {
                    source_page_id: None,
                    ..
                },
            ) => {
                return Err(FormatError::validation(
                    "page_source_missing",
                    "page migration provenance requires source_page_id",
                ));
            }
            _ => {}
        }
        self.provenance.validate()
    }
}

impl Validate for VersionRange {
    fn validate(&self) -> Result<()> {
        ensure(
            self.is_non_empty(),
            "invalid_version_range",
            "maximum_exclusive must be greater than minimum",
        )
    }
}

impl Validate for ProjectProvenance {
    fn validate(&self) -> Result<()> {
        if let Self::MigratedV1 {
            source_project_id,
            source_dashboard_id,
            source_page_id,
            ..
        } = self
        {
            ensure_slug(source_project_id, "source project id")?;
            ensure_slug(source_dashboard_id, "source dashboard id")?;
            if let Some(page_id) = source_page_id {
                ensure_slug(page_id, "source page id")?;
            }
        }
        Ok(())
    }
}

pub fn migrate_legacy_project(
    legacy: &PortableProject,
    selection: &LegacyRootSelection,
) -> Result<TypedProject> {
    legacy.validate()?;
    let (kind, root, source_dashboard_id, source_page_id) = match selection {
        LegacyRootSelection::Dashboard { dashboard_id } => {
            let dashboard = legacy
                .dashboards
                .iter()
                .find(|dashboard| dashboard.id == *dashboard_id)
                .ok_or_else(|| {
                    FormatError::validation(
                        "legacy_root_not_found",
                        format!("legacy dashboard {dashboard_id} was not found"),
                    )
                })?
                .clone();
            (
                ProjectKind::Dashboard,
                ProjectRoot::Dashboard(dashboard),
                dashboard_id.clone(),
                None,
            )
        }
        LegacyRootSelection::Page {
            dashboard_id,
            page_id,
        } => {
            let dashboard = legacy
                .dashboards
                .iter()
                .find(|dashboard| dashboard.id == *dashboard_id)
                .ok_or_else(|| {
                    FormatError::validation(
                        "legacy_root_not_found",
                        format!("legacy dashboard {dashboard_id} was not found"),
                    )
                })?;
            let page = dashboard
                .pages
                .iter()
                .find(|page| page.id == *page_id)
                .ok_or_else(|| {
                    FormatError::validation(
                        "legacy_root_not_found",
                        format!("legacy page {dashboard_id}/{page_id} was not found"),
                    )
                })?
                .clone();
            (
                ProjectKind::Page,
                ProjectRoot::Page(page),
                dashboard_id.clone(),
                Some(page_id.clone()),
            )
        }
    };
    let migrated = TypedProject {
        format: TypedProjectFormat::V2,
        project_id: legacy.project_id.clone(),
        kind,
        declared_compatibility: VersionRange {
            minimum: "7.4.0"
                .parse::<Version>()
                .map_err(|error| FormatError::validation("invalid_version", error.to_string()))?,
            maximum_exclusive: None,
        },
        provenance: ProjectProvenance::MigratedV1 {
            legacy_format: legacy.format,
            source_project_id: legacy.project_id.clone(),
            source_dashboard_id,
            source_page_id,
        },
        catalogs: legacy.catalogs.clone(),
        roles: legacy.roles.clone(),
        root,
    };
    migrated.validate()?;
    Ok(migrated)
}

fn validate_catalogs_and_roles(
    catalogs: &[CatalogReference],
    roles: &BTreeMap<String, Role>,
    allow_empty: bool,
) -> Result<()> {
    ensure(
        catalogs.len() <= 32 && (allow_empty || !catalogs.is_empty()),
        "catalog_count",
        if allow_empty {
            "project catalogs must contain 0..=32 entries"
        } else {
            "project catalogs must contain 1..=32 entries"
        },
    )?;
    ensure_unique(
        catalogs.iter().map(|entry| entry.id.as_str()),
        "duplicate_catalog",
        "catalog id",
    )?;
    for entry in catalogs {
        ensure_slug(&entry.id, "catalog id")?;
        let catalog_name = entry.file.strip_prefix("metrics/").and_then(|name| {
            name.strip_suffix(".yaml")
                .or_else(|| name.strip_suffix(".yml"))
        });
        ensure(
            catalog_name.is_some(),
            "catalog_path",
            format!("catalog path {:?} is not canonical", entry.file),
        )?;
        ensure_slug(catalog_name.unwrap_or_default(), "catalog filename")?;
    }
    ensure(
        roles.len() <= 64 && (allow_empty || !roles.is_empty()),
        "role_count",
        if allow_empty {
            "project roles must contain 0..=64 entries"
        } else {
            "project roles must contain 1..=64 entries"
        },
    )?;
    for (name, role) in roles {
        ensure_slug(name, "role id")?;
        if let Some(description) = &role.description {
            ensure_len(description, 0, 300, "role_description", "role description")?;
        }
        if let Some(slots) = &role.slots {
            ensure(
                (1..=256).contains(&slots.len()),
                "role_slots",
                "role slots must contain 1..=256 entries",
            )?;
            for slot in slots {
                ensure_slug(slot, "role slot")?;
            }
            ensure_unique(
                slots.iter().map(String::as_str),
                "duplicate_role_slot",
                "role slot",
            )?;
        }
    }
    Ok(())
}

/// Maximum number of pages in one dashboard of the legacy `project/v1` format.
pub const PROJECT_V1_MAX_PAGES: usize = 32;

/// Maximum number of pages in the dashboard root of `project/v2`; matches the Zabbix limit
/// for one dashboard (owner decision of 2026-10-26, coherence review question 1).
pub const PROJECT_V2_MAX_PAGES: usize = 50;

impl Dashboard {
    /// Validates the dashboard with the page limit of the format that contains it.
    pub fn validate_with_page_limit(&self, max_pages: usize) -> Result<()> {
        ensure_slug(&self.id, "dashboard id")?;
        ensure_len(&self.name, 1, 200, "dashboard_name", "dashboard name")?;
        ensure(
            (1..=max_pages).contains(&self.pages.len()),
            "page_count",
            format!("dashboard pages must contain 1..={max_pages} entries"),
        )?;
        for page in &self.pages {
            page.validate()?;
        }
        Ok(())
    }
}

impl Validate for Page {
    fn validate(&self) -> Result<()> {
        ensure_slug(&self.id, "page id")?;
        ensure_len(&self.name, 1, 200, "page_name", "page name")?;
        ensure(
            self.widgets.len() <= 256,
            "widget_count",
            "a page may contain at most 256 widgets",
        )?;
        for widget in &self.widgets {
            widget.validate()?;
        }
        Ok(())
    }
}

impl Validate for Widget {
    fn validate(&self) -> Result<()> {
        ensure_slug(self.id(), "widget id")?;
        validate_layout(self.layout())?;
        match self {
            Self::Graph { title, series, .. } => {
                ensure_len(title, 1, 200, "widget_title", "widget title")?;
                ensure(
                    (1..=32).contains(&series.len()),
                    "graph_series_count",
                    "graph series must contain 1..=32 entries",
                )?;
                for entry in series {
                    entry.metric.validate()?;
                    entry.hosts.validate()?;
                    if let Some(color) = &entry.color {
                        validate_color(color)?;
                    }
                }
            }
            Self::ItemValue {
                title,
                metric,
                hosts,
                ..
            } => {
                ensure_len(title, 1, 200, "widget_title", "widget title")?;
                metric.validate()?;
                hosts.validate()?;
            }
            Self::Zabbix {
                widget_type,
                name,
                fields,
                ..
            } => {
                ensure_widget_type(widget_type)?;
                ensure_len(name, 0, 200, "widget_name", "widget name")?;
                ensure(
                    fields.len() <= 1024,
                    "widget_field_count",
                    "a widget may contain at most 1024 fields",
                )?;
                for field in fields {
                    field.validate()?;
                }
                ensure_unique(
                    fields.iter().map(|field| field.name.as_str()),
                    "duplicate_widget_field",
                    "widget field name",
                )?;
            }
        }
        Ok(())
    }
}

impl Validate for ZabbixWidgetField {
    fn validate(&self) -> Result<()> {
        ensure_len(&self.name, 1, 255, "widget_field_name", "widget field name")?;
        ensure(
            !self.name.chars().any(char::is_control),
            "widget_field_name",
            "widget field name cannot contain control characters",
        )?;
        let known_type = self.field_type <= 13;
        ensure(
            known_type || self.registry_state == RegistryState::Unknown,
            "unknown_field_type_state",
            "an unknown field type must be marked unknown",
        )?;
        match (&self.value, self.field_type) {
            (ZabbixWidgetFieldValue::Integer { .. }, 0)
            | (ZabbixWidgetFieldValue::String { .. }, 1) => {}
            (ZabbixWidgetFieldValue::Host { .. }, 2 | 3) => {}
            (ZabbixWidgetFieldValue::Metric { .. }, 4 | 5) => {}
            (ZabbixWidgetFieldValue::Unresolved { object_kind, .. }, 2..=13) => ensure(
                object_kind.field_type() == Some(self.field_type),
                "widget_field_object_kind",
                "unresolved object kind does not match field type",
            )?,
            (
                ZabbixWidgetFieldValue::Unresolved {
                    object_kind: ZabbixObjectKind::Unknown,
                    ..
                },
                14..=u16::MAX,
            ) => {}
            _ => {
                return Err(FormatError::validation(
                    "widget_field_value_kind",
                    "widget field value kind does not match field type",
                ));
            }
        }
        match &self.value {
            ZabbixWidgetFieldValue::String { value } => ensure_len(
                value,
                0,
                16_384,
                "widget_field_value",
                "widget field string value",
            ),
            ZabbixWidgetFieldValue::Host {
                role,
                selection,
                slot_id,
            } => HostSelection {
                role: role.clone(),
                selection: *selection,
                slot_id: slot_id.clone(),
            }
            .validate(),
            ZabbixWidgetFieldValue::Metric {
                catalog,
                metric,
                hosts,
            } => {
                MetricReference {
                    catalog: catalog.clone(),
                    metric: metric.clone(),
                }
                .validate()?;
                hosts.validate()
            }
            ZabbixWidgetFieldValue::Unresolved { display_name, .. } => ensure_len(
                display_name,
                0,
                500,
                "widget_field_display_name",
                "unresolved object display name",
            ),
            ZabbixWidgetFieldValue::Integer { .. } => Ok(()),
        }
    }
}

impl ZabbixObjectKind {
    const fn field_type(self) -> Option<u16> {
        match self {
            Self::HostGroup => Some(2),
            Self::Host => Some(3),
            Self::Item => Some(4),
            Self::ItemPrototype => Some(5),
            Self::Graph => Some(6),
            Self::GraphPrototype => Some(7),
            Self::Map => Some(8),
            Self::Service => Some(9),
            Self::Sla => Some(10),
            Self::User => Some(11),
            Self::Action => Some(12),
            Self::MediaType => Some(13),
            Self::Unknown => None,
        }
    }
}

fn ensure_widget_type(value: &str) -> Result<()> {
    ensure(
        !value.is_empty()
            && value.len() <= 128
            && value.starts_with(|character: char| character.is_ascii_lowercase())
            && value.chars().all(|character| {
                character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || matches!(character, '_' | '-' | '.')
            }),
        "widget_type",
        "widget type must be a lowercase adapter identifier",
    )
}

impl Validate for MetricReference {
    fn validate(&self) -> Result<()> {
        ensure_slug(&self.catalog, "metric catalog reference")?;
        ensure_slug(&self.metric, "metric reference")
    }
}

impl Validate for HostSelection {
    fn validate(&self) -> Result<()> {
        ensure_slug(&self.role, "host role")?;
        match (self.selection, &self.slot_id) {
            (Selection::Slot, Some(slot)) => ensure_slug(slot, "host slot"),
            (Selection::Slot, None) => Err(FormatError::validation(
                "slot_required",
                "slot selection requires slot_id",
            )),
            (Selection::All | Selection::One, None) => Ok(()),
            (Selection::All | Selection::One, Some(_)) => Err(FormatError::validation(
                "unexpected_slot",
                "slot_id is allowed only for slot selection",
            )),
        }
    }
}
