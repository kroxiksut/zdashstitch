// SPDX-License-Identifier: MIT OR Apache-2.0

//! `zdashstitch.journal/v0` line format and its replay (O-018 §4, §6).
//!
//! One JSON object per line, one line per state transition. Every line
//! repeats `format`, `storage_scope`, `operation_id` and its `seq`; only the
//! first line (`seq` 0, state `confirmed`) carries the operation record.
//! A final line without the terminating newline is a write that did not
//! complete (a crash or a failed sync during append): it is not a
//! committed transition and is ignored. Anything else that does not
//! replay exactly — unknown fields, another format, a gap in `seq`, a
//! transition the state machine does not allow — makes the file corrupt,
//! and a corrupt file is never guessed around.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    JournalEntry, JournalError, JournalSource, JournalState, JournalTarget, JournalTransition,
    OperationKind,
};

pub(crate) const JOURNAL_FORMAT: &str = "zdashstitch.journal/v0";
pub(crate) const JOURNAL_FORMAT_FAMILY: &str = "zdashstitch.journal/";
pub(crate) const LOCAL_ONLY: &str = "local_only";

/// Upper bound of one journal file; a normal operation has 3–6 short lines.
pub(crate) const MAX_JOURNAL_FILE_BYTES: usize = 64 * 1024;
/// Upper bound of transitions in one file.
pub(crate) const MAX_JOURNAL_LINES: usize = 64;
const MAX_NAME_CHARS: usize = 255;
const MAX_CODE_CHARS: usize = 48;
const MAX_ACTOR_CHARS: usize = 64;
const MAX_CONNECTION_ID_BYTES: usize = 128;
const MAX_VERSION_BYTES: usize = 32;

