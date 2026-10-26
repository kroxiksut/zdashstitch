// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use serde::Serialize;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionKind {
    File,
    Directory,
    SaveDestination,
}

#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct OpaqueSelection {
    pub id: String,
    pub display_name: String,
    pub kind: SelectionKind,
    pub expires_in_seconds: u64,
}

impl std::fmt::Debug for OpaqueSelection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpaqueSelection")
            .field("id", &"<redacted>")
            .field("display_name", &self.display_name)
            .field("kind", &self.kind)
            .field("expires_in_seconds", &self.expires_in_seconds)
            .finish()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SelectionRegistryLimits {
    pub ttl: Duration,
    pub maximum_file_bytes: usize,
}

impl Default for SelectionRegistryLimits {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(10 * 60),
            maximum_file_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone)]
pub struct SelectionRegistry {
    inner: Arc<RegistryInner>,
}

struct RegistryInner {
    records: Mutex<HashMap<String, SelectionRecord>>,
    limits: SelectionRegistryLimits,
}

struct SelectionRecord {
    path: PathBuf,
    kind: SelectionKind,
    expires_at: Instant,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum SelectionError {
    #[error("selection is invalid or expired")]
    InvalidOrExpired,
    #[error("selection path is not permitted")]
    PathNotPermitted,
    #[error("selection object type does not match the operation")]
    KindMismatch,
    #[error("selected file exceeds the size limit")]
    FileTooLarge,
    #[error("selection filesystem operation failed")]
    Io,
    #[error("operating system random source failed")]
    RandomSource,
}

impl SelectionRegistry {
    #[must_use]
    pub fn new(limits: SelectionRegistryLimits) -> Self {
        Self {
            inner: Arc::new(RegistryInner {
                records: Mutex::new(HashMap::new()),
                limits,
            }),
        }
    }

    pub fn register(
        &self,
        path: PathBuf,
        kind: SelectionKind,
    ) -> Result<OpaqueSelection, SelectionError> {
        let resolved = resolve_selected_path(&path, kind)?;
        validate_selected_path(&resolved, kind)?;
        let display_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .ok_or(SelectionError::PathNotPermitted)?;
        let id = random_id()?;
        let expires_at = Instant::now() + self.inner.limits.ttl;
        let mut records = self
            .inner
            .records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        remove_expired(&mut records, Instant::now());
        records.insert(
            id.clone(),
            SelectionRecord {
                path: resolved,
                kind,
                expires_at,
            },
        );
        Ok(OpaqueSelection {
            id,
            display_name,
            kind,
            expires_in_seconds: self.inner.limits.ttl.as_secs(),
        })
    }

    pub fn read_file(&self, id: &str) -> Result<Vec<u8>, SelectionError> {
        let path = self.resolve(id, SelectionKind::File)?;
        read_absolute_regular(&path, self.inner.limits.maximum_file_bytes)
    }

    pub fn read_directory_file(
        &self,
        id: &str,
        relative: &Path,
    ) -> Result<Vec<u8>, SelectionError> {
        let root_path = self.resolve(id, SelectionKind::Directory)?;
        let root = open_directory_nofollow(&root_path)?;
        read_relative_regular(&root, relative, self.inner.limits.maximum_file_bytes)
    }

    pub fn consume_save_destination(&self, id: &str) -> Result<PathBuf, SelectionError> {
        let mut records = self
            .inner
            .records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        remove_expired(&mut records, Instant::now());
        let record = records.remove(id).ok_or(SelectionError::InvalidOrExpired)?;
        if record.kind != SelectionKind::SaveDestination {
            return Err(SelectionError::KindMismatch);
        }
        validate_selected_path(&record.path, record.kind)?;
        Ok(record.path)
    }

