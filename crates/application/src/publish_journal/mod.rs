// SPDX-License-Identifier: MIT OR Apache-2.0

//! Durable publication journal and recovery (P2-04D, O-018 §6, ADR-012
//! D-07/D-11, P1-01G §4, §7, P1-08G).
//!
//! ```text
//! <storage_root>/journal/<operation_id>.jsonl    zdashstitch.journal/v0, local only
//! ```
//!
//! One file per confirmed operation (the `plan_id` the user confirmed), one
//! line per state transition, `fsync` after every line. The record holds no
//! payload, secrets or raw server answers: the operation kind and intent,
//! the connection profile ID, server version, endpoint and server identity
//! fingerprints, the target, the source workcopy, the source and expected
//! fingerprints, for a create the expected owner (the reconciliation key
//! of P2-06D), and per transition the state, the time and what the
//! deciding read observed (for a create also a reported but unconfirmed
//! `dashboardid`).
//!
//! State machine ([`JournalState`]; any other transition is rejected, both
//! when writing and when replaying a file):
//!
//! ```text
//! confirmed ─┬─▶ submitting ─┬─▶ verified | reconciled_verified | reconciled_failed      (final)
//!            │               └─▶ unknown_outcome | failed_after_response | manual_check_required
//!            │                        └─▶ reconciled_verified | reconciled_failed (final)
//!            │                        └─▶ manual_check_required (again)
//!            │                        └─▶ closed_manually (final, explicit user command)
//!            └─▶ failed_before_send | conflict                                         (final)
//! ```
//!
//! `submitting` is committed to disk **before** the single write request;
//! when it cannot be committed nothing is sent. An entry that ends in
//! `submitting` (crash, cancelled request), `unknown_outcome`,
//! `failed_after_response` or `manual_check_required` needs
//! reconciliation: until it reaches a `reconciled_*` state or is closed
//! manually, every new plan and every send to the same target is refused
//! with `target_blocked`; only the operation-specific reconciliation reads
//! ([`reconcile_update`], [`crate::reconcile_create`]) are allowed. `closed_manually` never claims a
//! result; a new publication to the target still starts with a fresh read.
//!
//! An entry that ends in `confirmed` was never sent: while its operation
//! runs in this process it blocks the target, afterwards it is
//! [`JournalProgress::Abandoned`]. A journal file that does not replay is
//! damaged and is never guessed around: its target stays blocked, and when
//! even its target cannot be read every publication is refused with
//! `journal_damaged` until the file is dealt with.
//!
//! Retention ([`PublishJournal::start_up`], `retention.rs`): at start-up a
//! finished entry whose last line is older than [`JOURNAL_RETENTION_DAYS`]
//! is deleted; unfinished, blocking and damaged files are never touched.
//!
//! The API returns stable codes and data, never prose.

mod file;
mod quarantine;
mod reconcile;
mod record;
mod retention;

use std::collections::BTreeSet;
use std::fmt;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

use zdashstitch_zabbix::{FailureParam, ZabbixError};

pub use crate::publisher::JournalState;
use crate::settings::AppSettings;
use crate::workcopy::WorkcopyError;
use crate::workcopy::store::LocalStore;

#[cfg(test)]
pub(crate) use file::{FailKind, fail};
pub use quarantine::{QUARANTINE_DIR, QuarantineRequest};
pub use reconcile::{UpdateEvidence, decide_update_reconciliation, reconcile_update};
use record::{Damage, LineFile, check_details, is_actor, is_code, replay, transition_allowed};
pub(crate) use record::{OperationFile, SourceFile, TargetFile};
pub use retention::{JOURNAL_RETENTION_DAYS, RetentionFailure, RetentionReport, StartupReport};

/// What the operation does on the server.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum OperationKind {
    /// `dashboard.update` of an existing dashboard.
    Update,
    /// `dashboard.create` of a new dashboard (P2-06).
    Create,
}

impl OperationKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Update => "update",
            Self::Create => "create",
        }
    }

    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "update" => Some(Self::Update),
            "create" => Some(Self::Create),
            _ => None,
        }
    }
}

