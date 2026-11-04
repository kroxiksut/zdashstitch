// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-04D journal tests over a real temporary storage root: state machine,
//! injected write/fsync failures and torn writes, damaged files, recovery
//! after a simulated crash, «Закрыть без сверки», and the publication path
//! on a loopback mock (a failed `submitting` record means zero writes).

use std::cell::Cell;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, extract::State, routing::post};
use secrecy::SecretString;
use serde_json::{Value, json};
use zdashstitch_zabbix::{
    Authentication, ReadWriteGateway, VersionedDashboardSnapshot, WidgetFieldReplacement,
    ZabbixClient, ZabbixEndpoint,
};

use super::record::{LineFile, transition_allowed};
use super::*;
use crate::{LocalSession, build_verified_plan, publish_confirmed_plan};

const CONNECTION: &str = "conn-0123456789abcdef0123456789abcdef";
const LOCAL: &str = "wc-00112233445566778899aabbccddeeff";

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// A fresh operation ID; the running set is process-wide.
fn operation_id() -> String {
    format!(
        "zds-unit-{:x}-{:x}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    )
}

fn hex(digit: char) -> String {
    std::iter::repeat_n(digit, 64).collect()
}

fn record(dashboardid: &str) -> OperationRecord {
    OperationRecord {
        kind: OperationKind::Update,
        intent: "replace_host",
        connection_id: CONNECTION.to_owned(),
        server_version: "7.4.16".to_owned(),
        endpoint_fingerprint: hex('a'),
        server_identity: hex('b'),
        target: JournalTarget::ExistingDashboard {
            dashboardid: dashboardid.to_owned(),
            name: None,
        },
        source: Some(JournalSource {
            local_id: LOCAL.to_owned(),
            revision: 3,
        }),
        source_fingerprint: hex('c'),
        expected_fingerprint: hex('d'),
        owner_userid: None,
        plan_created_at_ms: 10,
        confirmed_at_ms: 11,
    }
}

fn create_record(name: &str, local_id: Option<&str>) -> OperationRecord {
    OperationRecord {
        kind: OperationKind::Create,
        intent: "save_as_copy",
        target: JournalTarget::NewDashboard {
            name: name.to_owned(),
        },
        source: local_id.map(|local_id| JournalSource {
            local_id: local_id.to_owned(),
            revision: 1,
        }),
        owner_userid: Some("3".to_owned()),
        ..record("1")
    }
}

fn setup() -> (tempfile::TempDir, PublishJournal) {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"));
    let journal = PublishJournal::new(dir.path().to_path_buf());
    (dir, journal)
}

fn key(dashboardid: &str) -> TargetKey {
    TargetKey::dashboard(CONNECTION, None, dashboardid)
}

fn begin(journal: &PublishJournal, id: &str, record: &OperationRecord) -> JournalHandle {
    journal
        .begin(id, record)
        .unwrap_or_else(|error| panic!("begin: {error}"))
}

fn entry(journal: &PublishJournal, id: &str) -> JournalEntry {
    journal
        .entry(id)
        .unwrap_or_else(|error| panic!("entry: {error}"))
}

fn code<T>(result: Result<T, JournalError>) -> Option<&'static str> {
    result.err().map(|error| error.code())
}

/// Injects `kind` for appends of `state` while alive (this thread only).
struct Failing;

impl Failing {
    fn at(state: JournalState, kind: FailKind) -> Self {
        fail::AT.with(|at| at.set(Some((state, kind))));
        Self
    }
}

impl Drop for Failing {
    fn drop(&mut self) {
        fail::AT.with(|at: &Cell<_>| at.set(None));
    }
}

fn journal_file(root: &Path, id: &str) -> std::path::PathBuf {
    root.join("journal").join(format!("{id}.jsonl"))
}

fn write_file(root: &Path, id: &str, raw: &[u8]) {
    std::fs::create_dir_all(root.join("journal")).unwrap_or_else(|error| panic!("dir: {error}"));
    std::fs::write(journal_file(root, id), raw).unwrap_or_else(|error| panic!("write: {error}"));
}

fn read_file(root: &Path, id: &str) -> String {
    std::fs::read_to_string(journal_file(root, id)).unwrap_or_else(|error| panic!("read: {error}"))
}

/// A valid first line of `id` as JSON.
fn header(id: &str, dashboardid: &str) -> Value {
    let mut line = LineFile::transition(id, 0, JournalState::Confirmed, 1);
    line.operation = Some(record(dashboardid).to_file());
    serde_json::to_value(&line).unwrap_or_else(|error| panic!("line: {error}"))
}

fn line(id: &str, seq: usize, state: JournalState) -> Value {
    serde_json::to_value(LineFile::transition(id, seq, state, 2))
        .unwrap_or_else(|error| panic!("line: {error}"))
}

fn lines(values: &[Value]) -> Vec<u8> {
    values
        .iter()
        .map(|value| format!("{value}\n"))
        .collect::<String>()
        .into_bytes()
}

const ALL: [JournalState; 13] = [
    JournalState::Planned,
    JournalState::Previewed,
    JournalState::Confirmed,
    JournalState::Submitting,
    JournalState::Verified,
    JournalState::Conflict,
    JournalState::FailedBeforeSend,
    JournalState::FailedAfterResponse,
    JournalState::UnknownOutcome,
    JournalState::ReconciledVerified,
    JournalState::ReconciledFailed,
    JournalState::ManualCheckRequired,
    JournalState::ClosedManually,
];

