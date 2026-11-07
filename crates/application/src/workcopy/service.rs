// SPDX-License-Identifier: MIT OR Apache-2.0

//! `WorkcopyService`: server dashboard list (P2-05A), consistent read-only
//! import into snapshot + workcopy (P2-05B/C) and the draft read model.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use zdashstitch_zabbix::{
    DashboardListing, DependencyKind, ItemMetadata, ListedWidget, MAX_DASHBOARD_LIST_LIMIT,
    MAX_DASHBOARD_SEARCH_CHARS, ReadOnlyGateway, VersionedDashboardSnapshot,
    WidgetCapabilityRegistry, WidgetFieldType, ZabbixError, is_zabbix_id,
};

use super::formats::{
    BaseReference, DraftKind, ItemMetadataState, LOCAL_ONLY, LocalMetadata, PageSelector,
    SNAPSHOT_FORMAT, SnapshotFile, SnapshotScope, StoredItemMetadata, StoredValueMap,
    StoredValueMapping, WORKCOPY_FORMAT, WorkcopyFile, WorkcopySource,
};
use super::store::LocalStore;
use super::view::{self, DamagedDraft, DashboardProperties, DraftDetail, DraftList, DraftSummary};
use super::{WorkcopyError, is_connection_id, is_workcopy_id};
use crate::AppSettings;

/// Full read attempts of one import: the initial attempt plus one retry
/// (P2-05C). Each attempt reads the dashboard, the referenced items and the
/// dashboard again, so at most 4 `dashboard.get` calls are made.
pub const MAX_IMPORT_ATTEMPTS: u8 = 2;
/// Largest page of the server dashboard list.
pub const MAX_LIST_PAGE: usize = 200;
/// Distinct referenced items whose metadata is kept per copy (I-4).
pub const MAX_ITEM_METADATA_ITEMS: usize = 10_000;
/// Longest local draft name, in Unicode scalar values.
pub const MAX_DRAFT_NAME_CHARS: usize = 255;
/// Mappings kept per value map; longer maps are cut and marked `partial`.
const MAX_VALUE_MAPPINGS: usize = 1_000;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AccessFilter {
    /// «Все».
    #[default]
    All,
    /// «Мои»: owned by the signed-in account.
    Mine,
    /// «Общие»: Zabbix `private = 0`.
    Shared,
}

/// Query of «Панели в Zabbix» → «Панели». Paging is local: Zabbix has no
/// offset, so up to [`MAX_DASHBOARD_LIST_LIMIT`] matching dashboards are
/// read and `truncated` reports more.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerDashboardQuery {
    /// Name substring, at most 255 characters, no control characters.
    pub search: Option<String>,
    pub access: AccessFilter,
    pub offset: usize,
    /// 1..=[`MAX_LIST_PAGE`].
    pub limit: usize,
}