/// The allowed transitions of a journal entry (ADR-012 D-07, P1-08G,
/// P1-01G §4, §7). `None` is the empty file.
#[must_use]
pub(crate) const fn transition_allowed(from: Option<JournalState>, to: JournalState) -> bool {
    use JournalState::{
        ClosedManually, Confirmed, Conflict, FailedAfterResponse, FailedBeforeSend,
        ManualCheckRequired, ReconciledFailed, ReconciledVerified, Submitting, UnknownOutcome,
        Verified,
    };
    matches!(
        (from, to),
        (None, Confirmed)
            | (Some(Confirmed), Submitting | FailedBeforeSend | Conflict)
            | (
                Some(Submitting),
                Verified
                    | FailedAfterResponse
                    | UnknownOutcome
                    | ReconciledVerified
                    | ReconciledFailed
                    | ManualCheckRequired
                    | ClosedManually
            )
            | (
                Some(UnknownOutcome | FailedAfterResponse | ManualCheckRequired),
                ReconciledVerified | ReconciledFailed | ManualCheckRequired | ClosedManually
            )
    )
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TargetFile {
    /// `existing_dashboard` or `new_dashboard`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashboardid: Option<String>,
    /// Server or user data; never a path component.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceFile {
    pub local_id: String,
    pub revision: u64,
}

/// The operation record of the first line. No payload, no secrets, no raw
/// server answers (O-018 §6): identities are SHA-256 fingerprints.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OperationFile {
    /// `update` or `create`.
    pub kind: String,
    /// `replace_host`, `replace_item_fields`, `save_as_copy`, …
    pub intent: String,
    pub connection_id: String,
    pub server_version: String,
    pub endpoint_fingerprint: String,
    pub server_identity: String,
    pub target: TargetFile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceFile>,
    pub source_fingerprint: String,
    pub expected_fingerprint: String,
    /// Create only (required there, absent on an update): `userid` of the
    /// connection account, the expected owner of the new dashboard. With
    /// `connection_id`, `server_version`, the target name and
    /// `expected_fingerprint` (content) it is the durable key of a
    /// read-only reconciliation after a restart (P2-06D).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_userid: Option<String>,
    pub plan_created_at_ms: u64,
    pub confirmed_at_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LineFile {
    pub format: String,
    pub storage_scope: String,
    pub operation_id: String,
    pub seq: u32,
    pub state: String,
    pub at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<OperationFile>,
    /// Fingerprint seen by the read that decided the state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_fingerprint: Option<String>,
    /// The confirmed new dashboard of a create (O-022).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_dashboardid: Option<String>,
    /// Create only, on `failed_after_response`: the `dashboardid` the
    /// server reported but the read-back did not confirm. Never a result;
    /// a later reconciliation reads it by ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_dashboardid: Option<String>,
    /// What the deciding read found (`expected_found`, `source_found`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    /// Local operating-system account that closed the entry manually.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_by: Option<String>,
}

impl LineFile {
    pub(crate) fn transition(
        operation_id: &str,
        seq: usize,
        state: JournalState,
        at_ms: u64,
    ) -> Self {
        Self {
            format: JOURNAL_FORMAT.to_owned(),
            storage_scope: LOCAL_ONLY.to_owned(),
            operation_id: operation_id.to_owned(),
            seq: u32::try_from(seq).unwrap_or(u32::MAX),
            state: state.as_str().to_owned(),
            at_ms,
            operation: None,
            observed_fingerprint: None,
            created_dashboardid: None,
            reported_dashboardid: None,
            evidence: None,
            closed_by: None,
        }
    }

    pub(crate) fn to_bytes(&self) -> Result<Vec<u8>, JournalError> {
        let mut raw = serde_json::to_vec(self).map_err(|_| JournalError::new("journal_corrupt"))?;
        raw.push(b'\n');
        if raw.len() > MAX_JOURNAL_FILE_BYTES {
            return Err(JournalError::new("journal_corrupt").with("reason", "line_too_large"));
        }
        Ok(raw)
    }
}

/// A journal file split into committed lines.
pub(crate) struct Committed {
    pub lines: Vec<LineFile>,
    /// Length of the committed prefix; bytes after it are a torn write.
    pub committed_len: usize,
}

/// Why a file could not be replayed, with the operation record when its
/// first line was still readable (so its target stays blocked).
pub(crate) struct Damage {
    pub error: JournalError,
    pub operation: Option<Box<OperationFile>>,
}

impl Damage {
    fn new(error: JournalError) -> Self {
        Self {
            error,
            operation: None,
        }
    }
}

fn corrupt(reason: &'static str) -> JournalError {
    JournalError::new("journal_corrupt").with("reason", reason)
}

/// Splits `raw` into committed lines. A trailing fragment without a newline
/// is a torn write and is excluded from `committed_len`.
pub(crate) fn split(raw: &[u8]) -> Result<Committed, Damage> {
    let committed_len = raw
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |position| position + 1);
    let mut lines = Vec::new();
    for chunk in raw[..committed_len].split_inclusive(|byte| *byte == b'\n') {
        let body = &chunk[..chunk.len() - 1];
        if lines.len() >= MAX_JOURNAL_LINES {
            return Err(Damage::new(corrupt("too_many_lines")));
        }
        let value: Value = serde_json::from_slice(body).map_err(|_| Damage {
            error: corrupt("line_not_json"),
            operation: lines
                .first()
                .and_then(|line: &LineFile| line.operation.clone())
                .map(Box::new),
        })?;
        let header = || {
            lines
                .first()
                .and_then(|line: &LineFile| line.operation.clone())
                .map(Box::new)
        };
        match value.get("format").and_then(Value::as_str) {
            Some(JOURNAL_FORMAT) => {}
            Some(other) if other.starts_with(JOURNAL_FORMAT_FAMILY) => {
                return Err(Damage {
                    error: JournalError::new("journal_unsupported_version"),
                    operation: header(),
                });
            }
            _ => {
                return Err(Damage {
                    error: corrupt("format"),
                    operation: header(),
                });
            }
        }
        let line: LineFile = serde_json::from_value(value).map_err(|_| Damage {
            error: corrupt("line_shape"),
            operation: header(),
        })?;
        lines.push(line);
    }
    Ok(Committed {
        lines,
        committed_len,
    })
}

