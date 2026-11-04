// SPDX-License-Identifier: MIT OR Apache-2.0

//! «Убрать в карантин» (P2-04F, OPEN_DECISIONS 28.10.2026 evening, item 4):
//! a journal file that does not replay blocks its target, or every target
//! when even its target cannot be read, and is never guessed around. After
//! the user's confirmation the file is moved, unchanged, to
//! `journal/quarantine/<operation_id>.jsonl`; the journal scan no longer
//! sees it, so the block is lifted. Nothing is deleted and nothing is sent:
//! a new publication to the same target still starts with a fresh read.
//!
//! Only a damaged file can be moved: an entry that replays has its own
//! recovery path (reconciliation or «Закрыть без сверки»).

use super::file::{self, JOURNAL_DIR};
use super::{JournalError, PublishJournal, is_operation_id, load, running};
use crate::workcopy::store::sync_dir;

/// Subdirectory of `journal/` that holds quarantined files.
pub const QUARANTINE_DIR: &str = "quarantine";

/// «Убрать в карантин»: the user confirmed the dialog for exactly this
/// damaged journal file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QuarantineRequest {
    pub operation_id: String,
    pub confirmed: bool,
}

impl PublishJournal {
    /// Moves a damaged journal file to `journal/quarantine/` after the
    /// user's confirmation; its target is no longer blocked. The file is
    /// moved byte for byte, never rewritten or deleted.
    ///
    /// Errors: `quarantine_not_confirmed`, `operation_id_invalid`,
    /// `operation_not_found`, `operation_running`, `journal_not_damaged`
    /// (the file replays: use reconciliation or «Закрыть без сверки»),
    /// `quarantine_exists`, storage codes.
    pub fn quarantine(&self, request: &QuarantineRequest) -> Result<String, JournalError> {
        if !request.confirmed {
            return Err(JournalError::new("quarantine_not_confirmed"));
        }
        let operation_id = request.operation_id.as_str();
        if !is_operation_id(operation_id) {
            return Err(JournalError::new("operation_id_invalid"));
        }
        let guard = running();
        if guard.contains(operation_id) {
            return Err(JournalError::new("operation_running"));
        }
        let directory = file::directory(&self.store, false)?
            .ok_or_else(|| JournalError::new("operation_not_found"))?;
        match load(&directory, operation_id, &guard) {
            Err(_damaged) => {}
            Ok(Some(_)) => return Err(JournalError::new("journal_not_damaged")),
            Ok(None) => {
                // No committed line: an empty file that nothing lists.
                if directory
                    .symlink_metadata(file::file_name(operation_id))
                    .is_err()
                {
                    return Err(JournalError::new("operation_not_found"));
                }
                return Err(JournalError::new("journal_not_damaged"));
            }
        }
        let quarantine = self
            .store
            .path(&[JOURNAL_DIR, QUARANTINE_DIR], true)
            .map_err(|error| JournalError::from_storage(&error))?
            .ok_or_else(|| JournalError::new("storage_unavailable"))?;
        let name = file::file_name(operation_id);
        if quarantine.symlink_metadata(&name).is_ok() {
            return Err(JournalError::new("quarantine_exists"));
        }
        directory
            .rename(&name, &quarantine, &name)
            .map_err(|error| {
                JournalError::new("journal_write_failed").with("io", format!("{:?}", error.kind()))
            })?;
        sync_dir(&quarantine);
        sync_dir(&directory);
        drop(guard);
        Ok(format!("{JOURNAL_DIR}/{QUARANTINE_DIR}/{name}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal() -> (tempfile::TempDir, PublishJournal) {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"));
        let journal = PublishJournal::new(dir.path().to_path_buf());
        (dir, journal)
    }

    fn request(operation_id: &str, confirmed: bool) -> QuarantineRequest {
        QuarantineRequest {
            operation_id: operation_id.to_owned(),
            confirmed,
        }
    }

    #[test]
    fn a_damaged_file_moves_to_quarantine_and_unblocks() {
        let (dir, journal) = journal();
        let journal_dir = dir.path().join(JOURNAL_DIR);
        std::fs::create_dir_all(&journal_dir).unwrap_or_else(|error| panic!("{error}"));
        let id = "zds-0123456789ab-0123456789ab-00000001-0123456789abcdef";
        let garbage = b"{not json}\n";
        std::fs::write(journal_dir.join(format!("{id}.jsonl")), garbage)
            .unwrap_or_else(|error| panic!("{error}"));
        let listing = journal.list().unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(listing.damaged.len(), 1);

        assert_eq!(
            journal
                .quarantine(&request(id, false))
                .map_err(|e| e.code()),
            Err("quarantine_not_confirmed")
        );
        assert_eq!(journal.list().map(|l| l.damaged.len()), Ok(1));

        let moved = journal
            .quarantine(&request(id, true))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(moved, format!("journal/quarantine/{id}.jsonl"));
        let listing = journal.list().unwrap_or_else(|error| panic!("{error}"));
        assert!(listing.damaged.is_empty() && listing.entries.is_empty());
        // Moved byte for byte, never rewritten.
        let kept = std::fs::read(journal_dir.join(QUARANTINE_DIR).join(format!("{id}.jsonl")))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(kept, garbage);
        assert_eq!(
            journal.quarantine(&request(id, true)).map_err(|e| e.code()),
            Err("operation_not_found")
        );
    }

    #[test]
    fn invalid_ids_are_refused() {
        let (_dir, journal) = journal();
        assert_eq!(
            journal
                .quarantine(&request("../escape", true))
                .map_err(|e| e.code()),
            Err("operation_id_invalid")
        );
        assert_eq!(
            journal
                .quarantine(&request("zds-missing", true))
                .map_err(|e| e.code()),
            Err("operation_not_found")
        );
    }
}