#[test]
fn the_state_machine_allows_exactly_the_documented_transitions() {
    use JournalState::*;
    let allowed: &[(Option<JournalState>, JournalState)] = &[
        (None, Confirmed),
        (Some(Confirmed), Submitting),
        (Some(Confirmed), FailedBeforeSend),
        (Some(Confirmed), Conflict),
        (Some(Submitting), Verified),
        (Some(Submitting), FailedAfterResponse),
        (Some(Submitting), UnknownOutcome),
        (Some(Submitting), ReconciledVerified),
        (Some(Submitting), ReconciledFailed),
        (Some(Submitting), ManualCheckRequired),
        (Some(Submitting), ClosedManually),
        (Some(UnknownOutcome), ReconciledVerified),
        (Some(UnknownOutcome), ReconciledFailed),
        (Some(UnknownOutcome), ManualCheckRequired),
        (Some(UnknownOutcome), ClosedManually),
        (Some(FailedAfterResponse), ReconciledVerified),
        (Some(FailedAfterResponse), ReconciledFailed),
        (Some(FailedAfterResponse), ManualCheckRequired),
        (Some(FailedAfterResponse), ClosedManually),
        (Some(ManualCheckRequired), ReconciledVerified),
        (Some(ManualCheckRequired), ReconciledFailed),
        (Some(ManualCheckRequired), ManualCheckRequired),
        (Some(ManualCheckRequired), ClosedManually),
    ];
    let froms = std::iter::once(None).chain(ALL.iter().copied().map(Some));
    for from in froms {
        for to in ALL {
            assert_eq!(
                transition_allowed(from, to),
                allowed.contains(&(from, to)),
                "{from:?} -> {to:?}"
            );
        }
    }
    // Every durable state round-trips through its code; the in-memory
    // ones are never read back.
    for state in ALL {
        let durable = !matches!(state, Planned | Previewed);
        assert_eq!(
            JournalState::from_code(state.as_str()) == Some(state),
            durable
        );
    }
}

#[test]
fn a_running_operation_blocks_its_target_and_is_abandoned_when_dropped_unsent() {
    let (dir, journal) = setup();
    let id = operation_id();
    let handle = begin(&journal, &id, &record("950"));
    let running = entry(&journal, &id);
    assert_eq!(running.state, JournalState::Confirmed);
    assert_eq!(running.progress, JournalProgress::Running);
    assert_eq!(running.sends, 0);
    assert_eq!(
        code(journal.check_target(&key("950"))),
        Some("target_blocked")
    );
    assert_eq!(journal.check_target(&key("951")), Ok(()));
    assert_eq!(
        code(journal.begin(&operation_id(), &record("950"))),
        Some("target_blocked")
    );
    assert_eq!(
        code(journal.begin(&id, &record("951"))),
        Some("operation_running")
    );
    // The file is local-only and holds the operation record only.
    let raw = read_file(dir.path(), &id);
    assert_eq!(raw.lines().count(), 1);
    assert!(raw.contains("\"storage_scope\":\"local_only\""));
    assert!(raw.contains("\"format\":\"zdashstitch.journal/v0\""));

    drop(handle);
    let abandoned = entry(&journal, &id);
    assert_eq!(abandoned.progress, JournalProgress::Abandoned);
    assert_eq!(journal.check_target(&key("950")), Ok(()));
    assert!(
        journal
            .recover()
            .unwrap_or_else(|error| panic!("recover: {error}"))
            .is_empty()
    );
}

#[test]
fn a_crash_after_submitting_leaves_an_entry_that_blocks_until_reconciled() {
    let (dir, journal) = setup();
    let id = operation_id();
    let mut handle = begin(&journal, &id, &record("950"));
    assert!(handle.admit_send());
    // The process dies here: no outcome line.
    drop(handle);

    let restarted = PublishJournal::new(dir.path().to_path_buf());
    let report = restarted
        .recover()
        .unwrap_or_else(|error| panic!("recover: {error}"));
    let [found] = report.unfinished.as_slice() else {
        panic!("one unfinished entry: {report:?}");
    };
    assert_eq!(found.state, JournalState::Submitting);
    assert_eq!(found.progress, JournalProgress::NeedsReconciliation);
    assert_eq!(found.sends, 1);
    assert!(found.submitted_at_ms().is_some());
    assert_eq!(
        code(restarted.check_target(&key("950"))),
        Some("target_blocked")
    );
    // Another profile of the same server address is the same target.
    let same_server = TargetKey::dashboard("conn-other", Some(hex('a')), "950");
    assert_eq!(
        code(restarted.check_target(&same_server)),
        Some("target_blocked")
    );
    let other_server = TargetKey::dashboard("conn-other", Some(hex('e')), "950");
    assert_eq!(restarted.check_target(&other_server), Ok(()));
    assert_eq!(
        restarted
            .recover_for_workcopy(LOCAL)
            .unwrap_or_else(|error| panic!("recover: {error}"))
            .unfinished
            .len(),
        1
    );
    assert!(
        restarted
            .recover_for_workcopy("wc-ffffffffffffffffffffffffffffffff")
            .unwrap_or_else(|error| panic!("recover: {error}"))
            .is_empty()
    );

    let reconciled = restarted
        .record_reconciliation(
            &id,
            OperationKind::Update,
            Reconciliation {
                state: JournalState::ReconciledFailed,
                evidence: UpdateEvidence::SourceFound.as_str(),
                observed_fingerprint: Some(hex('c')),
                created_dashboardid: None,
            },
        )
        .unwrap_or_else(|error| panic!("reconcile: {error}"));
    assert_eq!(reconciled.state, JournalState::ReconciledFailed);
    assert_eq!(reconciled.progress, JournalProgress::Finished);
    assert_eq!(restarted.check_target(&key("950")), Ok(()));
}

#[test]
fn a_failed_fsync_of_submitting_forbids_the_send_and_stays_blocking() {
    let (_dir, journal) = setup();
    let id = operation_id();
    let mut handle = begin(&journal, &id, &record("950"));
    {
        let _failing = Failing::at(JournalState::Submitting, FailKind::Sync);
        assert!(!handle.admit_send(), "a failed fsync must forbid the send");
    }
    let error = handle
        .take_submit_error()
        .unwrap_or_else(|| panic!("the submit error is kept"));
    assert_eq!(error.code(), "journal_write_failed");
    drop(handle);
    // The line may have reached the disk without the fsync: the entry is
    // treated as possibly sent, never as abandoned.
    let found = entry(&journal, &id);
    assert_eq!(found.state, JournalState::Submitting);
    assert_eq!(found.progress, JournalProgress::NeedsReconciliation);
}

#[test]
fn a_submit_record_that_was_never_written_leaves_the_entry_abandoned() {
    let (_dir, journal) = setup();
    for kind in [FailKind::BeforeWrite, FailKind::Torn] {
        let id = operation_id();
        let mut handle = begin(&journal, &id, &record("950"));
        {
            let _failing = Failing::at(JournalState::Submitting, kind);
            assert!(!handle.admit_send());
        }
        drop(handle);
        let found = entry(&journal, &id);
        assert_eq!(found.state, JournalState::Confirmed, "{kind:?}");
        assert_eq!(found.progress, JournalProgress::Abandoned);
        assert_eq!(found.sends, 0);
    }
    assert_eq!(journal.check_target(&key("950")), Ok(()));
}

