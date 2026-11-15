// SPDX-License-Identifier: MIT OR Apache-2.0

//! Independent, immutable copies of local pages for offline reuse.

use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use zdashstitch_core::formats::ProjectRoot;

use crate::{
    offline_dashboards::OfflineDashboardStore,
    offline_pages::{CatalogChoice, OfflinePage, OfflinePageStore},
    workcopy::{
        WorkcopyError, WorkcopyService,
        store::{LocalStore, read_bounded, write_atomic},
    },
};

const PATH: &[&str] = &["library", "pages"];
const FILE: &str = "pages.json";
const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_PAGES: usize = 2_000;
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryPage {
    pub library_id: String,
    pub source_local_id: String,
    pub saved_at_ms: u64,
    #[serde(default = "first_revision")]
    pub revision: u64,
    #[serde(default)]
    pub display_name: Option<String>,
    pub catalog_versions: Vec<CatalogChoice>,
    #[serde(default)]
    pub embedded_catalogs: Vec<String>,
    pub bundle: zdashstitch_core::formats::TypedPortableBundle,
}

#[derive(Serialize)]
pub struct LibraryPageSummary {
    pub library_id: String,
    pub source_local_id: String,
    pub saved_at_ms: u64,
    pub revision: u64,
    pub name: String,
    pub widgets: usize,
    pub catalog_versions: Vec<CatalogChoice>,
}

const fn first_revision() -> u64 {
    1
}

fn valid_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && trimmed == name
        && name.chars().count() <= 160
        && !name.chars().any(char::is_control)
}