fn is_lower_hex_64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_zabbix_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 20 && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Lowercase ASCII code such as `replace_host` or `expected_found`.
pub(crate) fn is_code(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CODE_CHARS
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn is_text(value: &str, max_chars: usize) -> bool {
    let count = value.chars().count();
    (1..=max_chars).contains(&count) && !value.chars().any(char::is_control)
}

pub(crate) fn is_connection_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CONNECTION_ID_BYTES
        && !value.chars().any(char::is_control)
}

pub(crate) fn is_actor(value: &str) -> bool {
    is_text(value, MAX_ACTOR_CHARS)
}

/// Checks the operation record for shape; returns the typed parts.
pub(crate) fn check_operation(
    operation: &OperationFile,
) -> Result<(OperationKind, JournalTarget, Option<JournalSource>), JournalError> {
    let kind = OperationKind::from_code(&operation.kind).ok_or_else(|| corrupt("kind"))?;
    let target = match (
        operation.target.kind.as_str(),
        &operation.target.dashboardid,
        &operation.target.name,
    ) {
        ("existing_dashboard", Some(dashboardid), name) if is_zabbix_id(dashboardid) => {
            if name
                .as_deref()
                .is_some_and(|name| !is_text(name, MAX_NAME_CHARS))
            {
                return Err(corrupt("target"));
            }
            JournalTarget::ExistingDashboard {
                dashboardid: dashboardid.clone(),
                name: name.clone(),
            }
        }
        ("new_dashboard", None, Some(name)) if is_text(name, MAX_NAME_CHARS) => {
            JournalTarget::NewDashboard { name: name.clone() }
        }
        _ => return Err(corrupt("target")),
    };
    // Updates change an existing dashboard, creates make a new one.
    if matches!(
        (kind, &target),
        (OperationKind::Update, JournalTarget::NewDashboard { .. })
            | (
                OperationKind::Create,
                JournalTarget::ExistingDashboard { .. }
            )
    ) {
        return Err(corrupt("target"));
    }
    // The reconciliation key of a create names the expected owner.
    match (kind, operation.owner_userid.as_deref()) {
        (OperationKind::Create, Some(owner)) if is_zabbix_id(owner) => {}
        (OperationKind::Update, None) => {}
        _ => return Err(corrupt("owner_userid")),
    }
    let source = match &operation.source {
        Some(source) if crate::workcopy::is_workcopy_id(&source.local_id) => Some(JournalSource {
            local_id: source.local_id.clone(),
            revision: source.revision,
        }),
        Some(_) => return Err(corrupt("source")),
        None => None,
    };
    if !is_code(&operation.intent)
        || !is_connection_id(&operation.connection_id)
        || operation.server_version.is_empty()
        || operation.server_version.len() > MAX_VERSION_BYTES
        || !is_lower_hex_64(&operation.endpoint_fingerprint)
        || !is_lower_hex_64(&operation.server_identity)
        || !is_lower_hex_64(&operation.source_fingerprint)
        || !is_lower_hex_64(&operation.expected_fingerprint)
    {
        return Err(corrupt("operation"));
    }
    Ok((kind, target, source))
}

