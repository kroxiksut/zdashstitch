// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reconciliation of an unfinished update (P1-08G, P1-01G §4): one read of
//! the target dashboard through the read-only gateway, compared with the
//! expected and the source fingerprint of the journal entry. No write is
//! possible here: the function accepts only a [`ReadOnlyGateway`].
//!
//! ```compile_fail
//! async fn no_write_gateway(
//!     journal: &zdashstitch_application::PublishJournal,
//!     writer: &zdashstitch_zabbix::ReadWriteGateway,
//! ) {
//!     let _ = zdashstitch_application::reconcile_update(journal, writer, "zds-1").await;
//! }
//! ```
//!
//! Control: the same call compiles with the read-only gateway.
//!
//! ```no_run
//! async fn read_gateway(
//!     journal: &zdashstitch_application::PublishJournal,
//!     reader: &zdashstitch_zabbix::ReadOnlyGateway,
//! ) {
//!     let _ = zdashstitch_application::reconcile_update(journal, reader, "zds-1").await;
//! }
//! ```

use zdashstitch_zabbix::{ReadOnlyGateway, VersionedDashboardSnapshot};

use super::{
    JournalEntry, JournalError, JournalProgress, JournalState, JournalTarget, OperationKind,
    PublishJournal, Reconciliation,
};

/// What the reconciliation read of an update found.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UpdateEvidence {
    /// The dashboard equals the expected result → `reconciled_verified`.
    ExpectedFound,
    /// The dashboard equals the source the plan was built on →
    /// `reconciled_failed` (the write did not take effect).
    SourceFound,
    /// The dashboard differs from both → `manual_check_required`.
    OtherFound,
    /// The dashboard is missing or not visible → `manual_check_required`.
    NotVisible,
}

impl UpdateEvidence {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExpectedFound => "expected_found",
            Self::SourceFound => "source_found",
            Self::OtherFound => "other_found",
            Self::NotVisible => "not_visible",
        }
    }

    /// The journal state this evidence leads to.
    #[must_use]
    pub const fn state(self) -> JournalState {
        match self {
            Self::ExpectedFound => JournalState::ReconciledVerified,
            Self::SourceFound => JournalState::ReconciledFailed,
            Self::OtherFound | Self::NotVisible => JournalState::ManualCheckRequired,
        }
    }
}

/// Pure decision on the read target dashboard. Only an exact fingerprint
/// match decides; anything else needs a manual check.
///
/// Entries record canonical fingerprints (LAB-02, 28.10.2026). An entry
/// recorded by an earlier build holds the raw-bytes fingerprint of the P0
/// algorithm instead, which equals the observed `storage_fingerprint` only
/// for byte-identical documents; matching it there is sound, because byte
/// equality implies canonical equality. The two kinds use different domain
/// tags, so a value of one kind never equals one of the other: an earlier
/// entry that matches neither way stays `other_found` (manual check), never
/// a decided result.
#[must_use]
pub fn decide_update_reconciliation(
    entry: &JournalEntry,
    observed: Option<&VersionedDashboardSnapshot>,
) -> UpdateEvidence {
    let Some(observed) = observed else {
        return UpdateEvidence::NotVisible;
    };
    let same_dashboard = matches!(
        &entry.target,
        JournalTarget::ExistingDashboard { dashboardid, .. } if dashboardid == observed.dashboard_id()
    );
    if !same_dashboard || observed.connection_id() != entry.connection_id {
        return UpdateEvidence::NotVisible;
    }
    let matches = |recorded: &str| {
        observed.fingerprint() == recorded || observed.storage_fingerprint() == recorded
    };
    if matches(&entry.expected_fingerprint) {
        UpdateEvidence::ExpectedFound
    } else if matches(&entry.source_fingerprint) {
        UpdateEvidence::SourceFound
    } else {
        UpdateEvidence::OtherFound
    }
}

/// Reconciles an unfinished update with one read and records the result.
///
/// The reader must be signed in to the same server identity (endpoint and
/// account) and see the same server version as the operation; otherwise
/// `reconcile_server_changed` (param `what`: `identity` or `version`) and
/// nothing is recorded. A failed read records nothing either.
///
/// Errors: `operation_id_invalid`, `operation_not_found`,
/// `reconcile_kind_mismatch`, `operation_running`, `reconcile_not_needed`,
/// `reconcile_server_changed`, connection taxonomy codes of the read,
/// `journal_corrupt`, `journal_write_failed`.
pub async fn reconcile_update(
    journal: &PublishJournal,
    reader: &ReadOnlyGateway,
    operation_id: &str,
) -> Result<JournalEntry, JournalError> {
    let entry = journal.entry(operation_id)?;
    if entry.kind != OperationKind::Update {
        return Err(JournalError::new("reconcile_kind_mismatch").with("kind", entry.kind.as_str()));
    }
    match entry.progress {
        JournalProgress::NeedsReconciliation => {}
        JournalProgress::Running => return Err(JournalError::new("operation_running")),
        JournalProgress::Abandoned | JournalProgress::Finished => {
            return Err(
                JournalError::new("reconcile_not_needed").with("state", entry.state.as_str())
            );
        }
    }
    if reader.identity().fingerprint() != entry.server_identity {
        return Err(JournalError::new("reconcile_server_changed").with("what", "identity"));
    }
    if reader.capabilities().version.to_string() != entry.server_version {
        return Err(JournalError::new("reconcile_server_changed").with("what", "version"));
    }
    let JournalTarget::ExistingDashboard { dashboardid, .. } = &entry.target else {
        return Err(JournalError::new("reconcile_kind_mismatch").with("kind", entry.kind.as_str()));
    };
    let observed = reader
        .dashboard_snapshot(&entry.connection_id, dashboardid)
        .await
        .map_err(|error| JournalError::from_zabbix(&error))?;
    let evidence = decide_update_reconciliation(&entry, observed.as_ref());
    journal.record_reconciliation(
        operation_id,
        OperationKind::Update,
        Reconciliation {
            state: evidence.state(),
            evidence: evidence.as_str(),
            observed_fingerprint: observed.map(|snapshot| snapshot.fingerprint().to_owned()),
            created_dashboardid: None,
        },
    )
}
