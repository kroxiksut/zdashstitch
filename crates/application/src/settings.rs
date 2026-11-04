// SPDX-License-Identifier: MIT OR Apache-2.0

//! Minimal pre-G1 storage for non-secret application preferences.

use std::{
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zdashstitch_core::formats::strict::{self, ParseError};

use crate::MessageId;

const SETTINGS_FILE: &str = "settings.yaml";
/// Last-known-good copies, newest first. `BACKUP_FILE` always equals the last
/// successfully committed settings; older valid saves rotate down (P1-09B).
const BACKUP_FILE: &str = "settings.yaml.bak";
const BACKUP_FILES: [&str; 3] = [BACKUP_FILE, "settings.yaml.bak.2", "settings.yaml.bak.3"];
const TEMP_PREFIX: &str = ".settings-";
const TEMP_SUFFIX: &str = ".tmp";
const SETTINGS_FORMAT: &str = "zdashstitch-settings/v0";
const MAX_SETTINGS_BYTES: usize = 64 * 1024;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LocalePreference {
    #[default]
    System,
    En,
    Ru,
}

/// Interface theme. `high_contrast` is the in-application light-based
/// high-contrast theme (owner decision O-038, 2026-10-28); the operating
/// system contrast mode (`forced-colors`) still takes priority over any theme.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
    HighContrast,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReducedMotionPreference {
    #[default]
    System,
    Reduce,
    NoPreference,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppSettings {
    format: String,
    pub locale: LocalePreference,
    pub theme: ThemePreference,
    pub reduced_motion: ReducedMotionPreference,
    pub storage_root: PathBuf,
}

impl AppSettings {
    #[must_use]
    pub fn new(storage_root: PathBuf) -> Self {
        Self {
            format: SETTINGS_FORMAT.to_owned(),
            locale: LocalePreference::default(),
            theme: ThemePreference::default(),
            reduced_motion: ReducedMotionPreference::default(),
            storage_root,
        }
    }

    fn validate(&self) -> Result<(), SettingsError> {
        if self.format != SETTINGS_FORMAT {
            return Err(SettingsError::InvalidFormat);
        }
        validate_storage_root_syntax(&self.storage_root)
    }

    /// Non-blocking warnings about the configured storage root (revised S-1).
    /// May touch the filesystem; see [`check_storage_root`].
    #[must_use]
    pub fn storage_root_warnings(&self) -> Vec<SettingsWarning> {
        storage_root_warnings(&self.storage_root)
    }
}

/// A non-blocking settings finding. Callers present it by message ID; the
/// application never returns ready-made prose.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SettingsWarning {
    /// The storage root is on a network path (UNC share, mapped network drive,
    /// or a link that resolves to one). Allowed by the owner's S-1 revision of
    /// 2026-10-25: where data is stored is the user's choice; the application
    /// only warns and never blocks the choice or a storage move because of it.
    StorageRootNetwork,
}

impl SettingsWarning {
    /// Stable machine-readable code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::StorageRootNetwork => "storage_root_network",
        }
    }

    /// Localization message ID of the single user-facing warning.
    #[must_use]
    pub const fn message_id(self) -> MessageId {
        match self {
            Self::StorageRootNetwork => MessageId::SETTINGS_STORAGE_NETWORK_WARNING,
        }
    }
}

/// Validates a candidate storage root and returns its non-blocking warnings.
///
/// Relative and device-namespace paths are rejected. Network locations and
/// paths through symbolic links are accepted (revised S-1). Publication safety
/// does not rely on this check: a publication still requires a confirmed
/// operation-journal write. Warning detection canonicalizes the nearest
/// existing ancestor once, which can wait for a slow or disconnected network
/// drive, so async callers run it on a blocking thread.
pub fn check_storage_root(path: &Path) -> Result<Vec<SettingsWarning>, SettingsError> {
    validate_storage_root_syntax(path)?;
    Ok(storage_root_warnings(path))
}

fn validate_storage_root_syntax(path: &Path) -> Result<(), SettingsError> {
    if !path.is_absolute() {
        return Err(SettingsError::StorageRootNotAbsolute);
    }
    if is_device_path(path) {
        return Err(SettingsError::StorageRootDevicePath);
    }
    Ok(())
}

