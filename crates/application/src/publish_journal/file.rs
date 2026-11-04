// SPDX-License-Identifier: MIT OR Apache-2.0

//! Journal files below `<storage_root>/journal/` (O-018 §2, §7).
//!
//! Each operation is `journal/<operation_id>.jsonl`. The first line is
//! written to a new file (`create_new`); later lines are appended through
//! one no-follow handle that first re-reads and replays the whole file, so
//! a transition is appended only onto the state it was decided from. Every
//! line is followed by `sync_all` (fsync) before the call returns; a
//! failed sync is a failed transition.

use std::io::{Read, Seek, SeekFrom, Write};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions};

use super::record::{Committed, Damage, LineFile, MAX_JOURNAL_FILE_BYTES, split};
use super::{JournalError, JournalState};
use crate::settings::cap_metadata_is_reparse;
use crate::workcopy::store::{LocalStore, restrict_file, sync_dir};

pub(crate) const JOURNAL_DIR: &str = "journal";
pub(crate) const JOURNAL_SUFFIX: &str = ".jsonl";
/// Upper bound of journal files scanned in one storage root.
pub(crate) const MAX_JOURNAL_FILES: usize = 10_000;

/// Injected failures of a journal append (tests only).
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FailKind {
    /// Nothing is written.
    BeforeWrite,
    /// The line is written but `sync_all` reports a failure.
    Sync,
    /// Half of the line is written, then the process "crashes".
    Torn,
}

fn write_error(error: &std::io::Error) -> JournalError {
    JournalError::new("journal_write_failed").with("io", format!("{:?}", error.kind()))
}

fn simulated() -> JournalError {
    JournalError::new("journal_write_failed").with("io", "Simulated")
}

pub(crate) fn file_name(operation_id: &str) -> String {
    format!("{operation_id}{JOURNAL_SUFFIX}")
}

/// The journal directory; `None` when it does not exist and `create` is off.
pub(crate) fn directory(store: &LocalStore, create: bool) -> Result<Option<Dir>, JournalError> {
    store
        .path(&[JOURNAL_DIR], create)
        .map_err(|error| JournalError::from_storage(&error))
}

fn open_existing(directory: &Dir, name: &str, write: bool) -> Result<Option<File>, JournalError> {
    let mut options = OpenOptions::new();
    options.read(true).write(write).follow(FollowSymlinks::No);
    let file = match directory.open_with(name, &options) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(JournalError::new("storage_linked_path")),
    };
    let metadata = file
        .metadata()
        .map_err(|_| JournalError::new("storage_unavailable"))?;
    if !metadata.is_file() || cap_metadata_is_reparse(&metadata) {
        return Err(JournalError::new("storage_linked_path"));
    }
    Ok(Some(file))
}

fn read_all(file: &mut File) -> Result<Vec<u8>, JournalError> {
    let limit = u64::try_from(MAX_JOURNAL_FILE_BYTES).unwrap_or(u64::MAX);
    let mut raw = Vec::new();
    Read::by_ref(file)
        .take(limit.saturating_add(1))
        .read_to_end(&mut raw)
        .map_err(|_| JournalError::new("storage_unavailable"))?;
    if raw.len() > MAX_JOURNAL_FILE_BYTES {
        return Err(JournalError::new("storage_file_too_large"));
    }
    Ok(raw)
}

/// Reads and splits one journal file; `None` when it does not exist.
pub(crate) fn read(directory: &Dir, name: &str) -> Result<Option<Committed>, Damage> {
    let to_damage = |error| Damage {
        error,
        operation: None,
    };
    let Some(mut file) = open_existing(directory, name, false).map_err(to_damage)? else {
        return Ok(None);
    };
    let raw = read_all(&mut file).map_err(to_damage)?;
    split(&raw).map(Some)
}

