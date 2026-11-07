// SPDX-License-Identifier: MIT OR Apache-2.0

//! Local store of snapshots and workcopies under the configured storage root
//! (O-018 §2, §7; P2-05B).
//!
//! ```text
//! <storage_root>/
//!   snapshots/<connection_id>/<dashboardid>/<fingerprint>.json   raw-snapshot/v0, write-once
//!   workcopies/<workcopy_id>/workcopy.json                       workcopy/v0
//! ```
//!
//! Path components are only local IDs, Zabbix numeric IDs and fingerprints,
//! never names. The file name is the storage fingerprint
//! (`VersionedDashboardSnapshot::storage_fingerprint`, exact raw bytes,
//! unchanged since P0), so snapshots stored by earlier builds keep loading.
//! Server state is compared with the canonical fingerprint, which is
//! recomputed from the raw document on every load and never persisted
//! here (LAB-02, 28.10.2026). The storage root itself may be a link or a network path
//! (revised S-1); everything **below** it is opened without following links
//! or reparse points. Writes go to a temporary file in the same directory,
//! are synced and renamed into place; a new workcopy is assembled in a
//! staging directory and renamed as a whole, so an interrupted import leaves
//! no partial workcopy. On Unix directories are created `0700` and files
//! `0600`; on Windows they inherit the ACL of the storage root, which by
//! default is the user's profile directory (`%LOCALAPPDATA%`).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cap_fs_ext::{DirExt as _, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, DirBuilder, OpenOptions},
};
use zdashstitch_zabbix::{VersionedDashboardSnapshot, ZabbixVersion};

use super::WorkcopyError;
use super::formats::{
    LOCAL_ONLY, SNAPSHOT_FORMAT, SnapshotFile, WORKCOPY_FORMAT, WorkcopyFile, check_format,
};
use crate::settings::cap_metadata_is_reparse;

const SNAPSHOTS: &str = "snapshots";
const WORKCOPIES: &str = "workcopies";
const WORKCOPY_FILE: &str = "workcopy.json";
const LOCAL_SNAPSHOT_FILE: &str = "local.json";
const TEMP_PREFIX: &str = ".zds-";
const TEMP_SUFFIX: &str = ".tmp";
const STAGING_PREFIX: &str = ".zds-staging-";

/// The Zabbix transport accepts responses up to 32 MiB; the envelope adds a
/// few hundred bytes.
pub(crate) const MAX_SNAPSHOT_FILE_BYTES: usize = 33 * 1024 * 1024;
pub(crate) const MAX_WORKCOPY_FILE_BYTES: usize = 8 * 1024 * 1024;
/// Upper bound of workcopies listed or created in one storage root.
pub(crate) const MAX_WORKCOPIES: usize = 2_000;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Crash points: a failure injected right before a commit rename.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CrashPoint {
    /// Before renaming any temporary file into place.
    FileRename,
    /// Before renaming a staged workcopy directory into place.
    WorkcopyCommit,
}

#[cfg(test)]
pub(crate) mod fail {
    //! Test-only crash simulation, per test thread.
    use std::cell::Cell;

    use super::CrashPoint;

    thread_local! {
        pub(crate) static AT: Cell<Option<CrashPoint>> = const { Cell::new(None) };
    }

    pub(crate) fn triggered(point: CrashPoint) -> bool {
        AT.with(Cell::get) == Some(point)
    }
}

#[cfg(not(test))]
mod fail {
    use super::CrashPoint;

    pub(crate) const fn triggered(_point: CrashPoint) -> bool {
        false
    }
}

fn io_error(error: &std::io::Error) -> WorkcopyError {
    if error.kind() == std::io::ErrorKind::NotFound {
        WorkcopyError::new("storage_unavailable")
    } else {
        WorkcopyError::new("storage_unavailable").with("io", format!("{:?}", error.kind()))
    }
}

fn write_error(error: &std::io::Error) -> WorkcopyError {
    WorkcopyError::new("storage_write_failed").with("io", format!("{:?}", error.kind()))
}

#[derive(Clone, Debug)]
pub(crate) struct LocalStore {
    root: PathBuf,
}

impl LocalStore {
    pub(crate) fn at(root: PathBuf) -> Self {
        Self { root }
    }

