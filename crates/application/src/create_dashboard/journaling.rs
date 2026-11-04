// SPDX-License-Identifier: MIT OR Apache-2.0

//! Publication journal of a Save as copy (P2-04D, P2-06C/D, O-018 §6,
//! O-022).
//!
//! The first line of the entry holds the durable reconciliation key of the
//! create: connection profile, exact server version, endpoint and server
//! identity fingerprints, the exact target name, the source workcopy and
//! its revision, the Local fingerprint (`source_fingerprint`), the content
//! fingerprint (`expected_fingerprint`) and the expected owner
//! (`owner_userid`); the operation ID is the confirmed `plan_id`.
//!
//! Outcome → journal state (`evidence` code in brackets):
//!
//! | Adapter outcome | Journal state |
//! |---|---|
//! | refused by the application before the adapter | `failed_before_send` [reason] |
//! | `FailedBeforeSend(SubmitNotRecorded)` | nothing appended (the failed `submitting` record) |
//! | `FailedBeforeSend(other)` | `failed_before_send` [refusal] |
//! | `NameTaken { sent: false }` | `failed_before_send` [`dashboard_name_taken`] |
//! | `NameTaken { sent: true }` | `reconciled_failed` [`dashboard_name_taken`], final: the server answered that nothing was created |
//! | `Verified` | `verified` [`read_back_matches`] + `created_dashboardid` |
//! | `FailedAfterResponse` | `failed_after_response` [reason] + `reported_dashboardid` when the server reported one; needs reconciliation |
//! | `ReconciledVerified` | `reconciled_verified` [`reconcile_confirmed`] + `created_dashboardid` |
//! | `UnknownOutcome` | `unknown_outcome` [reconciliation code]; needs reconciliation |
//!
//! Recovery ([`reconcile_create`]) reads only: by the reported
//! `dashboardid` when one was recorded, otherwise by exact name and content
//! fingerprint. Only `Confirmed` is a result (`reconciled_verified` and the
//! O-022 record in the workcopy). What the reads found — zero, several or a
//! different candidate — is `manual_check_required`, which keeps the
//! target blocked until a later reconciliation confirms it or the user
//! closes it manually. A read that did not see everything (failed or
//! truncated) is no evidence: nothing is recorded, the entry keeps its
//! state and the reconciliation can be retried (as for an update).

use zdashstitch_zabbix::{
    CreateExpectation, CreateReconciliation, DashboardCreateOutcome, ReadOnlyGateway, ZabbixVersion,
};

use super::{CreatePublishReport, PreparedCreateSend, SAVE_AS_COPY_OPERATION, millis};
use crate::publish_journal::{
    Finish, JournalEntry, JournalError, JournalHandle, JournalSource, JournalTarget, OperationKind,
    OperationRecord, PublishJournal, Reconciliation, require_reconciliation,
};
use crate::publisher::{JournalState, PlanInvalidReason};
use crate::workcopy::{WorkcopyError, WorkcopyService};

/// Evidence: the read-back matched the plan.
const READ_BACK_MATCHES: &str = "read_back_matches";
/// Evidence: reads found exactly the planned dashboard.
const RECONCILE_CONFIRMED: &str = "reconcile_confirmed";
/// Evidence: the name is taken (before send or `already exists`).
const NAME_TAKEN: &str = "dashboard_name_taken";

/// Endpoint and server identity fingerprints of the live connection.
pub(crate) struct JournalAuthority<'a> {
    pub endpoint_fingerprint: &'a str,
    pub server_identity: &'a str,
}

/// The first journal line of a create.
pub(crate) fn operation_record(
    prepared: &PreparedCreateSend,
    authority: &JournalAuthority<'_>,
) -> OperationRecord {
    let expectation = prepared.expectation();
    let plan = &prepared.plan;
    OperationRecord {
        kind: OperationKind::Create,
        intent: SAVE_AS_COPY_OPERATION,
        connection_id: expectation.connection_id.clone(),
        server_version: expectation.server_version.to_string(),
        endpoint_fingerprint: authority.endpoint_fingerprint.to_owned(),
        server_identity: authority.server_identity.to_owned(),
        target: JournalTarget::NewDashboard {
            name: expectation.name.clone(),
        },
        source: Some(JournalSource {
            local_id: plan.binding.local_id.clone(),
            revision: plan.binding.revision,
        }),
        source_fingerprint: plan.binding.local_fingerprint.clone(),
        expected_fingerprint: expectation.content_fingerprint.clone(),
        owner_userid: Some(expectation.owner_userid.clone()),
        plan_created_at_ms: millis(plan.created_at),
        confirmed_at_ms: millis(plan.confirmed_at),
    }
}