/// The target of an operation on its server.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum JournalTarget {
    /// An existing dashboard; `name` is server data when known.
    ExistingDashboard {
        dashboardid: String,
        name: Option<String>,
    },
    /// A dashboard to create with this exact name (server/user data).
    NewDashboard { name: String },
}

/// The workcopy an operation was planned from.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JournalSource {
    pub local_id: String,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JournalTransition {
    pub state: JournalState,
    /// Unix epoch milliseconds (UTC).
    pub at_ms: u64,
}

/// Where an entry stands for recovery.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum JournalProgress {
    /// The operation is running in this process (plan confirmed, write
    /// stage open). Blocks the target.
    Running,
    /// The write may have reached the server and the result is not
    /// confirmed: only reconciliation reads or «Закрыть без сверки».
    /// Blocks the target.
    NeedsReconciliation,
    /// Confirmed but never sent, and no longer running (the process ended
    /// or the submit record failed). Does not block.
    Abandoned,
    /// A final state. Does not block.
    Finished,
}

impl JournalProgress {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::NeedsReconciliation => "needs_reconciliation",
            Self::Abandoned => "abandoned",
            Self::Finished => "finished",
        }
    }

    #[must_use]
    pub const fn blocks_target(self) -> bool {
        matches!(self, Self::Running | Self::NeedsReconciliation)
    }
}

/// One replayed journal file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalEntry {
    /// The confirmed `plan_id`; also the file name.
    pub operation_id: String,
    pub kind: OperationKind,
    /// `replace_host`, `replace_item_fields`, `save_as_copy`, …
    pub intent: String,
    /// Connection profile ID.
    pub connection_id: String,
    pub server_version: String,
    pub target: JournalTarget,
    pub source: Option<JournalSource>,
    /// Remote (update) or Local (create) fingerprint the plan was built on.
    pub source_fingerprint: String,
    /// What a successful write leaves on the server.
    pub expected_fingerprint: String,
    /// Create only: the expected owner (`userid` of the connection account).
    pub owner_userid: Option<String>,
    pub plan_created_at_ms: u64,
    pub confirmed_at_ms: u64,
    pub transitions: Vec<JournalTransition>,
    /// The last committed state.
    pub state: JournalState,
    /// Write requests sent: always 0 or 1.
    pub sends: u8,
    pub observed_fingerprint: Option<String>,
    /// The confirmed new dashboard of a create (O-022).
    pub created_dashboardid: Option<String>,
    /// Create only: a `dashboardid` the server reported that the read-back
    /// did not confirm; reconciliation reads it by ID. Never a result.
    pub reported_dashboardid: Option<String>,
    /// What the deciding reconciliation read found.
    pub evidence: Option<String>,
    /// Local account that closed the entry without checking.
    pub closed_by: Option<String>,
    pub progress: JournalProgress,
    pub(crate) endpoint_fingerprint: String,
    pub(crate) server_identity: String,
}

impl JournalEntry {
    /// Time the `submitting` record was committed.
    #[must_use]
    pub fn submitted_at_ms(&self) -> Option<u64> {
        self.transitions
            .iter()
            .find(|transition| transition.state == JournalState::Submitting)
            .map(|transition| transition.at_ms)
    }

    fn matches(&self, key: &TargetKey) -> bool {
        matches_target(
            &self.connection_id,
            Some(self.endpoint_fingerprint.as_str()),
            &self.target,
            self.source.as_ref().map(|source| source.local_id.as_str()),
            key,
        )
    }
}

/// A journal file that could not be replayed. Listed, never modified.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DamagedJournal {
    /// From the file name.
    pub operation_id: String,
    pub error: JournalError,
    /// Connection and target, when the first line was still readable.
    pub connection_id: Option<String>,
    pub target: Option<JournalTarget>,
    endpoint_fingerprint: Option<String>,
    source_local_id: Option<String>,
}