    /// Opens the storage root. A missing root is created only for writes.
    fn root(&self, create: bool) -> Result<Option<Dir>, WorkcopyError> {
        if !self.root.is_absolute() {
            return Err(WorkcopyError::new("storage_unavailable"));
        }
        if !self.root.exists() {
            if !create {
                return Ok(None);
            }
            std::fs::create_dir_all(&self.root).map_err(|error| write_error(&error))?;
            restrict_ambient_directory(&self.root);
        }
        Dir::open_ambient_dir(&self.root, ambient_authority())
            .map(Some)
            .map_err(|error| io_error(&error))
    }

    /// Opens `name` below `parent` without following links; creates it
    /// (private) when `create` is set.
    fn subdir(parent: &Dir, name: &str, create: bool) -> Result<Option<Dir>, WorkcopyError> {
        match parent.symlink_metadata(name) {
            Ok(metadata)
                if metadata.file_type().is_symlink()
                    || cap_metadata_is_reparse(&metadata)
                    || !metadata.is_dir() =>
            {
                return Err(WorkcopyError::new("storage_linked_path"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !create {
                    return Ok(None);
                }
                match parent.create_dir_with(name, &private_dir_builder()) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(write_error(&error)),
                }
            }
            Err(error) => return Err(io_error(&error)),
        }
        parent
            .open_dir_nofollow(name)
            .map(Some)
            .map_err(|_| WorkcopyError::new("storage_linked_path"))
    }

    /// Opens `components` below the root without following links;
    /// creates missing private directories when `create` is set.
    pub(crate) fn path(
        &self,
        components: &[&str],
        create: bool,
    ) -> Result<Option<Dir>, WorkcopyError> {
        let Some(mut directory) = self.root(create)? else {
            return Ok(None);
        };
        for component in components {
            let Some(next) = Self::subdir(&directory, component, create)? else {
                return Ok(None);
            };
            directory = next;
        }
        Ok(Some(directory))
    }

    /// Stores an immutable snapshot. Returns `true` when an identical,
    /// verified snapshot already existed and was reused. An existing file
    /// with this fingerprint that fails verification is never overwritten.
    pub(crate) fn put_snapshot(&self, file: &SnapshotFile) -> Result<bool, WorkcopyError> {
        let raw = serde_json::to_vec(file).map_err(|_| WorkcopyError::new("snapshot_corrupt"))?;
        if raw.len() > MAX_SNAPSHOT_FILE_BYTES {
            return Err(WorkcopyError::new("snapshot_too_large"));
        }
        let directory = self
            .path(&[SNAPSHOTS, &file.connection_id, &file.dashboardid], true)?
            .ok_or_else(|| WorkcopyError::new("storage_unavailable"))?;
        let name = format!("{}.json", file.fingerprint);
        remove_stale_temporaries(&directory);
        if exists(&directory, &name)? {
            let existing = read_snapshot_file(&directory, &name)?;
            verify_snapshot(
                &existing,
                &file.connection_id,
                &file.dashboardid,
                &file.fingerprint,
            )?;
            return Ok(true);
        }
        write_atomic(&directory, &name, &raw)?;
        Ok(false)
    }

    /// Loads and verifies a snapshot: format, scope, path components and the
    /// recomputed storage fingerprint must all match. Any mismatch is
    /// corruption.
    pub(crate) fn load_snapshot(
        &self,
        connection_id: &str,
        dashboardid: &str,
        fingerprint: &str,
    ) -> Result<(SnapshotFile, VersionedDashboardSnapshot), WorkcopyError> {
        let directory = self
            .path(&[SNAPSHOTS, connection_id, dashboardid], false)?
            .ok_or_else(|| WorkcopyError::new("snapshot_missing"))?;
        let file = read_snapshot_file(&directory, &format!("{fingerprint}.json"))?;
        let snapshot = verify_snapshot(&file, connection_id, dashboardid, fingerprint)?;
        Ok((file, snapshot))
    }

    /// Creates a new workcopy directory atomically: the file is written into
    /// a staging directory, which is then renamed to the workcopy ID.
    pub(crate) fn create_workcopy(&self, file: &WorkcopyFile) -> Result<(), WorkcopyError> {
        let raw = workcopy_bytes(file)?;
        self.create_local_draft(WORKCOPIES, &file.workcopy_id, &raw, None)
    }

    pub(crate) fn create_local_draft(
        &self,
        namespace: &str,
        id: &str,
        raw: &[u8],
        local_snapshot: Option<&[u8]>,
    ) -> Result<(), WorkcopyError> {
        if !super::is_workcopy_id(id) {
            return Err(WorkcopyError::new("workcopy_id_invalid"));
        }
        if raw.len() > MAX_WORKCOPY_FILE_BYTES {
            return Err(WorkcopyError::new("workcopy_too_large"));
        }
        if local_snapshot.is_some_and(|raw| raw.len() > MAX_SNAPSHOT_FILE_BYTES) {
            return Err(WorkcopyError::new("snapshot_too_large"));
        }
        let workcopies = self
            .path(&[namespace], true)?
            .ok_or_else(|| WorkcopyError::new("storage_unavailable"))?;
        remove_stale_staging(&workcopies);
        if count_workcopies(&workcopies)? >= MAX_WORKCOPIES {
            return Err(
                WorkcopyError::new("storage_limit_reached").with("max", MAX_WORKCOPIES.to_string())
            );
        }
        if exists_any(&workcopies, id)? {
            return Err(WorkcopyError::new("storage_write_failed").with("io", "AlreadyExists"));
        }
        let staging_name = format!("{STAGING_PREFIX}{}-{id}", own_marker());
        workcopies
            .create_dir_with(&staging_name, &private_dir_builder())
            .map_err(|error| write_error(&error))?;
        let result = (|| {
            {
                // The handle is closed before the rename: Windows refuses to
                // rename a directory that is still open.
                let staging = workcopies
                    .open_dir_nofollow(&staging_name)
                    .map_err(|error| write_error(&error))?;
                write_atomic(&staging, WORKCOPY_FILE, raw)?;
                if let Some(local) = local_snapshot {
                    write_atomic(&staging, LOCAL_SNAPSHOT_FILE, local)?;
                }
            }
            if fail::triggered(CrashPoint::WorkcopyCommit) {
                return Err(WorkcopyError::new("storage_write_failed").with("io", "Simulated"));
            }
            workcopies
                .rename(&staging_name, &workcopies, id)
                .map_err(|error| write_error(&error))?;
            sync_dir(&workcopies);
            Ok(())
        })();
        if result.is_err() {
            let _ = workcopies.remove_dir_all(&staging_name);
        }
        result
    }

    /// Replaces `workcopy.json` of an existing workcopy (temp → sync → rename).
    pub(crate) fn replace_workcopy(&self, file: &WorkcopyFile) -> Result<(), WorkcopyError> {
        let raw = workcopy_bytes(file)?;
        let directory = self
            .path(&[WORKCOPIES, &file.workcopy_id], false)?
            .ok_or_else(|| WorkcopyError::new("workcopy_not_found"))?;
        remove_stale_temporaries(&directory);
        write_atomic(&directory, WORKCOPY_FILE, &raw)
    }

    pub(crate) fn load_workcopy(&self, workcopy_id: &str) -> Result<WorkcopyFile, WorkcopyError> {
        let raw = self.load_local_draft(WORKCOPIES, workcopy_id)?;
        check_format(
            &raw,
            WORKCOPY_FORMAT,
            "zdashstitch.workcopy/",
            "workcopy_unsupported_version",
            "workcopy_corrupt",
        )?;
        let file: WorkcopyFile =
            serde_json::from_slice(&raw).map_err(|_| WorkcopyError::new("workcopy_corrupt"))?;
        file.validate(workcopy_id)?;
        Ok(file)
    }

    pub(crate) fn load_local_draft(
        &self,
        namespace: &str,
        id: &str,
    ) -> Result<Vec<u8>, WorkcopyError> {
        self.read_local_draft_file(namespace, id, WORKCOPY_FILE, MAX_WORKCOPY_FILE_BYTES)
    }

    pub(crate) fn load_draft_snapshot(
        &self,
        namespace: &str,
        id: &str,
    ) -> Result<Vec<u8>, WorkcopyError> {
        self.read_local_draft_file(namespace, id, LOCAL_SNAPSHOT_FILE, MAX_SNAPSHOT_FILE_BYTES)
    }

    fn read_local_draft_file(
        &self,
        namespace: &str,
        id: &str,
        name: &str,
        limit: usize,
    ) -> Result<Vec<u8>, WorkcopyError> {
        if !super::is_workcopy_id(id) {
            return Err(WorkcopyError::new("workcopy_id_invalid"));
        }
        let directory = self
            .path(&[namespace, id], false)?
            .ok_or_else(|| WorkcopyError::new("workcopy_not_found"))?;
        read_bounded(&directory, name, limit)?.ok_or_else(|| WorkcopyError::new("workcopy_corrupt"))
    }

    /// IDs of committed workcopies, sorted. Staging and temporary entries
    /// are skipped; foreign or linked entries are ignored, never followed.
    pub(crate) fn workcopy_ids(&self) -> Result<Vec<String>, WorkcopyError> {
        let Some(workcopies) = self.path(&[WORKCOPIES], false)? else {
            return Ok(Vec::new());
        };
        let mut ids = Vec::new();
        for entry in workcopies.entries().map_err(|error| io_error(&error))? {
            let entry = entry.map_err(|error| io_error(&error))?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if !super::is_workcopy_id(&name) {
                continue;
            }
            ids.push(name);
            if ids.len() > MAX_WORKCOPIES {
                return Err(WorkcopyError::new("storage_limit_reached")
                    .with("max", MAX_WORKCOPIES.to_string()));
            }
        }
        ids.sort_unstable();
        Ok(ids)
    }
}

fn workcopy_bytes(file: &WorkcopyFile) -> Result<Vec<u8>, WorkcopyError> {
    let raw = serde_json::to_vec(file).map_err(|_| WorkcopyError::new("workcopy_corrupt"))?;
    if raw.len() > MAX_WORKCOPY_FILE_BYTES {
        return Err(WorkcopyError::new("workcopy_too_large"));
    }
    Ok(raw)
}

fn read_snapshot_file(directory: &Dir, name: &str) -> Result<SnapshotFile, WorkcopyError> {
    let raw = read_bounded(directory, name, MAX_SNAPSHOT_FILE_BYTES)?
        .ok_or_else(|| WorkcopyError::new("snapshot_missing"))?;
    check_format(
        &raw,
        SNAPSHOT_FORMAT,
        "zdashstitch.raw-snapshot/",
        "snapshot_unsupported_version",
        "snapshot_corrupt",
    )?;
    serde_json::from_slice(&raw).map_err(|_| WorkcopyError::new("snapshot_corrupt"))
}

fn verify_snapshot(
    file: &SnapshotFile,
    connection_id: &str,
    dashboardid: &str,
    fingerprint: &str,
) -> Result<VersionedDashboardSnapshot, WorkcopyError> {
    let corrupt = || WorkcopyError::new("snapshot_corrupt");
    if file.storage_scope != LOCAL_ONLY
        || file.connection_id != connection_id
        || file.dashboardid != dashboardid
        || file.fingerprint != fingerprint
        || file.first_read_fingerprint != fingerprint
        || file.last_read_fingerprint != fingerprint
    {
        return Err(corrupt());
    }
    let version: ZabbixVersion = file.server_version.parse().map_err(|_| corrupt())?;
    let snapshot = VersionedDashboardSnapshot::from_adapter_raw(
        file.connection_id.clone(),
        version,
        file.raw_api_document.clone(),
    )
    .map_err(|_| corrupt())?;
    if snapshot.adapter_id() != file.adapter_id {
        return Err(WorkcopyError::new("snapshot_unsupported_adapter"));
    }
    if snapshot.storage_fingerprint() != fingerprint || snapshot.dashboard_id() != dashboardid {
        return Err(corrupt());
    }
    Ok(snapshot)
}

fn exists(directory: &Dir, name: &str) -> Result<bool, WorkcopyError> {
    match directory.symlink_metadata(name) {
        Ok(metadata)
            if metadata.file_type().is_symlink()
                || cap_metadata_is_reparse(&metadata)
                || !metadata.is_file() =>
        {
            Err(WorkcopyError::new("storage_linked_path"))
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_error(&error)),
    }
}

fn exists_any(directory: &Dir, name: &str) -> Result<bool, WorkcopyError> {
    match directory.symlink_metadata(name) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_error(&error)),
    }
}