/// The final line of a send report (table in the module docs).
pub(crate) fn finish_of(report: &CreatePublishReport) -> Finish {
    let Some(outcome) = &report.outcome else {
        return Finish {
            evidence: report.invalidated.map(PlanInvalidReason::as_str),
            ..Finish::state(JournalState::FailedBeforeSend)
        };
    };
    match outcome {
        DashboardCreateOutcome::FailedBeforeSend(refusal) => Finish {
            evidence: Some(refusal.as_str()),
            ..Finish::state(JournalState::FailedBeforeSend)
        },
        DashboardCreateOutcome::NameTaken { sent: false } => Finish {
            evidence: Some(NAME_TAKEN),
            ..Finish::state(JournalState::FailedBeforeSend)
        },
        DashboardCreateOutcome::NameTaken { sent: true } => Finish {
            evidence: Some(NAME_TAKEN),
            ..Finish::state(JournalState::ReconciledFailed)
        },
        DashboardCreateOutcome::Verified {
            dashboardid,
            fingerprint,
        } => Finish {
            evidence: Some(READ_BACK_MATCHES),
            observed_fingerprint: Some(fingerprint.clone()),
            created_dashboardid: Some(dashboardid.clone()),
            ..Finish::state(JournalState::Verified)
        },
        DashboardCreateOutcome::FailedAfterResponse {
            reason,
            dashboardid,
        } => Finish {
            evidence: Some(reason.as_str()),
            reported_dashboardid: dashboardid.clone(),
            ..Finish::state(JournalState::FailedAfterResponse)
        },
        DashboardCreateOutcome::ReconciledVerified {
            dashboardid,
            fingerprint,
        } => Finish {
            evidence: Some(RECONCILE_CONFIRMED),
            observed_fingerprint: Some(fingerprint.clone()),
            created_dashboardid: Some(dashboardid.clone()),
            ..Finish::state(JournalState::ReconciledVerified)
        },
        DashboardCreateOutcome::UnknownOutcome { reconciliation } => Finish {
            evidence: Some(reconciliation.code()),
            ..Finish::state(JournalState::UnknownOutcome)
        },
    }
}

/// Opens the journal entry of `prepared` (refused while the target is
/// blocked: nothing is sent), runs `send` with the handle whose
/// `admit_send` is the adapter's `before_send`, and appends the outcome.
/// A refused `submitting` record appends nothing more; a failed outcome
/// append leaves the entry `submitting` (needs reconciliation).
pub(crate) async fn journaled_create<S>(
    journal: &PublishJournal,
    prepared: PreparedCreateSend,
    authority: &JournalAuthority<'_>,
    send: S,
) -> CreatePublishReport
where
    S: AsyncFnOnce(PreparedCreateSend, &mut JournalHandle) -> CreatePublishReport,
{
    let operation_id = prepared.plan_id().to_owned();
    let record = operation_record(&prepared, authority);
    let mut handle = match journal.begin(&operation_id, &record) {
        Ok(handle) => handle,
        Err(error) => {
            return CreatePublishReport {
                plan_id: operation_id,
                outcome: None,
                invalidated: None,
                record_error: None,
                journal_state: None,
                journal_error: Some(error),
            };
        }
    };
    let mut report = send(prepared, &mut handle).await;
    report.journal_error = match handle.take_submit_error() {
        Some(error) => {
            drop(handle);
            Some(error)
        }
        None => handle.finish_with(finish_of(&report)).err(),
    };
    report.journal_state = journal.entry(&operation_id).ok().map(|entry| entry.state);
    report
}

/// Result of reconciling an unfinished create.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateRecovery {
    /// The entry after the recorded result.
    pub entry: JournalEntry,
    /// What the reads found.
    pub reconciliation: CreateReconciliation,
    /// Recording the confirmed dashboard in the workcopy (O-022) failed;
    /// the journal result stands.
    pub record_error: Option<WorkcopyError>,
}

