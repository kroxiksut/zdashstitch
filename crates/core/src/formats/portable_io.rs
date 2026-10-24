// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    collections::{BTreeMap, HashSet},
    ffi::{OsStr, OsString},
    io::{Cursor, Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use serde::de::DeserializeOwned;
use thiserror::Error;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter, write::SimpleFileOptions};

use super::{
    FormatError, MetricCatalog, PortableProject, Validate, to_yaml_string, validate_project,
};

pub const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_ARCHIVE_ENTRIES: usize = 33;
pub const MAX_COMPRESSION_RATIO: u64 = 300;
const MAX_ARCHIVE_BYTES: usize = 40 * 1024 * 1024;
const MAX_YAML_DEPTH: usize = 40;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Error)]
pub enum PortableIoError {
    #[error("{context}: {source}")]
    Io {
        context: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("ZIP: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error(transparent)]
    Format(#[from] FormatError),
    #[error("{code}: {message}")]
    Policy { code: &'static str, message: String },
}

impl PortableIoError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io { .. } => "io",
            Self::Zip(_) => "zip",
            Self::Format(error) => error.code().unwrap_or("format"),
            Self::Policy { code, .. } => code,
        }
    }

    fn policy(code: &'static str, message: impl Into<String>) -> Self {
        Self::Policy {
            code,
            message: message.into(),
        }
    }

    fn io(context: &'static str, source: std::io::Error) -> Self {
        Self::Io { context, source }
    }
}

pub type Result<T> = std::result::Result<T, PortableIoError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortableBundle {
    pub project: PortableProject,
    pub catalogs: BTreeMap<String, MetricCatalog>,
}

impl PortableBundle {
    pub fn validate(&self) -> Result<()> {
        validate_project(&self.project, &self.catalogs)?;
        Ok(())
    }
}

pub fn load_project_file(path: &Path) -> Result<PortableBundle> {
    let absolute =
        std::path::absolute(path).map_err(|error| PortableIoError::io("project path", error))?;
    let parent = absolute.parent().ok_or_else(|| {
        PortableIoError::policy("project_path", "project file has no parent directory")
    })?;
    let project_name = absolute
        .file_name()
        .ok_or_else(|| PortableIoError::policy("project_path", "project path has no file name"))?;
    ensure_simple_file_name(project_name)?;

    let root = open_directory_nofollow(parent)?;
    let project_raw = read_relative_regular(&root, Path::new(project_name))?;
    let project: PortableProject = parse_yaml(&project_raw)?;

    let mut catalogs = BTreeMap::new();
    let mut total = project_raw.len();
    for reference in &project.catalogs {
        let raw = read_relative_regular(&root, Path::new(&reference.file))?;
        total = total
            .checked_add(raw.len())
            .ok_or_else(|| PortableIoError::policy("total_size", "project size overflow"))?;
        if total > MAX_TOTAL_BYTES {
            return Err(PortableIoError::policy(
                "total_size",
                "project exceeds total size limit",
            ));
        }
        let catalog: MetricCatalog = parse_yaml(&raw)?;
        catalogs.insert(reference.id.clone(), catalog);
    }

    let bundle = PortableBundle { project, catalogs };
    bundle.validate()?;
    Ok(bundle)
}

pub fn read_package(path: &Path) -> Result<PortableBundle> {
    let raw = read_absolute_regular(path, MAX_ARCHIVE_BYTES)?;
    read_package_bytes(&raw)
}

pub fn read_package_bytes(raw: &[u8]) -> Result<PortableBundle> {
    if raw.len() > MAX_ARCHIVE_BYTES {
        return Err(PortableIoError::policy(
            "archive_size",
            "archive file exceeds size limit",
        ));
    }
    let mut archive = ZipArchive::new(Cursor::new(raw))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(PortableIoError::policy(
            "archive_entries",
            "too many archive entries",
        ));
    }