fn count_workcopies(workcopies: &Dir) -> Result<usize, WorkcopyError> {
    let mut count = 0usize;
    for entry in workcopies.entries().map_err(|error| io_error(&error))? {
        let entry = entry.map_err(|error| io_error(&error))?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(super::is_workcopy_id)
        {
            count += 1;
        }
    }
    Ok(count)
}

/// Reads a regular file without following links, at most `limit` bytes.
/// `None` when the file does not exist.
pub(crate) fn read_bounded(
    directory: &Dir,
    name: &str,
    limit: usize,
) -> Result<Option<Vec<u8>>, WorkcopyError> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = match directory.open_with(name, &options) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(WorkcopyError::new("storage_linked_path")),
    };
    let metadata = file.metadata().map_err(|error| io_error(&error))?;
    if !metadata.is_file() || cap_metadata_is_reparse(&metadata) {
        return Err(WorkcopyError::new("storage_linked_path"));
    }
    let limit_u64 = u64::try_from(limit).unwrap_or(u64::MAX);
    if metadata.len() > limit_u64 {
        return Err(WorkcopyError::new("storage_file_too_large"));
    }
    let mut raw = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    Read::by_ref(&mut file)
        .take(limit_u64.saturating_add(1))
        .read_to_end(&mut raw)
        .map_err(|error| io_error(&error))?;
    if raw.len() > limit {
        return Err(WorkcopyError::new("storage_file_too_large"));
    }
    Ok(Some(raw))
}