#[test]
fn a_failed_first_line_opens_no_operation() {
    let (_dir, journal) = setup();
    let id = operation_id();
    {
        let _failing = Failing::at(JournalState::Confirmed, FailKind::BeforeWrite);
        assert_eq!(
            code(journal.begin(&id, &record("950"))),
            Some("journal_write_failed")
        );
    }
    assert_eq!(journal.check_target(&key("950")), Ok(()));
    // An empty file: nothing was committed, so nothing can have been sent.
    let listing = journal
        .list()
        .unwrap_or_else(|error| panic!("list: {error}"));
    assert!(listing.entries.is_empty() && listing.damaged.is_empty());
    // The same ID is never reused for another operation.
    assert_eq!(
        code(journal.begin(&id, &record("950"))),
        Some("journal_write_failed")
    );
}

#[test]
fn a_torn_outcome_line_is_ignored_and_cut_before_the_next_append() {
    let (dir, journal) = setup();
    let id = operation_id();
    let mut handle = begin(&journal, &id, &record("950"));
    assert!(handle.admit_send());
    {
        let _failing = Failing::at(JournalState::UnknownOutcome, FailKind::Torn);
        assert_eq!(
            code(handle.finish(JournalState::UnknownOutcome, None)),
            Some("journal_write_failed")
        );
    }
    assert!(!read_file(dir.path(), &id).ends_with('\n'));
    let found = entry(&journal, &id);
    assert_eq!(found.state, JournalState::Submitting);
    assert_eq!(found.progress, JournalProgress::NeedsReconciliation);

    let closed = journal
        .close_manually(&CloseManuallyRequest {
            operation_id: id.clone(),
            confirmed: true,
        })
        .unwrap_or_else(|error| panic!("close: {error}"));
    assert_eq!(closed.state, JournalState::ClosedManually);
    let raw = read_file(dir.path(), &id);
    assert!(raw.ends_with('\n'));
    assert_eq!(raw.lines().count(), 3);
    for text in raw.lines() {
        assert!(serde_json::from_str::<Value>(text).is_ok(), "{text}");
    }
}

#[test]
fn invalid_transitions_are_rejected_and_nothing_is_appended() {
    let (dir, journal) = setup();
    let id = operation_id();
    let handle = begin(&journal, &id, &record("950"));
    // Running: no reconciliation and no manual close.
    let reconcile = |journal: &PublishJournal| {
        journal.record_reconciliation(
            &id,
            OperationKind::Update,
            Reconciliation {
                state: JournalState::ReconciledVerified,
                evidence: "expected_found",
                observed_fingerprint: None,
                created_dashboardid: None,
            },
        )
    };
    assert_eq!(code(reconcile(&journal)), Some("operation_running"));
    let close = CloseManuallyRequest {
        operation_id: id.clone(),
        confirmed: true,
    };
    assert_eq!(
        code(journal.close_manually(&close)),
        Some("operation_running")
    );
    // A send that was never recorded cannot be verified.
    assert_eq!(
        code(handle.finish(JournalState::Verified, None)),
        Some("journal_transition_invalid")
    );
    assert_eq!(read_file(dir.path(), &id).lines().count(), 1);
    // Abandoned and finished entries need no reconciliation.
    assert_eq!(code(reconcile(&journal)), Some("reconcile_not_needed"));
    assert_eq!(
        code(journal.close_manually(&close)),
        Some("reconcile_not_needed")
    );

    let id = operation_id();
    let mut handle = begin(&journal, &id, &record("951"));
    assert!(handle.admit_send());
    assert_eq!(handle.finish(JournalState::Verified, None), Ok(()));
    let close = CloseManuallyRequest {
        operation_id: id.clone(),
        confirmed: true,
    };
    assert_eq!(
        code(journal.close_manually(&close)),
        Some("reconcile_not_needed")
    );
    // Only reconciliation states are recorded by a reconciliation, and
    // only for the operation kind that was read.
    let id = operation_id();
    let mut handle = begin(&journal, &id, &record("952"));
    assert!(handle.admit_send());
    drop(handle);
    let wrong = journal.record_reconciliation(
        &id,
        OperationKind::Update,
        Reconciliation {
            state: JournalState::Verified,
            evidence: "expected_found",
            observed_fingerprint: None,
            created_dashboardid: None,
        },
    );
    assert_eq!(code(wrong), Some("journal_transition_invalid"));
    let wrong_kind = journal.record_reconciliation(
        &id,
        OperationKind::Create,
        Reconciliation {
            state: JournalState::ReconciledVerified,
            evidence: "found_one",
            observed_fingerprint: None,
            created_dashboardid: Some("77".to_owned()),
        },
    );
    assert_eq!(code(wrong_kind), Some("reconcile_kind_mismatch"));
    let unconfirmed = CloseManuallyRequest {
        operation_id: id.clone(),
        confirmed: false,
    };
    assert_eq!(
        code(journal.close_manually(&unconfirmed)),
        Some("close_not_confirmed")
    );
    let manual = journal
        .record_reconciliation(
            &id,
            OperationKind::Update,
            Reconciliation {
                state: JournalState::ManualCheckRequired,
                evidence: "other_found",
                observed_fingerprint: Some(hex('e')),
                created_dashboardid: None,
            },
        )
        .unwrap_or_else(|error| panic!("manual: {error}"));
    assert_eq!(manual.progress, JournalProgress::NeedsReconciliation);
    assert_eq!(
        code(journal.check_target(&key("952"))),
        Some("target_blocked")
    );
    let closed = journal
        .close_manually(&CloseManuallyRequest {
            operation_id: id.clone(),
            confirmed: true,
        })
        .unwrap_or_else(|error| panic!("close: {error}"));
    assert_eq!(closed.state, JournalState::ClosedManually);
    assert_eq!(closed.evidence.as_deref(), Some("other_found"));
    assert_eq!(closed.closed_by.is_some(), super::local_account().is_some());
    assert_eq!(journal.check_target(&key("952")), Ok(()));
}

