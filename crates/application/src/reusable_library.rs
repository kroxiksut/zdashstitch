// SPDX-License-Identifier: MIT OR Apache-2.0

//! Canonical, bounded storage for reusable widget assets. Metadata stays local.

use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zdashstitch_core::formats::{
    BLUEPRINT_FORMAT, FormatError, MAX_FILE_BYTES, PROFILE_FORMAT, StyleProfile, Validate,
    WidgetBlueprint, strict, to_yaml_string,
};

use crate::{
    portable_scan::{CandidateScan, scan_data},
    workcopy::{
        WorkcopyError,
        store::{LocalStore, read_bounded, write_atomic},
    },
};

const PATH: &[&str] = &["library"];
const FILE: &str = "reusable-widgets.json";
const MAX_STORE: usize = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 512;
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    content = "document",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ReusableDocument {
    StyleProfile(StyleProfile),
    WidgetBlueprint(WidgetBlueprint),
}

impl ReusableDocument {
    pub fn parse_yaml(bytes: &[u8]) -> Result<Self, WorkcopyError> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(WorkcopyError::new("file_too_large"));
        }
        let value = strict::yaml_value_from_slice(bytes)
            .map_err(|error| format_error(&FormatError::from(error)))?;
        match value.get("format").and_then(Value::as_str) {
            Some(PROFILE_FORMAT) => {
                let doc = strict::decode(strict::DocumentSyntax::Yaml, value)
                    .map_err(|error| format_error(&FormatError::from(error)))?;
                Self::validated(Self::StyleProfile(doc))
            }
            Some(BLUEPRINT_FORMAT) => {
                let doc = strict::decode(strict::DocumentSyntax::Yaml, value)
                    .map_err(|error| format_error(&FormatError::from(error)))?;
                Self::validated(Self::WidgetBlueprint(doc))
            }
            Some(format) if is_newer(format) => Err(WorkcopyError::new("format_newer")),
            _ => Err(WorkcopyError::new("reusable_format_unsupported")),
        }
    }

    pub fn validated(doc: Self) -> Result<Self, WorkcopyError> {
        doc.validate().map_err(|e| format_error(&e))?;
        Ok(doc)
    }

    fn validate(&self) -> Result<(), FormatError> {
        match self {
            Self::StyleProfile(doc) => doc.validate(),
            Self::WidgetBlueprint(doc) => doc.validate(),
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::StyleProfile(doc) => &doc.profile_id,
            Self::WidgetBlueprint(doc) => &doc.blueprint_id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::StyleProfile(doc) => &doc.name,
            Self::WidgetBlueprint(doc) => &doc.name,
        }
    }

    pub fn adapter(&self) -> &str {
        match self {
            Self::StyleProfile(doc) => &doc.adapter,
            Self::WidgetBlueprint(doc) => &doc.adapter,
        }
    }

    pub const fn version(&self) -> u32 {
        match self {
            Self::StyleProfile(doc) => doc.version,
            Self::WidgetBlueprint(doc) => doc.version,
        }
    }

    pub const fn kind(&self) -> &'static str {
        match self {
            Self::StyleProfile(_) => "style_profile",
            Self::WidgetBlueprint(_) => "widget_blueprint",
        }
    }

    fn set_id(&mut self, id: String) {
        match self {
            Self::StyleProfile(doc) => doc.profile_id = id,
            Self::WidgetBlueprint(doc) => doc.blueprint_id = id,
        }
    }

    fn set_version(&mut self, version: u32) {
        match self {
            Self::StyleProfile(doc) => doc.version = version,
            Self::WidgetBlueprint(doc) => doc.version = version,
        }
    }

    fn edit_metadata(&mut self, update: Metadata) {
        match self {
            Self::StyleProfile(doc) => {
                doc.name = update.name;
                doc.description = update.description;
                doc.tags = update.tags;
            }
            Self::WidgetBlueprint(doc) => {
                doc.name = update.name;
                doc.description = update.description;
                doc.tags = update.tags;
            }
        }
    }

    pub fn scan(&self) -> Result<CandidateScan, WorkcopyError> {
        let data = match self {
            Self::StyleProfile(doc) => serde_json::to_value(doc),
            Self::WidgetBlueprint(doc) => serde_json::to_value(doc),
        }
        .map_err(|_| corrupt())?;
        Ok(scan_data(&data))
    }

    pub fn export(&self) -> Result<Vec<u8>, WorkcopyError> {
        let yaml = match self {
            Self::StyleProfile(doc) => to_yaml_string(doc),
            Self::WidgetBlueprint(doc) => to_yaml_string(doc),
        }
        .map_err(|error| format_error(&error))?;
        let bytes = yaml.into_bytes();
        if bytes.len() > MAX_FILE_BYTES {
            return Err(WorkcopyError::new("file_too_large"));
        }
        if Self::parse_yaml(&bytes)? != *self {
            return Err(corrupt());
        }
        Ok(bytes)
    }
}