/// Lexical detection first (UNC, verbatim UNC, `//server/share` on other
/// platforms). Otherwise the nearest existing ancestor is canonicalized: on
/// Windows this resolves mapped network drives and links or junctions that
/// point to a share. Linux network file systems are not detected. A path that
/// cannot be resolved produces no warning and is never rejected for it.
fn storage_root_warnings(path: &Path) -> Vec<SettingsWarning> {
    let resolves_to_network = || {
        path.ancestors()
            .find_map(|ancestor| std::fs::canonicalize(ancestor).ok())
            .is_some_and(|canonical| is_network_path(&canonical))
    };
    if is_network_path(path) || resolves_to_network() {
        vec![SettingsWarning::StorageRootNetwork]
    } else {
        Vec::new()
    }
}

fn is_network_path(path: &Path) -> bool {
    match path.components().next() {
        Some(Component::Prefix(prefix)) => matches!(
            prefix.kind(),
            std::path::Prefix::UNC(..) | std::path::Prefix::VerbatimUNC(..)
        ),
        _ => {
            let text = path.as_os_str().to_string_lossy();
            text.starts_with(r"\\") || text.starts_with("//")
        }
    }
}

/// Device namespaces (`\\.\…`) and verbatim prefixes other than a drive or a
/// UNC share (`\\?\GLOBALROOT\…`, `\\?\Volume{…}\…`) are not storage
/// locations and stay rejected.
fn is_device_path(path: &Path) -> bool {
    match path.components().next() {
        Some(Component::Prefix(prefix)) => matches!(
            prefix.kind(),
            std::path::Prefix::DeviceNS(_) | std::path::Prefix::Verbatim(_)
        ),
        _ => false,
    }
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("settings path is unavailable for this platform or environment")]
    PathUnavailable,
    #[error("settings format is unsupported")]
    InvalidFormat,
    #[error("storage root must be absolute")]
    StorageRootNotAbsolute,
    #[error("storage root on a device namespace path is not supported")]
    StorageRootDevicePath,
    #[error("settings file exceeds the size limit")]
    FileTooLarge,
    #[error("settings path contains a symbolic link or reparse point")]
    LinkedPath,
    #[error("settings YAML is invalid")]
    InvalidYaml {
        #[source]
        source: ParseError,
        line: Option<usize>,
        column: Option<usize>,
        key: Option<String>,
    },
    #[error("an interrupted settings save was recovered from the last valid backup")]
    InterruptedSave,
    #[error("settings filesystem operation failed")]
    Io(#[source] std::io::Error),
}

#[derive(Debug, Error)]
#[error("{error}")]
pub struct SettingsLoadError {
    #[source]
    pub error: Box<SettingsError>,
    pub last_valid: Option<AppSettings>,
}

#[derive(Clone, Debug)]
pub struct SettingsStore {
    directory: PathBuf,
}