impl Default for ServerDashboardQuery {
    fn default() -> Self {
        Self {
            search: None,
            access: AccessFilter::All,
            offset: 0,
            limit: 50,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DashboardOwner {
    /// «вы».
    Me,
    /// «другой пользователь»: Zabbix does not name other owners to
    /// non-Super admin accounts (P1-IN-08), so no name is returned.
    Other,
}

impl DashboardOwner {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Me => "me",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DashboardAccess {
    /// «Личная».
    Private,
    /// «Общая».
    Shared,
}

impl DashboardAccess {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Shared => "shared",
        }
    }
}

/// A local copy of a server dashboard («У меня»).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExistingCopy {
    pub local_id: String,
    pub kind: DraftKind,
    pub snapshot_at_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerDashboardRow {
    pub dashboardid: String,
    /// Server data.
    pub name: String,
    pub owner: DashboardOwner,
    pub access: DashboardAccess,
    pub page_count: u32,
    pub widget_count: u32,
    /// Widgets the capability registry allows ZDashstitch to edit.
    pub editable_widget_count: u32,
    /// Newest first.
    pub existing_copies: Vec<ExistingCopy>,
}

/// Matching dashboards per access filter, for «Все / Мои / Общие» counts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AccessCounts {
    pub all: usize,
    pub mine: usize,
    pub shared: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerDashboardList {
    pub rows: Vec<ServerDashboardRow>,
    /// Matching dashboards after the access filter, before paging.
    pub total: usize,
    /// Matching dashboards of the search per access filter, before paging.
    pub counts: AccessCounts,
    pub offset: usize,
    /// More dashboards matched on the server than one read returns.
    pub truncated: bool,
    /// `Some("not_visible_or_denied")` when nothing matched: Zabbix does not
    /// tell missing objects from objects hidden by permissions (P1-IN-08).
    pub empty_reason: Option<&'static str>,
    /// `false` when local storage could not be read; `existing_copies` is
    /// then empty and must not be shown as «no copy».
    pub existing_copies_known: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportScope {
    /// Copy the whole dashboard (P2-05).
    Dashboard,
    /// «Взять страницу»: Base is still the whole dashboard; the workcopy
    /// selects one page.
    Page { dashboard_pageid: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportRequest {
    pub dashboardid: String,
    pub scope: ImportScope,
    /// Local draft name chosen by the UI (P2-08A «… (копия N)»), validated
    /// like «Переименовать»; `None` keeps the source dashboard or page name.
    pub name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportOutcome {
    pub local_id: String,
    pub kind: DraftKind,
    /// User data: the stored local draft name.
    pub name: String,
    /// Full read attempts used, 1..=[`MAX_IMPORT_ATTEMPTS`].
    pub attempts: u8,
    /// Technical detail: storage fingerprint of Base (the snapshot file name).
    pub snapshot_fingerprint: String,
    /// An identical verified snapshot already existed and was reused.
    pub snapshot_reused: bool,
    pub item_metadata: ItemMetadataState,
}

/// One widget of «Состав» (P1-03C) and of the read-only server preview
/// (P2-05E), in page order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewWidget {
    /// Server `widgetid`: a key for the preview, never shown.
    pub id: String,
    /// Server data; may be empty.
    pub name: String,
    /// Technical type ID; the UI translates known types (O-035).
    pub widget_type: String,
    /// Position and size in dashboard grid cells.
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// The capability registry allows editing; otherwise copied as is.
    pub editable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewPage {
    pub dashboard_pageid: String,
    /// Server data.
    pub name: String,
    /// Seconds; `None` means the dashboard default.
    pub display_period: Option<u32>,
    pub widget_count: u32,
    pub editable_widget_count: u32,
    pub widgets: Vec<PreviewWidget>,
}

/// Distinct dependency values by kind (P1-03C «Зависимости»).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PreviewDependencies {
    pub hosts: u32,
    pub host_patterns: u32,
    pub items: u32,
    pub item_patterns: u32,
    pub graphs: u32,
    pub widget_references: u32,
    pub unknown: u32,
}

/// «Проверка перед копированием» (P1-03C). Read once; the import reads again.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportPreview {
    pub dashboardid: String,
    /// Server data.
    pub name: String,
    pub owner: DashboardOwner,
    pub access: DashboardAccess,
    pub pages: Vec<PreviewPage>,
    /// Dashboard properties for the read-only preview (P2-05E).
    pub properties: DashboardProperties,
    pub dependencies: PreviewDependencies,
    /// Widget types the registry does not know; copied as they are.
    pub unknown_widget_types: Vec<String>,
    /// Fields not known to the registry for their widget type; preserved.
    pub unknown_field_count: u32,
    pub existing_copies: Vec<ExistingCopy>,
}

/// A local draft name as stored: trimmed, 1..=[`MAX_DRAFT_NAME_CHARS`]
/// characters, no control characters.
fn draft_name(name: &str) -> Result<String, WorkcopyError> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > MAX_DRAFT_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(
            WorkcopyError::new("draft_name_invalid").with("max", MAX_DRAFT_NAME_CHARS.to_string())
        );
    }
    Ok(name.to_owned())
}

/// Application service for copies of server dashboards. Storage calls are
/// synchronous file I/O; async methods await only server reads.
#[derive(Clone, Debug)]
pub struct WorkcopyService {
    pub(crate) store: LocalStore,
    pub(crate) registry: WidgetCapabilityRegistry,
}

/// A workcopy ID with its load result; damaged files are reported, not hidden.
type LoadedWorkcopy = (String, Result<WorkcopyFile, WorkcopyError>);

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

pub(crate) fn new_workcopy_id() -> Result<String, WorkcopyError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| WorkcopyError::new("random_source_unavailable"))?;
    let mut id = String::with_capacity(35);
    id.push_str("wc-");
    for byte in bytes {
        id.push_str(&format!("{byte:02x}"));
    }
    Ok(id)
}

