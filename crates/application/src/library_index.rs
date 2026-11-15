// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rebuildable local library cache. Object stores remain authoritative.

use std::{path::PathBuf, sync::Mutex};

use serde::{Deserialize, Serialize};

use crate::{
    catalog_library::{CatalogLibrary, CatalogSummary},
    offline_dashboards::OfflineDashboardStore,
    offline_pages::OfflinePageStore,
    page_library::{LibraryPage, PageLibrary},
    snapshot_library::{DamagedSnapshot, SnapshotLibrary, SnapshotSummary},
    workcopy::{
        WorkcopyError,
        store::{LocalStore, read_bounded, write_atomic},
    },
};

const FILE: &str = "library-index.json";
const MAX_BYTES: usize = 2 * 1024 * 1024;
static CACHE_LOCK: Mutex<()> = Mutex::new(());

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LibraryIndex {
    format: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub cache_write_failed: bool,
    pub catalogs: Vec<CatalogEntry>,
    pub pages: Vec<PageEntry>,
    #[serde(default)]
    pub snapshots: Vec<SnapshotSummary>,
    #[serde(default)]
    pub damaged_snapshots: Vec<DamagedSnapshot>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub snapshot_references_incomplete: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    pub library_id: String,
    pub revision: u64,
    pub catalog_id: String,
    pub version: u32,
    pub name: String,
    pub export_format_version: Option<String>,
    pub metrics: usize,
    pub used_by: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PageEntry {
    pub library_id: String,
    pub revision: u64,
    pub source_local_id: String,
    pub saved_at_ms: u64,
    pub name: String,
    pub widgets: usize,
    pub catalog_versions: Vec<(String, u32)>,
}

impl LibraryIndex {
    /// Always read canonical objects first. A malformed object store is an error,
    /// never an empty result inferred from the cache.
    pub fn open(root: PathBuf) -> Result<Self, WorkcopyError> {
        let catalogs = CatalogLibrary::new(root.clone()).list()?;
        let pages = PageLibrary::new(root.clone()).entries()?;
        let snapshot_library = SnapshotLibrary::open(root.clone())?;
        let mut canonical = Self::build(root.clone(), catalogs, pages)?;
        canonical.snapshots = snapshot_library.snapshots;
        canonical.damaged_snapshots = snapshot_library.damaged;
        canonical.snapshot_references_incomplete = snapshot_library.references_incomplete;
        let _lock = CACHE_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        let store = LocalStore::at(root);
        let cached = match store.path(&[], false)? {
            Some(dir) => match read_bounded(&dir, FILE, MAX_BYTES) {
                Ok(Some(raw)) => serde_json::from_slice::<Self>(&raw).ok(),
                Ok(None) => None,
                Err(error) if error.code() == "storage_file_too_large" => None,
                Err(error) => return Err(error),
            },
            None => None,
        };
        if cached.as_ref() != Some(&canonical) {
            let raw = serde_json::to_vec(&canonical)
                .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
            if raw.len() > MAX_BYTES {
                return Err(WorkcopyError::new("storage_limit_reached"));
            }
            let dir = store
                .path(&[], true)?
                .ok_or_else(|| WorkcopyError::new("storage_unavailable"))?;
            if write_atomic(&dir, FILE, &raw).is_err() {
                canonical.cache_write_failed = true;
            }
        }
        Ok(canonical)
    }

    fn build(
        root: PathBuf,
        catalogs: Vec<CatalogSummary>,
        pages: Vec<LibraryPage>,
    ) -> Result<Self, WorkcopyError> {
        let draft_pages = OfflinePageStore::new(root.clone()).list()?;
        let draft_dashboards = OfflineDashboardStore::new(root).entries()?;
        let mut page_entries = Vec::with_capacity(pages.len());
        for page in pages {
            let summary = page.summary()?;
            page_entries.push(PageEntry {
                library_id: summary.library_id,
                revision: summary.revision,
                source_local_id: summary.source_local_id,
                saved_at_ms: summary.saved_at_ms,
                name: summary.name,
                widgets: summary.widgets,
                catalog_versions: page
                    .catalog_versions
                    .into_iter()
                    .map(|pin| (pin.catalog_id, pin.version))
                    .collect(),
            });
        }
        let catalog_entries = catalogs
            .into_iter()
            .map(|catalog| {
                Ok(CatalogEntry {
                    used_by: page_entries
                        .iter()
                        .filter(|page| {
                            page.catalog_versions
                                .iter()
                                .any(|pin| pin.0 == catalog.catalog_id && pin.1 == catalog.version)
                        })
                        .map(|page| page.library_id.clone())
                        .chain(
                            draft_pages
                                .iter()
                                .filter(|page| {
                                    page.catalog_versions.iter().any(|pin| {
                                        pin.catalog_id == catalog.catalog_id
                                            && pin.version == catalog.version
                                    })
                                })
                                .map(|page| page.local_id.clone()),
                        )
                        .chain(
                            draft_dashboards
                                .iter()
                                .filter(|dashboard| {
                                    dashboard.catalog_versions.iter().any(|pin| {
                                        pin.catalog_id == catalog.catalog_id
                                            && pin.version == catalog.version
                                    })
                                })
                                .map(|dashboard| dashboard.local_id.clone()),
                        )
                        .collect(),
                    catalog_id: catalog.catalog_id,
                    library_id: catalog
                        .library_id
                        .ok_or_else(|| WorkcopyError::new("catalog_library_corrupt"))?,
                    revision: catalog.revision,
                    version: catalog.version,
                    name: catalog.name,
                    export_format_version: catalog.export_format_version,
                    metrics: catalog.metrics,
                })
            })
            .collect::<Result<Vec<_>, WorkcopyError>>()?;
        Ok(Self {
            format: "zdashstitch.library-index/v0".to_owned(),
            cache_write_failed: false,
            catalogs: catalog_entries,
            pages: page_entries,
            snapshots: Vec::new(),
            damaged_snapshots: Vec::new(),
            snapshot_references_incomplete: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog_library::inspect_template_file;
    use crate::workcopy::store::{CrashPoint, fail};

    #[test]
    fn rebuilds_missing_stale_and_malformed_cache_without_losing_catalogs() {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let root_path = root.path().to_owned();
        let empty = LibraryIndex::open(root_path.clone()).unwrap_or_else(|error| panic!("{error}"));
        assert!(empty.catalogs.is_empty());
        let template = inspect_template_file("synthetic.json", br#"{"zabbix_export":{"version":"7.4","templates":[{"template":"synthetic","items":[{"name":"Ping","key":"agent.ping","delay":30}]}]}}"#)
            .unwrap_or_else(|| panic!("known extension"))
            .unwrap_or_else(|error| panic!("{error}"));
        CatalogLibrary::new(root_path.clone())
            .save(template)
            .unwrap_or_else(|error| panic!("{error}"));
        let populated =
            LibraryIndex::open(root_path.clone()).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(populated.catalogs.len(), 1);
        assert_eq!(
            LibraryIndex::open(root_path.clone()).unwrap_or_else(|error| panic!("{error}")),
            populated
        );
        std::fs::write(root.path().join(FILE), b"{partial")
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            LibraryIndex::open(root_path.clone()).unwrap_or_else(|error| panic!("{error}")),
            populated
        );
        std::fs::write(root.path().join(FILE), vec![b'x'; MAX_BYTES + 1])
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            LibraryIndex::open(root_path.clone()).unwrap_or_else(|error| panic!("{error}")),
            populated
        );
        assert_eq!(
            serde_json::from_slice::<LibraryIndex>(
                &std::fs::read(root.path().join(FILE)).unwrap_or_else(|error| panic!("{error}"))
            )
            .unwrap_or_else(|error| panic!("{error}")),
            populated
        );
        std::fs::write(
            root.path().join("library/catalogs/template-exports.json"),
            b"{broken",
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert!(LibraryIndex::open(root_path).is_err());
    }

    #[test]
    fn concurrent_rebuilds_produce_one_valid_cache() {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let expected =
            LibraryIndex::open(root.path().to_owned()).unwrap_or_else(|error| panic!("{error}"));
        std::fs::write(root.path().join(FILE), b"{partial")
            .unwrap_or_else(|error| panic!("{error}"));
        std::thread::scope(|scope| {
            let expected = &expected;
            for _ in 0..8 {
                let path = root.path().to_owned();
                scope.spawn(move || {
                    let actual = LibraryIndex::open(path).unwrap_or_else(|error| panic!("{error}"));
                    assert_eq!(&actual, expected);
                });
            }
        });
        assert!(
            serde_json::from_slice::<LibraryIndex>(
                &std::fs::read(root.path().join(FILE)).unwrap_or_else(|error| panic!("{error}"))
            )
            .is_ok()
        );
    }

    #[test]
    fn cache_write_failure_keeps_authoritative_listing_available() {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let path = root.path().to_owned();
        let _ = LibraryIndex::open(path.clone()).unwrap_or_else(|error| panic!("{error}"));
        std::fs::write(root.path().join(FILE), b"{partial")
            .unwrap_or_else(|error| panic!("{error}"));
        fail::AT.with(|point| point.set(Some(CrashPoint::FileRename)));
        let listing = LibraryIndex::open(path).unwrap_or_else(|error| panic!("{error}"));
        fail::AT.with(|point| point.set(None));
        assert!(listing.cache_write_failed);
        assert!(listing.catalogs.is_empty());
        assert_eq!(
            std::fs::read(root.path().join(FILE)).unwrap_or_else(|error| panic!("{error}")),
            b"{partial"
        );
    }
}