impl DamagedJournal {
    /// A damaged file blocks its target, or every target when its target
    /// cannot be read.
    fn blocks(&self, key: &TargetKey) -> bool {
        match (&self.connection_id, &self.target) {
            (Some(connection_id), Some(target)) => matches_target(
                connection_id,
                self.endpoint_fingerprint.as_deref(),
                target,
                self.source_local_id.as_deref(),
                key,
            ),
            _ => true,
        }
    }
}

/// Journal entries plus damaged files.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JournalListing {
    pub entries: Vec<JournalEntry>,
    pub damaged: Vec<DamagedJournal>,
}

/// What start-up (or opening a workcopy) must show before any new write
/// (P1-01G §4 `startup.recovery.found`): entries that block their target
/// and damaged files.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RecoveryReport {
    pub unfinished: Vec<JournalEntry>,
    pub damaged: Vec<DamagedJournal>,
}

impl RecoveryReport {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.unfinished.is_empty() && self.damaged.is_empty()
    }
}

/// The target a new plan or send would write to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetKey {
    pub connection_id: String,
    /// Endpoint fingerprint of the live connection, when known: another
    /// profile of the same server address is the same target.
    pub endpoint_fingerprint: Option<String>,
    pub target: TargetRef,
}

/// Target part of a [`TargetKey`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetRef {
    Dashboard {
        dashboardid: String,
    },
    /// A create: blocked by an unfinished create with the same name, or one
    /// planned from the same workcopy (it may have created the copy).
    NewDashboard {
        name: String,
        source_local_id: Option<String>,
    },
}

impl TargetKey {
    #[must_use]
    pub fn dashboard(
        connection_id: impl Into<String>,
        endpoint_fingerprint: Option<String>,
        dashboardid: impl Into<String>,
    ) -> Self {
        Self {
            connection_id: connection_id.into(),
            endpoint_fingerprint,
            target: TargetRef::Dashboard {
                dashboardid: dashboardid.into(),
            },
        }
    }
}

fn matches_target(
    connection_id: &str,
    endpoint: Option<&str>,
    target: &JournalTarget,
    source_local_id: Option<&str>,
    key: &TargetKey,
) -> bool {
    let same_server = connection_id == key.connection_id
        || endpoint.is_some_and(|endpoint| key.endpoint_fingerprint.as_deref() == Some(endpoint));
    same_server
        && match (target, &key.target) {
            (
                JournalTarget::ExistingDashboard { dashboardid, .. },
                TargetRef::Dashboard {
                    dashboardid: wanted,
                },
            ) => dashboardid == wanted,
            (
                JournalTarget::NewDashboard { name },
                TargetRef::NewDashboard {
                    name: wanted,
                    source_local_id: wanted_source,
                },
            ) => {
                name == wanted
                    || source_local_id
                        .is_some_and(|source| wanted_source.as_deref() == Some(source))
            }
            _ => false,
        }
}

/// Failure of a journal use case: a stable code plus safe parameters
/// (codes, IDs, counts; never names, paths or secrets). `Display` shows
/// only the code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalError {
    code: &'static str,
    params: Vec<FailureParam>,
}

impl JournalError {
    pub(crate) fn new(code: &'static str) -> Self {
        Self {
            code,
            params: Vec::new(),
        }
    }

    pub(crate) fn with(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.params.push(FailureParam {
            name,
            value: value.into(),
        });
        self
    }

    /// Storage failures keep their code (`storage_unavailable`, …).
    pub(crate) fn from_storage(error: &WorkcopyError) -> Self {
        Self {
            code: error.code(),
            params: error.params().to_vec(),
        }
    }

    /// Read failures keep the connection taxonomy code.
    pub(crate) fn from_zabbix(error: &ZabbixError) -> Self {
        Self::from_failure(error.failure())
    }

    /// A read failure already classified by the adapter.
    pub(crate) fn from_failure(failure: zdashstitch_zabbix::Failure) -> Self {
        Self {
            code: failure.code.as_str(),
            params: failure.params,
        }
    }

    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    #[must_use]
    pub fn params(&self) -> &[FailureParam] {
        &self.params
    }