pub(super) fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn owner_of(listing_owner: &str, reader: &ReadOnlyGateway) -> DashboardOwner {
    if listing_owner == reader.identity().account_userid() {
        DashboardOwner::Me
    } else {
        DashboardOwner::Other
    }
}

fn access_of(private: bool) -> DashboardAccess {
    if private {
        DashboardAccess::Private
    } else {
        DashboardAccess::Shared
    }
}

fn listed_editable(widget: &ListedWidget, registry: &WidgetCapabilityRegistry) -> bool {
    registry
        .find(&widget.widget_type)
        .is_some_and(|capability| {
            capability.flags.edit
                && widget.fields.iter().all(|field| {
                    field
                        .field_type
                        .and_then(|number| WidgetFieldType::try_from(number).ok())
                        .is_some_and(|field_type| {
                            capability.field_editable(&field.name, field_type)
                        })
                })
        })
}

fn validate_ids(connection_id: &str, dashboardid: &str) -> Result<(), WorkcopyError> {
    if !is_connection_id(connection_id) {
        return Err(WorkcopyError::new("connection_id_invalid"));
    }
    if !is_zabbix_id(dashboardid) {
        return Err(WorkcopyError::new("dashboard_id_invalid"));
    }
    Ok(())
}

pub(super) fn zabbix_error(error: &ZabbixError) -> WorkcopyError {
    match error {
        ZabbixError::SnapshotShape(_) => WorkcopyError::new("snapshot_shape_invalid"),
        other => WorkcopyError::from_zabbix(other),
    }
}

impl WorkcopyService {
    /// Service over a storage root (`AppSettings::storage_root`).
    #[must_use]
    pub fn new(storage_root: PathBuf) -> Self {
        Self {
            store: LocalStore::at(storage_root),
            registry: WidgetCapabilityRegistry::v7_4(),
        }
    }

    #[must_use]
    pub fn from_settings(settings: &AppSettings) -> Self {
        Self::new(settings.storage_root.clone())
    }

