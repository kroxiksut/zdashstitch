// SPDX-License-Identifier: MIT OR Apache-2.0

//! Start-up retention of the publication journal (owner decision
//! 28.10.2026, «Вопросы 28.10.2026 (вечер)» item 12).
//!
//! A journal file is deleted only when all of these hold:
//!
//! - it is a journal file directly in `journal/` (`<operation_id>.jsonl`);
//!   nothing below it (`journal/quarantine/`) and nothing outside it is
//!   looked at;
//! - it replays (a damaged file is never touched) and has committed lines;
//! - its entry is [`JournalProgress::Finished`]: a final state, not running;
//!   an entry that blocks its target (`submitting`, `unknown_outcome`,
//!   `failed_after_response`, `manual_check_required`, running) or was
//!   never sent (`confirmed`, abandoned) is kept;
//! - its last committed line is older than [`JOURNAL_RETENTION_DAYS`]; a
//!   time in the future (clock moved back) is never old.
//!
//! The file is opened without following links for the decision, and right
//! before the deletion its link metadata must still name a regular file.
//! A file that qualified but could not be deleted is reported and kept; the
//! next start tries again.

use cap_std::fs::Dir;

use super::{JournalError, JournalProgress, PublishJournal, RecoveryReport, file, load, running};
use crate::settings::cap_metadata_is_reparse;
use crate::workcopy::store::sync_dir;

/// Finished journal entries are kept this many days after their last line.
pub const JOURNAL_RETENTION_DAYS: u64 = 90;

const DAY_MS: u64 = 24 * 60 * 60 * 1_000;

/// A finished entry the retention pass could not delete; it stays.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionFailure {
    pub operation_id: String,
    pub error: JournalError,
}

/// What a retention pass deleted. Codes and IDs only.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RetentionReport {
    /// Operation IDs whose journal files were deleted, sorted.
    pub removed: Vec<String>,
    /// Expired finished entries that could not be deleted.
    pub failed: Vec<RetentionFailure>,
}

/// Start-up result: the retention pass, then the recovery scan.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StartupReport {
    pub retention: RetentionReport,
    pub recovery: RecoveryReport,
}

/// Whether a finished entry whose last line is at `last_at_ms` has expired
/// at `now_ms`.
const fn expired(last_at_ms: u64, now_ms: u64) -> bool {
    last_at_ms < now_ms.saturating_sub(JOURNAL_RETENTION_DAYS * DAY_MS)
}

/// The file is still a regular file (not a link, not a reparse point).
fn still_regular_file(directory: &Dir, name: &str) -> Result<(), JournalError> {
    let metadata = directory
        .symlink_metadata(name)
        .map_err(|_| JournalError::new("storage_unavailable"))?;
    if !metadata.is_file() || metadata.is_symlink() || cap_metadata_is_reparse(&metadata) {
        return Err(JournalError::new("storage_linked_path"));
    }
    Ok(())
}

fn remove(directory: &Dir, operation_id: &str) -> Result<(), JournalError> {
    let name = file::file_name(operation_id);
    still_regular_file(directory, &name)?;
    directory.remove_file(&name).map_err(|error| {
        JournalError::new("journal_write_failed").with("io", format!("{:?}", error.kind()))
    })
}

impl PublishJournal {
    /// Start-up (P1-01G §4): deletes expired finished entries, then runs
    /// the recovery scan. Call once when the application starts, before any
    /// plan; nothing is sent.
    pub fn start_up(&self) -> Result<StartupReport, JournalError> {
        let retention = self.prune_expired()?;
        let recovery = self.recover()?;
        Ok(StartupReport {
            retention,
            recovery,
        })
    }

    /// Deletes finished entries whose last line is older than
    /// [`JOURNAL_RETENTION_DAYS`] (rules in the module docs). A missing
    /// journal directory is an empty report.
    pub fn prune_expired(&self) -> Result<RetentionReport, JournalError> {
        self.prune_expired_at(crate::workcopy::now_ms())
    }

    /// [`Self::prune_expired`] with the current time `now_ms` (Unix epoch
    /// milliseconds) injected.
    pub(crate) fn prune_expired_at(&self, now_ms: u64) -> Result<RetentionReport, JournalError> {
        let Some(directory) = file::directory(&self.store, false)? else {
            return Ok(RetentionReport::default());
        };
        let mut report = RetentionReport::default();
        for operation_id in file::operation_ids(&directory)? {
            // Read again for every file: an operation that started since is
            // running and never finished.
            let running = running().clone();
            let Ok(Some(entry)) = load(&directory, &operation_id, &running) else {
                continue;
            };
            let Some(last) = entry.transitions.last() else {
                continue;
            };
            if entry.progress != JournalProgress::Finished || !expired(last.at_ms, now_ms) {
                continue;
            }
            match remove(&directory, &operation_id) {
                Ok(()) => report.removed.push(operation_id),
                Err(error) => report.failed.push(RetentionFailure {
                    operation_id,
                    error,
                }),
            }
        }
        if !report.removed.is_empty() {
            sync_dir(&directory);
        }
        Ok(report)
    }
}
