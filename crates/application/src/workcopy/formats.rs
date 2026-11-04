// SPDX-License-Identifier: MIT OR Apache-2.0

//! Internal local-only storage formats of O-018 (P2-05B). They are not part
//! of `schemas/`, carry no exchange guarantee and are never exported:
//!
//! - `zdashstitch.raw-snapshot/v0` — one immutable dashboard read;
//! - `zdashstitch.workcopy/v0` — Base reference, operation log, the list of
//!   dashboards created from the copy (O-022) and local item metadata (I-4).
//!
//! Every file carries `format` and `storage_scope: local_only`. A file with
//! another `format` is never rewritten: v0 is the first version, so there is
//! no migration yet and such a file is rejected with a stable code.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::WorkcopyError;

pub(crate) const SNAPSHOT_FORMAT: &str = "zdashstitch.raw-snapshot/v0";
pub(crate) const WORKCOPY_FORMAT: &str = "zdashstitch.workcopy/v0";
pub(crate) const LOCAL_ONLY: &str = "local_only";

/// Format probe read before the full document, so that a future or unknown
/// version is reported as such rather than as a parse error.
#[derive(Deserialize)]
pub(crate) struct FormatHeader {
    pub format: Option<String>,
}

/// Version gate shared by both internal formats. `Ok` means the document can
/// be parsed by this build without migration.
pub(crate) fn check_format(
    raw: &[u8],
    expected: &str,
    family: &str,
    unsupported_code: &'static str,
    corrupt_code: &'static str,
) -> Result<(), WorkcopyError> {
    let header: FormatHeader =
        serde_json::from_slice(raw).map_err(|_| WorkcopyError::new(corrupt_code))?;
    match header.format.as_deref() {
        Some(format) if format == expected => Ok(()),
        Some(format) if format.starts_with(family) => Err(WorkcopyError::new(unsupported_code)
            .with("format", format.chars().take(64).collect::<String>())),
        _ => Err(WorkcopyError::new(corrupt_code)),
    }
}

/// What a snapshot covers. P2-05 always reads a whole dashboard; a page copy
/// keeps the whole dashboard as Base and selects the page in the workcopy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SnapshotScope {
    Dashboard,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotFile {
    pub format: String,
    pub storage_scope: String,
    pub connection_id: String,
    pub adapter_id: String,
    pub server_version: String,
    pub dashboardid: String,
    pub scope: SnapshotScope,
    pub first_read_at_ms: u64,
    pub last_read_at_ms: u64,
    pub first_read_fingerprint: String,
    pub last_read_fingerprint: String,
    /// Storage fingerprint of `raw_api_document` (exact bytes, the P0
    /// algorithm); also the file name. The canonical fingerprint that
    /// compares server state is recomputed on load and never stored here.
    pub fingerprint: String,
    pub raw_api_document: Value,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftKind {
    Dashboard,
    Page,
}

impl DraftKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dashboard => "dashboard",
            Self::Page => "page",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PageSelector {
    pub dashboard_pageid: String,
    /// Server data captured at import.
    pub page_name: String,
    /// 1-based position of the page in Base.
    pub page_number: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkcopySource {
    pub connection_id: String,
    pub dashboardid: String,
    /// Server data captured at import, shown as the source name.
    pub dashboard_name: String,
    pub page: Option<PageSelector>,
    pub adapter_id: String,
    pub server_version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BaseReference {
    /// Snapshot file `snapshots/<connection_id>/<dashboardid>/<fingerprint>.json`:
    /// the storage fingerprint of Base, not a statement about the server.
    pub fingerprint: String,
    /// Time of the final consistent read.
    pub read_at_ms: u64,
    /// Last time the source was found unchanged on the server.
    pub source_checked_at_ms: u64,
    /// Size of the draft scope in Base, for lists without reading the snapshot.
    pub page_count: u32,
    pub widget_count: u32,
}

/// A dashboard created on the server from this copy (O-022). Written only
/// after a verified read-back; empty until P2-06.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreatedDashboardRecord {
    pub connection_id: String,
    pub dashboardid: String,
    pub fingerprint: String,
    pub created_at_ms: u64,
    /// Server data at creation time.
    pub name: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemMetadataState {
    /// The Base references no item IDs.
    NotNeeded,
    /// Every referenced item was read.
    Complete,
    /// Some referenced items were not returned (not visible or deleted) or
    /// the per-copy limit was reached.
    Partial,
    /// `item.get` was denied to the account or returned an unusable answer.
    Unavailable,
}

impl ItemMetadataState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotNeeded => "not_needed",
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StoredItemMetadata {
    pub value_type: u8,
    pub units: String,
    /// Key of [`LocalMetadata::value_maps`].
    pub valuemapid: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StoredValueMapping {
    pub mapping_type: u8,
    pub value: String,
    pub newvalue: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StoredValueMap {
    pub name: String,
    pub mappings: Vec<StoredValueMapping>,
}

/// Local-only item metadata of a copy (I-4): value type, units and value
/// map per referenced item, keyed by `itemid`. Never written to portable
/// files.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalMetadata {
    pub items_state: ItemMetadataState,
    pub items: BTreeMap<String, StoredItemMetadata>,
    pub value_maps: BTreeMap<String, StoredValueMap>,
    /// Referenced item IDs that `item.get` did not return.
    pub missing_items: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkcopyFile {
    pub format: String,
    pub storage_scope: String,
    pub workcopy_id: String,
    pub kind: DraftKind,
    /// User data; initially the source dashboard or page name.
    pub name: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    /// Incremented on every committed change of this file.
    pub revision: u64,
    pub source: WorkcopySource,
    pub base: BaseReference,
    /// Ordered, versioned operations replayed on Base (O-018 §3). Each entry
    /// is `{"seq": n, "op": "<name>", "op_version": n, ...}`; the known
    /// types are in [`super::operations`]. An entry this build does not
    /// know (name or version) rejects the whole workcopy, never dropped.
    pub operations: Vec<Value>,
    pub created_dashboards: Vec<CreatedDashboardRecord>,
    pub local_metadata: LocalMetadata,
}

impl WorkcopyFile {
    /// Structural checks after parsing: identifiers that become path
    /// components, scope and the operation log this build can replay.
    pub(crate) fn validate(&self, expected_id: &str) -> Result<(), WorkcopyError> {
        let corrupt = || WorkcopyError::new("workcopy_corrupt");
        if self.storage_scope != LOCAL_ONLY || self.workcopy_id != expected_id {
            return Err(corrupt());
        }
        if !super::is_connection_id(&self.source.connection_id)
            || !zdashstitch_zabbix::is_zabbix_id(&self.source.dashboardid)
            || !super::is_fingerprint(&self.base.fingerprint)
        {
            return Err(corrupt());
        }
        match (&self.kind, &self.source.page) {
            (DraftKind::Dashboard, None) => {}
            (DraftKind::Page, Some(page))
                if zdashstitch_zabbix::is_zabbix_id(&page.dashboard_pageid) => {}
            _ => return Err(corrupt()),
        }
        super::operations::parse_log(&self.operations, &self.base.fingerprint)?;
        Ok(())
    }
}