impl LibraryPage {
    pub(crate) fn summary(&self) -> Result<LibraryPageSummary, WorkcopyError> {
        let ProjectRoot::Page(page) = &self.bundle.project.root else {
            return Err(corrupt());
        };
        Ok(LibraryPageSummary {
            library_id: self.library_id.clone(),
            source_local_id: self.source_local_id.clone(),
            saved_at_ms: self.saved_at_ms,
            revision: self.revision,
            name: self
                .display_name
                .clone()
                .unwrap_or_else(|| page.name.clone()),
            widgets: page.widgets.len(),
            catalog_versions: self.catalog_versions.clone(),
        })
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageFile {
    pages: Vec<LibraryPage>,
}

pub struct PageLibrary {
    root: PathBuf,
}

impl PageLibrary {
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn load(&self) -> Result<PageFile, WorkcopyError> {
        let Some(dir) = LocalStore::at(self.root.clone()).path(PATH, false)? else {
            return Ok(PageFile::default());
        };
        let Some(raw) = read_bounded(&dir, FILE, MAX_BYTES)? else {
            return Ok(PageFile::default());
        };
        let file: PageFile = serde_json::from_slice(&raw).map_err(|_| corrupt())?;
        if file.pages.len() > MAX_PAGES {
            return Err(corrupt());
        }
        let mut ids = BTreeSet::new();
        for page in &file.pages {
            let pin_ids: BTreeSet<_> = page
                .catalog_versions
                .iter()
                .map(|pin| &pin.catalog_id)
                .collect();
            if !page.library_id.starts_with("lp-")
                || !ids.insert(&page.library_id)
                || page.revision == 0
                || page
                    .display_name
                    .as_deref()
                    .is_some_and(|name| !valid_name(name))
                || page.source_local_id != page.bundle.project.project_id
                || !matches!(page.bundle.project.root, ProjectRoot::Page(_))
                || pin_ids.len() != page.catalog_versions.len()
                || !crate::offline_pages::catalog_sources_valid(
                    &page.catalog_versions,
                    &page.embedded_catalogs,
                    &page.bundle,
                )
                || page.catalog_versions.iter().any(|pin| pin.version == 0)
                || page.bundle.validate().is_err()
            {
                return Err(corrupt());
            }
        }
        Ok(file)
    }

    pub fn list(&self) -> Result<Vec<LibraryPageSummary>, WorkcopyError> {
        self.load()?
            .pages
            .iter()
            .rev()
            .map(LibraryPage::summary)
            .collect()
    }

    pub(crate) fn entries(&self) -> Result<Vec<LibraryPage>, WorkcopyError> {
        Ok(self.load()?.pages)
    }

    pub fn get(&self, id: &str) -> Result<Option<LibraryPage>, WorkcopyError> {
        Ok(self
            .load()?
            .pages
            .into_iter()
            .find(|page| page.library_id == id))
    }

    pub fn duplicate(
        &self,
        id: &str,
        expected_revision: u64,
    ) -> Result<LibraryPageSummary, WorkcopyError> {
        let _pin_lock = crate::catalog_pin_lock::lock()?;
        let _lock = WRITE_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        let mut file = self.load()?;
        if file.pages.len() >= MAX_PAGES {
            return Err(WorkcopyError::new("storage_limit_reached"));
        }
        let source = file
            .pages
            .iter()
            .find(|page| page.library_id == id)
            .ok_or_else(|| WorkcopyError::new("page_library_not_found"))?;
        if source.revision != expected_revision {
            return Err(WorkcopyError::new("library_conflict"));
        }
        let mut copy = source.clone();
        copy.library_id = new_id("lp-")?;
        if file
            .pages
            .iter()
            .any(|page| page.library_id == copy.library_id)
        {
            return Err(WorkcopyError::new("storage_write_failed"));
        }
        copy.saved_at_ms = now_ms()?;
        copy.revision = 1;
        let summary = copy.summary()?;
        file.pages.push(copy);
        self.write(&file)?;
        Ok(summary)
    }

    pub fn rename(
        &self,
        id: &str,
        expected_revision: u64,
        display_name: &str,
    ) -> Result<LibraryPageSummary, WorkcopyError> {
        if !valid_name(display_name) {
            return Err(WorkcopyError::new("library_name_invalid"));
        }
        let _lock = WRITE_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        let mut file = self.load()?;
        let page = file
            .pages
            .iter_mut()
            .find(|page| page.library_id == id)
            .ok_or_else(|| WorkcopyError::new("page_library_not_found"))?;
        if page.revision != expected_revision {
            return Err(WorkcopyError::new("library_conflict"));
        }
        page.display_name = Some(display_name.to_owned());
        page.revision = page.revision.checked_add(1).ok_or_else(corrupt)?;
        let summary = page.summary()?;
        self.write(&file)?;
        Ok(summary)
    }

    pub fn delete(
        &self,
        id: &str,
        expected_revision: u64,
        confirmed_name: &str,
    ) -> Result<(), WorkcopyError> {
        let _pin_lock = crate::catalog_pin_lock::lock()?;
        let _lock = WRITE_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        let mut file = self.load()?;
        let position = file
            .pages
            .iter()
            .position(|page| page.library_id == id)
            .ok_or_else(|| WorkcopyError::new("page_library_not_found"))?;
        let page = &file.pages[position];
        if page.revision != expected_revision {
            return Err(WorkcopyError::new("library_conflict"));
        }
        if page.summary()?.name != confirmed_name {
            return Err(WorkcopyError::new("library_confirmation_mismatch"));
        }
        file.pages.remove(position);
        self.write(&file)
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

    pub fn save_draft(&self, source_id: &str) -> Result<LibraryPageSummary, WorkcopyError> {
        let source = OfflinePageStore::new(self.root.clone())
            .get(source_id)?
            .ok_or_else(|| WorkcopyError::new("offline_page_not_found"))?;
        self.save_page(source)
    }

    pub fn take_page(&self, id: &str) -> Result<OfflinePage, WorkcopyError> {
        let source = self
            .get(id)?
            .ok_or_else(|| WorkcopyError::new("page_library_not_found"))?;
        OfflinePageStore::new(self.root.clone()).save_library_snapshot(
            source.bundle,
            source.catalog_versions,
            source.embedded_catalogs,
        )
    }

    pub fn save_dashboard_page(
        &self,
        dashboard_id: &str,
        page_id: &str,
    ) -> Result<LibraryPageSummary, WorkcopyError> {
        let source = OfflineDashboardStore::new(self.root.clone())
            .page(dashboard_id, page_id)?
            .ok_or_else(|| WorkcopyError::new("offline_page_not_found"))?
            .page;
        self.save_page(source)
    }

    /// Save only a page workcopy that can be converted without server bindings.
    pub fn save_workcopy_page(&self, id: &str) -> Result<LibraryPageSummary, WorkcopyError> {
        let mut bundle =
            WorkcopyService::new(self.root.clone()).convert_dependency_free_page(id)?;
        bundle.project.project_id = id.to_owned();
        self.save_page(OfflinePage {
            local_id: id.to_owned(),
            created_at_ms: 0,
            updated_at_ms: 0,
            revision: 1,
            catalog_versions: Vec::new(),
            embedded_catalogs: Vec::new(),
            bundle,
        })
    }

    /// Snapshot one explicitly selected page of a dashboard Workcopy.
    pub fn save_dashboard_workcopy_page(
        &self,
        id: &str,
        page_id: &str,
    ) -> Result<LibraryPageSummary, WorkcopyError> {
        let mut bundle = WorkcopyService::new(self.root.clone())
            .convert_dependency_free_dashboard_page(id, page_id)?;
        bundle.project.project_id = id.to_owned();
        self.save_page(OfflinePage {
            local_id: id.to_owned(),
            created_at_ms: 0,
            updated_at_ms: 0,
            revision: 1,
            catalog_versions: Vec::new(),
            embedded_catalogs: Vec::new(),
            bundle,
        })
    }

    fn save_page(&self, source: OfflinePage) -> Result<LibraryPageSummary, WorkcopyError> {
        source.bundle.validate().map_err(|_| corrupt())?;
        let _pin_lock = crate::catalog_pin_lock::lock()?;
        let catalogs = crate::catalog_library::CatalogLibrary::new(self.root.clone());
        for pin in &source.catalog_versions {
            if catalogs.get(&pin.catalog_id, pin.version)?.is_none() {
                return Err(WorkcopyError::new("catalog_not_found"));
            }
        }
        let _lock = WRITE_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        let mut file = self.load()?;
        if file.pages.len() >= MAX_PAGES {
            return Err(WorkcopyError::new("storage_limit_reached"));
        }
        let id = new_id("lp-")?;
        if file.pages.iter().any(|page| page.library_id == id) {
            return Err(WorkcopyError::new("storage_write_failed"));
        }
        let saved_at_ms = now_ms()?;
        let page = LibraryPage {
            library_id: id,
            source_local_id: source.local_id,
            saved_at_ms,
            revision: 1,
            display_name: None,
            catalog_versions: source.catalog_versions,
            embedded_catalogs: source.embedded_catalogs,
            bundle: source.bundle,
        };
        let summary = page.summary()?;
        file.pages.push(page);
        self.write(&file)?;
        Ok(summary)
    }
}

fn corrupt() -> WorkcopyError {
    WorkcopyError::new("page_library_corrupt")
}

fn now_ms() -> Result<u64, WorkcopyError> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| WorkcopyError::new("storage_write_failed"))?
        .as_millis() as u64)
}

fn new_id(prefix: &str) -> Result<String, WorkcopyError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| WorkcopyError::new("random_source_unavailable"))?;
    Ok(format!(
        "{prefix}{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}