/// Replays committed lines into an entry. Every line must be the next
/// allowed transition of the same operation.
pub(crate) fn replay(operation_id: &str, lines: &[LineFile]) -> Result<JournalEntry, Damage> {
    let header = lines
        .first()
        .and_then(|line| line.operation.clone())
        .map(Box::new);
    let fail = |reason| Damage {
        error: corrupt(reason),
        operation: header.clone(),
    };
    let first = lines.first().ok_or_else(|| fail("empty"))?;
    let operation = first.operation.as_ref().ok_or_else(|| fail("operation"))?;
    let (kind, target, source) = check_operation(operation).map_err(|error| Damage {
        error,
        operation: None,
    })?;
    let mut entry = JournalEntry {
        operation_id: operation_id.to_owned(),
        kind,
        intent: operation.intent.clone(),
        connection_id: operation.connection_id.clone(),
        server_version: operation.server_version.clone(),
        target,
        source,
        source_fingerprint: operation.source_fingerprint.clone(),
        expected_fingerprint: operation.expected_fingerprint.clone(),
        owner_userid: operation.owner_userid.clone(),
        plan_created_at_ms: operation.plan_created_at_ms,
        confirmed_at_ms: operation.confirmed_at_ms,
        transitions: Vec::with_capacity(lines.len()),
        state: JournalState::Confirmed,
        sends: 0,
        observed_fingerprint: None,
        created_dashboardid: None,
        reported_dashboardid: None,
        evidence: None,
        closed_by: None,
        progress: super::JournalProgress::Finished,
        endpoint_fingerprint: operation.endpoint_fingerprint.clone(),
        server_identity: operation.server_identity.clone(),
    };
    let mut previous = None;
    for (seq, line) in lines.iter().enumerate() {
        if line.storage_scope != LOCAL_ONLY {
            return Err(fail("storage_scope"));
        }
        if line.operation_id != operation_id {
            return Err(fail("operation_id"));
        }
        if usize::try_from(line.seq).ok() != Some(seq) {
            return Err(fail("seq"));
        }
        if seq > 0 && line.operation.is_some() {
            return Err(fail("operation"));
        }
        let state = JournalState::from_code(&line.state).ok_or_else(|| fail("state"))?;
        if !transition_allowed(previous, state) {
            return Err(fail("transition"));
        }
        check_details(kind, state, line).map_err(&fail)?;
        if state == JournalState::Submitting {
            entry.sends = 1;
        }
        if line.observed_fingerprint.is_some() {
            entry
                .observed_fingerprint
                .clone_from(&line.observed_fingerprint);
        }
        if line.created_dashboardid.is_some() {
            entry
                .created_dashboardid
                .clone_from(&line.created_dashboardid);
        }
        if line.reported_dashboardid.is_some() {
            entry
                .reported_dashboardid
                .clone_from(&line.reported_dashboardid);
        }
        if line.evidence.is_some() {
            entry.evidence.clone_from(&line.evidence);
        }
        if line.closed_by.is_some() {
            entry.closed_by.clone_from(&line.closed_by);
        }
        entry.transitions.push(JournalTransition {
            state,
            at_ms: line.at_ms,
        });
        entry.state = state;
        previous = Some(state);
    }
    Ok(entry)
}

/// Optional fields allowed on a transition line. Checked when replaying a
/// file and before a line is appended.
pub(crate) fn check_details(
    kind: OperationKind,
    state: JournalState,
    line: &LineFile,
) -> Result<(), &'static str> {
    if line
        .observed_fingerprint
        .as_deref()
        .is_some_and(|fingerprint| !is_lower_hex_64(fingerprint))
    {
        return Err("observed_fingerprint");
    }
    if let Some(dashboardid) = &line.created_dashboardid {
        let confirmed = matches!(
            state,
            JournalState::Verified | JournalState::ReconciledVerified
        );
        if kind != OperationKind::Create || !confirmed || !is_zabbix_id(dashboardid) {
            return Err("created_dashboardid");
        }
    }
    if let Some(dashboardid) = &line.reported_dashboardid
        && (kind != OperationKind::Create
            || state != JournalState::FailedAfterResponse
            || !is_zabbix_id(dashboardid))
    {
        return Err("reported_dashboardid");
    }
    if line.evidence.as_deref().is_some_and(|code| !is_code(code)) {
        return Err("evidence");
    }
    match &line.closed_by {
        Some(actor) if state != JournalState::ClosedManually || !is_actor(actor) => {
            Err("closed_by")
        }
        _ => Ok(()),
    }
}