impl SettingsStore {
    #[must_use]
    pub fn at(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub fn platform_default() -> Result<(Self, AppSettings), SettingsError> {
        let (config, storage) = platform_paths(|name| std::env::var_os(name))?;
        Ok((Self::at(config), AppSettings::new(storage)))
    }

    pub fn load(&self) -> Result<Option<AppSettings>, SettingsLoadError> {
        let Some(directory) =
            self.open_existing_directory()
                .map_err(|error| SettingsLoadError {
                    error: Box::new(error),
                    last_valid: None,
                })?
        else {
            return Ok(None);
        };
        remove_stale_temporaries(&directory);
        match load_settings_file(&directory, SETTINGS_FILE) {
            Ok(Some(settings)) => Ok(Some(settings)),
            Ok(None) => match newest_valid_backup(&directory) {
                (Some(last_valid), _) => Err(SettingsLoadError {
                    error: Box::new(SettingsError::InterruptedSave),
                    last_valid: Some(last_valid),
                }),
                (None, Some(error)) => Err(SettingsLoadError {
                    error: Box::new(error),
                    last_valid: None,
                }),
                (None, None) => Ok(None),
            },
            Err(error) => Err(SettingsLoadError {
                error: Box::new(error),
                last_valid: newest_valid_backup(&directory).0,
            }),
        }
    }

    /// Commits the settings by atomically replacing the primary file, then refreshes
    /// the last-known-good copies. The rename of the temporary file is the only commit
    /// point: an interrupted save leaves either the previous or the new primary, never
    /// a missing one, and an unrenamed temporary file is discarded on the next load or
    /// save. Backup rotation runs after the commit and is best-effort: its failure
    /// leaves an older last-known-good copy but never fails a committed save.
    pub fn save(&self, settings: &AppSettings) -> Result<(), SettingsError> {
        settings.validate()?;
        ensure_directory_without_links(&self.directory)?;
        let directory = Dir::open_ambient_dir(&self.directory, ambient_authority())
            .map_err(SettingsError::Io)?;
        reject_link_if_present(&directory, SETTINGS_FILE)?;
        for backup in BACKUP_FILES {
            reject_link_if_present(&directory, backup)?;
        }
        remove_stale_temporaries(&directory);

        let raw = strict::yaml_to_string(settings)
            .map_err(|source| SettingsError::InvalidYaml {
                source,
                line: None,
                column: None,
                key: None,
            })?
            .into_bytes();
        if raw.len() > MAX_SETTINGS_BYTES {
            return Err(SettingsError::FileTooLarge);
        }
        replace_file(&directory, SETTINGS_FILE, &raw)?;
        sync_directory(&self.directory)?;

        if rotate_backups(&directory).is_ok() && replace_file(&directory, BACKUP_FILE, &raw).is_ok()
        {
            let _ = sync_directory(&self.directory);
        }
        Ok(())
    }

    fn open_existing_directory(&self) -> Result<Option<Dir>, SettingsError> {
        if !self.directory.exists() {
            return Ok(None);
        }
        check_existing_components(&self.directory)?;
        Dir::open_ambient_dir(&self.directory, ambient_authority())
            .map(Some)
            .map_err(SettingsError::Io)
    }
}

/// Writes `raw` to a fresh private temporary file, syncs it and atomically renames it
/// over `name`. The temporary file is removed when any step fails.
pub(crate) fn replace_file(directory: &Dir, name: &str, raw: &[u8]) -> Result<(), SettingsError> {
    let suffix = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temporary = format!("{}{suffix}{TEMP_SUFFIX}", own_temporary_prefix());
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    let mut file = directory
        .open_with(&temporary, &options)
        .map_err(SettingsError::Io)?;
    let result = (|| {
        file.write_all(raw).map_err(SettingsError::Io)?;
        file.sync_all().map_err(SettingsError::Io)?;
        set_private_permissions(&file)?;
        drop(file);
        directory
            .rename(&temporary, directory, name)
            .map_err(SettingsError::Io)
    })();
    if result.is_err() {
        let _ = directory.remove_file(&temporary);
    }
    result
}

/// Shifts `settings.yaml.bak` → `.bak.2` → `.bak.3`, dropping the oldest copy.
fn rotate_backups(directory: &Dir) -> Result<(), SettingsError> {
    let ignore_missing = |result: std::io::Result<()>| match result {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(SettingsError::Io(error)),
        _ => Ok(()),
    };
    let [newest, middle, oldest] = BACKUP_FILES;
    ignore_missing(directory.remove_file(oldest))?;
    ignore_missing(directory.rename(middle, directory, oldest))?;
    ignore_missing(directory.rename(newest, directory, middle))
}

/// Returns the newest valid last-known-good copy, or the error of the newest
/// existing backup when none of them is valid.
fn newest_valid_backup(directory: &Dir) -> (Option<AppSettings>, Option<SettingsError>) {
    let mut first_error = None;
    for backup in BACKUP_FILES {
        match load_settings_file(directory, backup) {
            Ok(Some(settings)) => return (Some(settings), None),
            Ok(None) => {}
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    (None, first_error)
}

fn own_temporary_prefix() -> String {
    format!("{TEMP_PREFIX}{}-", std::process::id())
}

/// Removes temporary files left by an interrupted save of another process. They
/// were never renamed, so they were never committed; the primary file and its
/// backups stay authoritative. Temporary files of this process are kept because a
/// concurrent save may still own them. Best-effort: failures are ignored.
pub(crate) fn remove_stale_temporaries(directory: &Dir) {
    let Ok(entries) = directory.entries() else {
        return;
    };
    let own = own_temporary_prefix();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with(TEMP_PREFIX) || !name.ends_with(TEMP_SUFFIX) || name.starts_with(&own)
        {
            continue;
        }
        if entry.file_type().is_ok_and(|kind| !kind.is_dir()) {
            let _ = directory.remove_file(name);
        }
    }
}

fn load_settings_file(directory: &Dir, name: &str) -> Result<Option<AppSettings>, SettingsError> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = match directory.open_with(name, &options) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(SettingsError::Io(error)),
    };
    let metadata = file.metadata().map_err(SettingsError::Io)?;
    if !metadata.is_file() || cap_metadata_is_reparse(&metadata) {
        return Err(SettingsError::LinkedPath);
    }
    if metadata.len() > u64::try_from(MAX_SETTINGS_BYTES).unwrap_or(u64::MAX) {
        return Err(SettingsError::FileTooLarge);
    }
    let mut raw = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    Read::by_ref(&mut file)
        .take(u64::try_from(MAX_SETTINGS_BYTES).unwrap_or(u64::MAX) + 1)
        .read_to_end(&mut raw)
        .map_err(SettingsError::Io)?;
    if raw.len() > MAX_SETTINGS_BYTES {
        return Err(SettingsError::FileTooLarge);
    }
    // Strict loader: anchors/aliases, duplicate or non-string keys, tags and
    // deep nesting are rejected like in portable project files (P0-11).
    let settings: AppSettings =
        strict::yaml_from_slice(&raw).map_err(|source| invalid_yaml_error(source, &raw))?;
    settings.validate()?;
    Ok(Some(settings))
}

