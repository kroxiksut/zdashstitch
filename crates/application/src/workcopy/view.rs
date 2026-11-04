// SPDX-License-Identifier: MIT OR Apache-2.0

//! Draft read model (P1-03H «Данные строки черновика», `ui/src/lib/shell/drafts.ts`)
//! and its materialization from Base + operations.
//!
//! Server data (dashboard, page and widget names) is returned as data and
//! never translated. Widget types are technical IDs; the UI translates them
//! through `widget.type.<id>` (O-035).

use serde_json::Value;
use zdashstitch_zabbix::{WidgetCapabilityRegistry, WidgetFieldType};

use super::WorkcopyError;
use super::formats::{DraftKind, WorkcopyFile};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DraftOrigin {
    /// «Локальная»: created here or imported from a file (P4).
    Local,
    /// «Копия с сервера»: a workcopy over a server snapshot.
    ServerCopy,
}

impl DraftOrigin {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::ServerCopy => "server_copy",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DraftLocalState {
    Saved,
    Dirty,
}

impl DraftLocalState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Saved => "saved",
            Self::Dirty => "dirty",
        }
    }
}

/// Freshness axis E of P1-01C.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DraftFreshnessState {
    NotChecked,
    Current,
    Stale,
    Conflict,
    SourceMissing,
}