    #[must_use]
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|param| param.name == name)
            .map(|param| param.value.as_str())
    }
}

impl fmt::Display for JournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "publication journal: {}", self.code)
    }
}

impl std::error::Error for JournalError {}

/// Operation ID: a `plan_id` such as `zds-<hex>-<hex>-<hex>`; lowercase
/// ASCII letters, digits and `-`, a safe path component.
#[must_use]
pub fn is_operation_id(value: &str) -> bool {
    value.starts_with("zds-")
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// «Закрыть без сверки» (P1-01G §7): the user confirmed the dialog
/// `startup.recovery.close_unverified.confirm` for exactly this operation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CloseManuallyRequest {
    pub operation_id: String,
    pub confirmed: bool,
}

/// Operations with an open journal handle in this process.
static RUNNING: LazyLock<Mutex<BTreeSet<String>>> = LazyLock::new(|| Mutex::new(BTreeSet::new()));

fn running() -> MutexGuard<'static, BTreeSet<String>> {
    RUNNING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What [`PublishJournal::begin`] records on the first line.
pub(crate) struct OperationRecord {
    pub kind: OperationKind,
    pub intent: &'static str,
    pub connection_id: String,
    pub server_version: String,
    pub endpoint_fingerprint: String,
    pub server_identity: String,
    pub target: JournalTarget,
    pub source: Option<JournalSource>,
    pub source_fingerprint: String,
    pub expected_fingerprint: String,
    /// Create only (required there): the expected owner `userid`.
    pub owner_userid: Option<String>,
    pub plan_created_at_ms: u64,
    pub confirmed_at_ms: u64,
}

impl OperationRecord {
    fn to_file(&self) -> OperationFile {
        let (kind, dashboardid, name) = match &self.target {
            JournalTarget::ExistingDashboard { dashboardid, name } => (
                "existing_dashboard",
                Some(dashboardid.clone()),
                name.clone(),
            ),
            JournalTarget::NewDashboard { name } => ("new_dashboard", None, Some(name.clone())),
        };
        OperationFile {
            kind: self.kind.as_str().to_owned(),
            intent: self.intent.to_owned(),
            connection_id: self.connection_id.clone(),
            server_version: self.server_version.clone(),
            endpoint_fingerprint: self.endpoint_fingerprint.clone(),
            server_identity: self.server_identity.clone(),
            target: TargetFile {
                kind: kind.to_owned(),
                dashboardid,
                name,
            },
            source: self.source.as_ref().map(|source| SourceFile {
                local_id: source.local_id.clone(),
                revision: source.revision,
            }),
            source_fingerprint: self.source_fingerprint.clone(),
            expected_fingerprint: self.expected_fingerprint.clone(),
            owner_userid: self.owner_userid.clone(),
            plan_created_at_ms: self.plan_created_at_ms,
            confirmed_at_ms: self.confirmed_at_ms,
        }
    }

    fn key(&self) -> TargetKey {
        TargetKey {
            connection_id: self.connection_id.clone(),
            endpoint_fingerprint: Some(self.endpoint_fingerprint.clone()),
            target: match &self.target {
                JournalTarget::ExistingDashboard { dashboardid, .. } => TargetRef::Dashboard {
                    dashboardid: dashboardid.clone(),
                },
                JournalTarget::NewDashboard { name } => TargetRef::NewDashboard {
                    name: name.clone(),
                    source_local_id: self.source.as_ref().map(|source| source.local_id.clone()),
                },
            },
        }
    }
}

/// Details of a reconciliation result.
pub(crate) struct Reconciliation {
    /// `reconciled_verified`, `reconciled_failed` or
    /// `manual_check_required`.
    pub state: JournalState,
    /// What the read found (a code).
    pub evidence: &'static str,
    pub observed_fingerprint: Option<String>,
    /// Create only: the single confirmed new dashboard.
    pub created_dashboardid: Option<String>,
}