#[test]
fn damaged_files_are_refused_never_guessed_around() {
    let (dir, journal) = setup();
    let root = dir.path();
    let valid = |id: &str| vec![header(id, "960"), line(id, 1, JournalState::Submitting)];

    // A damaged line after a readable first line blocks only its target.
    let id = "zds-damaged-tail";
    let mut raw = lines(&valid(id)[..1]);
    raw.extend_from_slice(b"{not json}\n");
    write_file(root, id, &raw);
    let listing = journal
        .list()
        .unwrap_or_else(|error| panic!("list: {error}"));
    let [damaged] = listing.damaged.as_slice() else {
        panic!("one damaged file: {listing:?}");
    };
    assert_eq!(damaged.error.code(), "journal_corrupt");
    assert_eq!(damaged.connection_id.as_deref(), Some(CONNECTION));
    assert_eq!(
        code(journal.check_target(&key("960"))),
        Some("journal_damaged")
    );
    assert_eq!(journal.check_target(&key("961")), Ok(()));
    let close = CloseManuallyRequest {
        operation_id: id.to_owned(),
        confirmed: true,
    };
    assert_eq!(
        code(journal.close_manually(&close)),
        Some("journal_corrupt")
    );
    assert_eq!(
        read_file(root, id).as_bytes(),
        raw.as_slice(),
        "never modified"
    );
    let recovery = journal
        .recover()
        .unwrap_or_else(|error| panic!("recover: {error}"));
    assert_eq!(recovery.damaged.len(), 1);
    std::fs::remove_file(journal_file(root, id)).unwrap_or_else(|error| panic!("rm: {error}"));

    // Each malformed file is damaged; when even the target cannot be read
    // every target is blocked.
    let mut unknown_field = header("zds-unknown-field", "960");
    unknown_field["payload"] = json!({"pages": []});
    let mut newer = header("zds-newer-format", "960");
    newer["format"] = json!("zdashstitch.journal/v1");
    let mut not_local = header("zds-not-local", "960");
    not_local["storage_scope"] = json!("portable");
    let mut gap = valid("zds-seq-gap");
    gap[1]["seq"] = json!(2);
    let mut skipped = valid("zds-bad-transition");
    skipped[1] = line("zds-bad-transition", 1, JournalState::ReconciledVerified);
    skipped.insert(
        1,
        line("zds-bad-transition", 1, JournalState::FailedBeforeSend),
    );
    skipped[2]["seq"] = json!(2);
    let mut renamed = valid("zds-other-name");
    renamed[1]["operation_id"] = json!("zds-someone-else");
    let mut second_header = valid("zds-second-header");
    second_header[1]["operation"] = header("zds-second-header", "960")["operation"].clone();
    let mut created_on_update = valid("zds-created-on-update");
    created_on_update.push(line(
        "zds-created-on-update",
        2,
        JournalState::ReconciledVerified,
    ));
    created_on_update[2]["created_dashboardid"] = json!("5");
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        ("zds-garbage", b"garbage\n".to_vec(), "journal_corrupt"),
        (
            "zds-unknown-field",
            lines(&[unknown_field]),
            "journal_corrupt",
        ),
        (
            "zds-newer-format",
            lines(&[newer]),
            "journal_unsupported_version",
        ),
        ("zds-not-local", lines(&[not_local]), "journal_corrupt"),
        ("zds-seq-gap", lines(&gap), "journal_corrupt"),
        ("zds-bad-transition", lines(&skipped), "journal_corrupt"),
        ("zds-other-name", lines(&renamed), "journal_corrupt"),
        (
            "zds-second-header",
            lines(&second_header),
            "journal_corrupt",
        ),
        (
            "zds-created-on-update",
            lines(&created_on_update),
            "journal_corrupt",
        ),
        (
            "zds-too-large",
            vec![b' '; record::MAX_JOURNAL_FILE_BYTES + 1],
            "storage_file_too_large",
        ),
    ];
    for (id, raw, expected) in &cases {
        write_file(root, id, raw);
        let error = journal
            .entry(id)
            .err()
            .unwrap_or_else(|| panic!("{id} must be refused"));
        assert_eq!(error.code(), *expected, "{id}");
        std::fs::remove_file(journal_file(root, id)).unwrap_or_else(|error| panic!("rm: {error}"));
    }
    write_file(root, "zds-garbage", b"garbage\n");
    assert_eq!(
        code(journal.check_target(&key("999"))),
        Some("journal_damaged")
    );
    assert_eq!(
        code(journal.begin(&operation_id(), &record("999"))),
        Some("journal_damaged")
    );
    // Files that are not journal entries are ignored.
    write_file(root, "notes", b"x");
    std::fs::remove_file(journal_file(root, "zds-garbage"))
        .unwrap_or_else(|error| panic!("rm: {error}"));
    assert_eq!(journal.check_target(&key("999")), Ok(()));
}

#[test]
fn create_targets_block_by_name_and_by_source_workcopy() {
    let (_dir, journal) = setup();
    let id = operation_id();
    let mut handle = begin(&journal, &id, &create_record("DB: Pilot", Some(LOCAL)));
    assert!(handle.admit_send());
    drop(handle);
    let create_key = |name: &str, local_id: Option<&str>| TargetKey {
        connection_id: CONNECTION.to_owned(),
        endpoint_fingerprint: None,
        target: TargetRef::NewDashboard {
            name: name.to_owned(),
            source_local_id: local_id.map(str::to_owned),
        },
    };
    assert_eq!(
        code(journal.check_target(&create_key("DB: Pilot", None))),
        Some("target_blocked")
    );
    assert_eq!(
        code(journal.check_target(&create_key("Another name", Some(LOCAL)))),
        Some("target_blocked")
    );
    assert_eq!(
        journal.check_target(&create_key(
            "Another name",
            Some("wc-ffffffffffffffffffffffffffffffff")
        )),
        Ok(())
    );
    // A create never blocks an update of an existing dashboard.
    assert_eq!(journal.check_target(&key("1")), Ok(()));
    let found = entry(&journal, &id);
    assert_eq!(found.kind, OperationKind::Create);
    let confirmed = journal
        .record_reconciliation(
            &id,
            OperationKind::Create,
            Reconciliation {
                state: JournalState::ReconciledVerified,
                evidence: "found_one",
                observed_fingerprint: Some(hex('d')),
                created_dashboardid: Some("77".to_owned()),
            },
        )
        .unwrap_or_else(|error| panic!("reconcile: {error}"));
    assert_eq!(confirmed.created_dashboardid.as_deref(), Some("77"));
    assert_eq!(
        journal.check_target(&create_key("DB: Pilot", Some(LOCAL))),
        Ok(())
    );
    // A create record must name a new dashboard.
    let mut mixed = create_record("x", None);
    mixed.target = JournalTarget::ExistingDashboard {
        dashboardid: "5".to_owned(),
        name: None,
    };
    assert_eq!(
        code(journal.begin(&operation_id(), &mixed)),
        Some("journal_corrupt")
    );
}