fn own_marker() -> String {
    format!("{}-", std::process::id())
}

/// Writes `raw` to a fresh private temporary file in `directory`, syncs it
/// and renames it over `name`. The temporary file is removed on failure.
pub(crate) fn write_atomic(directory: &Dir, name: &str, raw: &[u8]) -> Result<(), WorkcopyError> {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temporary = format!("{TEMP_PREFIX}{}{counter}{TEMP_SUFFIX}", own_marker());
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    let mut file = directory
        .open_with(&temporary, &options)
        .map_err(|error| write_error(&error))?;
    let result = (|| {
        restrict_file(&file)?;
        file.write_all(raw).map_err(|error| write_error(&error))?;
        file.sync_all().map_err(|error| write_error(&error))?;
        drop(file);
        if fail::triggered(CrashPoint::FileRename) {
            return Err(WorkcopyError::new("storage_write_failed").with("io", "Simulated"));
        }
        directory
            .rename(&temporary, directory, name)
            .map_err(|error| write_error(&error))?;
        sync_dir(directory);
        Ok(())
    })();
    if result.is_err() {
        let _ = directory.remove_file(&temporary);
    }
    result
}

/// Removes temporary files of other (crashed) processes. Uncommitted data
/// is discarded; committed files are never touched.
fn remove_stale_temporaries(directory: &Dir) {
    let Ok(entries) = directory.entries() else {
        return;
    };
    let own = format!("{TEMP_PREFIX}{}", own_marker());
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with(TEMP_PREFIX)
            && name.ends_with(TEMP_SUFFIX)
            && !name.starts_with(&own)
            && entry.file_type().is_ok_and(|kind| kind.is_file())
        {
            let _ = directory.remove_file(name);
        }
    }
}