    /// P2-05A: visible dashboards of the active connection.
    pub async fn list_server_dashboards(
        &self,
        reader: &ReadOnlyGateway,
        connection_id: &str,
        query: &ServerDashboardQuery,
    ) -> Result<ServerDashboardList, WorkcopyError> {
        if !is_connection_id(connection_id) {
            return Err(WorkcopyError::new("connection_id_invalid"));
        }
        if !(1..=MAX_LIST_PAGE).contains(&query.limit) {
            return Err(
                WorkcopyError::new("list_limit_invalid").with("max", MAX_LIST_PAGE.to_string())
            );
        }
        let search = query
            .search
            .as_deref()
            .map(str::trim)
            .filter(|search| !search.is_empty());
        if let Some(search) = search
            && (search.chars().count() > MAX_DASHBOARD_SEARCH_CHARS
                || search.chars().any(char::is_control))
        {
            return Err(WorkcopyError::new("search_invalid")
                .with("max", MAX_DASHBOARD_SEARCH_CHARS.to_string()));
        }
        let page = reader
            .list_dashboards(search, MAX_DASHBOARD_LIST_LIMIT)
            .await
            .map_err(|error| zabbix_error(&error))?;
        let (copies, existing_copies_known) = match self.copies_by_source() {
            Ok(copies) => (copies, true),
            Err(_) => (HashMap::new(), false),
        };
        let counts = AccessCounts {
            all: page.dashboards.len(),
            mine: page
                .dashboards
                .iter()
                .filter(|dashboard| owner_of(&dashboard.owner_userid, reader) == DashboardOwner::Me)
                .count(),
            shared: page
                .dashboards
                .iter()
                .filter(|dashboard| !dashboard.private)
                .count(),
        };
        let mut matching: Vec<&DashboardListing> = page
            .dashboards
            .iter()
            .filter(|dashboard| match query.access {
                AccessFilter::All => true,
                AccessFilter::Mine => {
                    owner_of(&dashboard.owner_userid, reader) == DashboardOwner::Me
                }
                AccessFilter::Shared => !dashboard.private,
            })
            .collect();
        matching.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.dashboardid.len().cmp(&right.dashboardid.len()))
                .then_with(|| left.dashboardid.cmp(&right.dashboardid))
        });
        let total = matching.len();
        let rows = matching
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .map(|dashboard| {
                let widgets = dashboard.pages.iter().flat_map(|page| page.widgets.iter());
                ServerDashboardRow {
                    dashboardid: dashboard.dashboardid.clone(),
                    name: dashboard.name.clone(),
                    owner: owner_of(&dashboard.owner_userid, reader),
                    access: access_of(dashboard.private),
                    page_count: count(dashboard.pages.len()),
                    widget_count: count(widgets.clone().count()),
                    editable_widget_count: count(
                        widgets
                            .filter(|widget| listed_editable(widget, &self.registry))
                            .count(),
                    ),
                    existing_copies: copies
                        .get(&(connection_id.to_owned(), dashboard.dashboardid.clone()))
                        .cloned()
                        .unwrap_or_default(),
                }
            })
            .collect();
        Ok(ServerDashboardList {
            rows,
            total,
            counts,
            offset: query.offset,
            truncated: page.truncated,
            empty_reason: (total == 0).then_some("not_visible_or_denied"),
            existing_copies_known,
        })
    }

    /// P1-03C: composition and dependencies before copying. One read; it
    /// creates nothing.
    pub async fn preview_import(
        &self,
        reader: &ReadOnlyGateway,
        connection_id: &str,
        dashboardid: &str,
    ) -> Result<ImportPreview, WorkcopyError> {
        validate_ids(connection_id, dashboardid)?;
        let snapshot = reader
            .dashboard_snapshot(connection_id, dashboardid)
            .await
            .map_err(|error| zabbix_error(&error))?
            .ok_or_else(|| WorkcopyError::new("not_visible_or_denied"))?;
        let raw = snapshot.raw_api_document();
        let analysis = snapshot.analyze().map_err(|error| zabbix_error(&error))?;
        let mut distinct: BTreeMap<DependencyKind, BTreeSet<String>> = BTreeMap::new();
        for dependency in &analysis.dependencies {
            distinct
                .entry(dependency.kind)
                .or_default()
                .insert(dependency.value.to_string());
        }
        let distinct_count = |kind| count(distinct.get(&kind).map_or(0, BTreeSet::len));
        let mut unknown_widget_types = BTreeSet::new();
        let mut unknown_field_count = 0usize;
        let pages_raw = raw
            .get("pages")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut pages = Vec::with_capacity(pages_raw.len());
        for page in pages_raw {
            let widgets = page
                .get("widgets")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            for widget in widgets {
                let widget_type = widget
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let capability = self.registry.find(widget_type);
                if capability.is_none() {
                    unknown_widget_types.insert(widget_type.to_owned());
                }
                let fields = widget
                    .get("fields")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                unknown_field_count += fields
                    .iter()
                    .filter(|field| {
                        let name = field.get("name").and_then(Value::as_str);
                        let field_type = field
                            .get("type")
                            .and_then(|value| {
                                value
                                    .as_u64()
                                    .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
                            })
                            .and_then(|number| u16::try_from(number).ok())
                            .and_then(|number| WidgetFieldType::try_from(number).ok());
                        !matches!((capability, name, field_type), (Some(capability), Some(name), Some(field_type))
                            if capability.knows_field(name, field_type))
                    })
                    .count();
            }
            // The same structure view as a draft («Структура», P2-05E).
            let page_view = view::page_view(page, &self.registry);
            let page_widgets: Vec<PreviewWidget> = page_view
                .widgets
                .into_iter()
                .map(|widget| PreviewWidget {
                    id: widget.id,
                    name: widget.name,
                    widget_type: widget.widget_type,
                    x: widget.x,
                    y: widget.y,
                    width: widget.width,
                    height: widget.height,
                    editable: widget.editable,
                })
                .collect();
            pages.push(PreviewPage {
                dashboard_pageid: page_view.id,
                name: page_view.name,
                display_period: page_view.display_period,
                widget_count: count(widgets.len()),
                editable_widget_count: count(
                    page_widgets.iter().filter(|widget| widget.editable).count(),
                ),
                widgets: page_widgets,
            });
        }
        let existing_copies = self
            .copies_by_source()
            .ok()
            .and_then(|mut copies| {
                copies.remove(&(connection_id.to_owned(), dashboardid.to_owned()))
            })
            .unwrap_or_default();
        Ok(ImportPreview {
            dashboardid: dashboardid.to_owned(),
            name: raw
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            owner: owner_of(
                raw.get("userid")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                reader,
            ),
            access: access_of(raw.get("private").and_then(Value::as_str) != Some("0")),
            pages,
            properties: view::dashboard_properties(raw),
            dependencies: PreviewDependencies {
                hosts: distinct_count(DependencyKind::Host),
                host_patterns: distinct_count(DependencyKind::HostPattern),
                items: distinct_count(DependencyKind::Item),
                item_patterns: distinct_count(DependencyKind::ItemPattern),
                graphs: distinct_count(DependencyKind::Graph),
                widget_references: distinct_count(DependencyKind::WidgetReference),
                unknown: distinct_count(DependencyKind::Unknown),
            },
            unknown_widget_types: unknown_widget_types.into_iter().collect(),
            unknown_field_count: count(unknown_field_count),
            existing_copies,
        })
    }

    /// P2-05: reads the dashboard consistently and stores an immutable
    /// snapshot (Base) and a new workcopy (Base + empty operation log).
    ///
    /// Consistency (P2-05C): each attempt reads the dashboard, then the
    /// referenced items, then the dashboard again; equal canonical
    /// fingerprints commit (the second read is stored).
    /// A difference triggers one more full attempt; a second difference ends
    /// with `import_inconsistent` and nothing is stored. Inconsistent reads
    /// are kept only as fingerprint prefixes in the error parameters.
    pub async fn import(
        &self,
        reader: &ReadOnlyGateway,
        connection_id: &str,
        request: &ImportRequest,
    ) -> Result<ImportOutcome, WorkcopyError> {
        validate_ids(connection_id, &request.dashboardid)?;
        if let ImportScope::Page { dashboard_pageid } = &request.scope
            && !is_zabbix_id(dashboard_pageid)
        {
            return Err(WorkcopyError::new("page_id_invalid"));
        }
        if let Some(name) = &request.name {
            draft_name(name)?;
        }
        let mut changes = Vec::new();
        for attempt in 1..=MAX_IMPORT_ATTEMPTS {
            let first_read_at_ms = now_ms();
            let first = self
                .read_dashboard(reader, connection_id, &request.dashboardid)
                .await?;
            let (itemids, capped) = referenced_items(&first)?;
            let items = if itemids.is_empty() {
                Ok(Vec::new())
            } else {
                reader.item_metadata(&itemids).await
            };
            let metadata = local_metadata(&itemids, capped, items)?;
            let last = reader
                .dashboard_snapshot(connection_id, &request.dashboardid)
                .await
                .map_err(|error| zabbix_error(&error))?;
            let last_read_at_ms = now_ms();
            match last {
                Some(last) if last.fingerprint() == first.fingerprint() => {
                    return self.commit(
                        connection_id,
                        request,
                        &last,
                        first_read_at_ms,
                        last_read_at_ms,
                        metadata,
                        attempt,
                    );
                }
                last => changes.push((
                    first.fingerprint()[..12].to_owned(),
                    last.map_or_else(
                        || "missing".to_owned(),
                        |snapshot| snapshot.fingerprint()[..12].to_owned(),
                    ),
                )),
            }
        }
        let mut error = WorkcopyError::new("import_inconsistent")
            .with("attempts", MAX_IMPORT_ATTEMPTS.to_string());
        for (first, last) in changes {
            error = error.with("change", format!("{first}->{last}"));
        }
        Err(error)
    }

    async fn read_dashboard(
        &self,
        reader: &ReadOnlyGateway,
        connection_id: &str,
        dashboardid: &str,
    ) -> Result<VersionedDashboardSnapshot, WorkcopyError> {
        reader
            .dashboard_snapshot(connection_id, dashboardid)
            .await
            .map_err(|error| zabbix_error(&error))?
            .ok_or_else(|| WorkcopyError::new("not_visible_or_denied"))
    }

    #[allow(clippy::too_many_arguments)]
    fn commit(
        &self,
        connection_id: &str,
        request: &ImportRequest,
        snapshot: &VersionedDashboardSnapshot,
        first_read_at_ms: u64,
        last_read_at_ms: u64,
        local_metadata: LocalMetadata,
        attempts: u8,
    ) -> Result<ImportOutcome, WorkcopyError> {
        let raw = snapshot.raw_api_document();
        let analysis = snapshot.analyze().map_err(|error| zabbix_error(&error))?;
        let (kind, page, page_count, widget_count) = match &request.scope {
            ImportScope::Dashboard => (
                DraftKind::Dashboard,
                None,
                count(analysis.page_count),
                count(analysis.widget_count),
            ),
            ImportScope::Page { dashboard_pageid } => {
                let (index, page) = view::find_page(raw, dashboard_pageid)
                    .ok_or_else(|| WorkcopyError::new("import_page_not_found"))?;
                (
                    DraftKind::Page,
                    Some(PageSelector {
                        dashboard_pageid: dashboard_pageid.clone(),
                        page_name: page
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        page_number: count(index + 1),
                    }),
                    1,
                    count(analysis.widgets_per_page.get(index).copied().unwrap_or(0)),
                )
            }
        };
        let fingerprint = snapshot.storage_fingerprint().to_owned();
        let server_version = snapshot.server_version().to_string();
        let snapshot_file = SnapshotFile {
            format: SNAPSHOT_FORMAT.to_owned(),
            storage_scope: LOCAL_ONLY.to_owned(),
            connection_id: connection_id.to_owned(),
            adapter_id: snapshot.adapter_id().to_owned(),
            server_version: server_version.clone(),
            dashboardid: request.dashboardid.clone(),
            scope: SnapshotScope::Dashboard,
            first_read_at_ms,
            last_read_at_ms,
            first_read_fingerprint: fingerprint.clone(),
            last_read_fingerprint: fingerprint.clone(),
            fingerprint: fingerprint.clone(),
            raw_api_document: raw.clone(),
        };
        let snapshot_reused = self.store.put_snapshot(&snapshot_file)?;
        let dashboard_name = raw
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let name = match &request.name {
            Some(name) => draft_name(name)?,
            None => page
                .as_ref()
                .map_or_else(|| dashboard_name.clone(), |page| page.page_name.clone()),
        };
        let item_metadata = local_metadata.items_state;
        let now = now_ms();
        let workcopy = WorkcopyFile {
            format: WORKCOPY_FORMAT.to_owned(),
            storage_scope: LOCAL_ONLY.to_owned(),
            workcopy_id: new_workcopy_id()?,
            kind,
            name,
            created_at_ms: now,
            updated_at_ms: now,
            revision: 0,
            source: WorkcopySource {
                connection_id: connection_id.to_owned(),
                dashboardid: request.dashboardid.clone(),
                dashboard_name,
                page,
                adapter_id: snapshot.adapter_id().to_owned(),
                server_version,
            },
            base: BaseReference {
                fingerprint: fingerprint.clone(),
                read_at_ms: last_read_at_ms,
                source_checked_at_ms: last_read_at_ms,
                page_count,
                widget_count,
            },
            operations: Vec::new(),
            created_dashboards: Vec::new(),
            local_metadata,
        };
        self.store.create_workcopy(&workcopy)?;
        Ok(ImportOutcome {
            local_id: workcopy.workcopy_id,
            kind,
            name: workcopy.name,
            attempts,
            snapshot_fingerprint: fingerprint,
            snapshot_reused,
            item_metadata,
        })
    }

    /// All drafts of the storage root. Unreadable workcopies are listed in
    /// `damaged` with their code and left untouched.
    pub fn list_drafts(&self) -> Result<DraftList, WorkcopyError> {
        let mut drafts = Vec::new();
        let mut damaged = Vec::new();
        for (local_id, loaded) in self.load_all()? {
            match loaded {
                Ok(file) => drafts.push(view::summary(&file)),
                Err(error) => damaged.push(DamagedDraft {
                    local_id,
                    code: error.code(),
                }),
            }
        }
        drafts.sort_by(|left, right| {
            right
                .updated_at_ms
                .cmp(&left.updated_at_ms)
                .then_with(|| left.local_id.cmp(&right.local_id))
        });
        Ok(DraftList { drafts, damaged })
    }

    /// One draft with pages and widgets materialized from the verified Base.
    pub fn draft(&self, local_id: &str) -> Result<DraftDetail, WorkcopyError> {
        let (file, snapshot) = self.load_with_base(local_id)?;
        view::materialize(&file, snapshot.raw_api_document(), &self.registry)
    }

    /// «Переименовать» (P1-03F): changes the local draft name only. Base,
    /// the operation log and the server are not touched.
    pub fn rename_draft(&self, local_id: &str, name: &str) -> Result<DraftSummary, WorkcopyError> {
        let name = draft_name(name)?;
        let mut file = self.load(local_id)?;
        file.name = name;
        file.revision = file.revision.saturating_add(1);
        file.updated_at_ms = now_ms().max(file.updated_at_ms);
        self.store.replace_workcopy(&file)?;
        Ok(view::summary(&file))
    }

    /// Local item metadata of a copy (I-4), for synthetic preview (P4) and
    /// item matching (P2-02).
    pub fn item_metadata(&self, local_id: &str) -> Result<LocalMetadata, WorkcopyError> {
        Ok(self.load(local_id)?.local_metadata)
    }

    /// The verified Base snapshot of a copy, for planning (P2-02/P2-03).
    pub fn base_snapshot(
        &self,
        local_id: &str,
    ) -> Result<VersionedDashboardSnapshot, WorkcopyError> {
        Ok(self.load_with_base(local_id)?.1)
    }

    pub(crate) fn load(&self, local_id: &str) -> Result<WorkcopyFile, WorkcopyError> {
        if !is_workcopy_id(local_id) {
            return Err(WorkcopyError::new("workcopy_id_invalid"));
        }
        self.store.load_workcopy(local_id)
    }

    pub(crate) fn load_with_base(
        &self,
        local_id: &str,
    ) -> Result<(WorkcopyFile, VersionedDashboardSnapshot), WorkcopyError> {
        let file = self.load(local_id)?;
        let (_, snapshot) = self.store.load_snapshot(
            &file.source.connection_id,
            &file.source.dashboardid,
            &file.base.fingerprint,
        )?;
        if snapshot.adapter_id() != file.source.adapter_id
            || snapshot.server_version().to_string() != file.source.server_version
        {
            return Err(WorkcopyError::new("workcopy_corrupt"));
        }
        Ok((file, snapshot))
    }

    fn load_all(&self) -> Result<Vec<LoadedWorkcopy>, WorkcopyError> {
        Ok(self
            .store
            .workcopy_ids()?
            .into_iter()
            .map(|local_id| {
                let loaded = self.store.load_workcopy(&local_id);
                (local_id, loaded)
            })
            .collect())
    }

    fn copies_by_source(
        &self,
    ) -> Result<HashMap<(String, String), Vec<ExistingCopy>>, WorkcopyError> {
        let mut copies: HashMap<(String, String), Vec<ExistingCopy>> = HashMap::new();
        for (_, loaded) in self.load_all()? {
            let Ok(file) = loaded else { continue };
            copies
                .entry((
                    file.source.connection_id.clone(),
                    file.source.dashboardid.clone(),
                ))
                .or_default()
                .push(ExistingCopy {
                    local_id: file.workcopy_id.clone(),
                    kind: file.kind,
                    snapshot_at_ms: file.base.read_at_ms,
                });
        }
        for list in copies.values_mut() {
            list.sort_by(|left, right| {
                right
                    .snapshot_at_ms
                    .cmp(&left.snapshot_at_ms)
                    .then_with(|| left.local_id.cmp(&right.local_id))
            });
        }
        Ok(copies)
    }
}