fn snapshot(dashboard: Value) -> VersionedDashboardSnapshot {
    VersionedDashboardSnapshot::from_adapter_raw(
        CONNECTION.to_owned(),
        "7.4.16"
            .parse()
            .unwrap_or_else(|error| panic!("version: {error}")),
        dashboard,
    )
    .unwrap_or_else(|error| panic!("snapshot: {error}"))
}

fn dashboard(itemid: &str) -> Value {
    json!({
        "dashboardid": "7", "name": "contract",
        "pages": [{
            "dashboard_pageid": "11", "name": "page", "display_period": "0",
            "widgets": [{
                "widgetid": "13", "type": "item", "name": "source",
                "x": "0", "y": "0", "width": "12", "height": "3", "view_mode": "0",
                "fields": [{"type": "4", "name": "itemid.0", "value": itemid}]
            }]
        }], "users": [], "userGroups": []
    })
}

#[test]
fn update_reconciliation_decides_only_on_an_exact_fingerprint() {
    let (_dir, journal) = setup();
    let source = snapshot(dashboard("101"));
    let expected = snapshot(dashboard("202"));
    let other = snapshot(dashboard("303"));
    let mut operation = record("7");
    operation.source_fingerprint = source.fingerprint().to_owned();
    operation.expected_fingerprint = expected.fingerprint().to_owned();
    let id = operation_id();
    drop(begin(&journal, &id, &operation));
    let entry = entry(&journal, &id);
    assert_eq!(
        decide_update_reconciliation(&entry, Some(&expected)),
        UpdateEvidence::ExpectedFound
    );
    assert_eq!(
        decide_update_reconciliation(&entry, Some(&source)),
        UpdateEvidence::SourceFound
    );
    assert_eq!(
        decide_update_reconciliation(&entry, Some(&other)),
        UpdateEvidence::OtherFound
    );
    assert_eq!(
        decide_update_reconciliation(&entry, None),
        UpdateEvidence::NotVisible
    );
    let mut elsewhere = dashboard("202");
    elsewhere["dashboardid"] = json!("8");
    assert_eq!(
        decide_update_reconciliation(&entry, Some(&snapshot(elsewhere))),
        UpdateEvidence::NotVisible
    );
    assert_eq!(
        UpdateEvidence::ExpectedFound.state(),
        JournalState::ReconciledVerified
    );
    assert_eq!(
        UpdateEvidence::SourceFound.state(),
        JournalState::ReconciledFailed
    );
    assert_eq!(
        UpdateEvidence::OtherFound.state(),
        JournalState::ManualCheckRequired
    );
}

// --- publication path on a loopback mock -------------------------------

#[derive(Default)]
struct Mock {
    dashboard: Value,
    updates: usize,
    /// A proxy answers 502 after the server committed the update.
    bad_gateway_after_commit: bool,
}

type Shared = Arc<Mutex<Mock>>;

async fn rpc(State(data): State<Shared>, Json(body): Json<Value>) -> Response {
    let id = body.get("id").cloned().unwrap_or(Value::Null);
    let mut data = data.lock().unwrap_or_else(PoisonError::into_inner);
    let result = match body["method"].as_str().unwrap_or_default() {
        "apiinfo.version" => json!("7.4.16"),
        "user.checkAuthentication" => json!({"userid": "3", "username": "synthetic"}),
        "dashboard.get" => json!([data.dashboard.clone()]),
        "dashboard.update" => {
            data.updates += 1;
            data.dashboard = dashboard("202");
            if data.bad_gateway_after_commit {
                return (StatusCode::BAD_GATEWAY, "bad gateway").into_response();
            }
            json!({"dashboardids": ["7"]})
        }
        _ => json!([]),
    };
    Json(json!({"jsonrpc": "2.0", "id": id, "result": result})).into_response()
}

async fn writer(data: &Shared) -> (ReadWriteGateway, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route("/api_jsonrpc.php", post(rpc))
        .with_state(Arc::clone(data));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("address: {error}"));
    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            panic!("mock API failed: {error}");
        }
    });
    let endpoint = ZabbixEndpoint::parse(&format!("http://{address}/api_jsonrpc.php"))
        .unwrap_or_else(|error| panic!("endpoint: {error}"));
    let gateway = ZabbixClient::new(endpoint)
        .unwrap_or_else(|error| panic!("client: {error}"))
        .authenticate(Authentication::ApiToken(SecretString::from(
            "synthetic-token".to_owned(),
        )))
        .await
        .unwrap_or_else(|error| panic!("sign-in: {error}"))
        .enable_writes();
    (gateway, task)
}

/// Builds, confirms and publishes the one-field P0-07 plan.
async fn publish(gateway: &ReadWriteGateway, journal: &PublishJournal) -> crate::PublishReport {
    let source = gateway
        .reader()
        .dashboard_snapshot(CONNECTION, "7")
        .await
        .unwrap_or_else(|error| panic!("read: {error}"))
        .unwrap_or_else(|| panic!("dashboard"));
    let session = LocalSession::new();
    let plan = build_verified_plan(
        journal,
        &source,
        &gateway.publication_context(),
        &[WidgetFieldReplacement {
            page_index: 0,
            widgetid: "13".to_owned(),
            field_name: "itemid.0".to_owned(),
            expected_value: "101".to_owned(),
            replacement_value: "202".to_owned(),
        }],
        session.current(),
    )
    .unwrap_or_else(|error| panic!("plan: {error}"));
    let shown = plan.preview().plan_id;
    let confirmed = plan
        .confirm(&shown)
        .unwrap_or_else(|error| panic!("confirm: {error}"));
    let stage = session
        .begin_write(confirmed.session())
        .unwrap_or_else(|_| panic!("write stage"));
    publish_confirmed_plan(gateway, journal, confirmed, &stage).await
}