    fn resolve(&self, id: &str, expected: SelectionKind) -> Result<PathBuf, SelectionError> {
        let mut records = self
            .inner
            .records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        remove_expired(&mut records, Instant::now());
        let record = records.get(id).ok_or(SelectionError::InvalidOrExpired)?;
        if record.kind != expected {
            return Err(SelectionError::KindMismatch);
        }
        Ok(record.path.clone())
    }
}

impl Default for SelectionRegistry {
    fn default() -> Self {
        Self::new(SelectionRegistryLimits::default())
    }
}

fn remove_expired(records: &mut HashMap<String, SelectionRecord>, now: Instant) {
    records.retain(|_, record| record.expires_at > now);
}

/// Resolves the user-selected path once, at selection time (revised S-1,
/// 2026-10-25): network locations and symbolic links in the selected path,
/// including the selected entry itself, are the user's explicit choice. The
/// canonical result is stored, and every later access opens it without
/// following links, so links inside a selected directory are never followed.
/// A save destination that does not exist yet resolves through its parent.
fn resolve_selected_path(path: &Path, kind: SelectionKind) -> Result<PathBuf, SelectionError> {
    if !path.is_absolute() || is_device_path(path) {
        return Err(SelectionError::PathNotPermitted);
    }
    let resolved = match std::fs::canonicalize(path) {
        Ok(resolved) => resolved,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && kind == SelectionKind::SaveDestination =>
        {
            let parent = path.parent().ok_or(SelectionError::PathNotPermitted)?;
            let name = path.file_name().ok_or(SelectionError::PathNotPermitted)?;
            ensure_simple_name(name)?;
            std::fs::canonicalize(parent)
                .map_err(|_| SelectionError::PathNotPermitted)?
                .join(name)
        }
        Err(_) => return Err(SelectionError::PathNotPermitted),
    };
    if is_device_path(&resolved) {
        return Err(SelectionError::PathNotPermitted);
    }
    Ok(resolved)
}

fn validate_selected_path(path: &Path, kind: SelectionKind) -> Result<(), SelectionError> {
    if !path.is_absolute() || is_device_path(path) {
        return Err(SelectionError::PathNotPermitted);
    }
    let parent = path.parent().ok_or(SelectionError::PathNotPermitted)?;
    let name = path.file_name().ok_or(SelectionError::PathNotPermitted)?;
    ensure_simple_name(name)?;
    let directory = open_directory_nofollow(parent)?;
    match directory.symlink_metadata(name) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || cap_metadata_is_reparse(&metadata) {
                return Err(SelectionError::PathNotPermitted);
            }
            let expected_type = match kind {
                SelectionKind::File => metadata.is_file(),
                SelectionKind::Directory => metadata.is_dir(),
                SelectionKind::SaveDestination => metadata.is_file(),
            };
            if !expected_type {
                return Err(SelectionError::KindMismatch);
            }
        }
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && kind == SelectionKind::SaveDestination => {}
        Err(_) => return Err(SelectionError::Io),
    }
    Ok(())
}

fn read_absolute_regular(path: &Path, maximum: usize) -> Result<Vec<u8>, SelectionError> {
    let parent = path.parent().ok_or(SelectionError::PathNotPermitted)?;
    let name = path.file_name().ok_or(SelectionError::PathNotPermitted)?;
    let directory = open_directory_nofollow(parent)?;
    read_regular(&directory, name, maximum)
}

fn read_relative_regular(
    root: &Dir,
    path: &Path,
    maximum: usize,
) -> Result<Vec<u8>, SelectionError> {
    let mut components = path.components().peekable();
    let mut directory = root.try_clone().map_err(|_| SelectionError::Io)?;
    loop {
        let component = components.next().ok_or(SelectionError::PathNotPermitted)?;
        let Component::Normal(name) = component else {
            return Err(SelectionError::PathNotPermitted);
        };
        if components.peek().is_none() {
            return read_regular(&directory, name, maximum);
        }
        directory = directory
            .open_dir_nofollow(name)
            .map_err(|_| SelectionError::PathNotPermitted)?;
    }
}

fn read_regular(directory: &Dir, name: &OsStr, maximum: usize) -> Result<Vec<u8>, SelectionError> {
    ensure_simple_name(name)?;
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = directory
        .open_with(name, &options)
        .map_err(|_| SelectionError::PathNotPermitted)?;
    let metadata = file.metadata().map_err(|_| SelectionError::Io)?;
    if !metadata.is_file() || cap_metadata_is_reparse(&metadata) {
        return Err(SelectionError::PathNotPermitted);
    }
    if metadata.len() > u64::try_from(maximum).unwrap_or(u64::MAX) {
        return Err(SelectionError::FileTooLarge);
    }
    let mut raw = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    Read::by_ref(&mut file)
        .take(u64::try_from(maximum).unwrap_or(u64::MAX) + 1)
        .read_to_end(&mut raw)
        .map_err(|_| SelectionError::Io)?;
    if raw.len() > maximum {
        return Err(SelectionError::FileTooLarge);
    }
    Ok(raw)
}

/// Opens `path` one component at a time without following symbolic links.
///
/// Applied to the path already resolved once at selection time: if an
/// ancestor is replaced by a symbolic link afterwards, the access fails closed
/// instead of following it.
fn open_directory_nofollow(path: &Path) -> Result<Dir, SelectionError> {
    let absolute = std::path::absolute(path).map_err(|_| SelectionError::PathNotPermitted)?;
    let mut anchor = PathBuf::new();
    let mut names = Vec::<OsString>::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => anchor.push(component.as_os_str()),
            Component::Normal(name) => names.push(name.to_owned()),
            Component::CurDir => {}
            Component::ParentDir => return Err(SelectionError::PathNotPermitted),
        }
    }
    if anchor.as_os_str().is_empty() {
        return Err(SelectionError::PathNotPermitted);
    }
    let mut directory =
        Dir::open_ambient_dir(&anchor, ambient_authority()).map_err(|_| SelectionError::Io)?;
    for name in names {
        directory = directory
            .open_dir_nofollow(name)
            .map_err(|_| SelectionError::PathNotPermitted)?;
    }
    Ok(directory)
}

