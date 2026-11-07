// SPDX-License-Identifier: MIT OR Apache-2.0

//! Versioned local template imports (P4-01, O-018). One atomic file commits
//! a batch and keeps each catalog's local preview metadata beside it.

use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};

use serde::{Deserialize, Serialize};
pub use zdashstitch_core::formats::MAX_FILE_BYTES as MAX_TEMPLATE_FILE_BYTES;
use zdashstitch_core::formats::{Diagnostic, Validate};
pub use zdashstitch_template_import::{ExportSyntax, ImportError, ImportedTemplate};

use crate::workcopy::{
    WorkcopyError,
    store::{LocalStore, read_bounded, write_atomic},
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

impl CatalogSummary {
    #[must_use]
    pub fn of(template: &ImportedTemplate, version: u32) -> Self {
        let catalog = &template.catalog;
        Self {
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
}

impl Default for LibraryFile {
    fn default() -> Self {
        Self {
            format: Format::V0,
            entries: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionedTemplate {
    version: u32,
    template: ImportedTemplate,
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
        let library: LibraryFile = serde_json::from_slice(&raw).map_err(|_| corrupt())?;
        if library.entries.len() > MAX_VERSIONS {
            return Err(corrupt());
        }
        let mut latest = BTreeMap::new();
        for entry in &library.entries {
            let catalog = &entry.template.catalog;
            let version = latest.entry(&catalog.catalog_id).or_insert(0);
            *version += 1;
            if entry.version != *version || catalog.validate().is_err() {
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
            .map(|entry| CatalogSummary::of(&entry.template, entry.version))
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
        let mut latest: BTreeMap<_, _> = library
            .entries
            .iter()
            .map(|entry| (entry.template.catalog.catalog_id.clone(), entry.version))
            .collect();
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
            });
        }
        let raw =
            serde_json::to_vec(&library).map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        if raw.len() > MAX_BYTES {
            return Err(WorkcopyError::new("storage_limit_reached"));
        }
        let dir = self
            .0
            .path(PATH, true)?
            .ok_or_else(|| WorkcopyError::new("storage_unavailable"))?;
        write_atomic(&dir, FILE, &raw)?;
        Ok(saved)
    }
}

fn corrupt() -> WorkcopyError {
    WorkcopyError::new("catalog_library_corrupt")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workcopy::store::{CrashPoint, fail};

    fn template(delay: u32) -> Vec<ImportedTemplate> {
        let input = format!(
            r#"{{"zabbix_export":{{"version":"7.4","templates":[{{"template":"synthetic","items":[{{"name":"Ping","key":"agent.ping","delay":{delay}}}]}}]}}}}"#
        );
        inspect_template_file("synthetic.json", input.as_bytes())
            .unwrap_or_else(|| panic!("known extension"))
            .unwrap_or_else(|error| panic!("invalid test fixture: {error}"))
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