#[tokio::test]
async fn no_write_is_sent_when_the_journal_cannot_be_made_durable() {
    let data: Shared = Arc::new(Mutex::new(Mock {
        dashboard: dashboard("101"),
        ..Mock::default()
    }));
    let (gateway, task) = writer(&data).await;
    let (_dir, journal) = setup();
    let updates = || data.lock().unwrap_or_else(PoisonError::into_inner).updates;

    for (state, kind) in [
        (JournalState::Confirmed, FailKind::BeforeWrite),
        (JournalState::Confirmed, FailKind::Sync),
        (JournalState::Submitting, FailKind::BeforeWrite),
        (JournalState::Submitting, FailKind::Torn),
    ] {
        let report = {
            let _failing = Failing::at(state, kind);
            publish(&gateway, &journal).await
        };
        assert_eq!(
            report.states.last(),
            Some(&JournalState::FailedBeforeSend),
            "{state:?} {kind:?}"
        );
        assert!(!report.states.contains(&JournalState::Submitting));
        assert_eq!(
            report.journal_error.as_ref().map(JournalError::code),
            Some("journal_write_failed")
        );
        assert_eq!(updates(), 0, "{state:?} {kind:?}: nothing may be sent");
        // Nothing was sent, so the next plan is not blocked.
        assert_eq!(journal.check_target(&key("7")), Ok(()));
    }

    // A submit record whose fsync failed forbids the send and, because it
    // may be on disk, keeps the target blocked until checked.
    let report = {
        let _failing = Failing::at(JournalState::Submitting, FailKind::Sync);
        publish(&gateway, &journal).await
    };
    assert_eq!(report.states.last(), Some(&JournalState::FailedBeforeSend));
    assert_eq!(updates(), 0);
    let found = entry(&journal, &report.plan_id);
    assert_eq!(found.progress, JournalProgress::NeedsReconciliation);
    let reconciled = reconcile_update(&journal, gateway.reader(), &report.plan_id)
        .await
        .unwrap_or_else(|error| panic!("reconcile: {error}"));
    assert_eq!(reconciled.state, JournalState::ReconciledFailed);
    assert_eq!(updates(), 0, "reconciliation only reads");

    // With a working journal the same plan is sent exactly once.
    let report = publish(&gateway, &journal).await;
    assert_eq!(report.states.last(), Some(&JournalState::Verified));
    assert_eq!(report.journal_error, None);
    assert_eq!(updates(), 1);
    let found = entry(&journal, &report.plan_id);
    let states: Vec<_> = found.transitions.iter().map(|step| step.state).collect();
    assert_eq!(
        states,
        [
            JournalState::Confirmed,
            JournalState::Submitting,
            JournalState::Verified
        ]
    );
    assert_eq!(found.intent, "replace_item_fields");
    assert_eq!(found.source, None);
    task.abort();
}

/// Owner decision 28.10.2026, item 11: an HTTP status after the update was
/// sent is an unknown outcome decided by reads only; the committed update
/// is journalled `reconciled_verified` and the target is free again.
#[tokio::test]
async fn http_status_after_a_committed_update_is_journalled_as_reconciled() {
    let data: Shared = Arc::new(Mutex::new(Mock {
        dashboard: dashboard("101"),
        bad_gateway_after_commit: true,
        ..Mock::default()
    }));
    let (gateway, task) = writer(&data).await;
    let (_dir, journal) = setup();
    let report = publish(&gateway, &journal).await;
    assert_eq!(report.journal_error, None);
    assert_eq!(
        data.lock().unwrap_or_else(PoisonError::into_inner).updates,
        1
    );
    let found = entry(&journal, &report.plan_id);
    let states: Vec<_> = found.transitions.iter().map(|step| step.state).collect();
    assert_eq!(
        states,
        [
            JournalState::Confirmed,
            JournalState::Submitting,
            JournalState::ReconciledVerified
        ]
    );
    assert_eq!(found.progress, JournalProgress::Finished);
    assert_eq!(journal.check_target(&key("7")), Ok(()));
    task.abort();
}