/// Writes `raw` (one complete line) and syncs it, honouring injected
/// failures for `state`.
fn write_line(file: &mut File, raw: &[u8], state: JournalState) -> Result<(), JournalError> {
    match fail::kind(state) {
        Some(FailKind::BeforeWrite) => return Err(simulated()),
        Some(FailKind::Torn) => {
            let half = raw.len() / 2;
            file.write_all(&raw[..half])
                .map_err(|error| write_error(&error))?;
            let _ = file.sync_all();
            return Err(simulated());
        }
        Some(FailKind::Sync) => {
            file.write_all(raw).map_err(|error| write_error(&error))?;
            return Err(simulated());
        }
        None => {}
    }
    file.write_all(raw).map_err(|error| write_error(&error))?;
    file.sync_all().map_err(|error| write_error(&error))
}

/// Creates the file of a new operation with its first line.
pub(crate) fn create(directory: &Dir, name: &str, line: &LineFile) -> Result<(), JournalError> {
    let raw = line.to_bytes()?;
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    let mut file = directory.open_with(name, &options).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            JournalError::new("journal_write_failed").with("io", "AlreadyExists")
        } else {
            write_error(&error)
        }
    })?;
    restrict_file(&file).map_err(|error| JournalError::from_storage(&error))?;
    write_line(&mut file, &raw, JournalState::Confirmed)?;
    drop(file);
    sync_dir(directory);
    Ok(())
}

/// Appends one line after `check` accepted the replayed committed lines.
/// A torn tail is cut off first, so the new line starts on a line boundary.
pub(crate) fn append<F>(
    directory: &Dir,
    name: &str,
    state: JournalState,
    check: F,
) -> Result<(), JournalError>
where
    F: FnOnce(&[LineFile]) -> Result<LineFile, JournalError>,
{
    let mut file = open_existing(directory, name, true)?
        .ok_or_else(|| JournalError::new("operation_not_found"))?;
    let raw = read_all(&mut file)?;
    let committed = split(&raw).map_err(|damage| damage.error)?;
    let line = check(&committed.lines)?;
    let bytes = line.to_bytes()?;
    if raw.len().saturating_add(bytes.len()) > MAX_JOURNAL_FILE_BYTES {
        return Err(JournalError::new("storage_file_too_large"));
    }
    if committed.committed_len != raw.len() {
        let length = u64::try_from(committed.committed_len).unwrap_or(u64::MAX);
        file.set_len(length).map_err(|error| write_error(&error))?;
    }
    file.seek(SeekFrom::End(0))
        .map_err(|error| write_error(&error))?;
    write_line(&mut file, &bytes, state)
}

/// Operation IDs of the journal files, sorted. Entries that are not
/// journal files are ignored; links are reported, never followed.
pub(crate) fn operation_ids(directory: &Dir) -> Result<Vec<String>, JournalError> {
    let mut ids = Vec::new();
    for entry in directory
        .entries()
        .map_err(|_| JournalError::new("storage_unavailable"))?
    {
        let entry = entry.map_err(|_| JournalError::new("storage_unavailable"))?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Some(stem) = name.strip_suffix(JOURNAL_SUFFIX) else {
            continue;
        };
        if !super::is_operation_id(stem) {
            continue;
        }
        ids.push(stem.to_owned());
        if ids.len() > MAX_JOURNAL_FILES {
            return Err(JournalError::new("storage_limit_reached")
                .with("max", MAX_JOURNAL_FILES.to_string()));
        }
    }
    ids.sort_unstable();
    Ok(ids)
}

#[cfg(test)]
pub(crate) mod fail {
    //! Test-only failure injection, per test thread.
    use std::cell::Cell;

    use super::{FailKind, JournalState};

    thread_local! {
        pub(crate) static AT: Cell<Option<(JournalState, FailKind)>> = const { Cell::new(None) };
    }

    pub(crate) fn kind(state: JournalState) -> Option<FailKind> {
        AT.with(Cell::get)
            .and_then(|(at, kind)| (at == state).then_some(kind))
    }
}

#[cfg(not(test))]
mod fail {
    use super::{FailKind, JournalState};

    pub(crate) const fn kind(_state: JournalState) -> Option<FailKind> {
        None
    }
}
