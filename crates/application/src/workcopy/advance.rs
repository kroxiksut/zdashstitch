// SPDX-License-Identifier: MIT OR Apache-2.0

//! P1-08G «Снимок после записи стал новой базой черновика»: after a
//! verified (or reconciled verified) update of the source dashboard, the
//! dashboard read now becomes the new Base of the workcopy and its
//! operations are cleared, so the next preview compares with what is on
//! the server.
//!
//! Only when everything still matches the published plan: the workcopy is
//! at the published revision, its Base is the plan's source, and the
//! dashboard read now has exactly the expected fingerprint (equal to the
//! published Local). Otherwise nothing changes and the draft keeps its old
//! Base (a later check shows the conflict). Reads only, through a
//! [`ReadOnlyGateway`]; the old snapshot stays on disk.

use serde_json::Value;
use zdashstitch_zabbix::ReadOnlyGateway;

use super::formats::{BaseReference, LOCAL_ONLY, SNAPSHOT_FORMAT, SnapshotFile, SnapshotScope};
use super::service::{count, local_metadata, now_ms, referenced_items, zabbix_error};
use super::view;
use super::{WorkcopyError, WorkcopyService};

/// What the published plan left on the server, from the journal entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedUpdate {
    pub local_id: String,
    pub connection_id: String,
    /// Workcopy revision the plan was built from.
    pub revision: u64,
    /// Canonical fingerprint of Base the plan was built on.
    pub source_fingerprint: String,
    /// Canonical fingerprint the write leaves (equals the published Local).
    pub expected_fingerprint: String,
}

/// The new Base after [`WorkcopyService::advance_base`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdvancedBase {
    pub local_id: String,
    pub revision: u64,
    /// Canonical fingerprint of the new Base.
    pub base_fingerprint: String,
    /// Unix epoch milliseconds of the read.
    pub read_at_ms: u64,
}

impl WorkcopyService {
    /// Makes the dashboard read now the new Base of `published.local_id`
    /// and clears its operations, when the workcopy and the server are
    /// exactly as the verified write left them (module docs).
    ///
    /// Errors: `workcopy_*`, `snapshot_*`, `workcopy_other_connection`,
    /// `workcopy_changed_concurrently` (the draft changed after the plan),
    /// `base_advance_mismatch` (Base or the server is not what the plan
    /// expected; nothing changed), connection taxonomy codes of the read,
    /// storage codes.
    pub async fn advance_base(
        &self,
        reader: &ReadOnlyGateway,
        published: &PublishedUpdate,
    ) -> Result<AdvancedBase, WorkcopyError> {
        let (file, base) = self.load_with_base(&published.local_id)?;
        if file.source.connection_id != published.connection_id {
            return Err(WorkcopyError::new("workcopy_other_connection"));
        }
        if file.revision != published.revision {
            return Err(WorkcopyError::new("workcopy_changed_concurrently"));
        }
        if base.fingerprint() != published.source_fingerprint {
            return Err(WorkcopyError::new("base_advance_mismatch").with("what", "base"));
        }
        let first_read_at_ms = now_ms();
        let remote = reader
            .dashboard_snapshot(&file.source.connection_id, &file.source.dashboardid)
            .await
            .map_err(|error| zabbix_error(&error))?
            .ok_or_else(|| WorkcopyError::new("base_advance_mismatch").with("what", "missing"))?;
        if remote.fingerprint() != published.expected_fingerprint
            || remote.server_version() != base.server_version()
            || remote.adapter_id() != base.adapter_id()
        {
            return Err(WorkcopyError::new("base_advance_mismatch").with("what", "remote"));
        }
        let (itemids, capped) = referenced_items(&remote)?;
        let items = if itemids.is_empty() {
            Ok(Vec::new())
        } else {
            reader.item_metadata(&itemids).await
        };
        let metadata = local_metadata(&itemids, capped, items)?;
        let read_at_ms = now_ms();
        let raw = remote.raw_api_document();
        let analysis = remote.analyze().map_err(|error| zabbix_error(&error))?;
        let (page_count, widget_count) = match &file.source.page {
            None => (count(analysis.page_count), count(analysis.widget_count)),
            Some(page) => {
                let (index, _) = view::find_page(raw, &page.dashboard_pageid)
                    .ok_or_else(|| WorkcopyError::new("import_page_not_found"))?;
                (
                    1,
                    count(analysis.widgets_per_page.get(index).copied().unwrap_or(0)),
                )
            }
        };
        let fingerprint = remote.storage_fingerprint().to_owned();
        self.store.put_snapshot(&SnapshotFile {
            format: SNAPSHOT_FORMAT.to_owned(),
            storage_scope: LOCAL_ONLY.to_owned(),
            connection_id: file.source.connection_id.clone(),
            adapter_id: remote.adapter_id().to_owned(),
            server_version: remote.server_version().to_string(),
            dashboardid: file.source.dashboardid.clone(),
            scope: SnapshotScope::Dashboard,
            first_read_at_ms,
            last_read_at_ms: read_at_ms,
            first_read_fingerprint: fingerprint.clone(),
            last_read_fingerprint: fingerprint.clone(),
            fingerprint: fingerprint.clone(),
            raw_api_document: raw.clone(),
        })?;
        // The draft may have changed during the reads: check again right
        // before replacing it.
        let mut current = self.load(&published.local_id)?;
        if current.revision != published.revision
            || current.base.fingerprint != file.base.fingerprint
        {
            return Err(WorkcopyError::new("workcopy_changed_concurrently"));
        }
        current.base = BaseReference {
            fingerprint,
            read_at_ms,
            source_checked_at_ms: read_at_ms,
            page_count,
            widget_count,
        };
        current.source.dashboard_name = raw
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        current.operations = Vec::new();
        current.local_metadata = metadata;
        current.revision = current.revision.saturating_add(1);
        current.updated_at_ms = read_at_ms.max(current.updated_at_ms);
        self.store.replace_workcopy(&current)?;
        Ok(AdvancedBase {
            local_id: published.local_id.clone(),
            revision: current.revision,
            base_fingerprint: remote.fingerprint().to_owned(),
            read_at_ms,
        })
    }
}