#[test]
fn create_records_carry_the_owner_and_an_unconfirmed_reported_dashboardid() {
    let (dir, journal) = setup();
    let root = dir.path();

    // The owner is required on a create and absent on an update.
    let mut no_owner = create_record("DB: Pilot", Some(LOCAL));
    no_owner.owner_userid = None;
    let mut bad_owner = create_record("DB: Pilot", Some(LOCAL));
    bad_owner.owner_userid = Some("admin".to_owned());
    let mut owned_update = record("5");
    owned_update.owner_userid = Some("3".to_owned());
    for invalid in [no_owner, bad_owner, owned_update] {
        let error = journal
            .begin(&operation_id(), &invalid)
            .err()
            .unwrap_or_else(|| panic!("must be refused"));
        assert_eq!(error.code(), "journal_corrupt");
        assert_eq!(error.param("reason"), Some("owner_userid"));
    }

    // A reported but unconfirmed ID is kept apart from a confirmed one.
    let id = operation_id();
    let mut handle = begin(&journal, &id, &create_record("DB: Pilot", Some(LOCAL)));
    assert!(handle.admit_send());
    handle
        .finish_with(Finish {
            evidence: Some("read_back_unavailable"),
            reported_dashboardid: Some("77".to_owned()),
            ..Finish::state(JournalState::FailedAfterResponse)
        })
        .unwrap_or_else(|error| panic!("finish: {error}"));
    let found = entry(&journal, &id);
    assert_eq!(found.owner_userid.as_deref(), Some("3"));
    assert_eq!(found.reported_dashboardid.as_deref(), Some("77"));
    assert_eq!(found.created_dashboardid, None);
    assert_eq!(found.evidence.as_deref(), Some("read_back_unavailable"));
    assert_eq!(found.progress, JournalProgress::NeedsReconciliation);
    let raw = read_file(root, &id);
    assert!(raw.contains("\"owner_userid\":\"3\""));
    assert!(raw.contains("\"reported_dashboardid\":\"77\""));

    // A line that would not replay is never written.
    let id = operation_id();
    let mut handle = begin(&journal, &id, &create_record("Other", None));
    assert!(handle.admit_send());
    let before = read_file(root, &id);
    let error = handle
        .finish_with(Finish {
            reported_dashboardid: Some("77".to_owned()),
            ..Finish::state(JournalState::Verified)
        })
        .err()
        .unwrap_or_else(|| panic!("must be refused"));
    assert_eq!(error.code(), "journal_transition_invalid");
    assert_eq!(error.param("field"), Some("reported_dashboardid"));
    assert_eq!(read_file(root, &id), before, "nothing appended");
    assert_eq!(entry(&journal, &id).state, JournalState::Submitting);

    // Hand-written files: the reported ID only on a create's
    // `failed_after_response`.
    let create_header = |id: &str| {
        let mut line = LineFile::transition(id, 0, JournalState::Confirmed, 1);
        line.operation = Some(create_record("Hand", None).to_file());
        serde_json::to_value(&line).unwrap_or_else(|error| panic!("line: {error}"))
    };
    let mut on_update = vec![
        header("zds-reported-update", "960"),
        line("zds-reported-update", 1, JournalState::Submitting),
        line("zds-reported-update", 2, JournalState::FailedAfterResponse),
    ];
    on_update[2]["reported_dashboardid"] = json!("5");
    let mut on_verified = vec![
        create_header("zds-reported-verified"),
        line("zds-reported-verified", 1, JournalState::Submitting),
        line("zds-reported-verified", 2, JournalState::Verified),
    ];
    on_verified[2]["reported_dashboardid"] = json!("5");
    let mut not_an_id = vec![
        create_header("zds-reported-text"),
        line("zds-reported-text", 1, JournalState::Submitting),
        line("zds-reported-text", 2, JournalState::FailedAfterResponse),
    ];
    not_an_id[2]["reported_dashboardid"] = json!("../5");
    let mut ownerless = create_header("zds-create-ownerless");
    ownerless["operation"]
        .as_object_mut()
        .map(|operation| operation.remove("owner_userid"));
    for (id, raw) in [
        ("zds-reported-update", lines(&on_update)),
        ("zds-reported-verified", lines(&on_verified)),
        ("zds-reported-text", lines(&not_an_id)),
        ("zds-create-ownerless", lines(&[ownerless])),
    ] {
        write_file(root, id, &raw);
        assert_eq!(code(journal.entry(id)), Some("journal_corrupt"), "{id}");
    }
}

/// LAB-02: a reordered read-back of the expected document reconciles as
/// verified; an entry recorded before the canonical fingerprint (P0
/// raw-bytes values) still decides on byte-identical documents and
/// otherwise needs a manual check, never a decided result.
#[test]
fn update_reconciliation_uses_the_canonical_fingerprint_and_reads_earlier_entries() {
    let (_dir, journal) = setup();
    let two_fields = |itemid: &str, first: bool| {
        let mut raw = dashboard(itemid);
        let fields = raw["pages"][0]["widgets"][0]["fields"]
            .as_array_mut()
            .unwrap_or_else(|| panic!("fields"));
        let show = json!({"type": "0", "name": "show.0", "value": "2"});
        if first {
            fields.insert(0, show);
        } else {
            fields.push(show);
        }
        raw
    };
    let source = snapshot(two_fields("101", true));
    let expected = snapshot(two_fields("202", true));
    let reordered = snapshot(two_fields("202", false));
    assert_eq!(expected.fingerprint(), reordered.fingerprint());

    let mut current = record("7");
    current.source_fingerprint = source.fingerprint().to_owned();
    current.expected_fingerprint = expected.fingerprint().to_owned();
    let id = operation_id();
    drop(begin(&journal, &id, &current));
    assert_eq!(
        decide_update_reconciliation(&entry(&journal, &id), Some(&reordered)),
        UpdateEvidence::ExpectedFound
    );

    let mut earlier = record("7");
    earlier.source_fingerprint = source.storage_fingerprint().to_owned();
    earlier.expected_fingerprint = expected.storage_fingerprint().to_owned();
    let id = operation_id();
    drop(begin(&journal, &id, &earlier));
    let earlier = entry(&journal, &id);
    assert_eq!(
        decide_update_reconciliation(&earlier, Some(&expected)),
        UpdateEvidence::ExpectedFound
    );
    assert_eq!(
        decide_update_reconciliation(&earlier, Some(&source)),
        UpdateEvidence::SourceFound
    );
    assert_eq!(
        decide_update_reconciliation(&earlier, Some(&reordered)),
        UpdateEvidence::OtherFound
    );
}

const DAY: u64 = 24 * 60 * 60 * 1_000;
/// The injected start-up time of the retention tests.
const NOW: u64 = 400 * DAY;

/// A journal file of `states` (first is `confirmed`), the last line at
/// `last_at`, earlier lines one millisecond apart before it.
fn timed(id: &str, states: &[JournalState], last_at: u64) -> Vec<u8> {
    let count = u64::try_from(states.len()).unwrap_or(u64::MAX);
    let values = states
        .iter()
        .enumerate()
        .map(|(seq, state)| {
            let offset = count - 1 - u64::try_from(seq).unwrap_or(0);
            let mut line = LineFile::transition(id, seq, *state, last_at.saturating_sub(offset));
            if seq == 0 {
                line.operation = Some(record("970").to_file());
            }
            serde_json::to_value(&line).unwrap_or_else(|error| panic!("line: {error}"))
        })
        .collect::<Vec<_>>();
    lines(&values)
}

fn prune(journal: &PublishJournal, now: u64) -> RetentionReport {
    journal
        .prune_expired_at(now)
        .unwrap_or_else(|error| panic!("prune: {error}"))
}

