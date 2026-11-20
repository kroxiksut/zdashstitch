// SPDX-License-Identifier: MIT OR Apache-2.0

//! Versioned local template imports (P4-01, O-018). One atomic file commits
//! a batch and keeps each catalog's local preview metadata beside it.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
pub use zdashstitch_core::formats::MAX_FILE_BYTES as MAX_TEMPLATE_FILE_BYTES;
use zdashstitch_core::formats::{Diagnostic, Validate};
pub use zdashstitch_template_import::{ExportSyntax, ImportError, ImportedTemplate};

use crate::workcopy::{
    WorkcopyError,
    store::{LocalStore, read_bounded, write_atomic},
};
use crate::{
    offline_dashboards::OfflineDashboardStore, offline_pages::OfflinePageStore,
    page_library::PageLibrary,
};

const PATH: &[&str] = &["library", "catalogs"];
const FILE: &str = "template-exports.json";
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_VERSIONS: usize = 2_000;
// The local instance is single-process; serialize read/modify/replace across
// HTTP requests even when a service is rebuilt from the current settings.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

pub fn inspect_template_file(
    name: &str,
    bytes: &[u8],
) -> Option<Result<Vec<ImportedTemplate>, ImportError>> {
    let syntax = match name.rsplit_once('.')?.1.to_ascii_lowercase().as_str() {
        "yaml" | "yml" => ExportSyntax::Yaml,
        "json" => ExportSyntax::Json,
        "xml" => ExportSyntax::Xml,
        _ => return None,
    };
    Some(zdashstitch_template_import::import_templates(bytes, syntax))
}

#[derive(Debug, Serialize)]
pub struct CatalogSummary {
    pub library_id: Option<String>,
    pub revision: u64,
    pub catalog_id: String,
    pub name: String,
    pub version: u32,
    pub export_format_version: Option<String>,
    pub metrics: usize,
    pub discovery_rules: usize,
    pub metrics_with_hints: usize,
    pub omitted_template_dashboards: usize,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Serialize)]
pub struct CatalogDeletionPreview {
    pub library_id: String,
    pub revision: u64,
    pub name: String,
    pub catalog_id: String,
    pub version: u32,
    pub used_by: Vec<String>,
}

impl CatalogSummary {
    #[must_use]
    pub fn of(template: &ImportedTemplate, version: u32) -> Self {
        let catalog = &template.catalog;
        Self {
            library_id: None,
            revision: 1,
            catalog_id: catalog.catalog_id.clone(),
            name: catalog.source.template_name.clone().unwrap_or_default(),
            version,
            export_format_version: catalog.source.export_format_version.clone(),
            metrics: catalog.metrics.len(),
            discovery_rules: template.discovery_rules.len(),
            metrics_with_hints: template.preview_hints.metrics.len(),
            omitted_template_dashboards: template.omitted_template_dashboards,
            diagnostics: catalog.diagnostics.clone(),
        }
    }
}