/// Distinct item IDs referenced by item fields (`ZBX_WIDGET_FIELD_TYPE_ITEM`)
/// of Base, sorted, at most [`MAX_ITEM_METADATA_ITEMS`]; `true` when capped.
pub(super) fn referenced_items(
    snapshot: &VersionedDashboardSnapshot,
) -> Result<(Vec<String>, bool), WorkcopyError> {
    let analysis = snapshot.analyze().map_err(|error| zabbix_error(&error))?;
    let distinct: BTreeSet<String> = analysis
        .dependencies
        .iter()
        .filter(|dependency| dependency.kind == DependencyKind::Item)
        .filter_map(|dependency| dependency.value.as_str())
        .filter(|itemid| is_zabbix_id(itemid) && *itemid != "0")
        .map(str::to_owned)
        .collect();
    let capped = distinct.len() > MAX_ITEM_METADATA_ITEMS;
    Ok((
        distinct.into_iter().take(MAX_ITEM_METADATA_ITEMS).collect(),
        capped,
    ))
}

/// Builds the I-4 metadata. A role without `item.get` or an unusable answer
/// leaves the copy without metadata (`unavailable`); a lost session or
/// connection fails the import.
pub(super) fn local_metadata(
    requested: &[String],
    capped: bool,
    result: Result<Vec<ItemMetadata>, ZabbixError>,
) -> Result<LocalMetadata, WorkcopyError> {
    let mut metadata = LocalMetadata {
        items_state: ItemMetadataState::NotNeeded,
        items: BTreeMap::new(),
        value_maps: BTreeMap::new(),
        missing_items: 0,
    };
    if requested.is_empty() && !capped {
        return Ok(metadata);
    }
    let items = match result {
        Ok(items) => items,
        Err(error)
            if error.is_permission_denied() || matches!(error, ZabbixError::SnapshotShape(_)) =>
        {
            metadata.items_state = ItemMetadataState::Unavailable;
            metadata.missing_items = count(requested.len());
            return Ok(metadata);
        }
        Err(error) => return Err(zabbix_error(&error)),
    };
    let requested_set: BTreeSet<&str> = requested.iter().map(String::as_str).collect();
    let mut partial = capped;
    for item in items {
        if !requested_set.contains(item.itemid.as_str()) {
            continue;
        }
        let valuemapid = item.value_map.map(|map| {
            let key = if map.valuemapid.is_empty() {
                format!("item-{}", item.itemid)
            } else {
                map.valuemapid
            };
            if map.mappings.len() > MAX_VALUE_MAPPINGS {
                partial = true;
            }
            metadata
                .value_maps
                .entry(key.clone())
                .or_insert_with(|| StoredValueMap {
                    name: map.name,
                    mappings: map
                        .mappings
                        .into_iter()
                        .take(MAX_VALUE_MAPPINGS)
                        .map(|mapping| StoredValueMapping {
                            mapping_type: mapping.mapping_type,
                            value: mapping.value,
                            newvalue: mapping.newvalue,
                        })
                        .collect(),
                });
            key
        });
        metadata.items.insert(
            item.itemid,
            StoredItemMetadata {
                value_type: item.value_type,
                units: item.units,
                valuemapid,
            },
        );
    }
    metadata.missing_items = count(requested.len().saturating_sub(metadata.items.len()));
    metadata.items_state = if partial || metadata.missing_items > 0 {
        ItemMetadataState::Partial
    } else {
        ItemMetadataState::Complete
    };
    Ok(metadata)
}
