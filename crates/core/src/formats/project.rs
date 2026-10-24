// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{FormatError, Result, Validate};
use crate::formats::validation::{
    ensure, ensure_len, ensure_slug, ensure_unique, validate_color, validate_layout,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ProjectFormat {
    #[serde(rename = "zdashstitch.project/v1")]
    V1,
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
            Self::Graph { id, .. } | Self::ItemValue { id, .. } => id,
        }
    }

    #[must_use]
    pub const fn layout(&self) -> &Layout {
        match self {
            Self::Graph { layout, .. } | Self::ItemValue { layout, .. } => layout,
        }
    }

    pub(crate) fn references(&self) -> Vec<(&MetricReference, &HostSelection)> {
        match self {
            Self::Graph { series, .. } => series
                .iter()
                .map(|entry| (&entry.metric, &entry.hosts))
                .collect(),
            Self::ItemValue { metric, hosts, .. } => vec![(metric, hosts)],
        }
    }
}

impl Validate for PortableProject {
    fn validate(&self) -> Result<()> {
        ensure_slug(&self.project_id, "project_id")?;
        ensure(
            (1..=32).contains(&self.catalogs.len()),
            "catalog_count",
            "project catalogs must contain 1..=32 entries",
        )?;
        ensure_unique(
            self.catalogs.iter().map(|entry| entry.id.as_str()),
            "duplicate_catalog",
            "catalog id",
        )?;
        for entry in &self.catalogs {
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
            (1..=64).contains(&self.roles.len()),
            "role_count",
            "project roles must contain 1..=64 entries",
        )?;
        for (name, role) in &self.roles {
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

        ensure(
            (1..=16).contains(&self.dashboards.len()),
            "dashboard_count",
            "project dashboards must contain 1..=16 entries",
        )?;
        for dashboard in &self.dashboards {
            dashboard.validate()?;
        }
        Ok(())
    }
}

impl Validate for Dashboard {
    fn validate(&self) -> Result<()> {
        ensure_slug(&self.id, "dashboard id")?;
        ensure_len(&self.name, 1, 200, "dashboard_name", "dashboard name")?;
        ensure(
            (1..=32).contains(&self.pages.len()),
            "page_count",
            "dashboard pages must contain 1..=32 entries",
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
        }
        Ok(())
    }
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
