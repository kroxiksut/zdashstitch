// SPDX-License-Identifier: MIT OR Apache-2.0

//! Local page drafts composed from pinned versions of imported metric catalogs.
//! These contain portable data only; no server identity or local bindings.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use zdashstitch_core::formats::{
    Cardinality, CatalogReference, GraphExpansion, GraphSeries, HostSelection, ItemExpansion,
    Layout, MetricCatalog, MetricReference, Page, ProjectKind, ProjectProvenance, ProjectRoot,
    RegistryState, Role, Selection, TypedPortableBundle, TypedProject, TypedProjectFormat, Widget,
    validate_typed_project,
};
use zdashstitch_zabbix::{AdapterProfile, WidgetFieldType};

use crate::compatibility::project_evaluation_profile;

use crate::{
    catalog_library::CatalogLibrary,
    workcopy::{
        WorkcopyError,
        store::{LocalStore, read_bounded, write_atomic},
    },
};

const PATH: &[&str] = &["drafts", "offline-pages"];
const FILE: &str = "pages.json";
const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_PAGES: usize = 2_000;
const MAX_WIDGETS_PER_PAGE: usize = 256;
const RECOVERY_FILE: &str = "save-attempts.json";
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
thread_local! { static FAIL_AFTER_RECOVERY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogChoice {
    pub catalog_id: String,
    pub version: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WidgetChoice {
    Graph {
        #[serde(default)]
        widget_id: Option<String>,
        #[serde(default)]
        layout: Option<Layout>,
        title: String,
        metrics: Vec<SeriesChoice>,
    },
    ItemValue {
        #[serde(default)]
        widget_id: Option<String>,
        #[serde(default)]
        layout: Option<Layout>,
        title: String,
        metric: MetricChoice,
        hosts: HostSelection,
        expansion: ItemExpansion,
    },
    /// Retain opaque fields from the current saved bundle. Only geometry is editable.
    Preserved {
        widget_id: String,
        #[serde(default)]
        layout: Option<Layout>,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricChoice {
    pub catalog_id: String,
    pub metric_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeriesChoice {
    pub catalog_id: String,
    pub metric_id: String,
    pub hosts: HostSelection,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePage {
    pub name: String,
    #[serde(default)]
    pub evaluation_profile: Option<AdapterProfile>,
    pub catalogs: Vec<CatalogChoice>,
    pub roles: BTreeMap<String, Role>,
    pub widgets: Vec<WidgetChoice>,
}

impl CreatePage {
    /// Reject repeated keys before decoding role maps; there is no last-key-wins
    /// interpretation of an explicit source or slot selection.
    #[must_use]
    pub fn parse_json(raw: &[u8]) -> Option<Self> {
        let raw = std::str::from_utf8(raw).ok()?;
        zdashstitch_core::formats::strict::json_from_str(raw).ok()
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatePage {
    pub expected_revision: u64,
    #[serde(default)]
    pub confirmed_widget_removals: Vec<String>,
    pub page: CreatePage,
}

impl UpdatePage {
    #[must_use]
    pub fn parse_json(raw: &[u8]) -> Option<Self> {
        zdashstitch_core::formats::strict::json_from_str(std::str::from_utf8(raw).ok()?).ok()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflinePage {
    pub local_id: String,
    pub created_at_ms: u64,
    #[serde(default)]
    pub updated_at_ms: u64,
    #[serde(default)]
    pub revision: u64,
    pub catalog_versions: Vec<CatalogChoice>,
    #[serde(default)]
    pub embedded_catalogs: Vec<String>,
    pub bundle: TypedPortableBundle,
}

pub(crate) fn catalog_sources_valid(
    pins: &[CatalogChoice],
    embedded: &[String],
    bundle: &TypedPortableBundle,
) -> bool {
    let pinned: BTreeSet<_> = pins.iter().map(|item| item.catalog_id.as_str()).collect();
    let independent: BTreeSet<_> = embedded.iter().map(String::as_str).collect();
    let referenced: BTreeSet<_> = bundle
        .project
        .catalogs
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    pinned.len() == pins.len()
        && independent.len() == embedded.len()
        && pinned.is_disjoint(&independent)
        && pinned.union(&independent).copied().collect::<BTreeSet<_>>() == referenced
        && pins.iter().all(|item| item.version != 0)
}

#[derive(Clone, Debug, Serialize)]
pub struct OfflinePageSummary {
    pub local_id: String,
    pub name: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub catalogs: usize,
    pub widgets: usize,
}

/// A logical expansion of the declared slots, never a binding or Zabbix payload.
/// Layout and target limits must be checked again after real hosts are assigned.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LogicalComposition {
    pub slots_declared: bool,
    pub instance_count: usize,
    pub source_count: usize,
    pub truncated: bool,
    pub instances: Vec<LogicalInstance>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LogicalInstance {
    pub widget_id: String,
    pub slot_id: Option<String>,
    pub sources: Vec<LogicalSource>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LogicalSource {
    pub catalog_id: String,
    pub metric_id: String,
    pub role: String,
    pub slot_id: String,
}

#[derive(Serialize)]
pub struct OfflinePageView {
    #[serde(flatten)]
    pub page: OfflinePage,
    pub composition: LogicalComposition,
    pub widget_evaluation: OfflineWidgetEvaluation,
    pub recovery: RecoveryInfo,
}

/// Local registry recognition only. Logical composition and the authorization
/// to publish to a connected server are evaluated independently.
#[derive(Serialize)]
pub struct OfflineWidgetEvaluation {
    pub profile: Option<AdapterProfile>,
    pub unrecognized: Vec<UnrecognizedWidget>,
}

#[derive(Serialize)]
pub struct UnrecognizedWidget {
    pub widget_id: String,
    pub widget_type: String,
    pub unknown_type: bool,
    pub unknown_field_count: usize,
}

#[derive(Serialize)]
pub struct RecoveryInfo {
    pub may_have_unsaved_changes: bool,
    pub last_saved_at_ms: u64,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SaveAttempts {
    attempts: BTreeMap<String, u64>,
}

impl OfflinePage {
    pub fn view(self) -> Result<OfflinePageView, WorkcopyError> {
        let composition = logical_composition(&self.bundle.project)?;
        let widget_evaluation = evaluate_widgets(&self.bundle.project)?;
        Ok(OfflinePageView {
            recovery: RecoveryInfo {
                may_have_unsaved_changes: false,
                last_saved_at_ms: self.updated_at_ms.max(self.created_at_ms),
            },
            page: self,
            composition,
            widget_evaluation,
        })
    }
}

fn evaluate_widgets(project: &TypedProject) -> Result<OfflineWidgetEvaluation, WorkcopyError> {
    let ProjectRoot::Page(page) = &project.root else {
        return Err(corrupt());
    };
    let profile = project_evaluation_profile(project);
    let mut unrecognized = Vec::new();
    if let Some(registry) = profile.map(AdapterProfile::widget_capabilities) {
        for widget in &page.widgets {
            let (widget_type, marked_unknown, fields) = match widget {
                Widget::Graph { .. } => ("svggraph", false, &[][..]),
                Widget::ItemValue { .. } => ("item", false, &[][..]),
                Widget::Zabbix {
                    widget_type,
                    registry_state,
                    fields,
                    ..
                } => (
                    widget_type.as_str(),
                    *registry_state == RegistryState::Unknown,
                    fields.as_slice(),
                ),
            };
            let capability = registry.find(widget_type);
            let unknown_type = marked_unknown || capability.is_none();
            let unknown_field_count = capability.map_or(0, |entry| {
                fields
                    .iter()
                    .filter(|field| {
                        field.registry_state == RegistryState::Unknown
                            || !WidgetFieldType::try_from(field.field_type)
                                .is_ok_and(|kind| entry.knows_field(&field.name, kind))
                    })
                    .count()
            });
            if unknown_type || unknown_field_count > 0 {
                unrecognized.push(UnrecognizedWidget {
                    widget_id: widget.id().to_owned(),
                    widget_type: widget_type.to_owned(),
                    unknown_type,
                    unknown_field_count,
                });
            }
        }
    }
    Ok(OfflineWidgetEvaluation {
        profile,
        unrecognized,
    })
}

impl TryFrom<&OfflinePage> for OfflinePageSummary {
    type Error = WorkcopyError;

    fn try_from(page: &OfflinePage) -> Result<Self, Self::Error> {
        let ProjectRoot::Page(root) = &page.bundle.project.root else {
            return Err(corrupt());
        };
        Ok(Self {
            local_id: page.local_id.clone(),
            name: root.name.clone(),
            created_at_ms: page.created_at_ms,
            updated_at_ms: page.updated_at_ms.max(page.created_at_ms),
            catalogs: page.bundle.project.catalogs.len(),
            widgets: root.widgets.len(),
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageFile {
    format: String,
    pages: Vec<OfflinePage>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    removed_widgets: BTreeMap<String, RemovedWidgets>,
}

impl Default for PageFile {
    fn default() -> Self {
        Self {
            format: "zdashstitch.offline-pages/v0".to_owned(),
            pages: Vec::new(),
            removed_widgets: BTreeMap::new(),
        }
    }
}

pub struct OfflinePageStore {
    root: PathBuf,
}

impl OfflinePageStore {
    pub(crate) fn save_converted(
        &self,
        bundle: TypedPortableBundle,
    ) -> Result<OfflinePage, WorkcopyError> {
        if bundle.project.provenance != ProjectProvenance::ServerSnapshot {
            return Err(WorkcopyError::new("portable_conversion_unsupported_data"));
        }
        self.save_imported(bundle)
    }

    pub fn save_imported(
        &self,
        mut bundle: TypedPortableBundle,
    ) -> Result<OfflinePage, WorkcopyError> {
        let _pin_lock = crate::catalog_pin_lock::lock()?;
        let _lock = WRITE_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        let mut file = self.load()?;
        if file.pages.len() >= MAX_PAGES {
            return Err(WorkcopyError::new("storage_limit_reached"));
        }
        if !matches!(bundle.project.root, ProjectRoot::Page(_)) || bundle.validate().is_err() {
            return Err(WorkcopyError::new("portable_conversion_unsupported_data"));
        }
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| WorkcopyError::new("random_source_unavailable"))?;
        let id = format!(
            "pg-{}",
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        if file.pages.iter().any(|page| page.local_id == id) {
            return Err(WorkcopyError::new("storage_write_failed"));
        }
        bundle.project.project_id.clone_from(&id);
        let ProjectRoot::Page(root) = &mut bundle.project.root else {
            return Err(WorkcopyError::new("portable_conversion_unsupported_data"));
        };
        root.id.clone_from(&id);
        bundle
            .validate()
            .map_err(|_| WorkcopyError::new("portable_conversion_unsupported_data"))?;
        let now = now_ms();
        let embedded_catalogs = bundle.catalogs.keys().cloned().collect();
        let page = OfflinePage {
            local_id: id,
            created_at_ms: now,
            updated_at_ms: now,
            revision: 1,
            catalog_versions: Vec::new(),
            embedded_catalogs,
            bundle,
        };
        file.pages.push(page.clone());
        self.write(&file)?;
        Ok(page)
    }
    /// Fork a validated library snapshot while preserving pinned versions.
    pub(crate) fn save_library_snapshot(
        &self,
        mut bundle: TypedPortableBundle,
        catalog_versions: Vec<CatalogChoice>,
        embedded_catalogs: Vec<String>,
    ) -> Result<OfflinePage, WorkcopyError> {
        let _pin_lock = crate::catalog_pin_lock::lock()?;
        let _lock = WRITE_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        let mut file = self.load()?;
        if file.pages.len() >= MAX_PAGES {
            return Err(WorkcopyError::new("storage_limit_reached"));
        }
        let pin_ids: BTreeSet<_> = catalog_versions.iter().map(|pin| &pin.catalog_id).collect();
        let reference_ids: BTreeSet<_> = bundle
            .project
            .catalogs
            .iter()
            .map(|item| &item.id)
            .collect();
        if !matches!(bundle.project.root, ProjectRoot::Page(_))
            || pin_ids.len() != catalog_versions.len()
            || (!embedded_catalogs.is_empty()
                && !catalog_sources_valid(&catalog_versions, &embedded_catalogs, &bundle))
            || (embedded_catalogs.is_empty() && pin_ids != reference_ids)
            || catalog_versions.iter().any(|pin| pin.version == 0)
            || bundle.validate().is_err()
        {
            return Err(corrupt());
        }
        let library = CatalogLibrary::new(self.root.clone());
        for pin in &catalog_versions {
            if library.get(&pin.catalog_id, pin.version)?.is_none() {
                return Err(WorkcopyError::new("catalog_not_found"));
            }
        }
        let id = loop {
            let mut bytes = [0u8; 16];
            getrandom::fill(&mut bytes)
                .map_err(|_| WorkcopyError::new("random_source_unavailable"))?;
            let candidate = format!(
                "pg-{}",
                bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            );
            if !file.pages.iter().any(|page| page.local_id == candidate) {
                break candidate;
            }
        };
        bundle.project.project_id.clone_from(&id);
        let ProjectRoot::Page(page_root) = &mut bundle.project.root else {
            return Err(corrupt());
        };
        page_root.id.clone_from(&id);
        for widget in &mut page_root.widgets {
            let new_id = new_widget_id()?;
            match widget {
                Widget::Graph { id, .. }
                | Widget::ItemValue { id, .. }
                | Widget::Zabbix { id, .. } => *id = new_id,
            }
        }
        bundle.validate().map_err(|_| corrupt())?;
        let now = now_ms();
        let page = OfflinePage {
            local_id: id,
            created_at_ms: now,
            updated_at_ms: now,
            revision: 1,
            catalog_versions,
            embedded_catalogs,
            bundle,
        };
        file.pages.push(page.clone());
        self.write(&file)?;
        Ok(page)
    }
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn load(&self) -> Result<PageFile, WorkcopyError> {
        let store = LocalStore::at(self.root.clone());
        let Some(dir) = store.path(PATH, false)? else {
            return Ok(PageFile::default());
        };
        let Some(raw) = read_bounded(&dir, FILE, MAX_BYTES)? else {
            return Ok(PageFile::default());
        };
        let file: PageFile = serde_json::from_slice(&raw).map_err(|_| corrupt())?;
        if file.format != "zdashstitch.offline-pages/v0" || file.pages.len() > MAX_PAGES {
            return Err(corrupt());
        }
        let mut ids = BTreeSet::new();
        for page in &file.pages {
            let version_ids: BTreeSet<_> = page
                .catalog_versions
                .iter()
                .map(|choice| &choice.catalog_id)
                .collect();
            if !ids.insert(&page.local_id)
                || page.local_id != page.bundle.project.project_id
                || !matches!(page.bundle.project.root, ProjectRoot::Page(_))
                || version_ids.len() != page.catalog_versions.len()
                || !catalog_sources_valid(
                    &page.catalog_versions,
                    &page.embedded_catalogs,
                    &page.bundle,
                )
                || page
                    .catalog_versions
                    .iter()
                    .any(|choice| choice.version == 0)
                || page.bundle.validate().is_err()
                || (!page.bundle.project.roles.is_empty()
                    && page
                        .bundle
                        .project
                        .roles
                        .values()
                        .all(|role| role.slots.is_some())
                    && validate_explicit_roles(&page.bundle.project.roles).is_err())
            {
                return Err(corrupt());
            }
        }
        for (id, archived) in &file.removed_widgets {
            let Some(page) = file.pages.iter().find(|page| &page.local_id == id) else {
                return Err(corrupt());
            };
            let ProjectRoot::Page(current) = &page.bundle.project.root else {
                return Err(corrupt());
            };
            if !valid_removed_widgets(archived, current) {
                return Err(corrupt());
            }
        }
        Ok(file)
    }

    pub fn list(&self) -> Result<Vec<OfflinePage>, WorkcopyError> {
        Ok(self.load()?.pages)
    }

    pub fn summaries(&self) -> Result<Vec<OfflinePageSummary>, WorkcopyError> {
        self.load()?
            .pages
            .iter()
            .map(OfflinePageSummary::try_from)
            .collect()
    }

    pub fn get(&self, local_id: &str) -> Result<Option<OfflinePage>, WorkcopyError> {
        Ok(self
            .load()?
            .pages
            .into_iter()
            .find(|page| page.local_id == local_id))
    }

    pub fn export_package(&self, local_id: &str) -> Result<Option<Vec<u8>>, WorkcopyError> {
        self.get(local_id)?
            .map(|page| {
                zdashstitch_core::formats::build_typed_package_bytes(&page.bundle)
                    .map_err(|_| WorkcopyError::new("offline_page_export_failed"))
            })
            .transpose()
    }

    pub fn export_yaml(&self, local_id: &str) -> Result<Option<Vec<u8>>, WorkcopyError> {
        self.get(local_id)?
            .map(|page| {
                zdashstitch_core::formats::build_typed_yaml_bytes(&page.bundle)
                    .map_err(|_| WorkcopyError::new("offline_page_export_failed"))
            })
            .transpose()
    }

    pub fn open(&self, local_id: &str) -> Result<Option<OfflinePageView>, WorkcopyError> {
        let Some(page) = self.get(local_id)? else {
            return Ok(None);
        };
        let attempted = self.save_attempts()?.attempts.get(local_id).copied();
        let revision = page.revision;
        let mut view = page.view()?;
        view.recovery.may_have_unsaved_changes =
            attempted.is_some_and(|attempt| attempt >= revision);
        Ok(Some(view))
    }

    fn save_attempts(&self) -> Result<SaveAttempts, WorkcopyError> {
        let Some(dir) = LocalStore::at(self.root.clone()).path(PATH, false)? else {
            return Ok(SaveAttempts::default());
        };
        let Some(raw) = read_bounded(&dir, RECOVERY_FILE, 1024 * 1024)? else {
            return Ok(SaveAttempts::default());
        };
        let value: SaveAttempts = serde_json::from_slice(&raw).map_err(|_| recovery_corrupt())?;
        if value.attempts.len() > MAX_PAGES
            || value
                .attempts
                .keys()
                .any(|key| key.len() > 128 || !key.starts_with("pg-"))
        {
            return Err(recovery_corrupt());
        }
        Ok(value)
    }

    /// Written before replacing the page. A successful next revision makes an
    /// old marker harmless even if the process stops before any cleanup.
    fn mark_save_attempt(&self, local_id: &str, revision: u64) -> Result<(), WorkcopyError> {
        let mut attempts = self.save_attempts()?;
        attempts.attempts.insert(local_id.to_owned(), revision);
        if attempts.attempts.len() > MAX_PAGES {
            return Err(WorkcopyError::new("storage_limit_reached"));
        }
        let raw = serde_json::to_vec(&attempts).map_err(|_| recovery_corrupt())?;
        let dir = LocalStore::at(self.root.clone())
            .path(PATH, true)?
            .ok_or_else(|| WorkcopyError::new("storage_unavailable"))?;
        write_atomic(&dir, RECOVERY_FILE, &raw)
    }

    pub fn create(&self, request: CreatePage) -> Result<OfflinePage, WorkcopyError> {
        let _pin_lock = crate::catalog_pin_lock::lock()?;
        let _lock = WRITE_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        let mut file = self.load()?;
        if file.pages.len() >= MAX_PAGES {
            return Err(WorkcopyError::new("storage_limit_reached"));
        }
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| WorkcopyError::new("random_source_unavailable"))?;
        let id = format!(
            "pg-{}",
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        let catalog_versions = request.catalogs.clone();
        let bundle = compose(&self.root, id.clone(), request, None)?;
        let created_at_ms = now_ms();
        let page = OfflinePage {
            local_id: id,
            created_at_ms,
            updated_at_ms: created_at_ms,
            revision: 1,
            catalog_versions,
            embedded_catalogs: Vec::new(),
            bundle,
        };
        file.pages.push(page.clone());
        self.write(&file)?;
        Ok(page)
    }

    pub fn update(
        &self,
        local_id: &str,
        request: UpdatePage,
    ) -> Result<OfflinePage, WorkcopyError> {
        let _pin_lock = crate::catalog_pin_lock::lock()?;
        let _lock = WRITE_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        let mut file = self.load()?;
        let page = file
            .pages
            .iter_mut()
            .find(|page| page.local_id == local_id)
            .ok_or_else(|| WorkcopyError::new("offline_page_not_found"))?;
        if page.revision != request.expected_revision {
            return Err(WorkcopyError::new("offline_page_conflict"));
        }
        let expected_revision = request.expected_revision;
        let catalogs = request.page.catalogs.clone();
        let mut removed = file.removed_widgets.remove(local_id).unwrap_or_default();
        let bundle = compose_edit(
            &self.root,
            page,
            &page.bundle.project,
            request,
            &mut removed,
        )?;
        if !removed.is_empty() {
            file.removed_widgets.insert(local_id.to_owned(), removed);
        }
        page.bundle = bundle;
        page.catalog_versions = catalogs;
        page.revision = page.revision.checked_add(1).ok_or_else(corrupt)?;
        page.updated_at_ms = now_ms().max(page.updated_at_ms).max(page.created_at_ms);
        let result = page.clone();
        self.mark_save_attempt(local_id, expected_revision)?;
        #[cfg(test)]
        if FAIL_AFTER_RECOVERY.with(std::cell::Cell::get) {
            return Err(WorkcopyError::new("storage_write_failed"));
        }
        self.write(&file)?;
        Ok(result)
    }

    fn write(&self, file: &PageFile) -> Result<(), WorkcopyError> {
        let raw = serde_json::to_vec(file).map_err(|_| corrupt())?;
        if raw.len() > MAX_BYTES {
            return Err(WorkcopyError::new("storage_limit_reached"));
        }
        let dir = LocalStore::at(self.root.clone())
            .path(PATH, true)?
            .ok_or_else(|| WorkcopyError::new("storage_unavailable"))?;
        write_atomic(&dir, FILE, &raw)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

pub(crate) type RemovedWidgets = BTreeMap<String, Widget>;

pub(crate) fn valid_removed_widgets(archive: &RemovedWidgets, page: &Page) -> bool {
    !archive.is_empty()
        && archive.len() <= MAX_WIDGETS_PER_PAGE
        && archive.iter().all(|(id, widget)| {
            matches!(widget, Widget::Zabbix { .. })
                && id == widget.id()
                && !page.widgets.iter().any(|current| current.id() == id)
        })
}

pub(crate) fn compose_edit(
    root: &Path,
    existing: &OfflinePage,
    project: &TypedProject,
    request: UpdatePage,
    removed: &mut RemovedWidgets,
) -> Result<TypedPortableBundle, WorkcopyError> {
    let ProjectRoot::Page(old) = &existing.bundle.project.root else {
        return Err(corrupt());
    };
    let retained: BTreeSet<_> = request
        .page
        .widgets
        .iter()
        .filter_map(|widget| match widget {
            WidgetChoice::Preserved { widget_id, .. } => Some(widget_id.clone()),
            _ => None,
        })
        .collect();
    let omitted: Vec<_> = old
        .widgets
        .iter()
        .enumerate()
        .filter(|(_, widget)| {
            matches!(widget, Widget::Zabbix { .. }) && !retained.contains(widget.id())
        })
        .collect();
    let confirmed: BTreeSet<_> = request
        .confirmed_widget_removals
        .iter()
        .map(String::as_str)
        .collect();
    if confirmed.len() != request.confirmed_widget_removals.len()
        || omitted.len() != confirmed.len()
        || omitted
            .iter()
            .any(|(_, widget)| !confirmed.contains(widget.id()))
    {
        return Err(invalid());
    }
    if !omitted.is_empty() {
        let page_index = match &project.root {
            ProjectRoot::Page(_) => 0,
            ProjectRoot::Dashboard(dashboard) => dashboard
                .pages
                .iter()
                .position(|page| page.id == old.id)
                .ok_or_else(corrupt)?,
        };
        let removals = omitted
            .iter()
            .map(|(index, _)| (page_index, *index))
            .collect();
        let registry = project_evaluation_profile(project).map(AdapterProfile::widget_capabilities);
        if !crate::portable_redaction::can_remove_widgets(
            registry.as_ref(),
            &project.root,
            &removals,
        ) {
            return Err(WorkcopyError::new("offline_page_widget_removal_dependency"));
        }
    }
    let restored = retained.iter().any(|id| removed.contains_key(id));
    let bundle = compose_with_removed(
        root,
        existing.local_id.clone(),
        request.page,
        Some(existing),
        removed,
    )?;
    if restored {
        let ProjectRoot::Page(page) = &bundle.project.root else {
            return Err(corrupt());
        };
        crate::offline_dashboards::validate_restored_page_links(page, &project.root)?;
    }
    removed.retain(|id, _| !retained.contains(id));
    for (_, widget) in omitted {
        removed.insert(widget.id().into(), widget.clone());
    }
    if removed.len() > MAX_WIDGETS_PER_PAGE {
        return Err(WorkcopyError::new("storage_limit_reached"));
    }
    Ok(bundle)
}

pub(crate) fn compose(
    root: &Path,
    id: String,
    request: CreatePage,
    existing: Option<&OfflinePage>,
) -> Result<TypedPortableBundle, WorkcopyError> {
    compose_with_removed(root, id, request, existing, &RemovedWidgets::new())
}

fn compose_with_removed(
    root: &Path,
    id: String,
    request: CreatePage,
    existing: Option<&OfflinePage>,
    removed: &RemovedWidgets,
) -> Result<TypedPortableBundle, WorkcopyError> {
    if request.name.trim().is_empty()
        || (existing.is_none() && request.catalogs.is_empty())
        || request.catalogs.len() > 32
        || (existing.is_none() && request.widgets.is_empty())
        || request.widgets.len() > MAX_WIDGETS_PER_PAGE
    {
        return Err(invalid());
    }
    if let (Some(existing), Some(profile)) = (existing, request.evaluation_profile)
        && project_evaluation_profile(&existing.bundle.project) != Some(profile)
    {
        return Err(invalid());
    }
    let mut catalogs = BTreeMap::<String, MetricCatalog>::new();
    if let Some(page) = existing {
        for id in &page.embedded_catalogs {
            let catalog = page.bundle.catalogs.get(id).ok_or_else(corrupt)?;
            catalogs.insert(id.clone(), catalog.clone());
        }
    }
    let library = CatalogLibrary::new(root.to_path_buf());
    for choice in request.catalogs {
        if choice.version == 0 || catalogs.contains_key(&choice.catalog_id) {
            return Err(invalid());
        }
        let pinned = existing
            .filter(|page| page.catalog_versions.iter().any(|pin| pin == &choice))
            .and_then(|page| page.bundle.catalogs.get(&choice.catalog_id));
        let catalog = if let Some(catalog) = pinned {
            catalog.clone()
        } else {
            library
                .get(&choice.catalog_id, choice.version)?
                .ok_or_else(invalid)?
                .catalog
        };
        catalogs.insert(choice.catalog_id, catalog);
    }
    let references = catalogs
        .keys()
        .map(|key| CatalogReference {
            id: key.clone(),
            file: format!("metrics/{key}.yaml"),
        })
        .collect();
    if !request.roles.is_empty()
        && !existing.is_some_and(|page| page.bundle.project.roles == request.roles)
    {
        validate_explicit_roles(&request.roles)?;
    }
    let mut widgets = Vec::new();
    let mut widget_ids = BTreeSet::new();
    for (index, choice) in request.widgets.into_iter().enumerate() {
        let requested_id = match &choice {
            WidgetChoice::Graph { widget_id, .. } | WidgetChoice::ItemValue { widget_id, .. } => {
                widget_id.clone()
            }
            WidgetChoice::Preserved { widget_id, .. } => Some(widget_id.clone()),
        };
        let previous = if let Some(widget_id) = &requested_id {
            existing
                .and_then(|page| match &page.bundle.project.root {
                    ProjectRoot::Page(root) => {
                        root.widgets.iter().find(|widget| widget.id() == widget_id)
                    }
                    _ => None,
                })
                .or_else(|| removed.get(widget_id))
        } else {
            None
        };
        let widget_id = if let Some(widget_id) = requested_id {
            widget_id.clone()
        } else if existing.is_some() {
            new_widget_id()?
        } else {
            format!("widget-{index}")
        };
        if !widget_ids.insert(widget_id.clone()) {
            return Err(invalid());
        }
        let widget = match choice {
            WidgetChoice::Graph {
                title,
                metrics,
                layout,
                ..
            } => {
                if matches!(previous, Some(Widget::Zabbix { .. })) {
                    return Err(invalid());
                }
                if title.trim().is_empty() || metrics.is_empty() || metrics.len() > 32 {
                    return Err(invalid());
                }
                Widget::Graph {
                    id: widget_id,
                    title,
                    expansion: GraphExpansion::SeriesPerHost,
                    series: metrics
                        .into_iter()
                        .map(|metric| GraphSeries {
                            metric: MetricReference {
                                catalog: metric.catalog_id,
                                metric: metric.metric_id,
                            },
                            hosts: metric.hosts,
                            color: metric.color,
                        })
                        .collect(),
                    layout: layout.unwrap_or_else(|| {
                        previous.map_or(Layout::Auto { w: 36, h: 5 }, |widget| {
                            widget.layout().clone()
                        })
                    }),
                }
            }
            WidgetChoice::ItemValue {
                title,
                metric,
                hosts,
                expansion,
                layout,
                ..
            } => {
                if matches!(previous, Some(Widget::Zabbix { .. })) {
                    return Err(invalid());
                }
                if title.trim().is_empty() {
                    return Err(invalid());
                }
                Widget::ItemValue {
                    id: widget_id,
                    title,
                    expansion,
                    metric: MetricReference {
                        catalog: metric.catalog_id,
                        metric: metric.metric_id,
                    },
                    hosts,
                    layout: layout.unwrap_or_else(|| {
                        previous.map_or(Layout::Auto { w: 12, h: 5 }, |widget| {
                            widget.layout().clone()
                        })
                    }),
                }
            }
            WidgetChoice::Preserved { layout, .. } => {
                let Some(widget @ Widget::Zabbix { .. }) = previous else {
                    return Err(invalid());
                };
                let mut retained = widget.clone();
                if let Some(layout) = layout
                    && let Widget::Zabbix { layout: target, .. } = &mut retained
                {
                    *target = layout;
                }
                retained
            }
        };
        widgets.push(widget);
    }
    let mut project = TypedProject {
        format: TypedProjectFormat::V2,
        project_id: id.clone(),
        kind: ProjectKind::Page,
        // Omitted profile is accepted for legacy local clients that created
        // 7.4 drafts before the constructor exposed a selector.
        declared_compatibility: request
            .evaluation_profile
            .unwrap_or(AdapterProfile::V7_4)
            .widget_version_range(),
        provenance: ProjectProvenance::Local,
        catalogs: references,
        roles: request.roles,
        root: ProjectRoot::Page(Page {
            id,
            name: request.name,
            widgets,
        }),
    };
    if let Some(existing) = existing {
        project.declared_compatibility = existing.bundle.project.declared_compatibility.clone();
        project.provenance = existing.bundle.project.provenance.clone();
        let ProjectRoot::Page(old_root) = &existing.bundle.project.root else {
            return Err(corrupt());
        };
        let ProjectRoot::Page(new_root) = &mut project.root else {
            return Err(corrupt());
        };
        new_root.id.clone_from(&old_root.id);
    }
    validate_typed_project(&project, &catalogs).map_err(|_| invalid())?;
    Ok(TypedPortableBundle { project, catalogs })
}

fn new_widget_id() -> Result<String, WorkcopyError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| WorkcopyError::new("random_source_unavailable"))?;
    Ok(format!(
        "widget-{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

fn validate_explicit_roles(roles: &BTreeMap<String, Role>) -> Result<(), WorkcopyError> {
    if roles.is_empty() || roles.len() > 64 {
        return Err(invalid());
    }
    let mut ids = BTreeSet::new();
    for role in roles.values() {
        let slots = role.slots.as_ref().ok_or_else(invalid)?;
        if slots.is_empty()
            || slots.len() > 256
            || (role.cardinality == Cardinality::One && slots.len() != 1)
            || slots.iter().any(|slot| !ids.insert(slot))
        {
            return Err(invalid());
        }
    }
    Ok(())
}

/// Expand only declared logical slots. No IDs, network requests or synthetic
/// values are involved. Old first-slice singleton drafts have no declared slots;
/// their view stays empty until the roles are edited in a later editor slice.
fn logical_composition(project: &TypedProject) -> Result<LogicalComposition, WorkcopyError> {
    let ProjectRoot::Page(page) = &project.root else {
        return Err(corrupt());
    };
    if project.roles.values().any(|role| role.slots.is_none()) {
        return Ok(LogicalComposition {
            slots_declared: false,
            instance_count: 0,
            source_count: 0,
            truncated: false,
            instances: Vec::new(),
        });
    }
    if !project.roles.is_empty() {
        validate_explicit_roles(&project.roles).map_err(|_| corrupt())?;
    }
    let source_slots = |hosts: &HostSelection| -> Result<Vec<String>, WorkcopyError> {
        let role = project.roles.get(&hosts.role).ok_or_else(corrupt)?;
        let slots = role.slots.as_ref().ok_or_else(corrupt)?;
        match hosts.selection {
            Selection::All => Ok(slots.clone()),
            Selection::One if role.cardinality == Cardinality::One => Ok(slots.clone()),
            Selection::Slot => hosts
                .slot_id
                .as_ref()
                .filter(|id| slots.contains(id))
                .map(|id| vec![id.clone()])
                .ok_or_else(corrupt),
            _ => Err(corrupt()),
        }
    };
    let sources = |metric: &MetricReference,
                   hosts: &HostSelection|
     -> Result<Vec<LogicalSource>, WorkcopyError> {
        Ok(source_slots(hosts)?
            .into_iter()
            .map(|slot_id| LogicalSource {
                catalog_id: metric.catalog.clone(),
                metric_id: metric.metric.clone(),
                role: hosts.role.clone(),
                slot_id,
            })
            .collect())
    };
    let mut instances = Vec::new();
    let mut instance_count = 0;
    let mut source_count = 0;
    let mut retained_sources = 0;
    let mut record = |mut instance: LogicalInstance| {
        instance_count += 1;
        source_count += instance.sources.len();
        if instances.len() < 256 {
            instance.sources.truncate(512 - retained_sources);
            retained_sources += instance.sources.len();
            instances.push(instance);
        }
    };
    for widget in &page.widgets {
        match widget {
            Widget::Graph { id, series, .. } => {
                let mut expanded = Vec::new();
                for series in series {
                    expanded.extend(sources(&series.metric, &series.hosts)?);
                }
                record(LogicalInstance {
                    widget_id: id.clone(),
                    slot_id: None,
                    sources: expanded,
                });
            }
            Widget::ItemValue {
                id,
                metric,
                hosts,
                expansion,
                ..
            } => {
                let expanded = sources(metric, hosts)?;
                if *expansion == ItemExpansion::WidgetPerHost {
                    for source in expanded {
                        record(LogicalInstance {
                            widget_id: id.clone(),
                            slot_id: Some(source.slot_id.clone()),
                            sources: vec![source],
                        });
                    }
                } else {
                    record(LogicalInstance {
                        widget_id: id.clone(),
                        slot_id: None,
                        sources: expanded,
                    });
                }
            }
            Widget::Zabbix { id, .. } => record(LogicalInstance {
                widget_id: id.clone(),
                slot_id: None,
                sources: Vec::new(),
            }),
        }
    }
    Ok(LogicalComposition {
        slots_declared: true,
        instance_count,
        source_count,
        truncated: instances.len() < instance_count || retained_sources < source_count,
        instances,
    })
}

fn invalid() -> WorkcopyError {
    WorkcopyError::new("offline_page_invalid")
}
fn corrupt() -> WorkcopyError {
    WorkcopyError::new("offline_pages_corrupt")
}
fn recovery_corrupt() -> WorkcopyError {
    WorkcopyError::new("offline_page_recovery_corrupt")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog_library::inspect_template_file;

    #[test]
    fn page_from_two_catalogs_reopens_with_pinned_metrics() {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let library = CatalogLibrary::new(root.path().to_owned());
        let raw = br#"{"zabbix_export":{"version":"7.4","templates":[
            {"template":"Synthetic A","items":[{"name":"A","key":"synthetic.a","value_type":"FLOAT"}]},
            {"template":"Synthetic B","items":[{"name":"B","key":"synthetic.b","value_type":"UNSIGNED"}]}
        ]}}"#;
        let imported = inspect_template_file("test.json", raw)
            .unwrap_or_else(|| panic!("known extension"))
            .unwrap_or_else(|error| panic!("invalid fixture: {error}"));
        let ids: Vec<_> = imported
            .iter()
            .map(|item| {
                (
                    item.catalog.catalog_id.clone(),
                    item.catalog.metrics[0].id.clone(),
                )
            })
            .collect();
        library
            .save(imported)
            .unwrap_or_else(|error| panic!("{error}"));
        let request = CreatePage {
            name: "ClickHouse and OS".to_owned(),
            evaluation_profile: None,
            catalogs: ids
                .iter()
                .map(|(id, _)| CatalogChoice {
                    catalog_id: id.clone(),
                    version: 1,
                })
                .collect(),
            widgets: vec![WidgetChoice::Graph {
                widget_id: None,
                layout: None,
                title: "Two series".to_owned(),
                metrics: ids
                    .iter()
                    .map(|(catalog_id, metric_id)| SeriesChoice {
                        color: None,
                        catalog_id: catalog_id.clone(),
                        metric_id: metric_id.clone(),
                        hosts: one_source(),
                    })
                    .collect(),
            }],
            roles: one_role(),
        };
        let store = OfflinePageStore::new(root.path().to_owned());
        let created = store
            .create(request)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(created.bundle.project.catalogs.len(), 2);
        assert_eq!(
            created.bundle.project.roles["source"].cardinality,
            Cardinality::One
        );
        let reopened = OfflinePageStore::new(root.path().to_owned())
            .get(&created.local_id)
            .unwrap_or_else(|error| panic!("{error}"));
        let package = store
            .export_package(&created.local_id)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("existing page exports"));
        let imported = zdashstitch_core::formats::read_typed_package_bytes(&package)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(imported, created.bundle);
        assert!(matches!(imported.project.root, ProjectRoot::Page(_)));
        assert!(
            store
                .export_package("missing")
                .unwrap_or_else(|error| panic!("{error}"))
                .is_none()
        );
        assert_eq!(reopened, Some(created));

        let rejected = store.create(CreatePage {
            name: "Unsafe graph".to_owned(),
            evaluation_profile: None,
            catalogs: vec![CatalogChoice {
                catalog_id: ids[0].0.clone(),
                version: 1,
            }],
            widgets: vec![WidgetChoice::Graph {
                widget_id: None,
                layout: None,
                title: "Missing metric".to_owned(),
                metrics: vec![SeriesChoice {
                    color: None,
                    catalog_id: ids[0].0.clone(),
                    metric_id: "missing".to_owned(),
                    hosts: one_source(),
                }],
            }],
            roles: one_role(),
        });
        assert_eq!(
            rejected.err().map(|error| error.code()),
            Some("offline_page_invalid")
        );
        assert_eq!(store.list().ok().map(|pages| pages.len()), Some(1));
    }

    fn one_source() -> HostSelection {
        HostSelection {
            role: "source".to_owned(),
            selection: Selection::One,
            slot_id: None,
        }
    }

    fn one_role() -> BTreeMap<String, Role> {
        BTreeMap::from([(
            "source".to_owned(),
            Role {
                cardinality: Cardinality::One,
                description: None,
                slots: Some(vec!["source-01".to_owned()]),
            },
        )])
    }

    fn fixture() -> (tempfile::TempDir, CreatePage) {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let mut imported = inspect_template_file("fixture.json", br#"{"zabbix_export":{"version":"7.4","templates":[
          {"template":"Synthetic mixed","items":[
            {"name":"Numeric","key":"synthetic.number","value_type":"FLOAT"},
            {"name":"Text","key":"synthetic.text","value_type":"TEXT"},
            {"name":"Binary","key":"synthetic.binary","value_type":"BINARY"}]},
          {"template":"Synthetic second","items":[{"name":"Second","key":"synthetic.second","value_type":"UNSIGNED"}]}
        ]}}"#).unwrap_or_else(|| panic!("extension")).unwrap_or_else(|error| panic!("{error}"));
        imported.sort_by_key(|template| std::cmp::Reverse(template.catalog.metrics.len()));
        imported[0]
            .catalog
            .metrics
            .sort_by_key(|metric| match metric.name.as_str() {
                "Numeric" => 0,
                "Text" => 1,
                _ => 2,
            });
        let catalogs = imported
            .iter()
            .map(|template| CatalogChoice {
                catalog_id: template.catalog.catalog_id.clone(),
                version: 1,
            })
            .collect();
        let metric = &imported[0].catalog.metrics[0];
        let request = CreatePage {
            name: "Local fixture".to_owned(),
            evaluation_profile: None,
            catalogs,
            roles: one_role(),
            widgets: vec![WidgetChoice::Graph {
                widget_id: None,
                layout: None,
                title: "Graph".to_owned(),
                metrics: vec![SeriesChoice {
                    color: None,
                    catalog_id: imported[0].catalog.catalog_id.clone(),
                    metric_id: metric.id.clone(),
                    hosts: one_source(),
                }],
            }],
        };
        CatalogLibrary::new(root.path().to_owned())
            .save(imported)
            .unwrap_or_else(|error| panic!("{error}"));
        (root, request)
    }

    fn many_source(selection: Selection, slot: Option<&str>) -> HostSelection {
        HostSelection {
            role: "workers".to_owned(),
            selection,
            slot_id: slot.map(str::to_owned),
        }
    }

    fn with_many(request: &mut CreatePage) {
        request.roles.insert(
            "workers".to_owned(),
            Role {
                cardinality: Cardinality::Many,
                description: None,
                slots: Some(vec!["worker-b".to_owned(), "worker-a".to_owned()]),
            },
        );
    }

    #[test]
    fn mixed_sources_repeat_in_slot_order_with_stable_instance_keys_and_reopen() {
        let (root, mut request) = fixture();
        with_many(&mut request);
        let second = CatalogLibrary::new(root.path().to_owned())
            .get(&request.catalogs[1].catalog_id, 1)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("catalog"));
        let metric = MetricChoice {
            catalog_id: second.catalog.catalog_id.clone(),
            metric_id: second.catalog.metrics[0].id.clone(),
        };
        if let WidgetChoice::Graph { metrics, .. } = &mut request.widgets[0] {
            metrics.push(SeriesChoice {
                color: None,
                catalog_id: metric.catalog_id.clone(),
                metric_id: metric.metric_id.clone(),
                hosts: many_source(Selection::All, None),
            });
        }
        request.widgets.push(WidgetChoice::ItemValue {
            widget_id: None,
            layout: None,
            title: "Named slot".to_owned(),
            metric: metric.clone(),
            hosts: many_source(Selection::Slot, Some("worker-a")),
            expansion: ItemExpansion::Single,
        });
        request.widgets.push(WidgetChoice::ItemValue {
            widget_id: None,
            layout: None,
            title: "Repeated".to_owned(),
            metric,
            hosts: many_source(Selection::All, None),
            expansion: ItemExpansion::WidgetPerHost,
        });
        let store = OfflinePageStore::new(root.path().to_owned());
        let created = store
            .create(request.clone())
            .unwrap_or_else(|error| panic!("{error}"));
        let view = created
            .clone()
            .view()
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(view.composition.instance_count, 4);
        assert_eq!(view.composition.source_count, 6);
        assert!(!view.composition.truncated);
        assert_eq!(
            view.composition.instances[0]
                .sources
                .iter()
                .map(|source| source.slot_id.as_str())
                .collect::<Vec<_>>(),
            ["source-01", "worker-b", "worker-a"]
        );
        assert_eq!(view.composition.instances[1].sources[0].slot_id, "worker-a");
        assert_eq!(
            view.composition.instances[2].slot_id.as_deref(),
            Some("worker-b")
        );
        assert_eq!(
            view.composition.instances[3].slot_id.as_deref(),
            Some("worker-a")
        );
        let mut changed = request;
        changed
            .roles
            .get_mut("workers")
            .and_then(|role| role.slots.as_mut())
            .unwrap_or_else(|| panic!("slots"))
            .reverse();
        let reordered = store
            .create(changed)
            .unwrap_or_else(|error| panic!("{error}"))
            .view()
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            reordered.composition.instances[2].slot_id.as_deref(),
            Some("worker-a")
        );
        let keys = |composition: &LogicalComposition| {
            composition
                .instances
                .iter()
                .map(|instance| (instance.widget_id.clone(), instance.slot_id.clone()))
                .collect::<BTreeSet<_>>()
        };
        assert_eq!(keys(&view.composition), keys(&reordered.composition));
        let portable =
            serde_json::to_value(&created.bundle).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            portable["project"]["root"]["widgets"]
                .as_array()
                .map(Vec::len),
            Some(3)
        );
        assert_eq!(
            portable["project"]["root"]["widgets"][2]["expansion"],
            "widget_per_host"
        );
        assert!(!portable.to_string().contains("preview_hints"));
        // The page contains catalog copies and remains readable without the library.
        std::fs::remove_file(root.path().join("library/catalogs/template-exports.json"))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            store
                .get(&created.local_id)
                .unwrap_or_else(|error| panic!("{error}")),
            Some(created)
        );
    }

    #[test]
    fn invalid_roles_selectors_and_metrics_never_create_a_page() {
        let (root, mut base) = fixture();
        with_many(&mut base);
        let store = OfflinePageStore::new(root.path().to_owned());
        let reject = |request: CreatePage| {
            assert_eq!(
                store.create(request).err().map(|error| error.code()),
                Some("offline_page_invalid")
            );
            assert!(
                store
                    .list()
                    .unwrap_or_else(|error| panic!("{error}"))
                    .is_empty()
            );
        };
        for slots in [
            None,
            Some(vec![]),
            Some(vec!["duplicate".to_owned(), "duplicate".to_owned()]),
            Some(vec!["source-01".to_owned()]),
            Some(vec!["bad slot".to_owned()]),
        ] {
            let mut request = base.clone();
            request
                .roles
                .get_mut("workers")
                .unwrap_or_else(|| panic!("role"))
                .slots = slots;
            reject(request);
        }
        let mut request = base.clone();
        request
            .roles
            .get_mut("source")
            .unwrap_or_else(|| panic!("role"))
            .slots = Some(vec!["one-a".to_owned(), "one-b".to_owned()]);
        reject(request);
        for hosts in [
            many_source(Selection::One, None),
            many_source(Selection::Slot, Some("source-01")),
            many_source(Selection::Slot, None),
            HostSelection {
                role: "missing".to_owned(),
                selection: Selection::All,
                slot_id: None,
            },
            many_source(Selection::All, Some("worker-a")),
        ] {
            let mut request = base.clone();
            if let WidgetChoice::Graph { metrics, .. } = &mut request.widgets[0] {
                metrics[0].hosts = hosts;
            }
            reject(request);
        }
        let catalog = CatalogLibrary::new(root.path().to_owned())
            .get(&base.catalogs[0].catalog_id, 1)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("catalog"));
        let metric_choice = |index: usize| MetricChoice {
            catalog_id: catalog.catalog.catalog_id.clone(),
            metric_id: catalog.catalog.metrics[index].id.clone(),
        };
        for (hosts, expansion, index) in [
            (many_source(Selection::All, None), ItemExpansion::Single, 0),
            (
                many_source(Selection::Slot, Some("worker-a")),
                ItemExpansion::WidgetPerHost,
                0,
            ),
            (one_source(), ItemExpansion::Single, 2),
        ] {
            let mut request = base.clone();
            request.widgets = vec![WidgetChoice::ItemValue {
                widget_id: None,
                layout: None,
                title: "Invalid".to_owned(),
                metric: metric_choice(index),
                hosts,
                expansion,
            }];
            reject(request);
        }
        for index in [1, 2] {
            let mut request = base.clone();
            if let WidgetChoice::Graph { metrics, .. } = &mut request.widgets[0] {
                metrics[0].metric_id = catalog.catalog.metrics[index].id.clone();
            }
            reject(request);
        }
        let mut request = base.clone();
        if let WidgetChoice::Graph { metrics, .. } = &mut request.widgets[0] {
            *metrics = vec![metrics[0].clone(); 33];
        }
        reject(request);
        let mut request = base;
        request.catalogs[0].version = 2;
        reject(request);
    }

    #[test]
    fn corrupt_dashboard_root_and_duplicate_pins_fail_closed_instead_of_panicking() {
        let (root, request) = fixture();
        let store = OfflinePageStore::new(root.path().to_owned());
        let page = store
            .create(request)
            .unwrap_or_else(|error| panic!("{error}"));
        let original = serde_json::to_value(PageFile {
            pages: vec![page],
            ..PageFile::default()
        })
        .unwrap_or_else(|error| panic!("{error}"));
        let dir = LocalStore::at(root.path().to_owned())
            .path(PATH, true)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("dir"));
        let mut wrong_root = original.clone();
        let page = wrong_root["pages"][0]["bundle"]["project"]["root"].clone();
        wrong_root["pages"][0]["bundle"]["project"]["kind"] = serde_json::json!("dashboard");
        wrong_root["pages"][0]["bundle"]["project"]["root"] = serde_json::json!({"id":"dashboard-root","name":"Wrong root","kind":"global","pages":[page]});
        let mut duplicate_pin = original.clone();
        duplicate_pin["pages"][0]["catalog_versions"][1] =
            duplicate_pin["pages"][0]["catalog_versions"][0].clone();
        for corrupt_file in [wrong_root, duplicate_pin] {
            write_atomic(
                &dir,
                FILE,
                &serde_json::to_vec(&corrupt_file).unwrap_or_else(|error| panic!("{error}")),
            )
            .unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(
                store.summaries().err().map(|error| error.code()),
                Some("offline_pages_corrupt")
            );
        }
    }

    #[test]
    fn large_logical_composition_is_bounded_with_exact_totals() {
        let (root, mut request) = fixture();
        with_many(&mut request);
        request
            .roles
            .get_mut("workers")
            .unwrap_or_else(|| panic!("role"))
            .slots = Some((0..256).map(|index| format!("slot-{index}")).collect());
        let metric = match &request.widgets[0] {
            WidgetChoice::Graph { metrics, .. } => MetricChoice {
                catalog_id: metrics[0].catalog_id.clone(),
                metric_id: metrics[0].metric_id.clone(),
            },
            _ => panic!("graph"),
        };
        request.widgets = (0..3)
            .map(|_| WidgetChoice::ItemValue {
                widget_id: None,
                layout: None,
                title: "Repeat".to_owned(),
                metric: metric.clone(),
                hosts: many_source(Selection::All, None),
                expansion: ItemExpansion::WidgetPerHost,
            })
            .collect();
        let created = OfflinePageStore::new(root.path().to_owned())
            .create(request)
            .unwrap_or_else(|error| panic!("{error}"));
        let view = created.view().unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(view.composition.instance_count, 768);
        assert_eq!(view.composition.source_count, 768);
        assert_eq!(view.composition.instances.len(), 256);
        assert!(view.composition.truncated);
    }

    fn request_for(page: &OfflinePage) -> CreatePage {
        let ProjectRoot::Page(root) = &page.bundle.project.root else {
            panic!("page");
        };
        CreatePage {
            name: root.name.clone(),
            evaluation_profile: None,
            catalogs: page.catalog_versions.clone(),
            roles: page.bundle.project.roles.clone(),
            widgets: root
                .widgets
                .iter()
                .map(|widget| match widget {
                    Widget::Graph {
                        id,
                        title,
                        series,
                        layout,
                        ..
                    } => WidgetChoice::Graph {
                        widget_id: Some(id.clone()),
                        layout: Some(layout.clone()),
                        title: title.clone(),
                        metrics: series
                            .iter()
                            .map(|series| SeriesChoice {
                                catalog_id: series.metric.catalog.clone(),
                                metric_id: series.metric.metric.clone(),
                                hosts: series.hosts.clone(),
                                color: series.color.clone(),
                            })
                            .collect(),
                    },
                    Widget::ItemValue {
                        id,
                        title,
                        metric,
                        hosts,
                        expansion,
                        layout,
                        ..
                    } => WidgetChoice::ItemValue {
                        widget_id: Some(id.clone()),
                        layout: Some(layout.clone()),
                        title: title.clone(),
                        metric: MetricChoice {
                            catalog_id: metric.catalog.clone(),
                            metric_id: metric.metric.clone(),
                        },
                        hosts: hosts.clone(),
                        expansion: *expansion,
                    },
                    Widget::Zabbix { id, layout, .. } => WidgetChoice::Preserved {
                        widget_id: id.clone(),
                        layout: Some(layout.clone()),
                    },
                })
                .collect(),
        }
    }

    #[test]
    fn one_graph_keeps_fixed_geometry_when_logical_hosts_change_after_reopen() {
        let (root, mut request) = fixture();
        with_many(&mut request);
        let expected_layout = Layout::Fixed {
            x: 5,
            y: 7,
            w: 30,
            h: 9,
        };
        if let WidgetChoice::Graph {
            metrics, layout, ..
        } = &mut request.widgets[0]
        {
            metrics[0].hosts = many_source(Selection::All, None);
            *layout = Some(expected_layout.clone());
        } else {
            panic!("graph fixture");
        }
        let store = OfflinePageStore::new(root.path().to_owned());
        let created = store
            .create(request)
            .unwrap_or_else(|error| panic!("{error}"));
        let reopened = store
            .get(&created.local_id)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("missing page"));
        let first_view = reopened
            .clone()
            .view()
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(first_view.composition.instance_count, 1);
        assert_eq!(first_view.composition.source_count, 2);
        let mut edit = request_for(&reopened);
        edit.roles
            .get_mut("workers")
            .unwrap_or_else(|| panic!("missing role"))
            .slots
            .as_mut()
            .unwrap_or_else(|| panic!("missing slots"))
            .push("worker-c".to_owned());
        let updated = store
            .update(
                &reopened.local_id,
                UpdatePage {
                    expected_revision: reopened.revision,
                    confirmed_widget_removals: vec![],
                    page: edit,
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let reopened_again = store
            .get(&created.local_id)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("missing page"));
        let view = reopened_again
            .clone()
            .view()
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(updated.revision, reopened.revision + 1);
        assert_eq!(view.composition.instance_count, 1);
        assert_eq!(view.composition.source_count, 3);
        assert_eq!(
            view.composition.instances[0]
                .sources
                .iter()
                .map(|source| source.slot_id.as_str())
                .collect::<Vec<_>>(),
            ["worker-b", "worker-a", "worker-c"]
        );
        let ProjectRoot::Page(page) = &reopened_again.bundle.project.root else {
            panic!("page");
        };
        assert_eq!(page.widgets.len(), 1);
        assert_eq!(page.widgets[0].layout(), &expected_layout);
        assert_eq!(
            page.widgets[0].id(),
            first_view.composition.instances[0].widget_id
        );
    }

    #[test]
    fn update_preserves_identity_layout_catalog_copies_and_rejects_stale_or_invalid_edits() {
        let (root, mut request) = fixture();
        with_many(&mut request);
        let store = OfflinePageStore::new(root.path().to_owned());
        let mut created = store
            .create(request)
            .unwrap_or_else(|error| panic!("{error}"));
        // A non-default layout and series color must survive a source/title edit.
        let ProjectRoot::Page(page) = &mut created.bundle.project.root else {
            panic!("page");
        };
        if let Widget::Graph { layout, series, .. } = &mut page.widgets[0] {
            *layout = Layout::Fixed {
                x: 5,
                y: 7,
                w: 30,
                h: 9,
            };
            series[0].color = Some("#102030".to_owned());
        }
        let dir = LocalStore::at(root.path().to_owned())
            .path(PATH, true)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("dir"));
        write_atomic(
            &dir,
            FILE,
            &serde_json::to_vec(&PageFile {
                pages: vec![created.clone()],
                ..PageFile::default()
            })
            .unwrap_or_else(|error| panic!("{error}")),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let mut edit = request_for(&created);
        edit.name = "Renamed locally".to_owned();
        if let WidgetChoice::Graph { metrics, title, .. } = &mut edit.widgets[0] {
            *title = "Updated graph".to_owned();
            metrics[0].hosts = many_source(Selection::Slot, Some("worker-a"));
        }
        edit.widgets.push(WidgetChoice::ItemValue {
            widget_id: None,
            layout: None,
            title: "New repeated value".to_owned(),
            metric: MetricChoice {
                catalog_id: edit.catalogs[0].catalog_id.clone(),
                metric_id: match &edit.widgets[0] {
                    WidgetChoice::Graph { metrics, .. } => metrics[0].metric_id.clone(),
                    _ => panic!("graph"),
                },
            },
            hosts: many_source(Selection::All, None),
            expansion: ItemExpansion::WidgetPerHost,
        });
        // Removing the external library proves unchanged pins use embedded catalogs.
        std::fs::remove_file(root.path().join("library/catalogs/template-exports.json"))
            .unwrap_or_else(|error| panic!("{error}"));
        let updated = store
            .update(
                &created.local_id,
                UpdatePage {
                    expected_revision: created.revision,
                    confirmed_widget_removals: vec![],
                    page: edit.clone(),
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(updated.local_id, created.local_id);
        assert_eq!(updated.created_at_ms, created.created_at_ms);
        assert_eq!(updated.revision, created.revision + 1);
        assert_eq!(updated.bundle.catalogs, created.bundle.catalogs);
        let ProjectRoot::Page(updated_root) = &updated.bundle.project.root else {
            panic!("page");
        };
        assert_eq!(updated_root.name, "Renamed locally");
        assert_eq!(updated_root.widgets[0].id(), "widget-0");
        assert_eq!(
            *updated_root.widgets[0].layout(),
            Layout::Fixed {
                x: 5,
                y: 7,
                w: 30,
                h: 9
            }
        );
        if let Widget::Graph { series, .. } = &updated_root.widgets[0] {
            assert_eq!(series[0].color.as_deref(), Some("#102030"));
        }
        assert_ne!(updated_root.widgets[1].id(), updated_root.widgets[0].id());
        assert_eq!(
            store
                .update(
                    &created.local_id,
                    UpdatePage {
                        expected_revision: created.revision,
                        confirmed_widget_removals: vec![],
                        page: edit
                    }
                )
                .err()
                .map(|error| error.code()),
            Some("offline_page_conflict")
        );
        let mut invalid = request_for(&updated);
        invalid.widgets.push(invalid.widgets[0].clone());
        assert_eq!(
            store
                .update(
                    &created.local_id,
                    UpdatePage {
                        expected_revision: updated.revision,
                        confirmed_widget_removals: vec![],
                        page: invalid
                    }
                )
                .err()
                .map(|error| error.code()),
            Some("offline_page_invalid")
        );
        assert_eq!(
            store
                .get(&created.local_id)
                .unwrap_or_else(|error| panic!("{error}")),
            Some(updated.clone())
        );
        let mut empty = request_for(&updated);
        empty.widgets.clear();
        let emptied = store
            .update(
                &updated.local_id,
                UpdatePage {
                    expected_revision: updated.revision,
                    confirmed_widget_removals: vec![],
                    page: empty,
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let ProjectRoot::Page(empty_root) = &emptied.bundle.project.root else {
            panic!("page");
        };
        assert!(empty_root.widgets.is_empty());
        assert_eq!(empty_root.id, updated_root.id);
    }

    #[test]
    fn group_edit_is_atomic_and_reopens_with_stable_ids() {
        let (root, request) = fixture();
        let store = OfflinePageStore::new(root.path().to_owned());
        let created = store
            .create(request)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut edit = request_for(&created);
        let mut copied = edit.widgets[0].clone();
        if let WidgetChoice::Graph {
            widget_id, layout, ..
        } = &mut copied
        {
            *widget_id = Some("widget-copy".to_owned());
            *layout = Some(Layout::Fixed {
                x: 40,
                y: 10,
                w: 12,
                h: 5,
            });
        }
        if let WidgetChoice::Graph { layout, .. } = &mut edit.widgets[0] {
            *layout = Some(Layout::Fixed {
                x: 0,
                y: 10,
                w: 12,
                h: 5,
            });
        }
        edit.widgets.push(copied);
        let saved = store
            .update(
                &created.local_id,
                UpdatePage {
                    expected_revision: created.revision,
                    confirmed_widget_removals: vec![],
                    page: edit.clone(),
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));
        if let WidgetChoice::Graph { layout, .. } = &mut edit.widgets[1] {
            *layout = Some(Layout::Fixed {
                x: 1,
                y: 10,
                w: 12,
                h: 5,
            });
        }
        assert!(
            store
                .update(
                    &created.local_id,
                    UpdatePage {
                        expected_revision: saved.revision,
                        confirmed_widget_removals: vec![],
                        page: edit
                    }
                )
                .is_err()
        );
        let reopened = store
            .open(&created.local_id)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("saved page"));
        assert_eq!(reopened.page.revision, saved.revision);
        assert_eq!(reopened.page.bundle, saved.bundle);
    }

    #[test]
    fn resolved_copy_uses_target_catalog_role_and_slots_without_changing_source() {
        let (root, mut request) = fixture();
        with_many(&mut request);
        if let WidgetChoice::Graph { metrics, .. } = &mut request.widgets[0] {
            metrics[0].hosts = many_source(Selection::All, None);
        }
        let store = OfflinePageStore::new(root.path().to_owned());
        let source = store
            .create(request)
            .unwrap_or_else(|error| panic!("{error}"));
        let imported = inspect_template_file("target.json", br#"{"zabbix_export":{"version":"7.4","templates":[{"template":"Synthetic target","items":[{"name":"Target metric","key":"synthetic.target","value_type":"FLOAT"}]}]}}"#)
            .unwrap_or_else(|| panic!("extension")).unwrap_or_else(|error| panic!("{error}"));
        let catalog_id = imported[0].catalog.catalog_id.clone();
        let metric_id = imported[0].catalog.metrics[0].id.clone();
        CatalogLibrary::new(root.path().to_owned())
            .save(imported)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut target_request = request_for(&source);
        target_request.name = "Target page".to_owned();
        target_request.catalogs = vec![CatalogChoice {
            catalog_id: catalog_id.clone(),
            version: 1,
        }];
        target_request.roles = BTreeMap::from([(
            "target".to_owned(),
            Role {
                cardinality: Cardinality::Many,
                description: None,
                slots: Some(vec!["target-x".to_owned(), "target-y".to_owned()]),
            },
        )]);
        target_request.widgets = vec![WidgetChoice::ItemValue {
            widget_id: None,
            layout: Some(Layout::Fixed {
                x: 50,
                y: 0,
                w: 8,
                h: 4,
            }),
            title: "Target neighbor".to_owned(),
            metric: MetricChoice {
                catalog_id: catalog_id.clone(),
                metric_id: metric_id.clone(),
            },
            hosts: HostSelection {
                role: "target".to_owned(),
                selection: Selection::Slot,
                slot_id: Some("target-x".to_owned()),
            },
            expansion: ItemExpansion::Single,
        }];
        let target = store
            .create(target_request)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut pasted = request_for(&target);
        pasted.widgets.extend(request_for(&source).widgets);
        if let WidgetChoice::Graph {
            widget_id, metrics, ..
        } = &mut pasted.widgets[1]
        {
            *widget_id = Some("copied-graph".to_owned());
            metrics[0].catalog_id = catalog_id;
            metrics[0].metric_id = metric_id;
            metrics[0].hosts.role = "target".to_owned();
        }
        let saved = store
            .update(
                &target.local_id,
                UpdatePage {
                    expected_revision: target.revision,
                    confirmed_widget_removals: vec![],
                    page: pasted,
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let reopened = store
            .open(&target.local_id)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("target"));
        assert_eq!(reopened.page.bundle, saved.bundle);
        assert_eq!(
            reopened.composition.instances[1]
                .sources
                .iter()
                .map(|source| source.slot_id.as_str())
                .collect::<Vec<_>>(),
            vec!["target-x", "target-y"]
        );
        assert_eq!(
            store
                .get(&source.local_id)
                .unwrap_or_else(|error| panic!("{error}")),
            Some(source)
        );
    }

    #[test]
    fn interrupted_update_preserves_last_saved_revision_and_reopens() {
        use crate::workcopy::store::{CrashPoint, fail};
        let (root, request) = fixture();
        let store = OfflinePageStore::new(root.path().to_owned());
        let created = store
            .create(request)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut edit = request_for(&created);
        edit.name = "Must not appear after failed save".to_owned();
        fail::AT.with(|point| point.set(Some(CrashPoint::FileRename)));
        let failed = store.update(
            &created.local_id,
            UpdatePage {
                expected_revision: created.revision,
                confirmed_widget_removals: vec![],
                page: edit.clone(),
            },
        );
        fail::AT.with(|point| point.set(None));
        assert!(failed.is_err());
        assert_eq!(
            store
                .get(&created.local_id)
                .unwrap_or_else(|error| panic!("{error}")),
            Some(created.clone())
        );
        let retried = store
            .update(
                &created.local_id,
                UpdatePage {
                    expected_revision: created.revision,
                    confirmed_widget_removals: vec![],
                    page: edit,
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(retried.revision, created.revision + 1);
        assert_eq!(
            store.list().unwrap_or_else(|error| panic!("{error}")).len(),
            1
        );
    }

    #[test]
    fn editor_undo_after_saved_delete_restores_logical_id_and_geometry() {
        let (root, request) = fixture();
        let store = OfflinePageStore::new(root.path().to_owned());
        let created = store
            .create(request)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut moved = request_for(&created);
        if let WidgetChoice::Graph { layout, .. } = &mut moved.widgets[0] {
            *layout = Some(Layout::Fixed {
                x: 6,
                y: 4,
                w: 30,
                h: 8,
            });
        }
        let moved = store
            .update(
                &created.local_id,
                UpdatePage {
                    expected_revision: created.revision,
                    confirmed_widget_removals: vec![],
                    page: moved,
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let restore = request_for(&moved);
        let mut deleted = restore.clone();
        deleted.widgets.clear();
        let deleted = store
            .update(
                &created.local_id,
                UpdatePage {
                    expected_revision: moved.revision,
                    confirmed_widget_removals: vec![],
                    page: deleted,
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let restored = store
            .update(
                &created.local_id,
                UpdatePage {
                    expected_revision: deleted.revision,
                    confirmed_widget_removals: vec![],
                    page: restore,
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(restored.bundle, moved.bundle);
        assert_eq!(
            store
                .get(&created.local_id)
                .unwrap_or_else(|error| panic!("{error}")),
            Some(restored)
        );
    }

    #[test]
    fn editor_keeps_opaque_8_0_fields_and_rejects_invalid_edits_without_a_write() {
        let (root, mut request) = fixture();
        request.evaluation_profile = Some(AdapterProfile::V8_0);
        let store = OfflinePageStore::new(root.path().to_owned());
        let mut created = store
            .create(request)
            .unwrap_or_else(|error| panic!("{error}"));
        let opaque: Widget = serde_json::from_value(serde_json::json!({
            "id":"opaque-widget", "type":"zabbix", "widget_type":"svggraph", "name":"Opaque",
            "registry_state":"known", "layout":{"mode":"fixed","x":40,"y":20,"w":12,"h":5},
            "fields":[
                {"name":"show_hostnames","field_type":0,"registry_state":"known","value":{"kind":"integer","value":1}},
                {"name":"vendor.hint","field_type":1,"registry_state":"known","value":{"kind":"string","value":"preserve exactly"}},
                {"name":"ds.0.invert_values","field_type":0,"registry_state":"unknown","value":{"kind":"integer","value":1}}
            ]
        })).unwrap_or_else(|error| panic!("{error}"));
        let ProjectRoot::Page(page) = &mut created.bundle.project.root else {
            panic!("page");
        };
        page.widgets.push(opaque.clone());
        let dir = LocalStore::at(root.path().to_owned())
            .path(PATH, true)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("dir"));
        write_atomic(
            &dir,
            FILE,
            &serde_json::to_vec(&PageFile {
                pages: vec![created.clone()],
                ..PageFile::default()
            })
            .unwrap_or_else(|error| panic!("{error}")),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let opened = store
            .open(&created.local_id)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("saved page"));
        assert_eq!(opened.widget_evaluation.profile, Some(AdapterProfile::V8_0));
        let [issue] = opened.widget_evaluation.unrecognized.as_slice() else {
            panic!("one widget with unrecognized fields");
        };
        assert_eq!(issue.widget_id, "opaque-widget");
        assert!(!issue.unknown_type);
        assert_eq!(issue.unknown_field_count, 2);

        let mut legacy = created.clone();
        legacy.bundle.project.declared_compatibility = AdapterProfile::V7_4.widget_version_range();
        let legacy_eval =
            evaluate_widgets(&legacy.bundle.project).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(legacy_eval.profile, Some(AdapterProfile::V7_4));
        assert_eq!(legacy_eval.unrecognized[0].unknown_field_count, 3);
        legacy.bundle.project.declared_compatibility.minimum =
            zdashstitch_core::Version::new(8, 1, 0);
        assert!(
            legacy
                .view()
                .unwrap_or_else(|error| panic!("{error}"))
                .widget_evaluation
                .profile
                .is_none()
        );

        let mut dropped = request_for(&created);
        dropped.widgets.pop();
        let mut switched = request_for(&created);
        switched.evaluation_profile = Some(AdapterProfile::V7_4);
        for page in [dropped, switched] {
            assert!(
                store
                    .update(
                        &created.local_id,
                        UpdatePage {
                            expected_revision: created.revision,
                            confirmed_widget_removals: vec![],
                            page,
                        }
                    )
                    .is_err()
            );
        }
        assert_eq!(
            store
                .get(&created.local_id)
                .unwrap_or_else(|error| panic!("{error}")),
            Some(created.clone())
        );

        let mut edit = request_for(&created);
        if let WidgetChoice::Preserved { layout, .. } = &mut edit.widgets[1] {
            *layout = Some(Layout::Fixed {
                x: 42,
                y: 22,
                w: 12,
                h: 5,
            });
        }
        let saved = store
            .update(
                &created.local_id,
                UpdatePage {
                    expected_revision: created.revision,
                    confirmed_widget_removals: vec![],
                    page: edit,
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let ProjectRoot::Page(root_page) = &saved.bundle.project.root else {
            panic!("page");
        };
        let (
            Widget::Zabbix { fields: before, .. },
            Widget::Zabbix {
                fields: after,
                layout,
                ..
            },
        ) = (&opaque, &root_page.widgets[1])
        else {
            panic!("opaque");
        };
        assert_eq!(before, after);
        assert_eq!(
            *layout,
            Layout::Fixed {
                x: 42,
                y: 22,
                w: 12,
                h: 5
            }
        );
        let reopened = store
            .open(&saved.local_id)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("edited page"));
        assert_eq!(&reopened.page, &saved);
        assert_eq!(
            reopened.widget_evaluation.unrecognized[0].unknown_field_count,
            2
        );
        for bad in [
            Layout::Fixed {
                x: 71,
                y: 63,
                w: 2,
                h: 2,
            },
            Layout::Fixed {
                x: 42,
                y: 22,
                w: 12,
                h: 5,
            },
        ] {
            let mut edit = request_for(&saved);
            if let WidgetChoice::Graph { layout, .. } = &mut edit.widgets[0] {
                *layout = Some(bad);
            }
            assert_eq!(
                store
                    .update(
                        &saved.local_id,
                        UpdatePage {
                            expected_revision: saved.revision,
                            confirmed_widget_removals: vec![],
                            page: edit
                        }
                    )
                    .err()
                    .map(|error| error.code()),
                Some("offline_page_invalid")
            );
        }
        assert_eq!(
            store
                .get(&saved.local_id)
                .unwrap_or_else(|error| panic!("{error}")),
            Some(saved.clone())
        );
        let mut with_uncertain_consumer = saved.clone();
        let ProjectRoot::Page(page) = &mut with_uncertain_consumer.bundle.project.root else {
            panic!("expected page");
        };
        let mut sibling = opaque;
        let Widget::Zabbix { id, layout, .. } = &mut sibling else {
            panic!("expected opaque widget");
        };
        *id = "opaque-consumer".into();
        *layout = Layout::Fixed {
            x: 58,
            y: 22,
            w: 12,
            h: 5,
        };
        page.widgets.push(sibling);
        let mut proposed = request_for(&with_uncertain_consumer);
        proposed.widgets.remove(1);
        assert_eq!(
            compose_edit(
                root.path(),
                &with_uncertain_consumer,
                &with_uncertain_consumer.bundle.project,
                UpdatePage {
                    expected_revision: saved.revision,
                    confirmed_widget_removals: vec!["opaque-widget".into()],
                    page: proposed,
                },
                &mut RemovedWidgets::new(),
            )
            .err()
            .map(|error| error.code()),
            Some("offline_page_widget_removal_dependency"),
        );
        let mut without_opaque = request_for(&saved);
        without_opaque.widgets.pop();
        let deleted = store
            .update(
                &saved.local_id,
                UpdatePage {
                    expected_revision: saved.revision,
                    confirmed_widget_removals: vec!["opaque-widget".to_owned()],
                    page: without_opaque,
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let ProjectRoot::Page(deleted_root) = &deleted.bundle.project.root else {
            panic!("expected page");
        };
        assert_eq!(deleted_root.widgets.len(), 1);
        assert!(
            !serde_json::to_string(&deleted.view().unwrap_or_else(|error| panic!("{error}")))
                .unwrap_or_else(|error| panic!("{error}"))
                .contains("opaque-widget")
        );
        let restored = OfflinePageStore::new(root.path().to_owned())
            .update(
                &saved.local_id,
                UpdatePage {
                    expected_revision: saved.revision + 1,
                    confirmed_widget_removals: vec![],
                    page: request_for(&saved),
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(restored.bundle, saved.bundle);
    }

    #[test]
    fn failed_save_marker_survives_reopen_and_successful_next_revision_resolves_it() {
        let (root, request) = fixture();
        let store = OfflinePageStore::new(root.path().to_owned());
        let created = store
            .create(request)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut edit = request_for(&created);
        edit.name = "Pending name".to_owned();
        FAIL_AFTER_RECOVERY.with(|flag| flag.set(true));
        let failed = store.update(
            &created.local_id,
            UpdatePage {
                expected_revision: created.revision,
                confirmed_widget_removals: vec![],
                page: edit.clone(),
            },
        );
        FAIL_AFTER_RECOVERY.with(|flag| flag.set(false));
        assert!(failed.is_err());
        let reopened = OfflinePageStore::new(root.path().to_owned())
            .open(&created.local_id)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("page"));
        assert!(reopened.recovery.may_have_unsaved_changes);
        assert_eq!(reopened.recovery.last_saved_at_ms, created.updated_at_ms);
        assert_eq!(reopened.page.bundle, created.bundle);
        let portable =
            serde_json::to_value(&reopened.page.bundle).unwrap_or_else(|error| panic!("{error}"));
        assert!(portable.get("recovery").is_none());
        assert!(portable.get("edit_session").is_none());
        let saved = store
            .update(
                &created.local_id,
                UpdatePage {
                    expected_revision: created.revision,
                    confirmed_widget_removals: vec![],
                    page: edit,
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let reopened = OfflinePageStore::new(root.path().to_owned())
            .open(&saved.local_id)
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("page"));
        assert!(!reopened.recovery.may_have_unsaved_changes);
        assert_eq!(reopened.page, saved);
    }
}