/// The state a write path appends itself, with what decided it.
pub(crate) struct Finish {
    pub state: JournalState,
    /// A code: what the response or the read-back found.
    pub evidence: Option<&'static str>,
    pub observed_fingerprint: Option<String>,
    /// Create only, on `verified` / `reconciled_verified`.
    pub created_dashboardid: Option<String>,
    /// Create only, on `failed_after_response`.
    pub reported_dashboardid: Option<String>,
}

impl Finish {
    pub(crate) const fn state(state: JournalState) -> Self {
        Self {
            state,
            evidence: None,
            observed_fingerprint: None,
            created_dashboardid: None,
            reported_dashboardid: None,
        }
    }
}

/// A line that would not replay is never written.
fn checked(
    kind: OperationKind,
    state: JournalState,
    line: LineFile,
) -> Result<LineFile, JournalError> {
    check_details(kind, state, &line).map_err(|field| {
        JournalError::new("journal_transition_invalid")
            .with("to", state.as_str())
            .with("field", field)
    })?;
    Ok(line)
}

/// The publication journal of one storage root. Cheap to clone; the set
/// of operations running in this process is shared by all instances.
#[derive(Clone, Debug)]
pub struct PublishJournal {
    store: LocalStore,
}

impl PublishJournal {
    /// Journal over a storage root (`AppSettings::storage_root`).
    #[must_use]
    pub fn new(storage_root: PathBuf) -> Self {
        Self {
            store: LocalStore::at(storage_root),
        }
    }

    #[must_use]
    pub fn from_settings(settings: &AppSettings) -> Self {
        Self::new(settings.storage_root.clone())
    }

    /// Every journal file, replayed. Damaged files are listed, never
    /// modified.
    pub fn list(&self) -> Result<JournalListing, JournalError> {
        let Some(directory) = file::directory(&self.store, false)? else {
            return Ok(JournalListing::default());
        };
        let running = running().clone();
        let mut listing = JournalListing::default();
        for operation_id in file::operation_ids(&directory)? {
            match load(&directory, &operation_id, &running) {
                Ok(Some(entry)) => listing.entries.push(entry),
                // No committed line: the first write never completed, so
                // nothing can have been sent.
                Ok(None) => {}
                Err(damage) => listing.damaged.push(damaged(&operation_id, damage)),
            }
        }
        Ok(listing)
    }

    /// Start-up recovery scan (P1-01G §4): entries that block their target
    /// and damaged files. Reads local files only; nothing is sent and
    /// nothing is modified.
    pub fn recover(&self) -> Result<RecoveryReport, JournalError> {
        let listing = self.list()?;
        Ok(RecoveryReport {
            unfinished: listing
                .entries
                .into_iter()
                .filter(|entry| entry.progress.blocks_target())
                .collect(),
            damaged: listing.damaged,
        })
    }

    /// The recovery scan restricted to one workcopy, for opening it.
    /// Damaged files whose source cannot be read are included: they may
    /// belong to it.
    pub fn recover_for_workcopy(&self, local_id: &str) -> Result<RecoveryReport, JournalError> {
        let report = self.recover()?;
        Ok(RecoveryReport {
            unfinished: report
                .unfinished
                .into_iter()
                .filter(|entry| {
                    entry
                        .source
                        .as_ref()
                        .is_some_and(|source| source.local_id == local_id)
                })
                .collect(),
            damaged: report
                .damaged
                .into_iter()
                .filter(|damaged| {
                    damaged
                        .source_local_id
                        .as_deref()
                        .is_none_or(|source| source == local_id)
                })
                .collect(),
        })
    }

    /// One entry by operation ID.
    pub fn entry(&self, operation_id: &str) -> Result<JournalEntry, JournalError> {
        if !is_operation_id(operation_id) {
            return Err(JournalError::new("operation_id_invalid"));
        }
        let directory = file::directory(&self.store, false)?
            .ok_or_else(|| JournalError::new("operation_not_found"))?;
        let running = running().clone();
        load(&directory, operation_id, &running)
            .map_err(|damage| damage.error)?
            .ok_or_else(|| JournalError::new("operation_not_found"))
    }

