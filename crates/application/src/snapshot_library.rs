// SPDX-License-Identifier: MIT OR Apache-2.0

//! Read-only local library view of immutable server snapshots.
//! Raw documents and server object identifiers never enter this view or its cache.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::publish_journal::PublishJournal;
use crate::workcopy::{WorkcopyError, store::LocalStore};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SnapshotSummary {
    pub library_id: String,
    pub connection_id: String,
    pub server_version: String,
    pub read_at_ms: u64,
    pub used_by: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DamagedSnapshot {
    pub library_id: String,
    pub code: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SnapshotLibrary {
    pub snapshots: Vec<SnapshotSummary>,
    pub damaged: Vec<DamagedSnapshot>,
    pub references_incomplete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SnapshotComparison {
    pub identical: bool,
    pub left_pages: usize,
    pub right_pages: usize,
    pub left_widgets: usize,
    pub right_widgets: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SnapshotDeletionPreview {
    pub library_id: String,
    pub workcopy_count: usize,
    pub journal_count: usize,
    pub references_unknown: bool,
    pub deletable: bool,
}

impl SnapshotLibrary {
    pub fn deletion_preview(
        root: PathBuf,
        id: &str,
    ) -> Result<SnapshotDeletionPreview, WorkcopyError> {
        let _guard =
            crate::snapshot_guard::lock().map_err(|_| WorkcopyError::new("storage_unavailable"))?;
        Self::inspect_deletion(root, id).map(|(preview, _)| preview)
    }

    pub fn delete(root: PathBuf, id: &str, confirmed: bool) -> Result<(), WorkcopyError> {
        if !confirmed {
            return Err(WorkcopyError::new("snapshot_delete_not_confirmed"));
        }
        let _guard =
            crate::snapshot_guard::lock().map_err(|_| WorkcopyError::new("storage_unavailable"))?;
        let (preview, (connection, dashboard, fingerprint)) =
            Self::inspect_deletion(root.clone(), id)?;
        if !preview.deletable {
            return Err(WorkcopyError::new("snapshot_delete_blocked"));
        }
        LocalStore::at(root).remove_snapshot(&connection, &dashboard, &fingerprint)
    }

    fn inspect_deletion(
        root: PathBuf,
        id: &str,
    ) -> Result<(SnapshotDeletionPreview, (String, String, String)), WorkcopyError> {
        let store = LocalStore::at(root.clone());
        let key = store
            .snapshot_keys()?
            .into_iter()
            .find(|(connection, dashboard, fingerprint)| {
                opaque_id(connection, dashboard, fingerprint) == id
            })
            .ok_or_else(|| WorkcopyError::new("snapshot_library_not_found"))?;
        store.load_snapshot(&key.0, &key.1, &key.2)?;
        let mut workcopy_count = 0;
        let mut references_unknown = store.clone_branches_present()?;
        for copy_id in store.workcopy_ids()? {
            match store.load_workcopy(&copy_id) {
                Ok(copy) => {
                    if copy.source.connection_id == key.0
                        && copy.source.dashboardid == key.1
                        && copy.base.fingerprint == key.2
                    {
                        workcopy_count += 1;
                    }
                }
                Err(_) => references_unknown = true,
            }
        }
        // A journal records a canonical or Local fingerprint, not the raw
        // storage fingerprint. Its source Workcopy may have since advanced or
        // vanished. Any entry for this connection is therefore conservatively
        // a possible reference to any of its saved Base snapshots.
        let journal = PublishJournal::new(root)
            .list()
            .map_err(|_| WorkcopyError::new("snapshot_references_unknown"))?;
        if !journal.damaged.is_empty() {
            references_unknown = true;
        }
        let journal_count = journal
            .entries
            .iter()
            .filter(|entry| entry.connection_id == key.0)
            .count();
        let preview = SnapshotDeletionPreview {
            library_id: id.to_owned(),
            workcopy_count,
            journal_count,
            references_unknown,
            deletable: workcopy_count == 0 && journal_count == 0 && !references_unknown,
        };
        Ok((preview, key))
    }

    /// Compare two already stored Base reads of the same dashboard. The
    /// response contains only counts and equality, never raw field values.
    pub fn compare(
        root: PathBuf,
        left: &str,
        right: &str,
    ) -> Result<SnapshotComparison, WorkcopyError> {
        let store = LocalStore::at(root);
        let mut selected = Vec::new();
        for (connection, dashboard, fingerprint) in store.snapshot_keys()? {
            let id = opaque_id(&connection, &dashboard, &fingerprint);
            if id == left || id == right {
                selected.push((id, connection, dashboard, fingerprint));
            }
        }
        let find = |id: &str| {
            selected
                .iter()
                .find(|entry| entry.0 == id)
                .ok_or_else(|| WorkcopyError::new("snapshot_library_not_found"))
        };
        let a = find(left)?;
        let b = find(right)?;
        if a.1 != b.1 || a.2 != b.2 {
            return Err(WorkcopyError::new("snapshot_scope_mismatch"));
        }
        let (left_file, left_snapshot) = store.load_snapshot(&a.1, &a.2, &a.3)?;
        let (right_file, right_snapshot) = store.load_snapshot(&b.1, &b.2, &b.3)?;
        if left_file.scope != right_file.scope
            || left_file.adapter_id != right_file.adapter_id
            || left_file.server_version != right_file.server_version
        {
            return Err(WorkcopyError::new("snapshot_scope_mismatch"));
        }
        let counts = |raw: &serde_json::Value| {
            let pages = raw.get("pages").and_then(serde_json::Value::as_array);
            (
                pages.map_or(0, Vec::len),
                pages.map_or(0, |pages| {
                    pages
                        .iter()
                        .map(|page| {
                            page.get("widgets")
                                .and_then(serde_json::Value::as_array)
                                .map_or(0, Vec::len)
                        })
                        .sum()
                }),
            )
        };
        let (left_pages, left_widgets) = counts(left_snapshot.raw_api_document());
        let (right_pages, right_widgets) = counts(right_snapshot.raw_api_document());
        Ok(SnapshotComparison {
            identical: left_snapshot.fingerprint() == right_snapshot.fingerprint(),
            left_pages,
            right_pages,
            left_widgets,
            right_widgets,
        })
    }

    pub fn open(root: PathBuf) -> Result<Self, WorkcopyError> {
        let store = LocalStore::at(root);
        let mut references = Vec::new();
        let mut references_incomplete = false;
        // A damaged workcopy can hold a Base reference. The view remains
        // available, but removal must stay disabled until all references can
        // be proven absent. No removal action is exposed in this slice.
        for id in store.workcopy_ids()? {
            match store.load_workcopy(&id) {
                Ok(copy) => references.push((
                    copy.source.connection_id,
                    copy.source.dashboardid,
                    copy.base.fingerprint,
                    id,
                )),
                Err(_) => references_incomplete = true,
            }
        }
        let mut snapshots = Vec::new();
        let mut damaged = Vec::new();
        for (connection, dashboard, fingerprint) in store.snapshot_keys()? {
            let library_id = opaque_id(&connection, &dashboard, &fingerprint);
            match store.load_snapshot(&connection, &dashboard, &fingerprint) {
                Ok((file, _)) => {
                    let mut used_by: Vec<String> = references
                        .iter()
                        .filter(|(c, d, f, _)| {
                            c == &connection && d == &dashboard && f == &fingerprint
                        })
                        .map(|(_, _, _, id)| id.clone())
                        .collect();
                    used_by.sort_unstable();
                    snapshots.push(SnapshotSummary {
                        library_id,
                        connection_id: connection,
                        server_version: file.server_version,
                        read_at_ms: file.last_read_at_ms,
                        used_by,
                    });
                }
                Err(error) => damaged.push(DamagedSnapshot {
                    library_id,
                    code: error.code().to_owned(),
                }),
            }
        }
        snapshots.sort_by(|a, b| {
            b.read_at_ms
                .cmp(&a.read_at_ms)
                .then_with(|| a.library_id.cmp(&b.library_id))
        });
        Ok(Self {
            snapshots,
            damaged,
            references_incomplete,
        })
    }
}

fn opaque_id(connection: &str, dashboard: &str, fingerprint: &str) -> String {
    let mut digest = Sha256::new();
    for part in [connection, dashboard, fingerprint] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    format!("snap-{:x}", digest.finalize())
}