    let mut names = HashSet::new();
    let mut entries = BTreeMap::new();
    let mut total = 0usize;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = std::str::from_utf8(entry.name_raw())
            .map_err(|_| {
                PortableIoError::policy("archive_name", "archive entry name is not UTF-8")
            })?
            .to_owned();
        if !names.insert(name.clone()) {
            return Err(PortableIoError::policy(
                "duplicate_archive_entry",
                format!("duplicate archive entry {name}"),
            ));
        }
        if name != "project.yaml" && !is_catalog_path(&name) {
            return Err(PortableIoError::policy(
                "archive_name",
                format!("unrecognized or unsafe archive entry {name}"),
            ));
        }
        if entry.encrypted() {
            return Err(PortableIoError::policy(
                "archive_encrypted",
                format!("encrypted archive entry {name}"),
            ));
        }
        if !entry.is_file() || entry.is_symlink() {
            return Err(PortableIoError::policy(
                "archive_file_type",
                format!("non-regular archive entry {name}"),
            ));
        }
        if !matches!(
            entry.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            return Err(PortableIoError::policy(
                "archive_compression",
                format!("unsupported compression for {name}"),
            ));
        }
        let size = usize::try_from(entry.size()).map_err(|_| {
            PortableIoError::policy("entry_size", format!("oversized archive entry {name}"))
        })?;
        if size > MAX_FILE_BYTES {
            return Err(PortableIoError::policy(
                "entry_size",
                format!("oversized archive entry {name}"),
            ));
        }
        total = total
            .checked_add(size)
            .ok_or_else(|| PortableIoError::policy("total_size", "archive total size overflow"))?;
        if total > MAX_TOTAL_BYTES {
            return Err(PortableIoError::policy(
                "total_size",
                "archive exceeds total size limit",
            ));
        }
        let compressed = entry.compressed_size();
        if entry.size() > 0
            && (compressed == 0 || entry.size() / compressed.max(1) > MAX_COMPRESSION_RATIO)
        {
            return Err(PortableIoError::policy(
                "compression_ratio",
                format!("suspicious compression ratio for {name}"),
            ));
        }
        let mut content = Vec::with_capacity(size);
        Read::by_ref(&mut entry)
            .take(u64::try_from(MAX_FILE_BYTES).unwrap_or(u64::MAX) + 1)
            .read_to_end(&mut content)
            .map_err(|error| PortableIoError::io("read archive entry", error))?;
        if content.len() != size {
            return Err(PortableIoError::policy(
                "entry_size",
                format!("inconsistent archive entry size for {name}"),
            ));
        }
        entries.insert(name, content);
    }

    let project_raw = entries
        .get("project.yaml")
        .ok_or_else(|| PortableIoError::policy("archive_project", "package has no project.yaml"))?;
    let project: PortableProject = parse_yaml(project_raw)?;
    let needed: HashSet<&str> = std::iter::once("project.yaml")
        .chain(project.catalogs.iter().map(|entry| entry.file.as_str()))
        .collect();
    if needed.len() != entries.len() || !entries.keys().all(|name| needed.contains(name.as_str())) {
        return Err(PortableIoError::policy(
            "archive_contents",
            "missing or unexpected package entries",
        ));
    }

    let mut catalogs = BTreeMap::new();
    for reference in &project.catalogs {
        let raw = entries.get(&reference.file).ok_or_else(|| {
            PortableIoError::policy(
                "archive_contents",
                format!("missing catalog {}", reference.file),
            )
        })?;
        catalogs.insert(reference.id.clone(), parse_yaml(raw)?);
    }
    let bundle = PortableBundle { project, catalogs };
    bundle.validate()?;
    Ok(bundle)
}

pub fn build_package_bytes(bundle: &PortableBundle) -> Result<Vec<u8>> {
    bundle.validate()?;
    let mut files = BTreeMap::new();
    files.insert(
        "project.yaml".to_owned(),
        to_yaml_string(&bundle.project)?.into_bytes(),
    );
    for reference in &bundle.project.catalogs {
        let catalog = bundle.catalogs.get(&reference.id).ok_or_else(|| {
            PortableIoError::policy("catalog_set", format!("missing catalog {}", reference.id))
        })?;
        files.insert(
            reference.file.clone(),
            to_yaml_string(catalog)?.into_bytes(),
        );
    }
    enforce_file_limits(files.values().map(Vec::len))?;

    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(6))
        .last_modified_time(DateTime::DEFAULT)
        .unix_permissions(0o100644);
    for (name, content) in files {
        writer.start_file(name, options)?;
        writer
            .write_all(&content)
            .map_err(|error| PortableIoError::io("write archive entry", error))?;
    }
    let raw = writer.finish()?.into_inner();
    read_package_bytes(&raw)?;
    Ok(raw)
}