    /// Refuses a new plan or send to `key` while an entry blocks it:
    /// `target_blocked` (params `operation_id`, `state`) or
    /// `journal_damaged` (param `operation_id`).
    pub fn check_target(&self, key: &TargetKey) -> Result<(), JournalError> {
        let listing = self.list()?;
        check_listing(&listing, key)
    }

    /// Records that the user confirmed operation `operation_id` and opens
    /// its journal: the target must be free, and the first line
    /// (`confirmed`) is on disk before this returns. The check and the
    /// creation are atomic within the process.
    pub(crate) fn begin(
        &self,
        operation_id: &str,
        record: &OperationRecord,
    ) -> Result<JournalHandle, JournalError> {
        if !is_operation_id(operation_id) {
            return Err(JournalError::new("operation_id_invalid"));
        }
        let operation = record.to_file();
        record::check_operation(&operation)?;
        let mut running = running();
        if running.contains(operation_id) {
            return Err(JournalError::new("operation_running"));
        }
        let directory = file::directory(&self.store, true)?
            .ok_or_else(|| JournalError::new("storage_unavailable"))?;
        let snapshot = running.clone();
        let mut listing = JournalListing::default();
        for id in file::operation_ids(&directory)? {
            match load(&directory, &id, &snapshot) {
                Ok(Some(entry)) => listing.entries.push(entry),
                Ok(None) => {}
                Err(damage) => listing.damaged.push(damaged(&id, damage)),
            }
        }
        check_listing(&listing, &record.key())?;
        let mut line = LineFile::transition(
            operation_id,
            0,
            JournalState::Confirmed,
            crate::workcopy::now_ms(),
        );
        line.operation = Some(operation);
        file::create(&directory, &file::file_name(operation_id), &line)?;
        running.insert(operation_id.to_owned());
        Ok(JournalHandle {
            journal: self.clone(),
            operation_id: operation_id.to_owned(),
            state: JournalState::Confirmed,
            submit_error: None,
        })
    }

    /// Appends `state` onto the entry after `check` accepted the replayed
    /// entry; `fill` adds the optional fields of the line.
    fn transition<C, F>(
        &self,
        operation_id: &str,
        state: JournalState,
        check: C,
        fill: F,
    ) -> Result<JournalEntry, JournalError>
    where
        C: FnOnce(&JournalEntry) -> Result<(), JournalError>,
        F: FnOnce(&mut LineFile),
    {
        if !is_operation_id(operation_id) {
            return Err(JournalError::new("operation_id_invalid"));
        }
        let directory = file::directory(&self.store, false)?
            .ok_or_else(|| JournalError::new("operation_not_found"))?;
        let name = file::file_name(operation_id);
        let running = running().clone();
        file::append(&directory, &name, state, |lines| {
            if lines.is_empty() {
                return Err(JournalError::new("operation_not_found"));
            }
            let mut entry = replay(operation_id, lines).map_err(|damage| damage.error)?;
            entry.progress = progress_of(&entry, &running);
            check(&entry)?;
            if !transition_allowed(Some(entry.state), state) {
                return Err(JournalError::new("journal_transition_invalid")
                    .with("from", entry.state.as_str())
                    .with("to", state.as_str()));
            }
            let mut line =
                LineFile::transition(operation_id, lines.len(), state, crate::workcopy::now_ms());
            fill(&mut line);
            checked(entry.kind, state, line)
        })?;
        self.entry(operation_id)
    }

