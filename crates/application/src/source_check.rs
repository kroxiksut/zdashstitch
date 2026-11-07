// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-04C: re-reading Remote and comparing it with Base.
//!
//! The application re-reads the source dashboard of a workcopy
//!
//! - when the workcopy is opened and on the explicit «Проверить» command:
//!   [`WorkcopyService::check_source`];
//! - before the publication preview: [`recheck_replace_host`] (Remote plus
//!   the fresh revalidation of the stored ReplaceHost calculation);
//! - before building the plan: [`crate::plan_replace_host`] (`Remote !=
//!   Base` → `PlanError::Invalidated(SourceChanged)`, no plan);
//! - immediately before the write: the adapter's closed publication path
//!   (`Remote != source` → `Conflict`, zero `dashboard.update`).
//!
//! Every check is a read through a [`ReadOnlyGateway`]; nothing here can
//! write. A check that does not find Remote equal to Base returns the
//! typed invalidation reason of P1-01G §6, so a plan or preview built
//! before it is dropped.
//!
//! ```compile_fail
//! async fn no_write_gateway(
//!     service: &zdashstitch_application::workcopy::WorkcopyService,
//!     writer: &zdashstitch_zabbix::ReadWriteGateway,
//! ) {
//!     let _ = service.check_source(writer, "conn-0", "wc-0").await;
//! }
//! ```
//!
//! Control: the same call compiles with the read-only gateway.
//!
//! ```no_run
//! async fn read_gateway(
//!     service: &zdashstitch_application::workcopy::WorkcopyService,
//!     reader: &zdashstitch_zabbix::ReadOnlyGateway,
//! ) {
//!     let _ = service.check_source(reader, "conn-0", "wc-0").await;
//! }
//! ```

use zdashstitch_zabbix::{ReadOnlyGateway, VersionedDashboardSnapshot, ZabbixError};

use crate::PlanInvalidReason;
use crate::replace_host::{ReplaceHostPreview, SessionContext};
use crate::workcopy::{
    DraftFreshness, DraftFreshnessState, WorkcopyError, WorkcopyService, now_ms,
};

/// Result of one re-read of Remote for a workcopy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceCheck {
    pub local_id: String,
    /// `current`, `conflict`, `source_missing`, or `stale` when the server
    /// version differs from Base (fingerprints are then not comparable and
    /// no read is made). `checked_at_ms` only for `current`.
    pub freshness: DraftFreshness,
    pub base_fingerprint: String,
    /// Fingerprint of the re-read Remote; `None` when missing, hidden or
    /// not read.
    pub remote_fingerprint: Option<String>,
    /// Why a preview or plan built before this check is no longer valid:
    /// `source_changed` (Remote differs or is gone) or `connection_changed`
    /// (another server version). `None` when Remote equals Base.
    pub invalidated: Option<PlanInvalidReason>,
}

impl SourceCheck {
    /// Remote equals Base.
    #[must_use]
    pub const fn is_current(&self) -> bool {
        self.invalidated.is_none()
    }
}

fn read_error(error: &ZabbixError) -> WorkcopyError {
    match error {
        ZabbixError::SnapshotShape(_) => WorkcopyError::new("snapshot_shape_invalid"),
        other => WorkcopyError::from_zabbix(other),
    }
}

impl WorkcopyService {
    /// Re-reads the source dashboard of `local_id` on the connection
    /// `connection_id` (the active profile) and compares it with Base.
    /// Writes nothing, locally or on the server.
    ///
    /// Errors: `workcopy_id_invalid`, `workcopy_not_found`,
    /// `workcopy_corrupt`, `snapshot_*`, `workcopy_other_connection` (the
    /// workcopy belongs to another profile), connection taxonomy codes of
    /// the read.
    pub async fn check_source(
        &self,
        reader: &ReadOnlyGateway,
        connection_id: &str,
        local_id: &str,
    ) -> Result<SourceCheck, WorkcopyError> {
        let (file, base) = self.load_with_base(local_id)?;
        if file.source.connection_id != connection_id {
            return Err(WorkcopyError::new("workcopy_other_connection"));
        }
        check_snapshot_source(reader, local_id, &base).await
    }

    /// Compares the source with the Base pinned by a Clone branch. This
    /// observation can be taken before create and after read-back even if
    /// the source workcopy was updated independently in the meantime.
    pub async fn check_clone_source(
        &self,
        reader: &ReadOnlyGateway,
        clone_id: &str,
    ) -> Result<SourceCheck, WorkcopyError> {
        let branch = self.open_clone_workcopy(clone_id)?;
        let draft = branch.draft();
        if reader.identity().fingerprint() != draft.intent().server_identity_fingerprint() {
            return Err(WorkcopyError::new("clone_intent_server_changed"));
        }
        check_snapshot_source(reader, clone_id, draft.base()).await
    }
}

pub(crate) async fn check_snapshot_source(
    reader: &ReadOnlyGateway,
    local_id: &str,
    base: &VersionedDashboardSnapshot,
) -> Result<SourceCheck, WorkcopyError> {
    let check = |state, checked_at_ms, remote_fingerprint, invalidated| SourceCheck {
        local_id: local_id.to_owned(),
        freshness: DraftFreshness {
            state,
            checked_at_ms,
        },
        base_fingerprint: base.fingerprint().to_owned(),
        remote_fingerprint,
        invalidated,
    };
    if reader.capabilities().version != *base.server_version() {
        return Ok(check(
            DraftFreshnessState::Stale,
            None,
            None,
            Some(PlanInvalidReason::ConnectionChanged),
        ));
    }
    let remote = reader
        .dashboard_snapshot(base.connection_id(), base.dashboard_id())
        .await
        .map_err(|error| read_error(&error))?;
    let checked_at_ms = now_ms();
    Ok(match remote {
        None => check(
            DraftFreshnessState::SourceMissing,
            None,
            None,
            Some(PlanInvalidReason::SourceChanged),
        ),
        Some(remote) if remote.fingerprint() == base.fingerprint() => check(
            DraftFreshnessState::Current,
            Some(checked_at_ms),
            Some(remote.fingerprint().to_owned()),
            None,
        ),
        Some(remote) => check(
            DraftFreshnessState::Conflict,
            None,
            Some(remote.fingerprint().to_owned()),
            Some(PlanInvalidReason::SourceChanged),
        ),
    })
}

/// The publication preview inputs re-checked against the server.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaceHostRecheck {
    pub source: SourceCheck,
    /// The stored calculation revalidated with fresh facts.
    pub preview: ReplaceHostPreview,
}

impl ReplaceHostRecheck {
    /// Whether a plan may be requested for this preview: Remote equals Base
    /// and no finding blocks publication. The plan itself repeats every
    /// check.
    #[must_use]
    pub fn allows_plan(&self) -> bool {
        self.source.is_current() && self.preview.report.allows_publication()
    }
}

/// «Before preview» (P1-08 `publication.preview` / `publication.recheck`):
/// re-reads Remote and revalidates the stored ReplaceHost calculation, both
/// read-only. When Remote differs from Base the preview is still returned
/// for the conflict screen (P1-08D), with `source.invalidated` set.
pub async fn recheck_replace_host(
    service: &WorkcopyService,
    reader: &ReadOnlyGateway,
    connection_id: &str,
    local_id: &str,
    session: SessionContext<'_>,
) -> Result<ReplaceHostRecheck, WorkcopyError> {
    let source = service
        .check_source(reader, connection_id, local_id)
        .await?;
    let preview = service
        .revalidate_replace_host(reader, local_id, session)
        .await?;
    Ok(ReplaceHostRecheck { source, preview })
}
