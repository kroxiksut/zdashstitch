// SPDX-License-Identifier: MIT OR Apache-2.0

//! Offline dashboard drafts assembled from independent, saved local pages.
//! A page is copied at insertion time; later changes to its source do not
//! silently alter an existing dashboard.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use zdashstitch_core::formats::{
    CatalogReference, Dashboard, DashboardKind, MetricCatalog, Page, ProjectKind,
    ProjectProvenance, ProjectRoot, Role, TypedPortableBundle, TypedProject, TypedProjectFormat,
    Widget, ZabbixWidgetFieldValue, build_typed_package_bytes, validate_typed_project,
};

use crate::{
    offline_pages::{
        self, CatalogChoice, OfflinePage, OfflinePageStore, OfflinePageView, UpdatePage,
    },
    workcopy::{
        WorkcopyError,
        store::{LocalStore, read_bounded, write_atomic},
    },
};

const PATH: &[&str] = &["drafts", "offline-dashboards"];
const FILE: &str = "dashboards.json";
const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_DASHBOARDS: usize = 2_000;
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateDashboard {
    pub name: String,
    pub page_ids: Vec<String>,
}

impl CreateDashboard {
    #[must_use]
    pub fn parse_json(raw: &[u8]) -> Option<Self> {
        zdashstitch_core::formats::strict::json_from_str(std::str::from_utf8(raw).ok()?).ok()
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateDashboard {
    pub expected_revision: u64,
    pub name: String,
    pub page_order: Vec<String>,
    pub append_page_ids: Vec<String>,
}

impl UpdateDashboard {
    #[must_use]
    pub fn parse_json(raw: &[u8]) -> Option<Self> {
        zdashstitch_core::formats::strict::json_from_str(std::str::from_utf8(raw).ok()?).ok()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineDashboard {
    pub local_id: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub revision: u64,
    pub catalog_versions: Vec<CatalogChoice>,
    pub bundle: TypedPortableBundle,
}

#[derive(Clone, Debug, Serialize)]
pub struct OfflineDashboardSummary {
    pub local_id: String,
    pub name: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub pages: usize,
    pub widgets: usize,
}

impl OfflineDashboard {
    pub fn summary(&self) -> Result<OfflineDashboardSummary, WorkcopyError> {
        let ProjectRoot::Dashboard(root) = &self.bundle.project.root else {
            return Err(corrupt());
        };
        Ok(OfflineDashboardSummary {
            local_id: self.local_id.clone(),
            name: root.name.clone(),
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
            pages: root.pages.len(),
            widgets: root.pages.iter().map(|page| page.widgets.len()).sum(),
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DashboardFile {
    format: String,
    dashboards: Vec<OfflineDashboard>,
}

pub struct OfflineDashboardStore {
    root: PathBuf,
}

impl OfflineDashboardStore {
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn load(&self) -> Result<DashboardFile, WorkcopyError> {
        let Some(dir) = LocalStore::at(self.root.clone()).path(PATH, false)? else {
            return Ok(DashboardFile::default());
        };
        let Some(raw) = read_bounded(&dir, FILE, MAX_BYTES)? else {
            return Ok(DashboardFile::default());
        };
        let file: DashboardFile = serde_json::from_slice(&raw).map_err(|_| corrupt())?;
        if file.format != "zdashstitch.offline-dashboards/v0"
            || file.dashboards.len() > MAX_DASHBOARDS
        {
            return Err(corrupt());
        }
        let mut ids = BTreeSet::new();
        for dashboard in &file.dashboards {
            let pin_ids: BTreeSet<_> = dashboard
                .catalog_versions
                .iter()
                .map(|pin| &pin.catalog_id)
                .collect();
            let catalog_ids: BTreeSet<_> = dashboard
                .bundle
                .project
                .catalogs
                .iter()
                .map(|catalog| &catalog.id)
                .collect();
            if !ids.insert(&dashboard.local_id)
                || !dashboard.local_id.starts_with("db-")
                || dashboard.revision == 0
                || dashboard.local_id != dashboard.bundle.project.project_id
                || dashboard.bundle.project.kind != ProjectKind::Dashboard
                || dashboard.bundle.project.provenance != ProjectProvenance::Local
                || !matches!(&dashboard.bundle.project.root, ProjectRoot::Dashboard(root) if root.id == dashboard.local_id)
                || dashboard.bundle.validate().is_err()
                || pin_ids.len() != dashboard.catalog_versions.len()
                || pin_ids != catalog_ids
                || dashboard
                    .catalog_versions
                    .iter()
                    .any(|pin| pin.version == 0)
            {
                return Err(corrupt());
            }
        }
        Ok(file)
    }

    pub fn summaries(&self) -> Result<Vec<OfflineDashboardSummary>, WorkcopyError> {
        self.load()?
            .dashboards
            .iter()
            .map(OfflineDashboard::summary)
            .collect()
    }

    pub fn get(&self, id: &str) -> Result<Option<OfflineDashboard>, WorkcopyError> {
        Ok(self
            .load()?
            .dashboards
            .into_iter()
            .find(|item| item.local_id == id))
    }

    pub fn export_package(&self, id: &str) -> Result<Option<Vec<u8>>, WorkcopyError> {
        self.get(id)?
            .map(|dashboard| {
                build_typed_package_bytes(&dashboard.bundle)
                    .map_err(|_| WorkcopyError::new("offline_dashboard_export_failed"))
            })
            .transpose()
    }

    pub fn page(
        &self,
        dashboard_id: &str,
        page_id: &str,
    ) -> Result<Option<OfflinePageView>, WorkcopyError> {
        let Some(dashboard) = self.get(dashboard_id)? else {
            return Ok(None);
        };
        let ProjectRoot::Dashboard(root) = &dashboard.bundle.project.root else {
            return Err(corrupt());
        };
        let Some(page) = root.pages.iter().find(|page| page.id == page_id) else {
            return Ok(None);
        };
        self.page_snapshot(&dashboard, page.clone())?
            .view()
            .map(Some)
    }

    fn page_snapshot(
        &self,
        dashboard: &OfflineDashboard,
        page: Page,
    ) -> Result<OfflinePage, WorkcopyError> {
        let mut project = dashboard.bundle.project.clone();
        project.project_id.clone_from(&page.id);
        project.kind = ProjectKind::Page;
        project.root = ProjectRoot::Page(page);
        let snapshot = OfflinePage {
            local_id: project.project_id.clone(),
            created_at_ms: dashboard.created_at_ms,
            updated_at_ms: dashboard.updated_at_ms,
            revision: dashboard.revision,
            catalog_versions: dashboard.catalog_versions.clone(),
            bundle: TypedPortableBundle {
                project,
                catalogs: dashboard.bundle.catalogs.clone(),
            },
        };
        snapshot.bundle.validate().map_err(|_| corrupt())?;
        Ok(snapshot)
    }

    pub fn update_page(
        &self,
        dashboard_id: &str,
        page_id: &str,
        request: UpdatePage,
    ) -> Result<OfflinePageView, WorkcopyError> {
        let _lock = WRITE_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        let mut file = self.load()?;
        let dashboard = file
            .dashboards
            .iter_mut()
            .find(|item| item.local_id == dashboard_id)
            .ok_or_else(|| WorkcopyError::new("offline_dashboard_not_found"))?;
        if dashboard.revision != request.expected_revision {
            return Err(WorkcopyError::new("offline_page_conflict"));
        }
        let ProjectRoot::Dashboard(root) = &dashboard.bundle.project.root else {
            return Err(corrupt());
        };
        let old_page = root
            .pages
            .iter()
            .find(|page| page.id == page_id)
            .ok_or_else(|| WorkcopyError::new("offline_page_not_found"))?;
        let old = self.page_snapshot(dashboard, old_page.clone())?;
        // Sources belong to the dashboard. Editing one page cannot redefine a
        // role or catalog used by its siblings; explicit resolution is P4-09.
        if request.page.catalogs != dashboard.catalog_versions
            || request.page.roles != dashboard.bundle.project.roles
        {
            return Err(sources_conflict());
        }
        let edited =
            offline_pages::compose(&self.root, page_id.to_owned(), request.page, Some(&old))?;
        let ProjectRoot::Page(new_page) = edited.project.root else {
            return Err(corrupt());
        };
        let ProjectRoot::Dashboard(root) = &mut dashboard.bundle.project.root else {
            return Err(corrupt());
        };
        let target = root
            .pages
            .iter_mut()
            .find(|page| page.id == page_id)
            .ok_or_else(corrupt)?;
        *target = new_page.clone();
        validate_typed_project(&dashboard.bundle.project, &dashboard.bundle.catalogs)
            .map_err(|_| invalid())?;
        dashboard.revision = dashboard.revision.checked_add(1).ok_or_else(corrupt)?;
        dashboard.updated_at_ms = now_ms().max(dashboard.updated_at_ms);
        let snapshot = self.page_snapshot(dashboard, new_page)?.view()?;
        self.write(&file)?;
        Ok(snapshot)
    }

    pub fn create(&self, request: CreateDashboard) -> Result<OfflineDashboard, WorkcopyError> {
        let _lock = WRITE_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        let mut file = self.load()?;
        if file.dashboards.len() >= MAX_DASHBOARDS {
            return Err(WorkcopyError::new("storage_limit_reached"));
        }
        let id = random_id()?;
        let bundle = self.compose(&id, &request.name, &[], &request.page_ids, None)?;
        let now = now_ms();
        let dashboard = OfflineDashboard {
            local_id: id,
            created_at_ms: now,
            updated_at_ms: now,
            revision: 1,
            catalog_versions: bundle.1,
            bundle: bundle.0,
        };
        file.dashboards.push(dashboard.clone());
        self.write(&file)?;
        Ok(dashboard)
    }

    pub fn update(
        &self,
        id: &str,
        request: UpdateDashboard,
    ) -> Result<OfflineDashboard, WorkcopyError> {
        let _lock = WRITE_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        let mut file = self.load()?;
        let existing = file
            .dashboards
            .iter()
            .find(|item| item.local_id == id)
            .ok_or_else(|| WorkcopyError::new("offline_dashboard_not_found"))?;
        if existing.revision != request.expected_revision {
            return Err(WorkcopyError::new("offline_dashboard_conflict"));
        }
        let (bundle, pins) = self.compose(
            id,
            &request.name,
            &request.page_order,
            &request.append_page_ids,
            Some(existing),
        )?;
        let dashboard = file
            .dashboards
            .iter_mut()
            .find(|item| item.local_id == id)
            .ok_or_else(corrupt)?;
        dashboard.bundle = bundle;
        dashboard.catalog_versions = pins;
        dashboard.revision = dashboard.revision.checked_add(1).ok_or_else(corrupt)?;
        dashboard.updated_at_ms = now_ms().max(dashboard.updated_at_ms);
        let result = dashboard.clone();
        self.write(&file)?;
        Ok(result)
    }

    fn compose(
        &self,
        id: &str,
        name: &str,
        page_order: &[String],
        append: &[String],
        existing: Option<&OfflineDashboard>,
    ) -> Result<(TypedPortableBundle, Vec<CatalogChoice>), WorkcopyError> {
        if name.trim().is_empty() {
            return Err(invalid());
        }
        let mut pages = Vec::<Page>::new();
        let mut catalogs = BTreeMap::<String, MetricCatalog>::new();
        let mut pins = BTreeMap::<String, u32>::new();
        let mut roles = BTreeMap::<String, Role>::new();
        let mut compatibility = None;
        if let Some(existing) = existing {
            let ProjectRoot::Dashboard(root) = &existing.bundle.project.root else {
                return Err(corrupt());
            };
            let mut seen = BTreeSet::new();
            for page_id in page_order {
                if !seen.insert(page_id) {
                    return Err(invalid());
                }
                pages.push(
                    root.pages
                        .iter()
                        .find(|page| &page.id == page_id)
                        .ok_or_else(invalid)?
                        .clone(),
                );
            }
            catalogs.clone_from(&existing.bundle.catalogs);
            pins.extend(
                existing
                    .catalog_versions
                    .iter()
                    .map(|pin| (pin.catalog_id.clone(), pin.version)),
            );
            roles.clone_from(&existing.bundle.project.roles);
            compatibility = Some(existing.bundle.project.declared_compatibility.clone());
        } else if !page_order.is_empty() {
            return Err(invalid());
        }
        if pages.len() + append.len() == 0 || pages.len() + append.len() > 50 {
            return Err(invalid());
        }
        let mut used_references = BTreeSet::new();
        for page in &pages {
            for reference in page_references(page)? {
                if !used_references.insert(reference) {
                    return Err(reference_conflict());
                }
            }
        }
        let store = OfflinePageStore::new(self.root.clone());
        let mut page_ids: BTreeSet<String> = pages.iter().map(|page| page.id.clone()).collect();
        for source_id in append {
            let source = store
                .get(source_id)?
                .ok_or_else(|| WorkcopyError::new("offline_page_not_found"))?;
            let ProjectRoot::Page(page) = &source.bundle.project.root else {
                return Err(corrupt());
            };
            if !page_ids.insert(page.id.clone()) {
                return Err(invalid());
            }
            if let Some(range) = &compatibility {
                if range != &source.bundle.project.declared_compatibility {
                    return Err(sources_conflict());
                }
            } else {
                compatibility = Some(source.bundle.project.declared_compatibility.clone());
            }
            for pin in &source.catalog_versions {
                if pins
                    .get(&pin.catalog_id)
                    .is_some_and(|version| *version != pin.version)
                {
                    return Err(sources_conflict());
                }
                pins.insert(pin.catalog_id.clone(), pin.version);
            }
            for (key, catalog) in &source.bundle.catalogs {
                if catalogs.get(key).is_some_and(|old| old != catalog) {
                    return Err(sources_conflict());
                }
                catalogs.insert(key.clone(), catalog.clone());
            }
            for (key, role) in &source.bundle.project.roles {
                if roles.get(key).is_some_and(|old| old != role) {
                    return Err(sources_conflict());
                }
                roles.insert(key.clone(), role.clone());
            }
            let mut appended = page.clone();
            rewrite_page_references(&mut appended, &mut used_references)?;
            pages.push(appended);
        }
        let references: Vec<CatalogReference> = catalogs
            .keys()
            .map(|key| CatalogReference {
                id: key.clone(),
                file: format!("metrics/{key}.yaml"),
            })
            .collect();
        let project = TypedProject {
            format: TypedProjectFormat::V2,
            project_id: id.to_owned(),
            kind: ProjectKind::Dashboard,
            declared_compatibility: compatibility.ok_or_else(invalid)?,
            provenance: ProjectProvenance::Local,
            catalogs: references,
            roles,
            root: ProjectRoot::Dashboard(Dashboard {
                id: id.to_owned(),
                name: name.to_owned(),
                kind: DashboardKind::Global,
                pages,
            }),
        };
        validate_typed_project(&project, &catalogs).map_err(|_| sources_conflict())?;
        Ok((
            TypedPortableBundle { project, catalogs },
            pins.into_iter()
                .map(|(catalog_id, version)| CatalogChoice {
                    catalog_id,
                    version,
                })
                .collect(),
        ))
    }

    fn write(&self, file: &DashboardFile) -> Result<(), WorkcopyError> {
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

impl Default for DashboardFile {
    fn default() -> Self {
        Self {
            format: "zdashstitch.offline-dashboards/v0".to_owned(),
            dashboards: Vec::new(),
        }
    }
}

fn random_id() -> Result<String, WorkcopyError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| WorkcopyError::new("random_source_unavailable"))?;
    Ok(format!(
        "db-{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}
fn invalid() -> WorkcopyError {
    WorkcopyError::new("offline_dashboard_invalid")
}
fn corrupt() -> WorkcopyError {
    WorkcopyError::new("offline_dashboards_corrupt")
}
fn sources_conflict() -> WorkcopyError {
    WorkcopyError::new("offline_dashboard_sources_conflict")
}

fn reference_conflict() -> WorkcopyError {
    WorkcopyError::new("offline_dashboard_reference_conflict")
}

fn reference_unresolved() -> WorkcopyError {
    WorkcopyError::new("offline_dashboard_reference_unresolved")
}

fn page_references(page: &Page) -> Result<BTreeSet<String>, WorkcopyError> {
    let mut references = BTreeSet::new();
    for widget in &page.widgets {
        let Widget::Zabbix { fields, .. } = widget else {
            continue;
        };
        for field in fields.iter().filter(|field| field.name == "reference") {
            let ZabbixWidgetFieldValue::String { value } = &field.value else {
                return Err(reference_unresolved());
            };
            if field.field_type != 1
                || (!value.is_empty()
                    && (value.len() != 5
                        || !value.bytes().all(|byte| byte.is_ascii_alphanumeric())))
            {
                return Err(reference_unresolved());
            }
            if !value.is_empty() && !references.insert(value.clone()) {
                return Err(reference_conflict());
            }
        }
    }
    Ok(references)
}

/// Keeps each inserted page's links local when its provider code collides with
/// a provider already in the target dashboard. A link to a provider outside
/// this page is ambiguous until P4-09 carries source dashboard identity.
fn rewrite_page_references(
    page: &mut Page,
    used: &mut BTreeSet<String>,
) -> Result<(), WorkcopyError> {
    let local = page_references(page)?;
    let mut reserved = used.clone();
    reserved.extend(local.iter().cloned());
    let mut rewritten = BTreeMap::<String, String>::new();
    for reference in &local {
        if used.contains(reference) {
            let replacement = (0..=0xFFFF)
                .map(|number| format!("R{number:04X}"))
                .find(|candidate| !reserved.contains(candidate))
                .ok_or_else(reference_conflict)?;
            reserved.insert(replacement.clone());
            used.insert(replacement.clone());
            rewritten.insert(reference.clone(), replacement);
        } else {
            used.insert(reference.clone());
        }
    }
    for widget in &mut page.widgets {
        let Widget::Zabbix { fields, .. } = widget else {
            continue;
        };
        for field in fields {
            if field.name == "reference" {
                let ZabbixWidgetFieldValue::String { value } = &mut field.value else {
                    return Err(reference_unresolved());
                };
                if let Some(replacement) = rewritten.get(value) {
                    *value = replacement.clone();
                }
            } else if field.name.ends_with("._reference") {
                let ZabbixWidgetFieldValue::String { value } = &mut field.value else {
                    return Err(reference_unresolved());
                };
                if field.field_type != 1 {
                    return Err(reference_unresolved());
                }
                if value.is_empty() {
                    continue;
                }
                let provider = value
                    .split_once("._")
                    .map_or(value.as_str(), |(provider, _)| provider);
                if provider == "DASHBOARD" {
                    continue;
                }
                if !local.contains(provider) {
                    return Err(reference_unresolved());
                }
                if let Some(replacement) = rewritten.get(provider) {
                    *value = value.replacen(provider, replacement, 1);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod reference_tests {
    use super::*;
    use zdashstitch_core::formats::{Layout, RegistryState, ZabbixWidgetField};

    fn field(name: &str, value: &str) -> ZabbixWidgetField {
        ZabbixWidgetField {
            name: name.to_owned(),
            field_type: 1,
            registry_state: RegistryState::Known,
            value: ZabbixWidgetFieldValue::String {
                value: value.to_owned(),
            },
        }
    }

    fn linked_page(id: &str, reference: &str) -> Page {
        Page {
            id: id.to_owned(),
            name: id.to_owned(),
            widgets: vec![
                Widget::Zabbix {
                    id: "provider".to_owned(),
                    widget_type: "item".to_owned(),
                    name: String::new(),
                    registry_state: RegistryState::Known,
                    fields: vec![field("reference", reference)],
                    layout: Layout::Auto { w: 12, h: 5 },
                },
                Widget::Zabbix {
                    id: "consumer".to_owned(),
                    widget_type: "svggraph".to_owned(),
                    name: String::new(),
                    registry_state: RegistryState::Known,
                    fields: vec![
                        field(
                            "ds.0.override_hostid._reference",
                            &format!("{reference}._hostid"),
                        ),
                        field("time_period._reference", "DASHBOARD._hostid"),
                    ],
                    layout: Layout::Auto { w: 12, h: 5 },
                },
            ],
        }
    }

    fn values(page: &Page) -> Vec<String> {
        page.widgets
            .iter()
            .flat_map(|widget| match widget {
                Widget::Zabbix { fields, .. } => fields
                    .iter()
                    .filter_map(|field| match &field.value {
                        ZabbixWidgetFieldValue::String { value } => Some(value.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            })
            .collect()
    }

    #[test]
    fn three_pages_with_same_provider_keep_their_own_consumers() {
        let source = linked_page("third", "ABCDE");
        let mut used = BTreeSet::new();
        let mut first = linked_page("first", "ABCDE");
        let mut second = linked_page("second", "ABCDE");
        let mut third = source.clone();
        rewrite_page_references(&mut first, &mut used)
            .unwrap_or_else(|error| panic!("first: {error}"));
        rewrite_page_references(&mut second, &mut used)
            .unwrap_or_else(|error| panic!("second: {error}"));
        rewrite_page_references(&mut third, &mut used)
            .unwrap_or_else(|error| panic!("third: {error}"));
        assert_eq!(
            values(&first),
            ["ABCDE", "ABCDE._hostid", "DASHBOARD._hostid"]
        );
        assert_eq!(
            values(&second),
            ["R0000", "R0000._hostid", "DASHBOARD._hostid"]
        );
        assert_eq!(
            values(&third),
            ["R0001", "R0001._hostid", "DASHBOARD._hostid"]
        );
        assert_eq!(
            values(&source),
            ["ABCDE", "ABCDE._hostid", "DASHBOARD._hostid"]
        );
    }

    #[test]
    fn missing_or_duplicate_provider_is_rejected_before_save() {
        let mut used = BTreeSet::new();
        let mut missing = linked_page("missing", "ABCDE");
        if let Widget::Zabbix { fields, .. } = &mut missing.widgets[1] {
            fields[0] = field("ds.0.override_hostid._reference", "OTHER._hostid");
        }
        let Err(error) = rewrite_page_references(&mut missing, &mut used) else {
            panic!("dangling link must be rejected");
        };
        assert_eq!(error.code(), "offline_dashboard_reference_unresolved");

        let mut duplicate = linked_page("duplicate", "ABCDE");
        if let Widget::Zabbix { fields, .. } = &mut duplicate.widgets[1] {
            fields.push(field("reference", "ABCDE"));
        }
        let Err(error) = rewrite_page_references(&mut duplicate, &mut BTreeSet::new()) else {
            panic!("duplicate provider must be rejected");
        };
        assert_eq!(error.code(), "offline_dashboard_reference_conflict");
    }
}