impl DraftFreshnessState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotChecked => "not_checked",
            Self::Current => "current",
            Self::Stale => "stale",
            Self::Conflict => "conflict",
            Self::SourceMissing => "source_missing",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DraftFreshness {
    pub state: DraftFreshnessState,
    /// Only for `current`: the last time the source was found unchanged.
    pub checked_at_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DraftSource {
    /// Source `dashboardid` (server ID, local use only, never displayed).
    pub dashboardid: String,
    /// Server data captured at import.
    pub dashboard_name: String,
    /// Only for page copies; server data.
    pub page_name: Option<String>,
    /// 1-based position of the source page in Base; only for page copies.
    pub page_number: Option<u32>,
    /// Connection profile ID; the HTTP layer resolves the profile name.
    pub connection_id: String,
    pub server_version: String,
    /// Time of the final consistent read of Base.
    pub snapshot_at_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DraftWidgetCounts {
    pub pages: u32,
    pub widgets: u32,
}

/// A dashboard created from this copy (O-022).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatedDashboard {
    pub connection_id: String,
    /// Server data.
    pub name: String,
    pub created_at_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DraftSummary {
    pub local_id: String,
    pub kind: DraftKind,
    pub origin: DraftOrigin,
    /// User data.
    pub name: String,
    pub source: DraftSource,
    pub freshness: DraftFreshness,
    pub local_state: DraftLocalState,
    pub updated_at_ms: u64,
    pub counts: DraftWidgetCounts,
    pub created_dashboards: Vec<CreatedDashboard>,
}

/// A workcopy that could not be read. Listed so the user sees it; the file
/// is never modified or deleted by listing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DamagedDraft {
    pub local_id: String,
    pub code: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DraftList {
    /// Newest first, then by `local_id`.
    pub drafts: Vec<DraftSummary>,
    pub damaged: Vec<DamagedDraft>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DraftWidget {
    /// Server `widgetid`, a stable local address of the widget in Base.
    pub id: String,
    pub name: String,
    pub widget_type: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// The capability registry (P0-09) knows the type and every field and
    /// allows editing; `false` means «Editing not supported».
    pub editable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DraftPage {
    /// Server `dashboard_pageid`.
    pub id: String,
    pub name: String,
    /// Seconds; `None` means the dashboard default.
    pub display_period: Option<u32>,
    pub widgets: Vec<DraftWidget>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DashboardProperties {
    pub display_period: u32,
    pub auto_start: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DraftDetail {
    pub summary: DraftSummary,
    pub pages: Vec<DraftPage>,
    /// Only for dashboard drafts.
    pub properties: Option<DashboardProperties>,
}

pub(crate) fn summary(file: &WorkcopyFile) -> DraftSummary {
    DraftSummary {
        local_id: file.workcopy_id.clone(),
        kind: file.kind,
        origin: DraftOrigin::ServerCopy,
        name: file.name.clone(),
        source: DraftSource {
            dashboardid: file.source.dashboardid.clone(),
            dashboard_name: file.source.dashboard_name.clone(),
            page_name: file.source.page.as_ref().map(|page| page.page_name.clone()),
            page_number: file.source.page.as_ref().map(|page| page.page_number),
            connection_id: file.source.connection_id.clone(),
            server_version: file.source.server_version.clone(),
            snapshot_at_ms: file.base.read_at_ms,
        },
        freshness: DraftFreshness {
            state: DraftFreshnessState::Current,
            checked_at_ms: Some(file.base.source_checked_at_ms),
        },
        local_state: DraftLocalState::Saved,
        updated_at_ms: file.updated_at_ms,
        counts: DraftWidgetCounts {
            pages: file.base.page_count,
            widgets: file.base.widget_count,
        },
        created_dashboards: file
            .created_dashboards
            .iter()
            .map(|created| CreatedDashboard {
                connection_id: created.connection_id.clone(),
                name: created.name.clone(),
                created_at_ms: created.created_at_ms,
            })
            .collect(),
    }
}

fn number(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn small(value: Option<&Value>) -> u32 {
    number(value)
        .and_then(|number| u32::try_from(number).ok())
        .unwrap_or(0)
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Whether the registry allows editing this widget: known type with the
/// `edit` flag and every field known with its type and not fixed — the mark
/// is per field, so a simple classic graph (`itemid.#`, only resent
/// unchanged) is not editable while a `graphid` graph is.
pub(crate) fn widget_editable(widget: &Value, registry: &WidgetCapabilityRegistry) -> bool {
    let Some(capability) = widget
        .get("type")
        .and_then(Value::as_str)
        .and_then(|widget_type| registry.find(widget_type))
    else {
        return false;
    };
    capability.flags.edit
        && widget
            .get("fields")
            .and_then(Value::as_array)
            .is_some_and(|fields| {
                fields.iter().all(|field| {
                    let name = field.get("name").and_then(Value::as_str);
                    let field_type = number(field.get("type"))
                        .and_then(|number| u16::try_from(number).ok())
                        .and_then(|number| WidgetFieldType::try_from(number).ok());
                    matches!((name, field_type), (Some(name), Some(field_type))
                        if capability.field_editable(name, field_type))
                })
            })
}

pub(crate) fn page_view(page: &Value, registry: &WidgetCapabilityRegistry) -> DraftPage {
    let widgets = page
        .get("widgets")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    DraftPage {
        id: text(page, "dashboard_pageid"),
        name: text(page, "name"),
        display_period: number(page.get("display_period"))
            .and_then(|seconds| u32::try_from(seconds).ok())
            .filter(|seconds| *seconds > 0),
        widgets: widgets
            .iter()
            .map(|widget| DraftWidget {
                id: text(widget, "widgetid"),
                name: text(widget, "name"),
                widget_type: text(widget, "type"),
                x: small(widget.get("x")),
                y: small(widget.get("y")),
                width: small(widget.get("width")),
                height: small(widget.get("height")),
                editable: widget_editable(widget, registry),
            })
            .collect(),
    }
}

/// Dashboard properties as in Zabbix (P1-01B §2a): default page period and
/// «Start slideshow automatically».
pub(crate) fn dashboard_properties(raw: &Value) -> DashboardProperties {
    DashboardProperties {
        display_period: small(raw.get("display_period")),
        auto_start: number(raw.get("auto_start")) == Some(1),
    }
}

/// Position of the selected page in Base.
pub(crate) fn find_page<'a>(raw: &'a Value, dashboard_pageid: &str) -> Option<(usize, &'a Value)> {
    raw.get("pages")
        .and_then(Value::as_array)?
        .iter()
        .enumerate()
        .find(|(_, page)| {
            page.get("dashboard_pageid").and_then(Value::as_str) == Some(dashboard_pageid)
        })
}

/// Draft structure: pages, widgets and geometry of the draft scope. The
/// operations of this build (`replace_host`, P2-03) change only widget field
/// values, never structure, so the structure equals Base; field values of
/// Local come from `WorkcopyService::local_snapshot`.
pub(crate) fn materialize(
    file: &WorkcopyFile,
    raw: &Value,
    registry: &WidgetCapabilityRegistry,
) -> Result<DraftDetail, WorkcopyError> {
    let summary = summary(file);
    let all_pages = raw
        .get("pages")
        .and_then(Value::as_array)
        .ok_or_else(|| WorkcopyError::new("snapshot_corrupt"))?;
    let (pages, properties) = match (&file.kind, &file.source.page) {
        (DraftKind::Dashboard, _) => (
            all_pages
                .iter()
                .map(|page| page_view(page, registry))
                .collect(),
            Some(dashboard_properties(raw)),
        ),
        (DraftKind::Page, Some(selector)) => {
            let (_, page) = find_page(raw, &selector.dashboard_pageid)
                .ok_or_else(|| WorkcopyError::new("workcopy_corrupt"))?;
            (vec![page_view(page, registry)], None)
        }
        (DraftKind::Page, None) => return Err(WorkcopyError::new("workcopy_corrupt")),
    };
    Ok(DraftDetail {
        summary,
        pages,
        properties,
    })
}