fn invalid_yaml_error(source: ParseError, raw: &[u8]) -> SettingsError {
    let line = source.line().and_then(|line| usize::try_from(line).ok());
    let column = source
        .column()
        .and_then(|column| usize::try_from(column).ok());
    let key = line
        .and_then(|line| {
            String::from_utf8_lossy(raw)
                .lines()
                .nth(line.saturating_sub(1))
                .map(str::to_owned)
        })
        .and_then(|line| {
            line.trim()
                .split_once(':')
                .map(|(key, _)| key.trim().to_owned())
        })
        .filter(|key| !key.is_empty());
    SettingsError::InvalidYaml {
        source,
        line,
        column,
        key,
    }
}

pub(crate) fn platform_paths(
    environment: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Result<(PathBuf, PathBuf), SettingsError> {
    #[cfg(target_os = "windows")]
    {
        let base = environment("LOCALAPPDATA")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or(SettingsError::PathUnavailable)?;
        let root = base.join("ZDashstitch");
        return Ok((root.clone(), root));
    }
    #[cfg(target_os = "linux")]
    {
        let home = environment("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute());
        let config_base = environment("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| home.as_ref().map(|path| path.join(".config")))
            .ok_or(SettingsError::PathUnavailable)?;
        let data_base = environment("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| home.map(|path| path.join(".local/share")))
            .ok_or(SettingsError::PathUnavailable)?;
        return Ok((
            config_base.join("zdashstitch"),
            data_base.join("zdashstitch"),
        ));
    }
    #[allow(unreachable_code)]
    Err(SettingsError::PathUnavailable)
}

pub(crate) fn ensure_directory_without_links(path: &Path) -> Result<(), SettingsError> {
    if !path.is_absolute() {
        return Err(SettingsError::PathUnavailable);
    }
    check_existing_components(path)?;
    std::fs::create_dir_all(path).map_err(SettingsError::Io)?;
    check_existing_components(path)
}

pub(crate) fn check_existing_components(path: &Path) -> Result<(), SettingsError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                current.push(component.as_os_str());
            }
            Component::CurDir => continue,
            Component::ParentDir => return Err(SettingsError::LinkedPath),
        }
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || is_reparse_point(&metadata) => {
                return Err(SettingsError::LinkedPath);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(SettingsError::Io(error)),
        }
    }
    Ok(())
}

pub(crate) fn reject_link_if_present(directory: &Dir, name: &str) -> Result<(), SettingsError> {
    match directory.symlink_metadata(name) {
        Ok(metadata) if metadata.file_type().is_symlink() || cap_metadata_is_reparse(&metadata) => {
            Err(SettingsError::LinkedPath)
        }
        Ok(metadata) if !metadata.is_file() => Err(SettingsError::LinkedPath),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SettingsError::Io(error)),
    }
}