fn is_newer(format: &str) -> bool {
    let Some((namespace, version)) = format.rsplit_once("/v") else {
        return false;
    };
    matches!(
        namespace,
        "zdashstitch.style-profile" | "zdashstitch.widget-blueprint"
    ) && !version.is_empty()
        && version.bytes().all(|b| b.is_ascii_digit())
        && (version.parse::<u64>().map_or(version.len() > 20, |n| n > 1))
}

fn format_error(error: &FormatError) -> WorkcopyError {
    WorkcopyError::new(error.code().unwrap_or("reusable_invalid"))
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReusableOrigin {
    Builtin,
    File,
    Local,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReusableEntry {
    pub revision: u64,
    pub origin: ReusableOrigin,
    pub asset: ReusableDocument,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum ImportMode {
    New,
    Replace { expected_revision: u64 },
    Copy,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LibraryFile {
    format: LibraryFormat,
    entries: BTreeMap<String, ReusableEntry>,
}

#[derive(Deserialize, Serialize)]
enum LibraryFormat {
    #[serde(rename = "zdashstitch.reusable-widget-library/v0")]
    V0,
}

pub struct ReusableLibrary {
    root: PathBuf,
}

impl ReusableLibrary {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn read(&self) -> Result<LibraryFile, WorkcopyError> {
        let store = LocalStore::at(self.root.clone());
        let Some(dir) = store.path(PATH, false)? else {
            return Ok(empty());
        };
        let Some(raw) = read_bounded(&dir, FILE, MAX_STORE)? else {
            return Ok(empty());
        };
        let text = std::str::from_utf8(&raw).map_err(|_| corrupt())?;
        let library: LibraryFile = strict::json_from_str(text).map_err(|_| corrupt())?;
        if library.entries.len() > MAX_ENTRIES {
            return Err(corrupt());
        }
        for (id, entry) in &library.entries {
            if entry.revision == 0
                || entry.asset.id() != id
                || entry.origin == ReusableOrigin::Builtin
                || builtin(id).is_some()
            {
                return Err(corrupt());
            }
            entry.asset.validate().map_err(|_| corrupt())?;
        }
        Ok(library)
    }

    fn modify<T>(
        &self,
        apply: impl FnOnce(&mut BTreeMap<String, ReusableEntry>) -> Result<T, WorkcopyError>,
    ) -> Result<T, WorkcopyError> {
        let _lock = WRITE_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
        let mut library = self.read()?;
        let result = apply(&mut library.entries)?;
        if library.entries.len() > MAX_ENTRIES {
            return Err(WorkcopyError::new("storage_limit_reached"));
        }
        let bytes = serde_json::to_vec(&library).map_err(|_| corrupt())?;
        if bytes.len() > MAX_STORE {
            return Err(WorkcopyError::new("storage_limit_reached"));
        }
        let directory = LocalStore::at(self.root.clone())
            .path(PATH, true)?
            .ok_or_else(|| WorkcopyError::new("storage_unavailable"))?;
        write_atomic(&directory, FILE, &bytes)?;
        Ok(result)
    }

    pub fn list(&self) -> Result<Vec<ReusableEntry>, WorkcopyError> {
        let mut entries: Vec<_> = self.read()?.entries.into_values().collect();
        entries.extend(builtins().iter().cloned());
        Ok(entries)
    }

    pub fn get(&self, id: &str) -> Result<ReusableEntry, WorkcopyError> {
        if let Some(entry) = builtin(id) {
            return Ok(entry.clone());
        }
        self.read()?.entries.remove(id).ok_or_else(not_found)
    }

    pub fn import(
        &self,
        mut asset: ReusableDocument,
        mode: ImportMode,
    ) -> Result<ReusableEntry, WorkcopyError> {
        asset.validate().map_err(|error| format_error(&error))?;
        self.modify(move |entries| {
            let revision = match mode {
                ImportMode::New => {
                    if builtin(asset.id()).is_some() || entries.contains_key(asset.id()) {
                        return Err(conflict());
                    }
                    1
                }
                ImportMode::Replace { expected_revision } => {
                    let old = editable(entries, asset.id(), expected_revision)?;
                    if old.asset.kind() != asset.kind() {
                        return Err(conflict());
                    }
                    let version = asset
                        .version()
                        .max(old.asset.version())
                        .checked_add(1)
                        .ok_or_else(limit)?;
                    asset.set_version(version);
                    increment(old)?
                }
                ImportMode::Copy => {
                    asset.set_id(new_id(entries)?);
                    asset.set_version(1);
                    1
                }
            };
            let entry = ReusableEntry {
                asset,
                revision,
                origin: ReusableOrigin::File,
            };
            entries.insert(entry.asset.id().to_owned(), entry.clone());
            Ok(entry)
        })
    }

    pub fn create(&self, asset: ReusableDocument) -> Result<ReusableEntry, WorkcopyError> {
        self.modify(move |entries| new_local(entries, asset))
    }

    pub fn update_style(
        &self,
        asset: StyleProfile,
        expected_revision: u64,
    ) -> Result<ReusableEntry, WorkcopyError> {
        self.modify(move |entries| {
            let mut asset = ReusableDocument::StyleProfile(asset);
            let old = editable(entries, asset.id(), expected_revision)?;
            if old.asset.kind() != asset.kind() {
                return Err(conflict());
            }
            asset.set_version(old.asset.version().checked_add(1).ok_or_else(limit)?);
            asset.validate().map_err(|error| format_error(&error))?;
            old.asset = asset;
            old.revision = increment(old)?;
            Ok(old.clone())
        })
    }

    pub fn duplicate(
        &self,
        id: &str,
        expected_revision: u64,
    ) -> Result<ReusableEntry, WorkcopyError> {
        self.modify(|entries| {
            let old = builtin(id)
                .or_else(|| entries.get(id))
                .ok_or_else(not_found)?;
            if old.revision != expected_revision {
                return Err(conflict());
            }
            new_local(entries, old.asset.clone())
        })
    }

    pub fn edit_metadata(
        &self,
        id: &str,
        expected_revision: u64,
        metadata: Metadata,
    ) -> Result<ReusableEntry, WorkcopyError> {
        self.modify(move |entries| {
            let entry = editable(entries, id, expected_revision)?;
            entry.asset.edit_metadata(metadata);
            entry
                .asset
                .validate()
                .map_err(|error| format_error(&error))?;
            entry.revision = increment(entry)?;
            Ok(entry.clone())
        })
    }

    pub fn delete(&self, id: &str, expected_revision: u64) -> Result<(), WorkcopyError> {
        self.modify(|entries| {
            editable(entries, id, expected_revision)?;
            entries.remove(id);
            Ok(())
        })
    }
}

fn new_local(
    entries: &mut BTreeMap<String, ReusableEntry>,
    mut asset: ReusableDocument,
) -> Result<ReusableEntry, WorkcopyError> {
    asset.set_id(new_id(entries)?);
    asset.set_version(1);
    asset.validate().map_err(|error| format_error(&error))?;
    let entry = ReusableEntry {
        asset,
        revision: 1,
        origin: ReusableOrigin::Local,
    };
    entries.insert(entry.asset.id().to_owned(), entry.clone());
    Ok(entry)
}

fn editable<'a>(
    entries: &'a mut BTreeMap<String, ReusableEntry>,
    id: &str,
    expected: u64,
) -> Result<&'a mut ReusableEntry, WorkcopyError> {
    if builtin(id).is_some() {
        return Err(WorkcopyError::new("reusable_builtin_read_only"));
    }
    let entry = entries.get_mut(id).ok_or_else(not_found)?;
    if entry.revision != expected {
        return Err(conflict());
    }
    Ok(entry)
}

fn increment(entry: &ReusableEntry) -> Result<u64, WorkcopyError> {
    entry.revision.checked_add(1).ok_or_else(limit)
}

fn new_id(entries: &BTreeMap<String, ReusableEntry>) -> Result<String, WorkcopyError> {
    for _ in 0..4 {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| WorkcopyError::new("random_source_unavailable"))?;
        bytes[6] = bytes[6] & 0x0f | 0x40;
        bytes[8] = bytes[8] & 0x3f | 0x80;
        let digits: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let id = format!(
            "{}-{}-{}-{}-{}",
            &digits[..8],
            &digits[8..12],
            &digits[12..16],
            &digits[16..20],
            &digits[20..]
        );
        if !entries.contains_key(&id) && builtin(&id).is_none() {
            return Ok(id);
        }
    }
    Err(WorkcopyError::new("random_source_unavailable"))
}

fn empty() -> LibraryFile {
    LibraryFile {
        format: LibraryFormat::V0,
        entries: BTreeMap::new(),
    }
}

fn corrupt() -> WorkcopyError {
    WorkcopyError::new("reusable_storage_invalid")
}
fn conflict() -> WorkcopyError {
    WorkcopyError::new("reusable_conflict")
}
fn not_found() -> WorkcopyError {
    WorkcopyError::new("reusable_not_found")
}
fn limit() -> WorkcopyError {
    WorkcopyError::new("storage_limit_reached")
}

fn builtin(id: &str) -> Option<&'static ReusableEntry> {
    builtins().iter().find(|entry| entry.asset.id() == id)
}