/// Removes staging directories of other (crashed) processes.
fn remove_stale_staging(workcopies: &Dir) {
    let Ok(entries) = workcopies.entries() else {
        return;
    };
    let own = format!("{STAGING_PREFIX}{}", own_marker());
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with(STAGING_PREFIX)
            && !name.starts_with(&own)
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            let _ = workcopies.remove_dir_all(name);
        }
    }
}

pub(crate) fn private_dir_builder() -> DirBuilder {
    #[allow(unused_mut)]
    let mut builder = DirBuilder::new();
    #[cfg(unix)]
    {
        use cap_std::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder
}

#[cfg(unix)]
pub(crate) fn restrict_file(file: &cap_std::fs::File) -> Result<(), WorkcopyError> {
    use cap_std::fs::PermissionsExt as _;
    let mut permissions = file
        .metadata()
        .map_err(|error| write_error(&error))?
        .permissions();
    permissions.set_mode(0o600);
    file.set_permissions(permissions)
        .map_err(|error| write_error(&error))
}

#[cfg(not(unix))]
pub(crate) fn restrict_file(_file: &cap_std::fs::File) -> Result<(), WorkcopyError> {
    Ok(())
}

#[cfg(unix)]
fn restrict_ambient_directory(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn restrict_ambient_directory(_path: &Path) {}

#[cfg(unix)]
pub(crate) fn sync_dir(directory: &Dir) {
    if let Ok(clone) = directory.try_clone() {
        let _ = clone.into_std_file().sync_all();
    }
}

#[cfg(not(unix))]
pub(crate) fn sync_dir(_directory: &Dir) {}