pub fn write_package(bundle: &PortableBundle, destination: &Path) -> Result<()> {
    let raw = build_package_bytes(bundle)?;
    let absolute = std::path::absolute(destination)
        .map_err(|error| PortableIoError::io("package destination", error))?;
    let parent = absolute.parent().ok_or_else(|| {
        PortableIoError::policy("destination", "package destination has no parent")
    })?;
    let name = absolute.file_name().ok_or_else(|| {
        PortableIoError::policy("destination", "package destination has no file name")
    })?;
    ensure_simple_file_name(name)?;
    let directory = open_directory_nofollow(parent)?;
    if directory.symlink_metadata(name).is_ok() {
        return Err(PortableIoError::policy(
            "destination_exists",
            "package destination already exists",
        ));
    }

    let temp_name = create_temp_name();
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    let mut file = directory
        .open_with(&temp_name, &options)
        .map_err(|error| PortableIoError::io("create package temporary file", error))?;
    let result = (|| {
        file.write_all(&raw)
            .map_err(|error| PortableIoError::io("write package", error))?;
        file.sync_all()
            .map_err(|error| PortableIoError::io("sync package", error))?;
        drop(file);
        directory
            .hard_link(&temp_name, &directory, name)
            .map_err(|error| PortableIoError::io("publish package", error))?;
        // The destination now owns the same synced inode. A failed cleanup is
        // harmless and must not turn a successful publication into ambiguity.
        let _ = directory.remove_file(&temp_name);
        Ok(())
    })();
    if result.is_err() {
        let _ = directory.remove_file(&temp_name);
    }
    result
}

fn create_temp_name() -> OsString {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    OsString::from(format!(".zdashstitch-{}-{counter}.tmp", std::process::id()))
}

fn parse_yaml<T>(raw: &[u8]) -> Result<T>
where
    T: DeserializeOwned + Validate,
{
    if raw.len() > MAX_FILE_BYTES {
        return Err(PortableIoError::policy("file_size", "file too large"));
    }
    let text = std::str::from_utf8(raw)
        .map_err(|_| PortableIoError::policy("utf8", "file is not valid UTF-8"))?;
    if contains_yaml_alias_marker(text) {
        return Err(PortableIoError::policy(
            "yaml_alias",
            "YAML anchors and aliases are prohibited",
        ));
    }
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(text).map_err(FormatError::from)?;
    if yaml_depth(&value) > MAX_YAML_DEPTH {
        return Err(PortableIoError::policy(
            "yaml_depth",
            "YAML nesting is too deep",
        ));
    }
    let parsed: T = serde_yaml_ng::from_value(value).map_err(FormatError::from)?;
    parsed.validate()?;
    Ok(parsed)
}

fn yaml_depth(value: &serde_yaml_ng::Value) -> usize {
    match value {
        serde_yaml_ng::Value::Sequence(items) => {
            1 + items.iter().map(yaml_depth).max().unwrap_or(0)
        }
        serde_yaml_ng::Value::Mapping(items) => {
            1 + items
                .iter()
                .map(|(key, value)| yaml_depth(key).max(yaml_depth(value)))
                .max()
                .unwrap_or(0)
        }
        serde_yaml_ng::Value::Tagged(tagged) => 1 + yaml_depth(&tagged.value),
        _ => 0,
    }
}

fn contains_yaml_alias_marker(text: &str) -> bool {
    for line in text.lines() {
        let mut single_quoted = false;
        let mut double_quoted = false;
        let mut escaped = false;
        let characters: Vec<char> = line.chars().collect();
        for (index, character) in characters.iter().copied().enumerate() {
            if double_quoted && escaped {
                escaped = false;
                continue;
            }
            if double_quoted && character == '\\' {
                escaped = true;
                continue;
            }
            if !double_quoted && character == '\'' {
                single_quoted = !single_quoted;
                continue;
            }
            if !single_quoted && character == '"' {
                double_quoted = !double_quoted;
                continue;
            }
            if !single_quoted && !double_quoted && character == '#' {
                break;
            }
            if !single_quoted
                && !double_quoted
                && matches!(character, '&' | '*')
                && index
                    .checked_sub(1)
                    .and_then(|previous| characters.get(previous))
                    .is_none_or(|previous| {
                        previous.is_ascii_whitespace()
                            || matches!(previous, '[' | '{' | ',' | ':' | '?' | '-')
                    })
                && characters
                    .get(index + 1)
                    .is_some_and(|next| next.is_ascii_alphanumeric() || matches!(next, '_' | '-'))
            {
                return true;
            }
        }
    }
    false
}

fn enforce_file_limits(lengths: impl IntoIterator<Item = usize>) -> Result<()> {
    let mut total = 0usize;
    for length in lengths {
        if length > MAX_FILE_BYTES {
            return Err(PortableIoError::policy("file_size", "file too large"));
        }
        total = total
            .checked_add(length)
            .ok_or_else(|| PortableIoError::policy("total_size", "total size overflow"))?;
        if total > MAX_TOTAL_BYTES {
            return Err(PortableIoError::policy(
                "total_size",
                "project exceeds total size limit",
            ));
        }
    }
    Ok(())
}