    /// Records the result of an operation-specific reconciliation read.
    /// Allowed only while the entry needs reconciliation.
    pub(crate) fn record_reconciliation(
        &self,
        operation_id: &str,
        kind: OperationKind,
        result: Reconciliation,
    ) -> Result<JournalEntry, JournalError> {
        if !matches!(
            result.state,
            JournalState::ReconciledVerified
                | JournalState::ReconciledFailed
                | JournalState::ManualCheckRequired
        ) || !is_code(result.evidence)
        {
            return Err(
                JournalError::new("journal_transition_invalid").with("to", result.state.as_str())
            );
        }
        self.transition(
            operation_id,
            result.state,
            |entry| {
                if entry.kind != kind {
                    return Err(JournalError::new("reconcile_kind_mismatch")
                        .with("kind", entry.kind.as_str()));
                }
                require_reconciliation(entry)
            },
            |line| {
                line.evidence = Some(result.evidence.to_owned());
                line.observed_fingerprint = result.observed_fingerprint;
                line.created_dashboardid = result.created_dashboardid;
            },
        )
    }

    /// «Закрыть без сверки» (P1-01G §7, P1-11H D6): only with the user's
    /// confirmation, only for an entry that needs reconciliation and is not
    /// running. Appends `closed_manually` with the time and the local
    /// account; the entry stays in the journal, the target is unblocked,
    /// and nothing claims the write succeeded or failed.
    ///
    /// Errors: `close_not_confirmed`, `operation_id_invalid`,
    /// `operation_not_found`, `operation_running`, `reconcile_not_needed`,
    /// `journal_corrupt`, `journal_write_failed`.
    pub fn close_manually(
        &self,
        request: &CloseManuallyRequest,
    ) -> Result<JournalEntry, JournalError> {
        if !request.confirmed {
            return Err(JournalError::new("close_not_confirmed"));
        }
        let actor = local_account();
        self.transition(
            &request.operation_id,
            JournalState::ClosedManually,
            require_reconciliation,
            |line| line.closed_by = actor,
        )
    }
}

pub(crate) fn require_reconciliation(entry: &JournalEntry) -> Result<(), JournalError> {
    match entry.progress {
        JournalProgress::NeedsReconciliation => Ok(()),
        JournalProgress::Running => Err(JournalError::new("operation_running")),
        JournalProgress::Abandoned | JournalProgress::Finished => {
            Err(JournalError::new("reconcile_not_needed").with("state", entry.state.as_str()))
        }
    }
}

fn check_listing(listing: &JournalListing, key: &TargetKey) -> Result<(), JournalError> {
    if let Some(entry) = listing
        .entries
        .iter()
        .find(|entry| entry.progress.blocks_target() && entry.matches(key))
    {
        return Err(JournalError::new("target_blocked")
            .with("operation_id", entry.operation_id.clone())
            .with("state", entry.state.as_str()));
    }
    if let Some(damaged) = listing.damaged.iter().find(|damaged| damaged.blocks(key)) {
        return Err(JournalError::new("journal_damaged")
            .with("operation_id", damaged.operation_id.clone())
            .with("cause", damaged.error.code()));
    }
    Ok(())
}

fn progress_of(entry: &JournalEntry, running: &BTreeSet<String>) -> JournalProgress {
    if running.contains(&entry.operation_id) {
        return JournalProgress::Running;
    }
    match entry.state {
        JournalState::Confirmed => JournalProgress::Abandoned,
        JournalState::Submitting
        | JournalState::UnknownOutcome
        | JournalState::FailedAfterResponse
        | JournalState::ManualCheckRequired => JournalProgress::NeedsReconciliation,
        JournalState::Planned
        | JournalState::Previewed
        | JournalState::Verified
        | JournalState::Conflict
        | JournalState::FailedBeforeSend
        | JournalState::ReconciledVerified
        | JournalState::ReconciledFailed
        | JournalState::ClosedManually => JournalProgress::Finished,
    }
}

fn load(
    directory: &cap_std::fs::Dir,
    operation_id: &str,
    running: &BTreeSet<String>,
) -> Result<Option<JournalEntry>, Damage> {
    let Some(committed) = file::read(directory, &file::file_name(operation_id))? else {
        return Ok(None);
    };
    if committed.lines.is_empty() {
        return Ok(None);
    }
    let mut entry = replay(operation_id, &committed.lines)?;
    entry.progress = progress_of(&entry, running);
    Ok(Some(entry))
}