/// Owner decision 28.10.2026 item 12: at start-up only finished entries
/// whose last line is older than 90 days are deleted; entries that need
/// reconciliation, never-sent entries, damaged files, files in
/// `journal/quarantine/` and files outside `journal/` are kept.
#[test]
fn retention_deletes_only_expired_finished_entries() {
    use JournalState::{
        ClosedManually, Confirmed, FailedBeforeSend, ManualCheckRequired, ReconciledVerified,
        Submitting, UnknownOutcome, Verified,
    };
    let (dir, journal) = setup();
    let root = dir.path();
    let old = NOW - 91 * DAY;
    let recent = NOW - 89 * DAY;

    let removed = [
        ("zds-old-verified", vec![Confirmed, Submitting, Verified]),
        ("zds-old-not-sent", vec![Confirmed, FailedBeforeSend]),
        (
            "zds-old-closed",
            vec![Confirmed, Submitting, UnknownOutcome, ClosedManually],
        ),
    ];
    for (id, states) in &removed {
        write_file(root, id, &timed(id, states, old));
    }
    let kept = [
        (
            "zds-recent-verified",
            vec![Confirmed, Submitting, Verified],
            recent,
        ),
        ("zds-old-submitting", vec![Confirmed, Submitting], old),
        (
            "zds-old-unknown",
            vec![Confirmed, Submitting, UnknownOutcome],
            old,
        ),
        (
            "zds-old-manual",
            vec![Confirmed, Submitting, UnknownOutcome, ManualCheckRequired],
            old,
        ),
        ("zds-old-abandoned", vec![Confirmed], old),
        (
            "zds-reconciled-recently",
            vec![Confirmed, Submitting, UnknownOutcome, ReconciledVerified],
            recent,
        ),
        (
            "zds-future-verified",
            vec![Confirmed, Submitting, Verified],
            NOW + DAY,
        ),
    ];
    for (id, states, at) in &kept {
        write_file(root, id, &timed(id, states, *at));
    }
    // A damaged file whose readable lines are old and final.
    let mut damaged = timed("zds-old-damaged", &[Confirmed, Submitting, Verified], old);
    damaged.extend_from_slice(b"{not json}\n");
    write_file(root, "zds-old-damaged", &damaged);
    // Quarantine and the storage root are never looked at.
    let quarantined = root.join("journal").join("quarantine");
    std::fs::create_dir_all(&quarantined).unwrap_or_else(|error| panic!("dir: {error}"));
    let finished_old = timed("zds-quarantined", &[Confirmed, FailedBeforeSend], old);
    std::fs::write(quarantined.join("zds-quarantined.jsonl"), &finished_old)
        .unwrap_or_else(|error| panic!("write: {error}"));
    std::fs::write(root.join("zds-outside.jsonl"), &finished_old)
        .unwrap_or_else(|error| panic!("write: {error}"));

    let report = prune(&journal, NOW);
    let mut expected = removed
        .iter()
        .map(|(id, _)| (*id).to_owned())
        .collect::<Vec<_>>();
    expected.sort_unstable();
    assert_eq!(report.removed, expected);
    assert!(report.failed.is_empty(), "{report:?}");
    for (id, _) in &removed {
        assert!(!journal_file(root, id).exists(), "{id} deleted");
        assert_eq!(code(journal.entry(id)), Some("operation_not_found"));
    }
    for (id, states, at) in &kept {
        assert_eq!(
            read_file(root, id).into_bytes(),
            timed(id, states, *at),
            "{id} kept"
        );
    }
    assert_eq!(read_file(root, "zds-old-damaged").into_bytes(), damaged);
    assert!(quarantined.join("zds-quarantined.jsonl").exists());
    assert!(root.join("zds-outside.jsonl").exists());

    // Recovery still sees every entry that needs reconciliation and the
    // damaged file; a second pass deletes nothing more.
    let recovery = journal
        .recover()
        .unwrap_or_else(|error| panic!("recover: {error}"));
    let mut unfinished = recovery
        .unfinished
        .iter()
        .map(|entry| entry.operation_id.as_str())
        .collect::<Vec<_>>();
    unfinished.sort_unstable();
    assert_eq!(
        unfinished,
        ["zds-old-manual", "zds-old-submitting", "zds-old-unknown"]
    );
    assert_eq!(recovery.damaged.len(), 1);
    assert_eq!(prune(&journal, NOW), RetentionReport::default());

    // Exactly 90 days is not older than 90 days.
    write_file(
        root,
        "zds-boundary",
        &timed(
            "zds-boundary",
            &[Confirmed, FailedBeforeSend],
            NOW - 90 * DAY,
        ),
    );
    assert!(prune(&journal, NOW).removed.is_empty());
    assert_eq!(prune(&journal, NOW + 1).removed, ["zds-boundary"]);
}

/// Start-up on an empty storage root creates nothing; with entries it
/// prunes first and then reports what blocks.
#[test]
fn start_up_prunes_then_recovers() {
    let (dir, journal) = setup();
    let root = dir.path();
    assert_eq!(
        journal
            .start_up()
            .unwrap_or_else(|error| panic!("start up: {error}")),
        StartupReport::default()
    );
    assert!(!root.join("journal").exists(), "nothing created");

    // An entry finished at the epoch start has expired by any real clock.
    let id = "zds-epoch-not-sent";
    write_file(
        root,
        id,
        &timed(
            id,
            &[JournalState::Confirmed, JournalState::FailedBeforeSend],
            1,
        ),
    );
    let blocking = "zds-epoch-submitting";
    write_file(
        root,
        blocking,
        &timed(
            blocking,
            &[JournalState::Confirmed, JournalState::Submitting],
            1,
        ),
    );
    let report = journal
        .start_up()
        .unwrap_or_else(|error| panic!("start up: {error}"));
    assert_eq!(report.retention.removed, [id]);
    let [unfinished] = report.recovery.unfinished.as_slice() else {
        panic!("one unfinished entry: {report:?}");
    };
    assert_eq!(unfinished.operation_id, blocking);
    assert!(journal_file(root, blocking).exists());
}

/// A link named like a journal file is never followed and never deleted,
/// and neither is its target.
#[cfg(unix)]
#[test]
fn retention_never_follows_or_deletes_links() {
    let (dir, journal) = setup();
    let root = dir.path();
    let outside = root.join("outside.jsonl");
    std::fs::write(
        &outside,
        timed(
            "zds-linked",
            &[JournalState::Confirmed, JournalState::FailedBeforeSend],
            1,
        ),
    )
    .unwrap_or_else(|error| panic!("write: {error}"));
    std::fs::create_dir_all(root.join("journal")).unwrap_or_else(|error| panic!("dir: {error}"));
    std::os::unix::fs::symlink(&outside, journal_file(root, "zds-linked"))
        .unwrap_or_else(|error| panic!("link: {error}"));
    assert_eq!(prune(&journal, NOW), RetentionReport::default());
    assert!(outside.exists());
    assert!(
        std::fs::symlink_metadata(journal_file(root, "zds-linked")).is_ok(),
        "link kept"
    );
}