#[cfg(windows)]
pub(crate) fn cap_metadata_is_reparse(metadata: &cap_std::fs::Metadata) -> bool {
    use cap_fs_ext::OsMetadataExt as _;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
pub(crate) fn cap_metadata_is_reparse(_metadata: &cap_std::fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
pub(crate) fn sync_directory(path: &Path) -> Result<(), SettingsError> {
    std::fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(SettingsError::Io)
}

#[cfg(not(unix))]
pub(crate) fn sync_directory(_path: &Path) -> Result<(), SettingsError> {
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_metadata: &std::fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
fn set_private_permissions(file: &cap_std::fs::File) -> Result<(), SettingsError> {
    // `cap_std` permissions need the `cap_std` extension trait, not `std`'s.
    use cap_std::fs::PermissionsExt as _;
    let mut permissions = file.metadata().map_err(SettingsError::Io)?.permissions();
    permissions.set_mode(0o600);
    file.set_permissions(permissions).map_err(SettingsError::Io)
}

#[cfg(not(unix))]
fn set_private_permissions(_file: &cap_std::fs::File) -> Result<(), SettingsError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, fs};

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn settings_round_trip_and_replace_without_partial_content() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let store = SettingsStore::at(temporary.path().join("config"));
        assert_eq!(
            store.load().unwrap_or_else(|error| panic!("load: {error}")),
            None
        );

        let mut settings = AppSettings::new(temporary.path().join("data"));
        assert_eq!(settings.locale, LocalePreference::System);
        settings.locale = LocalePreference::Ru;
        settings.theme = ThemePreference::Dark;
        settings.reduced_motion = ReducedMotionPreference::Reduce;
        store
            .save(&settings)
            .unwrap_or_else(|error| panic!("save: {error}"));
        assert_eq!(
            store.load().unwrap_or_else(|error| panic!("load: {error}")),
            Some(settings.clone())
        );

        settings.theme = ThemePreference::Light;
        store
            .save(&settings)
            .unwrap_or_else(|error| panic!("replace: {error}"));
        assert_eq!(
            store.load().unwrap_or_else(|error| panic!("load: {error}")),
            Some(settings)
        );
        let mut names = fs::read_dir(temporary.path().join("config"))
            .unwrap_or_else(|error| panic!("read directory: {error}"))
            .map(|entry| {
                entry
                    .unwrap_or_else(|error| panic!("entry: {error}"))
                    .file_name()
            })
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(
            names,
            [
                OsString::from(SETTINGS_FILE),
                OsString::from(BACKUP_FILE),
                OsString::from(BACKUP_FILES[1]),
            ]
        );
    }

    #[test]
    fn unknown_fields_formats_and_relative_storage_roots_fail_closed() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let directory = temporary.path().join("config");
        fs::create_dir(&directory).unwrap_or_else(|error| panic!("create directory: {error}"));
        let store = SettingsStore::at(directory.clone());
        for document in [
            "format: zdashstitch-settings/v0\nlocale: en\ntheme: system\nreduced_motion: system\nstorage_root: relative\n",
            "format: future\nlocale: en\ntheme: system\nreduced_motion: system\nstorage_root: C:/absolute\n",
            "format: zdashstitch-settings/v0\nlocale: en\ntheme: system\nreduced_motion: system\nstorage_root: C:/absolute\nsecret: forbidden\n",
            "format: zdashstitch-settings/v0\nlocale: en\nlocale: ru\ntheme: system\nreduced_motion: system\nstorage_root: C:/absolute\n",
        ] {
            fs::write(directory.join(SETTINGS_FILE), document)
                .unwrap_or_else(|error| panic!("write fixture: {error}"));
            assert!(store.load().is_err());
        }
    }

    /// O-038: `high_contrast` is stored as is and survives save and load.
    #[test]
    fn high_contrast_theme_round_trips_as_snake_case() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let directory = temporary.path().join("config");
        let store = SettingsStore::at(directory.clone());
        let mut settings = settings_with_root(temporary.path(), "root");
        settings.theme = ThemePreference::HighContrast;
        store
            .save(&settings)
            .unwrap_or_else(|error| panic!("save: {error}"));
        let raw = fs::read_to_string(directory.join(SETTINGS_FILE))
            .unwrap_or_else(|error| panic!("read settings: {error}"));
        assert!(raw.contains("theme: high_contrast\n"), "{raw}");
        assert_eq!(
            store.load().unwrap_or_else(|error| panic!("load: {error}")),
            Some(settings)
        );
    }

    /// Theme values outside the closed set fail closed, including spellings
    /// close to `high_contrast`.
    #[test]
    fn unknown_theme_values_are_rejected() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let directory = temporary.path().join("config");
        let store = SettingsStore::at(directory.clone());
        store
            .save(&settings_with_root(temporary.path(), "root"))
            .unwrap_or_else(|error| panic!("save: {error}"));
        let original = fs::read_to_string(directory.join(SETTINGS_FILE))
            .unwrap_or_else(|error| panic!("read settings: {error}"));
        assert!(original.contains("theme: system\n"));
        for theme in ["light", "dark", "high_contrast"] {
            let document = original.replacen("theme: system\n", &format!("theme: {theme}\n"), 1);
            fs::write(directory.join(SETTINGS_FILE), document)
                .unwrap_or_else(|error| panic!("write fixture: {error}"));
            assert!(store.load().is_ok(), "{theme} must be accepted");
        }
        for theme in [
            "highcontrast",
            "high-contrast",
            "High_Contrast",
            "contrast",
            "sepia",
            "\"\"",
        ] {
            let document = original.replacen("theme: system\n", &format!("theme: {theme}\n"), 1);
            fs::write(directory.join(SETTINGS_FILE), document)
                .unwrap_or_else(|error| panic!("write fixture: {error}"));
            assert!(store.load().is_err(), "{theme} must be rejected");
        }
    }

    /// P0-11: settings use the strict YAML loader with reference codes.
    #[test]
    fn duplicate_keys_and_aliases_are_rejected_with_reference_codes() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let directory = temporary.path().join("config");
        let store = SettingsStore::at(directory.clone());
        store
            .save(&settings_with_root(temporary.path(), "root"))
            .unwrap_or_else(|error| panic!("save: {error}"));
        let original = fs::read_to_string(directory.join(SETTINGS_FILE))
            .unwrap_or_else(|error| panic!("read settings: {error}"));
        assert!(original.contains("theme: system\n"));
        for (code, document) in [
            ("yaml_duplicate_key", format!("{original}theme: dark\n")),
            (
                "yaml_alias",
                original.replacen("theme: system\n", "theme: &theme system\n", 1),
            ),
            ("yaml_key", format!("{original}1: one\n")),
        ] {
            fs::write(directory.join(SETTINGS_FILE), &document)
                .unwrap_or_else(|error| panic!("write fixture: {error}"));
            let error = match store.load() {
                Err(error) => error,
                Ok(_) => panic!("{code} must be rejected"),
            };
            match error.error.as_ref() {
                SettingsError::InvalidYaml { source, .. } => {
                    assert_eq!(source.code(), code);
                }
                other => panic!("{code}: unexpected error {other:?}"),
            }
        }
    }

    #[test]
    fn settings_file_link_is_rejected_when_platform_allows_creating_one() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let directory = temporary.path().join("config");
        fs::create_dir(&directory).unwrap_or_else(|error| panic!("create directory: {error}"));
        let target = temporary.path().join("target.yaml");
        fs::write(&target, "outside").unwrap_or_else(|error| panic!("write target: {error}"));
        let link = directory.join(SETTINGS_FILE);
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&target, &link).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&target, &link).is_ok();
        if linked {
            let store = SettingsStore::at(directory);
            match store.load() {
                Err(error) => assert!(matches!(
                    error.error.as_ref(),
                    SettingsError::LinkedPath | SettingsError::Io(_)
                )),
                Ok(_) => panic!("linked settings file must be rejected"),
            }
        }
    }

    #[test]
    fn invalid_or_missing_primary_reports_the_last_valid_backup() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let directory = temporary.path().join("config");
        let store = SettingsStore::at(directory.clone());
        let first = AppSettings::new(temporary.path().join("first-data"));
        store
            .save(&first)
            .unwrap_or_else(|error| panic!("first save: {error}"));
        let mut second = first.clone();
        second.theme = ThemePreference::Dark;
        store
            .save(&second)
            .unwrap_or_else(|error| panic!("second save: {error}"));

        fs::write(directory.join(SETTINGS_FILE), "format: broken: yaml")
            .unwrap_or_else(|error| panic!("corrupt primary: {error}"));
        let invalid = match store.load() {
            Err(error) => error,
            Ok(_) => panic!("invalid primary must remain visible to the caller"),
        };
        assert!(matches!(
            invalid.error.as_ref(),
            SettingsError::InvalidYaml {
                line: Some(_),
                column: Some(_),
                ..
            }
        ));
        assert_eq!(invalid.last_valid, Some(second.clone()));

        fs::remove_file(directory.join(SETTINGS_FILE))
            .unwrap_or_else(|error| panic!("remove primary: {error}"));
        let interrupted = match store.load() {
            Err(error) => error,
            Ok(_) => panic!("missing primary with backup must report interrupted save"),
        };
        assert!(matches!(
            interrupted.error.as_ref(),
            SettingsError::InterruptedSave
        ));
        assert_eq!(interrupted.last_valid, Some(second));

        fs::write(directory.join(BACKUP_FILE), "format: broken: yaml")
            .unwrap_or_else(|error| panic!("corrupt newest backup: {error}"));
        let older = match store.load() {
            Err(error) => error,
            Ok(_) => panic!("missing primary must report interrupted save"),
        };
        assert_eq!(older.last_valid, Some(first));
    }

    fn settings_with_root(base: &Path, name: &str) -> AppSettings {
        AppSettings::new(base.join(name))
    }

    #[test]
    fn three_valid_backups_rotate_newest_first() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let directory = temporary.path().join("config");
        let store = SettingsStore::at(directory.clone());
        let saves =
            ["a", "b", "c", "d", "e"].map(|name| settings_with_root(temporary.path(), name));
        for settings in &saves {
            store
                .save(settings)
                .unwrap_or_else(|error| panic!("save: {error}"));
        }
        let read = |name: &str| {
            let dir = Dir::open_ambient_dir(&directory, ambient_authority())
                .unwrap_or_else(|error| panic!("open directory: {error}"));
            load_settings_file(&dir, name).unwrap_or_else(|error| panic!("{name}: {error}"))
        };
        assert_eq!(read(SETTINGS_FILE), Some(saves[4].clone()));
        assert_eq!(read(BACKUP_FILES[0]), Some(saves[4].clone()));
        assert_eq!(read(BACKUP_FILES[1]), Some(saves[3].clone()));
        assert_eq!(read(BACKUP_FILES[2]), Some(saves[2].clone()));
        assert_eq!(fs::read_dir(&directory).map(Iterator::count).ok(), Some(4));
    }

    #[test]
    fn stale_temporary_files_are_discarded_and_never_promoted() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let directory = temporary.path().join("config");
        let store = SettingsStore::at(directory.clone());
        let committed = settings_with_root(temporary.path(), "committed");
        store
            .save(&committed)
            .unwrap_or_else(|error| panic!("save: {error}"));
        let uncommitted = settings_with_root(temporary.path(), "uncommitted");
        let raw = strict::yaml_to_string(&uncommitted)
            .unwrap_or_else(|error| panic!("serialize: {error}"));
        // A foreign process ID cannot match this test process.
        let stale = directory.join(format!("{TEMP_PREFIX}0-7{TEMP_SUFFIX}"));
        let partial = directory.join(format!("{TEMP_PREFIX}0-8{TEMP_SUFFIX}"));
        fs::write(&stale, &raw).unwrap_or_else(|error| panic!("write stale: {error}"));
        fs::write(&partial, "format: zdash")
            .unwrap_or_else(|error| panic!("write partial: {error}"));

        assert_eq!(
            store.load().unwrap_or_else(|error| panic!("load: {error}")),
            Some(committed.clone())
        );
        assert!(!stale.exists() && !partial.exists());

        fs::remove_file(directory.join(SETTINGS_FILE))
            .unwrap_or_else(|error| panic!("remove primary: {error}"));
        fs::write(&stale, &raw).unwrap_or_else(|error| panic!("write stale: {error}"));
        let interrupted = match store.load() {
            Err(error) => error,
            Ok(_) => panic!("missing primary must report interrupted save"),
        };
        assert!(matches!(
            interrupted.error.as_ref(),
            SettingsError::InterruptedSave
        ));
        assert_eq!(interrupted.last_valid, Some(committed));
        assert!(!stale.exists());

        fs::write(&stale, &raw).unwrap_or_else(|error| panic!("write stale: {error}"));
        store
            .save(&uncommitted)
            .unwrap_or_else(|error| panic!("save: {error}"));
        assert!(!stale.exists());
    }

    #[test]
    fn network_storage_roots_are_accepted_with_one_warning() {
        #[cfg(windows)]
        let roots = [
            r"\\server\share\zdashstitch",
            r"\\?\UNC\server\share\zdashstitch",
        ];
        #[cfg(not(windows))]
        let roots = ["//server/share/zdashstitch"];
        for root in roots {
            let settings = AppSettings::new(PathBuf::from(root));
            assert!(settings.validate().is_ok(), "{root}");
            let warnings = check_storage_root(Path::new(root))
                .unwrap_or_else(|error| panic!("{root}: {error}"));
            assert_eq!(warnings, [SettingsWarning::StorageRootNetwork], "{root}");
            assert_eq!(settings.storage_root_warnings(), warnings);
        }
        assert_eq!(
            SettingsWarning::StorageRootNetwork.code(),
            "storage_root_network"
        );
        assert_eq!(
            SettingsWarning::StorageRootNetwork.message_id(),
            MessageId::SETTINGS_STORAGE_NETWORK_WARNING
        );
    }

    #[test]
    fn network_storage_root_survives_save_and_load() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let store = SettingsStore::at(temporary.path().join("config"));
        #[cfg(windows)]
        let root = r"\\server\share\zdashstitch";
        #[cfg(not(windows))]
        let root = "//server/share/zdashstitch";
        let settings = AppSettings::new(PathBuf::from(root));
        store
            .save(&settings)
            .unwrap_or_else(|error| panic!("save: {error}"));
        let loaded = store
            .load()
            .unwrap_or_else(|error| panic!("load: {error}"))
            .unwrap_or_else(|| panic!("saved settings must load"));
        assert_eq!(loaded, settings);
        assert_eq!(
            loaded.storage_root_warnings(),
            [SettingsWarning::StorageRootNetwork]
        );
    }

    #[test]
    fn local_storage_roots_have_no_warning() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let missing = temporary.path().join("not-created-yet").join("data");
        assert_eq!(
            check_storage_root(&missing).unwrap_or_else(|error| panic!("check: {error}")),
            []
        );
        #[cfg(windows)]
        for local in [r"C:\ZDashstitch", r"\\?\C:\ZDashstitch"] {
            assert_eq!(
                check_storage_root(Path::new(local))
                    .unwrap_or_else(|error| panic!("{local}: {error}")),
                []
            );
        }
    }

    #[test]
    fn storage_root_through_a_symbolic_link_is_accepted_when_supported() {
        let temporary = tempdir().unwrap_or_else(|error| panic!("temp directory: {error}"));
        let target = temporary.path().join("real-data");
        fs::create_dir(&target).unwrap_or_else(|error| panic!("create target: {error}"));
        let link = temporary.path().join("linked-data");
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&target, &link).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&target, &link).is_ok();
        if linked {
            let root = link.join("zdashstitch");
            assert_eq!(
                check_storage_root(&root).unwrap_or_else(|error| panic!("check: {error}")),
                []
            );
            let store = SettingsStore::at(temporary.path().join("config"));
            store
                .save(&AppSettings::new(root))
                .unwrap_or_else(|error| panic!("save: {error}"));
        }
    }

    #[test]
    fn relative_and_device_storage_roots_are_rejected() {
        assert!(matches!(
            check_storage_root(Path::new("relative/data")),
            Err(SettingsError::StorageRootNotAbsolute)
        ));
        #[cfg(windows)]
        for device in [
            r"\\.\pipe\zdashstitch",
            r"\\.\C:\zdashstitch",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\zdashstitch",
            r"\\?\Volume{00000000-0000-0000-0000-000000000000}\zdashstitch",
        ] {
            assert!(
                matches!(
                    AppSettings::new(PathBuf::from(device)).validate(),
                    Err(SettingsError::StorageRootDevicePath)
                ),
                "{device}"
            );
            assert!(
                matches!(
                    check_storage_root(Path::new(device)),
                    Err(SettingsError::StorageRootDevicePath)
                ),
                "{device}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_defaults_follow_o_018() {
        let (config, storage) = platform_paths(|name| {
            (name == "LOCALAPPDATA").then(|| OsString::from(r"C:\Users\test\AppData\Local"))
        })
        .unwrap_or_else(|error| panic!("platform paths: {error}"));
        assert_eq!(
            config,
            PathBuf::from(r"C:\Users\test\AppData\Local\ZDashstitch")
        );
        assert_eq!(storage, config);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_defaults_follow_xdg_and_o_018() {
        let (config, storage) = platform_paths(|name| match name {
            "HOME" => Some(OsString::from("/home/test")),
            _ => None,
        })
        .unwrap_or_else(|error| panic!("platform paths: {error}"));
        assert_eq!(config, PathBuf::from("/home/test/.config/zdashstitch"));
        assert_eq!(
            storage,
            PathBuf::from("/home/test/.local/share/zdashstitch")
        );
    }
}