#[derive(Serialize, Deserialize)]
enum Format {
    #[serde(rename = "zdashstitch.template-library/v0")]
    V0,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LibraryFile {
    format: Format,
    entries: Vec<VersionedTemplate>,
    #[serde(default)]
    version_counters: BTreeMap<String, u32>,
}

impl Default for LibraryFile {
    fn default() -> Self {
        Self {
            format: Format::V0,
            entries: Vec::new(),
            version_counters: BTreeMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionedTemplate {
    version: u32,
    template: ImportedTemplate,
    #[serde(default)]
    library_id: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default = "first_revision")]
    revision: u64,
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

impl VersionedTemplate {
    fn id(&self) -> String {
        self.library_id.clone().unwrap_or_else(|| {
            let mut digest = Sha256::new();
            digest.update(self.template.catalog.catalog_id.as_bytes());
            digest.update(self.version.to_be_bytes());
            format!("ct-{:x}", digest.finalize())
        })
    }

    fn summary(&self) -> CatalogSummary {
        let mut summary = CatalogSummary::of(&self.template, self.version);
        summary.library_id = Some(self.id());
        summary.revision = self.revision;
        if let Some(name) = &self.display_name {
            summary.name = name.clone();
        }
        summary
    }
}

pub struct CatalogLibrary(LocalStore);

impl CatalogLibrary {
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self(LocalStore::at(root))
    }

    fn load(&self) -> Result<LibraryFile, WorkcopyError> {
        let Some(dir) = self.0.path(PATH, false)? else {
            return Ok(LibraryFile::default());
        };
        let Some(raw) = read_bounded(&dir, FILE, MAX_BYTES)? else {
            return Ok(LibraryFile::default());
        };
        let mut library: LibraryFile = serde_json::from_slice(&raw).map_err(|_| corrupt())?;
        if library.entries.len() > MAX_VERSIONS {
            return Err(corrupt());
        }
        let mut latest = BTreeMap::new();
        let mut ids = BTreeSet::new();
        for entry in &library.entries {
            let catalog = &entry.template.catalog;
            let version = latest.entry(&catalog.catalog_id).or_insert(0);
            if entry.version <= *version
                || entry.revision == 0
                || !ids.insert(entry.id())
                || entry
                    .display_name
                    .as_deref()
                    .is_some_and(|name| !valid_name(name))
                || catalog.validate().is_err()
            {
                return Err(corrupt());
            }
            *version = entry.version;
        }
        for (catalog_id, highest) in latest {
            let counter = library
                .version_counters
                .entry(catalog_id.clone())
                .or_insert(highest);
            if *counter < highest {
                return Err(corrupt());
            }
        }
        Ok(library)
    }

    pub fn list(&self) -> Result<Vec<CatalogSummary>, WorkcopyError> {
        let library = self.load()?;
        Ok(library
            .entries
            .iter()
            .rev()
            .map(VersionedTemplate::summary)
            .collect())
    }

    /// Opens exactly the catalog version selected by the consumer, including
    /// its local preview inputs. No latest-version substitution.
    pub fn get(
        &self,
        catalog_id: &str,
        version: u32,
    ) -> Result<Option<ImportedTemplate>, WorkcopyError> {
        Ok(self
            .load()?
            .entries
            .into_iter()
            .find(|entry| {
                entry.version == version && entry.template.catalog.catalog_id == catalog_id
            })
            .map(|entry| entry.template))
    }

    pub fn duplicate(
        &self,
        library_id: &str,
        expected_revision: u64,
    ) -> Result<CatalogSummary, WorkcopyError> {
        let _lock = WRITE_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        let mut library = self.load()?;
        if library.entries.len() >= MAX_VERSIONS {
            return Err(WorkcopyError::new("storage_limit_reached"));
        }
        let source = library
            .entries
            .iter()
            .find(|entry| entry.id() == library_id)
            .ok_or_else(|| WorkcopyError::new("catalog_not_found"))?;
        if source.revision != expected_revision {
            return Err(WorkcopyError::new("library_conflict"));
        }
        let catalog_id = source.template.catalog.catalog_id.clone();
        let version = library
            .version_counters
            .get(&catalog_id)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(corrupt)?;
        let copy = VersionedTemplate {
            version,
            template: source.template.clone(),
            library_id: Some(new_id()?),
            display_name: source.display_name.clone(),
            revision: 1,
        };
        if library.entries.iter().any(|entry| entry.id() == copy.id()) {
            return Err(WorkcopyError::new("storage_write_failed"));
        }
        copy.template.catalog.validate().map_err(|_| corrupt())?;
        let summary = copy.summary();
        library.entries.push(copy);
        library.version_counters.insert(catalog_id, version);
        self.write(&library)?;
        Ok(summary)
    }

    pub fn rename(
        &self,
        library_id: &str,
        expected_revision: u64,
        display_name: &str,
    ) -> Result<CatalogSummary, WorkcopyError> {
        if !valid_name(display_name) {
            return Err(WorkcopyError::new("library_name_invalid"));
        }
        let _lock = WRITE_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        let mut library = self.load()?;
        let entry = library
            .entries
            .iter_mut()
            .find(|entry| entry.id() == library_id)
            .ok_or_else(|| WorkcopyError::new("catalog_not_found"))?;
        if entry.revision != expected_revision {
            return Err(WorkcopyError::new("library_conflict"));
        }
        entry.display_name = Some(display_name.to_owned());
        entry.revision = entry.revision.checked_add(1).ok_or_else(corrupt)?;
        let summary = entry.summary();
        self.write(&library)?;
        Ok(summary)
    }

    pub fn deletion_preview(
        &self,
        library_id: &str,
    ) -> Result<CatalogDeletionPreview, WorkcopyError> {
        let _pin_lock = crate::catalog_pin_lock::lock()?;
        let library = self.load()?;
        let entry = library
            .entries
            .iter()
            .find(|entry| entry.id() == library_id)
            .ok_or_else(|| WorkcopyError::new("catalog_not_found"))?;
        let summary = entry.summary();
        let used_by = self.usage(&summary.catalog_id, summary.version)?;
        Ok(CatalogDeletionPreview {
            library_id: entry.id(),
            revision: entry.revision,
            name: summary.name,
            catalog_id: summary.catalog_id,
            version: summary.version,
            used_by,
        })
    }

    pub fn delete(
        &self,
        library_id: &str,
        expected_revision: u64,
        confirmed_name: &str,
    ) -> Result<(), WorkcopyError> {
        let _pin_lock = crate::catalog_pin_lock::lock()?;
        let _lock = WRITE_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        let mut library = self.load()?;
        let position = library
            .entries
            .iter()
            .position(|entry| entry.id() == library_id)
            .ok_or_else(|| WorkcopyError::new("catalog_not_found"))?;
        let summary = library.entries[position].summary();
        if summary.revision != expected_revision {
            return Err(WorkcopyError::new("library_conflict"));
        }
        if summary.name != confirmed_name {
            return Err(WorkcopyError::new("library_confirmation_mismatch"));
        }
        if !self.usage(&summary.catalog_id, summary.version)?.is_empty() {
            return Err(WorkcopyError::new("catalog_in_use"));
        }
        library.entries.remove(position);
        self.write(&library)
    }

    fn usage(&self, catalog_id: &str, version: u32) -> Result<Vec<String>, WorkcopyError> {
        let root = self.0.root_path();
        let mut used_by = Vec::new();
        for page in PageLibrary::new(root.clone()).entries()? {
            if page
                .catalog_versions
                .iter()
                .any(|pin| pin.catalog_id == catalog_id && pin.version == version)
            {
                used_by.push(page.library_id);
            }
        }
        for page in OfflinePageStore::new(root.clone()).list()? {
            if page
                .catalog_versions
                .iter()
                .any(|pin| pin.catalog_id == catalog_id && pin.version == version)
            {
                used_by.push(page.local_id);
            }
        }
        for dashboard in OfflineDashboardStore::new(root).entries()? {
            if dashboard
                .catalog_versions
                .iter()
                .any(|pin| pin.catalog_id == catalog_id && pin.version == version)
            {
                used_by.push(dashboard.local_id);
            }
        }
        Ok(used_by)
    }

    fn write(&self, library: &LibraryFile) -> Result<(), WorkcopyError> {
        let raw =
            serde_json::to_vec(library).map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        if raw.len() > MAX_BYTES {
            return Err(WorkcopyError::new("storage_limit_reached"));
        }
        let dir = self
            .0
            .path(PATH, true)?
            .ok_or_else(|| WorkcopyError::new("storage_unavailable"))?;
        write_atomic(&dir, FILE, &raw)
    }

    pub fn save(
        &self,
        templates: Vec<ImportedTemplate>,
    ) -> Result<Vec<CatalogSummary>, WorkcopyError> {
        if templates.is_empty() {
            return Ok(Vec::new());
        }
        let _lock = WRITE_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        let mut library = self.load()?;
        if templates.len().saturating_add(library.entries.len()) > MAX_VERSIONS {
            return Err(WorkcopyError::new("storage_limit_reached"));
        }
        let mut latest = library.version_counters.clone();
        let mut saved = Vec::with_capacity(templates.len());
        for template in templates {
            template.catalog.validate().map_err(|_| corrupt())?;
            let version = latest
                .entry(template.catalog.catalog_id.clone())
                .or_insert(0);
            *version += 1;
            saved.push(CatalogSummary::of(&template, *version));
            library.entries.push(VersionedTemplate {
                version: *version,
                template,
                library_id: Some(new_id()?),
                display_name: None,
                revision: 1,
            });
        }
        library.version_counters = latest;
        self.write(&library)?;
        Ok(saved)
    }
}

fn corrupt() -> WorkcopyError {
    WorkcopyError::new("catalog_library_corrupt")
}

fn new_id() -> Result<String, WorkcopyError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| WorkcopyError::new("random_source_unavailable"))?;
    Ok(format!(
        "ct-{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workcopy::store::{CrashPoint, fail};

    #[test]
    fn duplicate_and_display_name_preserve_catalog_payload_and_pinned_version() {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let library = CatalogLibrary::new(root.path().to_owned());
        let imported = template(30);
        let original_payload = imported[0].clone();
        library
            .save(imported)
            .unwrap_or_else(|error| panic!("{error}"));
        let original = library.list().unwrap_or_else(|error| panic!("{error}"));
        let first = &original[0];
        let id = first
            .library_id
            .as_deref()
            .unwrap_or_else(|| panic!("library id"));
        let renamed = library
            .rename(id, 1, "Display only")
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(renamed.name, "Display only");
        assert_eq!(renamed.revision, 2);
        assert_eq!(
            library
                .rename(id, 1, "Stale")
                .err()
                .map(|error| error.code()),
            Some("library_conflict")
        );
        let copied = library
            .duplicate(id, 2)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(copied.version, 2);
        assert_ne!(copied.library_id, first.library_id);
        assert_eq!(
            library
                .get(&first.catalog_id, 1)
                .unwrap_or_else(|error| panic!("{error}")),
            Some(original_payload.clone())
        );
        assert_eq!(
            library
                .get(&first.catalog_id, 2)
                .unwrap_or_else(|error| panic!("{error}")),
            Some(original_payload)
        );
        let reopened = CatalogLibrary::new(root.path().to_owned())
            .list()
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(reopened.len(), 2);
    }

    #[test]
    fn concurrent_renames_reject_one_stale_revision() {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let path = root.path().to_owned();
        CatalogLibrary::new(path.clone())
            .save(template(30))
            .unwrap_or_else(|error| panic!("{error}"));
        let id = CatalogLibrary::new(path.clone())
            .list()
            .unwrap_or_else(|error| panic!("{error}"))[0]
            .library_id
            .clone()
            .unwrap_or_else(|| panic!("entry id"));
        let (first, second) = std::thread::scope(|scope| {
            let a_path = path.clone();
            let a_id = id.clone();
            let a = scope.spawn(move || CatalogLibrary::new(a_path).rename(&a_id, 1, "First"));
            let b = scope.spawn(move || CatalogLibrary::new(path).rename(&id, 1, "Second"));
            (
                a.join().unwrap_or_else(|_| panic!("thread")),
                b.join().unwrap_or_else(|_| panic!("thread")),
            )
        });
        assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
        assert_eq!(
            usize::from(
                first
                    .err()
                    .is_some_and(|error| error.code() == "library_conflict")
            ) + usize::from(
                second
                    .err()
                    .is_some_and(|error| error.code() == "library_conflict")
            ),
            1
        );
    }

    #[test]
    fn deleting_unused_version_keeps_high_water_mark_and_other_versions() {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let library = CatalogLibrary::new(root.path().to_owned());
        library
            .save(template(30))
            .unwrap_or_else(|error| panic!("{error}"));
        let first = library.list().unwrap_or_else(|error| panic!("{error}"));
        let source = &first[0];
        let first_id = source
            .library_id
            .as_deref()
            .unwrap_or_else(|| panic!("entry id"));
        let copy = library
            .duplicate(first_id, 1)
            .unwrap_or_else(|error| panic!("{error}"));
        let copy_id = copy
            .library_id
            .as_deref()
            .unwrap_or_else(|| panic!("copy id"));
        let preview = library
            .deletion_preview(copy_id)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(preview.used_by.is_empty());
        assert_eq!(
            library
                .delete(copy_id, 1, "wrong")
                .err()
                .map(|error| error.code()),
            Some("library_confirmation_mismatch")
        );
        library
            .delete(copy_id, 1, &copy.name)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(
            library
                .get(&source.catalog_id, 1)
                .unwrap_or_else(|error| panic!("{error}"))
                .is_some()
        );
        assert!(
            library
                .get(&source.catalog_id, 2)
                .unwrap_or_else(|error| panic!("{error}"))
                .is_none()
        );
        let next = library
            .duplicate(first_id, 1)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(next.version, 3);
    }

    #[test]
    fn concurrent_page_create_and_catalog_delete_cannot_orphan_a_pin() {
        use crate::offline_pages::{CreatePage, OfflinePageStore};
        for _ in 0..8 {
            let root = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
            let path = root.path().to_owned();
            let imported = template(30);
            let catalog_id = imported[0].catalog.catalog_id.clone();
            let metric_id = imported[0].catalog.metrics[0].id.clone();
            let library = CatalogLibrary::new(path.clone());
            library
                .save(imported)
                .unwrap_or_else(|error| panic!("{error}"));
            let entry = library.list().unwrap_or_else(|error| panic!("{error}"));
            let entry_id = entry[0]
                .library_id
                .clone()
                .unwrap_or_else(|| panic!("entry id"));
            let name = entry[0].name.clone();
            let request = CreatePage::parse_json(serde_json::json!({
                "name":"Concurrent page", "catalogs":[{"catalog_id":catalog_id,"version":1}],
                "roles":{"source":{"cardinality":"one","slots":["node-a"]}},
                "widgets":[{"kind":"item_value","title":"Item","metric":{"catalog_id":catalog_id,"metric_id":metric_id},"hosts":{"role":"source","selection":"one"},"expansion":"single"}]
            }).to_string().as_bytes()).unwrap_or_else(|| panic!("request"));
            let start = std::sync::Barrier::new(3);
            let (created, deleted) = std::thread::scope(|scope| {
                let page_path = path.clone();
                let barrier = &start;
                let page = scope.spawn(move || {
                    barrier.wait();
                    OfflinePageStore::new(page_path).create(request)
                });
                let barrier = &start;
                let catalog = scope.spawn(move || {
                    barrier.wait();
                    CatalogLibrary::new(path).delete(&entry_id, 1, &name)
                });
                start.wait();
                (
                    page.join().unwrap_or_else(|_| panic!("page thread")),
                    catalog.join().unwrap_or_else(|_| panic!("catalog thread")),
                )
            });
            assert_ne!(created.is_ok(), deleted.is_ok());
            let page_exists = !OfflinePageStore::new(root.path().to_owned())
                .list()
                .unwrap_or_else(|error| panic!("{error}"))
                .is_empty();
            let catalog_exists = CatalogLibrary::new(root.path().to_owned())
                .get(&catalog_id, 1)
                .unwrap_or_else(|error| panic!("{error}"))
                .is_some();
            assert_eq!(page_exists, catalog_exists);
        }
    }

    #[test]
    fn stale_page_snapshot_cannot_recreate_a_deleted_pin() {
        use crate::offline_pages::{CatalogChoice, CreatePage, OfflinePageStore, compose};
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let path = root.path().to_owned();
        let imported = template(30);
        let catalog_id = imported[0].catalog.catalog_id.clone();
        let metric_id = imported[0].catalog.metrics[0].id.clone();
        let library = CatalogLibrary::new(path.clone());
        library
            .save(imported)
            .unwrap_or_else(|error| panic!("{error}"));
        let entry = library.list().unwrap_or_else(|error| panic!("{error}"));
        let request = CreatePage::parse_json(serde_json::json!({
            "name":"Stale source", "catalogs":[{"catalog_id":catalog_id,"version":1}],
            "roles":{"source":{"cardinality":"one","slots":["node-a"]}},
            "widgets":[{"kind":"item_value","title":"Item","metric":{"catalog_id":catalog_id,"metric_id":metric_id},"hosts":{"role":"source","selection":"one"},"expansion":"single"}]
        }).to_string().as_bytes()).unwrap_or_else(|| panic!("request"));
        let bundle = compose(
            &path,
            "pg-00000000000000000000000000000000".to_owned(),
            request,
            None,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let id = entry[0]
            .library_id
            .as_deref()
            .unwrap_or_else(|| panic!("entry id"));
        library
            .delete(id, 1, &entry[0].name)
            .unwrap_or_else(|error| panic!("{error}"));
        let result = OfflinePageStore::new(path).save_library_snapshot(
            bundle,
            vec![CatalogChoice {
                catalog_id,
                version: 1,
            }],
            Vec::new(),
        );
        assert_eq!(
            result.err().map(|error| error.code()),
            Some("catalog_not_found")
        );
    }

    #[test]
    fn unreadable_consumer_store_blocks_catalog_delete() {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let library = CatalogLibrary::new(root.path().to_owned());
        library
            .save(template(30))
            .unwrap_or_else(|error| panic!("{error}"));
        let entry = library.list().unwrap_or_else(|error| panic!("{error}"));
        let dir = root.path().join("library/pages");
        std::fs::create_dir_all(&dir).unwrap_or_else(|error| panic!("{error}"));
        std::fs::write(dir.join("pages.json"), b"{damaged")
            .unwrap_or_else(|error| panic!("{error}"));
        let id = entry[0]
            .library_id
            .as_deref()
            .unwrap_or_else(|| panic!("entry id"));
        assert_eq!(
            library
                .delete(id, 1, &entry[0].name)
                .err()
                .map(|error| error.code()),
            Some("page_library_corrupt")
        );
        assert!(
            library
                .get(&entry[0].catalog_id, 1)
                .unwrap_or_else(|error| panic!("{error}"))
                .is_some()
        );
    }

    fn template(delay: u32) -> Vec<ImportedTemplate> {
        let input = format!(
            r#"{{"zabbix_export":{{"version":"7.4","templates":[{{"template":"synthetic","items":[{{"name":"Ping","key":"agent.ping","delay":{delay}}}]}}]}}}}"#
        );
        inspect_template_file("synthetic.json", input.as_bytes())
            .unwrap_or_else(|| panic!("known extension"))
            .unwrap_or_else(|error| panic!("invalid test fixture: {error}"))
    }

    #[test]
    fn file_import_saves_and_reopens_mixed_8_0_catalogs() {
        use zdashstitch_core::formats::MetricsFormat;

        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let library = CatalogLibrary::new(root.path().to_owned());
        let filename = "template-export-synthetic.json";
        let old_input = br#"{"zabbix_export":{"version":"7.4","templates":[{"uuid":"4bd523679faa48778aa59efa07ef9f01","template":"Synthetic database","items":[{"name":"Pool size","key":"synthetic.pool.size"}]}]}}"#;
        let old = inspect_template_file(filename, old_input)
            .unwrap_or_else(|| panic!("known extension"))
            .unwrap_or_else(|error| panic!("{error}"));
        library
            .save(old.clone())
            .unwrap_or_else(|error| panic!("{error}"));

        let input = include_bytes!("../../../fixtures/zabbix/8.0/template-export-synthetic.json");
        let templates = inspect_template_file(filename, input)
            .unwrap_or_else(|| panic!("known extension"))
            .unwrap_or_else(|error| panic!("{error}"));
        let saved = library
            .save(templates.clone())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(saved.len(), 2);
        assert!(
            saved
                .iter()
                .all(|summary| { summary.export_format_version.as_deref() == Some("8.0") })
        );
        assert_eq!(saved[0].catalog_id, old[0].catalog.catalog_id);
        assert_eq!(saved[0].version, 2);
        assert_eq!(saved[1].version, 1);

        let reopened = CatalogLibrary::new(root.path().to_owned());
        assert_eq!(templates[0].catalog.format, MetricsFormat::V2);
        assert_eq!(templates[1].catalog.format, MetricsFormat::V1);
        assert_eq!(
            reopened
                .get(&old[0].catalog.catalog_id, 1)
                .unwrap_or_else(|error| panic!("{error}"))
                .as_ref(),
            old.first(),
        );
        for (template, summary) in templates.into_iter().zip(saved) {
            let actual = reopened
                .get(&template.catalog.catalog_id, summary.version)
                .unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(actual, Some(template));
        }
    }

    #[test]
    fn interrupted_batch_preserves_previous_version_and_hints() {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let library = CatalogLibrary::new(root.path().to_owned());
        let original = template(30);
        let replacement = template(45);
        let id = &original[0].catalog.catalog_id;
        library
            .save(original.clone())
            .unwrap_or_else(|error| panic!("{error}"));

        fail::AT.with(|point| point.set(Some(CrashPoint::FileRename)));
        let failed = library.save(replacement.clone());
        fail::AT.with(|point| point.set(None));
        assert!(failed.is_err());
        let reopened = CatalogLibrary::new(root.path().to_owned());
        assert_eq!(reopened.list().ok().map(|items| items.len()), Some(1));
        assert_eq!(
            reopened.get(id, 1).ok().flatten().as_ref(),
            original.first()
        );

        let saved = reopened
            .save(replacement.clone())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(saved[0].version, 2);
        assert_eq!(
            reopened.get(id, 1).ok().flatten().as_ref(),
            original.first()
        );
        assert_eq!(
            reopened.get(id, 2).ok().flatten().as_ref(),
            replacement.first()
        );
    }
}