fn builtins() -> &'static [ReusableEntry] {
    use std::sync::LazyLock;
    static ENTRIES: LazyLock<[ReusableEntry; 2]> = LazyLock::new(|| {
        [
            include_bytes!("../../../examples/style-profile.yaml").as_slice(),
            br#"format: zdashstitch.style-profile/v1
profile_id: 45d10c01-1a20-47f7-98f2-4bdad3475de3
name: CPU load
version: 1
adapter: "7.4"
widget_types:
  svggraph:
    dataset_fields: { type: 0, width: 2 }
    series_palette: ["3156A3", "1F6B43"]
"#,
        ]
        .map(|bytes| ReusableEntry {
            revision: 1,
            origin: ReusableOrigin::Builtin,
            asset: ReusableDocument::parse_yaml(bytes)
                .unwrap_or_else(|error| panic!("invalid bundled reusable asset: {error}")),
        })
    });
    ENTRIES.as_slice()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_collision_versions_and_safe_export() {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let library = ReusableLibrary::new(root.path().to_owned());
        let document =
            ReusableDocument::parse_yaml(include_bytes!("../../../examples/widget-blueprint.yaml"))
                .unwrap_or_else(|error| panic!("{error}"));
        let first = library
            .import(document.clone(), ImportMode::New)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(first.revision, 1);
        let bad = library.import(document.clone(), ImportMode::New);
        assert!(matches!(bad, Err(error) if error.code() == "reusable_conflict"));
        assert_eq!(
            library.get(document.id()).map(|entry| entry.revision).ok(),
            Some(1)
        );
        assert!(
            library
                .import(
                    document.clone(),
                    ImportMode::Replace {
                        expected_revision: 2
                    }
                )
                .is_err()
        );
        let replace = library
            .import(
                document,
                ImportMode::Replace {
                    expected_revision: 1,
                },
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(replace.revision, 2);
        assert_eq!(replace.asset.version(), 2);
        let copy = library
            .duplicate(replace.asset.id(), replace.revision)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_ne!(copy.asset.id(), replace.asset.id());
        assert_eq!(copy.asset.version(), 1);
        let bytes = copy
            .asset
            .export()
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(ReusableDocument::parse_yaml(&bytes).ok(), Some(copy.asset));
        let data = String::from_utf8(bytes).unwrap_or_else(|error| panic!("{error}"));
        for private in [
            "origin:",
            "revision:",
            "hostid:",
            "itemid:",
            "dashboardid:",
            "catalog:",
        ] {
            assert!(!data.contains(private), "private field {private}");
        }
    }

    #[test]
    fn builtins_are_immutable_and_newer_documents_are_refused() {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let library = ReusableLibrary::new(root.path().to_owned());
        let entries = library.list().unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .all(|entry| entry.origin == ReusableOrigin::Builtin)
        );
        assert!(
            library
                .delete(entries[0].asset.id(), entries[0].revision)
                .is_err()
        );
        assert!(
            library
                .duplicate(entries[0].asset.id(), entries[0].revision)
                .is_ok()
        );
        assert!(matches!(
            ReusableDocument::parse_yaml(b"format: zdashstitch.style-profile/v2\n"),
            Err(error) if error.code() == "format_newer"
        ));
        assert!(matches!(
            ReusableDocument::parse_yaml(b"a: &anchor value\nb: *anchor\n"),
            Err(error) if error.code() == "yaml_alias"
        ));
    }
}