/// The reconciliation key recorded on the first line.
fn expectation_of(entry: &JournalEntry) -> Result<CreateExpectation, JournalError> {
    let JournalTarget::NewDashboard { name } = &entry.target else {
        return Err(JournalError::new("reconcile_kind_mismatch").with("kind", entry.kind.as_str()));
    };
    let corrupt =
        |reason: &'static str| JournalError::new("journal_corrupt").with("reason", reason);
    let owner_userid = entry
        .owner_userid
        .clone()
        .ok_or_else(|| corrupt("owner_userid"))?;
    let server_version = entry
        .server_version
        .parse::<ZabbixVersion>()
        .map_err(|_| corrupt("server_version"))?;
    Ok(CreateExpectation {
        connection_id: entry.connection_id.clone(),
        server_version,
        name: name.clone(),
        content_fingerprint: entry.expected_fingerprint.clone(),
        owner_userid,
    })
}

/// Reconciles an unfinished create (after a restart, or on «Проверить
/// снова») with reads only and records the result; never sends a create.
///
/// The reader must be signed in to the same server identity (endpoint and
/// account) and see the same server version as the operation; otherwise
/// `reconcile_server_changed` (param `what`: `identity` or `version`) and
/// nothing is recorded. `Confirmed` → `reconciled_verified` with the new
/// `dashboardid`, and the dashboard is added to the source workcopy's list
/// of created dashboards (O-022); `NotFound`, `Ambiguous` and `Mismatch`
/// (evidence) → `manual_check_required` with the finding's code (the target
/// stays blocked). A failed read (connection taxonomy code) or a truncated
/// one (`reconcile_partial_visibility`) records nothing: the entry keeps
/// its state and the reconciliation can be retried.
///
/// Errors: `operation_id_invalid`, `operation_not_found`,
/// `reconcile_kind_mismatch`, `operation_running`, `reconcile_not_needed`,
/// `reconcile_server_changed`, `reconcile_partial_visibility`, connection
/// taxonomy codes of the read, `journal_corrupt`, `journal_write_failed`.
pub async fn reconcile_create(
    journal: &PublishJournal,
    service: &WorkcopyService,
    reader: &ReadOnlyGateway,
    operation_id: &str,
) -> Result<CreateRecovery, JournalError> {
    let entry = journal.entry(operation_id)?;
    if entry.kind != OperationKind::Create {
        return Err(JournalError::new("reconcile_kind_mismatch").with("kind", entry.kind.as_str()));
    }
    require_reconciliation(&entry)?;
    if reader.identity().fingerprint() != entry.server_identity {
        return Err(JournalError::new("reconcile_server_changed").with("what", "identity"));
    }
    if reader.capabilities().version.to_string() != entry.server_version {
        return Err(JournalError::new("reconcile_server_changed").with("what", "version"));
    }
    let expectation = expectation_of(&entry)?;
    let reconciliation =
        super::reconcile_save_as_copy(reader, &expectation, entry.reported_dashboardid.as_deref())
            .await;
    let result = match &reconciliation {
        CreateReconciliation::ConnectionMismatch => {
            return Err(JournalError::new("reconcile_server_changed").with("what", "identity"));
        }
        // Not every candidate was seen: no evidence, nothing recorded.
        CreateReconciliation::ReadFailed { failure } => {
            return Err(JournalError::from_failure(failure.clone()));
        }
        CreateReconciliation::PartialVisibility => {
            return Err(JournalError::new(reconciliation.code()));
        }
        CreateReconciliation::Confirmed {
            dashboardid,
            fingerprint,
        } => Reconciliation {
            state: JournalState::ReconciledVerified,
            evidence: RECONCILE_CONFIRMED,
            observed_fingerprint: Some(fingerprint.clone()),
            created_dashboardid: Some(dashboardid.clone()),
        },
        evidence @ (CreateReconciliation::NotFound
        | CreateReconciliation::Ambiguous { .. }
        | CreateReconciliation::Mismatch { .. }) => Reconciliation {
            state: JournalState::ManualCheckRequired,
            evidence: evidence.code(),
            observed_fingerprint: None,
            created_dashboardid: None,
        },
    };
    let entry = journal.record_reconciliation(operation_id, OperationKind::Create, result)?;
    let record_error = match (&reconciliation, &entry.source) {
        (
            CreateReconciliation::Confirmed {
                dashboardid,
                fingerprint,
            },
            Some(source),
        ) => service
            .record_created_dashboard(&source.local_id, &expectation, dashboardid, fingerprint)
            .err(),
        _ => None,
    };
    Ok(CreateRecovery {
        entry,
        reconciliation,
        record_error,
    })
}