fn damaged(operation_id: &str, damage: Damage) -> DamagedJournal {
    let header = damage.operation.as_ref().and_then(|operation| {
        record::check_operation(operation)
            .ok()
            .map(|typed| (operation, typed))
    });
    match header {
        Some((operation, (_, target, source))) => DamagedJournal {
            operation_id: operation_id.to_owned(),
            error: damage.error,
            connection_id: Some(operation.connection_id.clone()),
            target: Some(target),
            endpoint_fingerprint: Some(operation.endpoint_fingerprint.clone()),
            source_local_id: source.map(|source| source.local_id),
        },
        None => DamagedJournal {
            operation_id: operation_id.to_owned(),
            error: damage.error,
            connection_id: None,
            target: None,
            endpoint_fingerprint: None,
            source_local_id: None,
        },
    }
}

/// The local operating-system account, recorded locally on a manual close.
fn local_account() -> Option<String> {
    ["USERNAME", "USER"]
        .iter()
        .find_map(|name| std::env::var(name).ok())
        .filter(|name| is_actor(name))
}

/// Open journal of one running operation. Dropping it without a final
/// state leaves the last committed state on disk (`confirmed` → abandoned,
/// `submitting` → needs reconciliation).
pub(crate) struct JournalHandle {
    journal: PublishJournal,
    operation_id: String,
    state: JournalState,
    submit_error: Option<JournalError>,
}

impl JournalHandle {
    /// Commits `submitting` (fsync) right before the write request.
    /// `false` forbids the send; the error is kept for the report.
    pub(crate) fn admit_send(&mut self) -> bool {
        match self.append(JournalState::Submitting, |_| {}) {
            Ok(()) => true,
            Err(error) => {
                self.submit_error = Some(error);
                false
            }
        }
    }

    /// The error of a refused `submitting` record.
    pub(crate) fn take_submit_error(&mut self) -> Option<JournalError> {
        self.submit_error.take()
    }

    /// Appends a state decided by the write path itself.
    pub(crate) fn finish(
        self,
        state: JournalState,
        created_dashboardid: Option<String>,
    ) -> Result<(), JournalError> {
        self.finish_with(Finish {
            created_dashboardid,
            ..Finish::state(state)
        })
    }

    /// Appends a state decided by the write path itself, with its evidence.
    pub(crate) fn finish_with(mut self, finish: Finish) -> Result<(), JournalError> {
        let Finish {
            state,
            evidence,
            observed_fingerprint,
            created_dashboardid,
            reported_dashboardid,
        } = finish;
        self.append(state, |line| {
            line.evidence = evidence.map(str::to_owned);
            line.observed_fingerprint = observed_fingerprint;
            line.created_dashboardid = created_dashboardid;
            line.reported_dashboardid = reported_dashboardid;
        })
    }

    fn append<F: FnOnce(&mut LineFile)>(
        &mut self,
        state: JournalState,
        fill: F,
    ) -> Result<(), JournalError> {
        let expected = self.state;
        let operation_id = self.operation_id.clone();
        let directory = file::directory(&self.journal.store, false)?
            .ok_or_else(|| JournalError::new("operation_not_found"))?;
        file::append(
            &directory,
            &file::file_name(&operation_id),
            state,
            |lines| {
                let entry = replay(&operation_id, lines).map_err(|damage| damage.error)?;
                // Only this handle writes a running entry; anything else on
                // disk means the file was changed behind it.
                if entry.state != expected || !transition_allowed(Some(expected), state) {
                    return Err(JournalError::new("journal_transition_invalid")
                        .with("from", entry.state.as_str())
                        .with("to", state.as_str()));
                }
                let mut line = LineFile::transition(
                    &operation_id,
                    lines.len(),
                    state,
                    crate::workcopy::now_ms(),
                );
                fill(&mut line);
                checked(entry.kind, state, line)
            },
        )?;
        self.state = state;
        Ok(())
    }
}

impl Drop for JournalHandle {
    fn drop(&mut self) {
        running().remove(&self.operation_id);
    }
}

#[cfg(test)]
mod tests;