fn ensure_simple_name(name: &OsStr) -> Result<(), SelectionError> {
    let path = Path::new(name);
    if path.components().count() == 1
        && matches!(path.components().next(), Some(Component::Normal(_)))
    {
        Ok(())
    } else {
        Err(SelectionError::PathNotPermitted)
    }
}

fn random_id() -> Result<String, SelectionError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| SelectionError::RandomSource)?;
    let mut id = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(id, "{byte:02x}");
    }
    Ok(id)
}

/// Device namespaces (`\\.\…`) and verbatim prefixes other than a drive or a
/// UNC share (`\\?\GLOBALROOT\…`, `\\?\Volume{…}\…`) are not file-system
/// locations and are rejected. UNC shares, verbatim UNC and mapped network
/// drives are accepted for reading (revised S-1).
#[cfg(windows)]
fn is_device_path(path: &Path) -> bool {
    use std::path::Prefix;
    match path.components().next() {
        Some(Component::Prefix(prefix)) => {
            matches!(prefix.kind(), Prefix::DeviceNS(_) | Prefix::Verbatim(_))
        }
        _ => false,
    }
}

#[cfg(not(windows))]
fn is_device_path(_path: &Path) -> bool {
    false
}

#[cfg(windows)]
fn cap_metadata_is_reparse(metadata: &cap_std::fs::Metadata) -> bool {
    use cap_fs_ext::OsMetadataExt as _;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn cap_metadata_is_reparse(_metadata: &cap_std::fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use std::{fs, thread};

    use tempfile::tempdir;

    use super::*;

    fn registry(ttl: Duration) -> SelectionRegistry {
        SelectionRegistry::new(SelectionRegistryLimits {
            ttl,
            maximum_file_bytes: 1024,
        })
    }

    #[test]
    fn opaque_dto_contains_no_full_path_and_expires() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let path = temporary.path().join("dashboard.yaml");
        fs::write(&path, "safe").unwrap_or_else(|error| panic!("write fixture: {error}"));
        let registry = registry(Duration::from_millis(1));
        let selection = registry
            .register(path.clone(), SelectionKind::File)
            .unwrap_or_else(|error| panic!("register: {error}"));
        let serialized =
            serde_json::to_string(&selection).unwrap_or_else(|error| panic!("serialize: {error}"));
        assert_eq!(selection.display_name, "dashboard.yaml");
        assert!(!serialized.contains(&path.to_string_lossy().into_owned()));
        assert_eq!(selection.id.len(), 64);
        thread::sleep(Duration::from_millis(5));
        assert_eq!(
            registry.read_file(&selection.id),
            Err(SelectionError::InvalidOrExpired)
        );
    }

    #[test]
    fn directory_reads_stay_beneath_the_selected_object() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let selected = temporary.path().join("selected");
        fs::create_dir(&selected).unwrap_or_else(|error| panic!("create selected: {error}"));
        fs::write(selected.join("inside.yaml"), "inside")
            .unwrap_or_else(|error| panic!("write inside: {error}"));
        fs::write(temporary.path().join("outside.yaml"), "outside")
            .unwrap_or_else(|error| panic!("write outside: {error}"));
        let registry = registry(Duration::from_secs(30));
        let opaque = registry
            .register(selected.clone(), SelectionKind::Directory)
            .unwrap_or_else(|error| panic!("register: {error}"));
        assert_eq!(
            registry
                .read_directory_file(&opaque.id, Path::new("inside.yaml"))
                .unwrap_or_else(|error| panic!("read inside: {error}")),
            b"inside"
        );
        assert_eq!(
            registry.read_directory_file(&opaque.id, Path::new("../outside.yaml")),
            Err(SelectionError::PathNotPermitted)
        );
        assert_eq!(
            registry.read_directory_file(&opaque.id, &temporary.path().join("outside.yaml")),
            Err(SelectionError::PathNotPermitted)
        );
    }

    #[test]
    fn symlink_inside_selected_directory_is_not_followed_when_supported() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let selected = temporary.path().join("selected");
        fs::create_dir(&selected).unwrap_or_else(|error| panic!("create selected: {error}"));
        let target = temporary.path().join("outside.yaml");
        fs::write(&target, "outside").unwrap_or_else(|error| panic!("write target: {error}"));
        let link = selected.join("linked.yaml");
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&target, &link).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&target, &link).is_ok();
        if linked {
            let registry = registry(Duration::from_secs(30));
            let opaque = registry
                .register(selected, SelectionKind::Directory)
                .unwrap_or_else(|error| panic!("register: {error}"));
            assert_eq!(
                registry.read_directory_file(&opaque.id, Path::new("linked.yaml")),
                Err(SelectionError::PathNotPermitted)
            );
        }
    }

    #[test]
    fn selected_symlink_is_resolved_once_when_supported() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let target = temporary.path().join("target.yaml");
        fs::write(&target, "outside").unwrap_or_else(|error| panic!("write target: {error}"));
        let link = temporary.path().join("selected.yaml");
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&target, &link).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&target, &link).is_ok();
        if linked {
            let registry = registry(Duration::from_secs(30));
            let opaque = registry
                .register(link, SelectionKind::File)
                .unwrap_or_else(|error| panic!("register: {error}"));
            assert_eq!(opaque.display_name, "selected.yaml");
            assert_eq!(
                registry
                    .read_file(&opaque.id)
                    .unwrap_or_else(|error| panic!("read: {error}")),
                b"outside"
            );
        }
    }

    #[test]
    fn symlinked_ancestor_of_selected_directory_is_resolved_when_supported() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let real = temporary.path().join("real");
        fs::create_dir_all(real.join("selected"))
            .unwrap_or_else(|error| panic!("create selected: {error}"));
        fs::write(real.join("selected").join("inside.yaml"), "inside")
            .unwrap_or_else(|error| panic!("write inside: {error}"));
        let link = temporary.path().join("linked");
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&real, &link).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&real, &link).is_ok();
        if linked {
            let registry = registry(Duration::from_secs(30));
            let opaque = registry
                .register(link.join("selected"), SelectionKind::Directory)
                .unwrap_or_else(|error| panic!("register: {error}"));
            assert_eq!(
                registry
                    .read_directory_file(&opaque.id, Path::new("inside.yaml"))
                    .unwrap_or_else(|error| panic!("read: {error}")),
                b"inside"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn network_paths_are_not_classified_as_device_paths() {
        for path in [
            r"\\server\share\dashboard.yaml",
            r"\\?\UNC\server\share\dashboard.yaml",
        ] {
            assert!(!is_device_path(Path::new(path)), "{path}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn device_paths_are_rejected_before_access() {
        let registry = registry(Duration::from_secs(30));
        for path in [
            PathBuf::from(r"\\.\PhysicalDrive0"),
            PathBuf::from(r"\\?\GLOBALROOT\Device\HarddiskVolume1\dashboard.yaml"),
            PathBuf::from(r"\\?\Volume{00000000-0000-0000-0000-000000000000}\dashboard.yaml"),
            PathBuf::from(r"\\.\C:\dashboard.yaml"),
        ] {
            assert_eq!(
                registry.register(path, SelectionKind::File),
                Err(SelectionError::PathNotPermitted)
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_drive_paths_remain_accepted() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let path = temporary.path().join("dashboard.yaml");
        fs::write(&path, "safe").unwrap_or_else(|error| panic!("write fixture: {error}"));
        let absolute =
            std::path::absolute(&path).unwrap_or_else(|error| panic!("absolute path: {error}"));
        let mut verbatim = OsString::from(r"\\?\");
        verbatim.push(absolute.as_os_str());
        let registry = registry(Duration::from_secs(30));
        let opaque = registry
            .register(PathBuf::from(verbatim), SelectionKind::File)
            .unwrap_or_else(|error| panic!("register verbatim drive path: {error}"));
        assert_eq!(
            registry
                .read_file(&opaque.id)
                .unwrap_or_else(|error| panic!("read: {error}")),
            b"safe"
        );
    }

    #[test]
    fn debug_output_masks_the_selection_id() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let path = temporary.path().join("dashboard.yaml");
        fs::write(&path, "safe").unwrap_or_else(|error| panic!("write fixture: {error}"));
        let selection = registry(Duration::from_secs(30))
            .register(path, SelectionKind::File)
            .unwrap_or_else(|error| panic!("register: {error}"));
        let rendered = format!("{selection:?}");
        assert!(!rendered.contains(&selection.id));
        assert!(rendered.contains("dashboard.yaml"));
    }

    #[test]
    fn oversized_and_kind_mismatched_reads_fail_closed() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let path = temporary.path().join("large.bin");
        fs::write(&path, vec![0_u8; 1025]).unwrap_or_else(|error| panic!("write fixture: {error}"));
        let registry = registry(Duration::from_secs(30));
        let opaque = registry
            .register(path, SelectionKind::File)
            .unwrap_or_else(|error| panic!("register: {error}"));
        assert_eq!(
            registry.read_file(&opaque.id),
            Err(SelectionError::FileTooLarge)
        );
        assert_eq!(
            registry.consume_save_destination(&opaque.id),
            Err(SelectionError::KindMismatch)
        );
    }
}