fn read_absolute_regular(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let absolute =
        std::path::absolute(path).map_err(|error| PortableIoError::io("input path", error))?;
    let parent = absolute.parent().ok_or_else(|| {
        PortableIoError::policy("input_path", "input file has no parent directory")
    })?;
    let name = absolute
        .file_name()
        .ok_or_else(|| PortableIoError::policy("input_path", "input path has no file name"))?;
    ensure_simple_file_name(name)?;
    let directory = open_directory_nofollow(parent)?;
    read_regular(&directory, name, maximum)
}

fn read_relative_regular(root: &Dir, path: &Path) -> Result<Vec<u8>> {
    let mut components = path.components().peekable();
    let mut directory = root
        .try_clone()
        .map_err(|error| PortableIoError::io("clone project directory handle", error))?;
    loop {
        let component = components.next().ok_or_else(|| {
            PortableIoError::policy("relative_path", "empty project-relative path")
        })?;
        let Component::Normal(name) = component else {
            return Err(PortableIoError::policy(
                "relative_path",
                "project-relative path contains a forbidden component",
            ));
        };
        if components.peek().is_none() {
            return read_regular(&directory, name, MAX_FILE_BYTES);
        }
        directory = directory.open_dir_nofollow(name).map_err(|error| {
            PortableIoError::io(
                "open project directory component without following links",
                error,
            )
        })?;
    }
}

fn read_regular(directory: &Dir, name: &OsStr, maximum: usize) -> Result<Vec<u8>> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = directory
        .open_with(name, &options)
        .map_err(|error| PortableIoError::io("open regular file without following links", error))?;
    let metadata = file
        .metadata()
        .map_err(|error| PortableIoError::io("read file metadata", error))?;
    if !metadata.is_file() {
        return Err(PortableIoError::policy(
            "file_type",
            "input is not a regular file",
        ));
    }
    if metadata.len() > u64::try_from(maximum).unwrap_or(u64::MAX) {
        return Err(PortableIoError::policy("file_size", "file too large"));
    }
    let mut raw = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(maximum));
    Read::by_ref(&mut file)
        .take(u64::try_from(maximum).unwrap_or(u64::MAX) + 1)
        .read_to_end(&mut raw)
        .map_err(|error| PortableIoError::io("read regular file", error))?;
    if raw.len() > maximum {
        return Err(PortableIoError::policy("file_size", "file too large"));
    }
    Ok(raw)
}

fn open_directory_nofollow(path: &Path) -> Result<Dir> {
    let absolute =
        std::path::absolute(path).map_err(|error| PortableIoError::io("directory path", error))?;
    let mut anchor = PathBuf::new();
    let mut names = Vec::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => anchor.push(component.as_os_str()),
            Component::Normal(name) => names.push(name.to_owned()),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(PortableIoError::policy(
                    "directory_path",
                    "parent traversal is forbidden",
                ));
            }
        }
    }
    if anchor.as_os_str().is_empty() {
        return Err(PortableIoError::policy(
            "directory_path",
            "directory path has no filesystem root",
        ));
    }
    let mut directory = Dir::open_ambient_dir(&anchor, ambient_authority())
        .map_err(|error| PortableIoError::io("open filesystem root", error))?;
    for name in names {
        directory = directory.open_dir_nofollow(name).map_err(|error| {
            PortableIoError::io("open directory component without following links", error)
        })?;
    }
    Ok(directory)
}

fn ensure_simple_file_name(name: &OsStr) -> Result<()> {
    let path = Path::new(name);
    if path.components().count() == 1
        && matches!(path.components().next(), Some(Component::Normal(_)))
    {
        Ok(())
    } else {
        Err(PortableIoError::policy(
            "file_name",
            "file name contains a forbidden path component",
        ))
    }
}

fn is_catalog_path(value: &str) -> bool {
    let Some(name) = value.strip_prefix("metrics/") else {
        return false;
    };
    let Some(stem) = name
        .strip_suffix(".yaml")
        .or_else(|| name.strip_suffix(".yml"))
    else {
        return false;
    };
    let mut characters = stem.chars();
    characters
        .next()
        .is_some_and(|character| character.is_ascii_lowercase())
        && (2..=64).contains(&stem.len())
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '_' | '-')
        })
}
