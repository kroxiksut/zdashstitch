// SPDX-License-Identifier: MIT OR Apache-2.0

//! Typed entries of the workcopy operation log (O-018 §3, P2-03A).
//!
//! Every entry is a JSON object `{"seq", "op", "op_version",
//! "recorded_at_ms", ...}`; `seq` grows strictly. This build knows:
//!
//! - `replace_host` v1 — intent (source host, target host, scope) and the
//!   frozen read-only calculation;
//! - `replace_host_decision` v1 — a user decision on one row of a
//!   `replace_host` entry (`replace_seq`); the latest one per row wins;
//! - `replace_host_confirm_kept` v1 — confirmation of the list of
//!   references that stay on the source host, bound by digest.
//! - `clone_mapping_set` v1 — complete local-only P3 mapping revision.
//!
//! An entry with another name or version rejects the whole workcopy with
//! `workcopy_unsupported_operation` (params `op`, `op_version`); a known
//! entry that does not parse or breaks the log rules is `workcopy_corrupt`.
//! Loading never rewrites the file.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zdashstitch_zabbix::is_zabbix_id;

use super::{WorkcopyError, is_fingerprint};
use crate::clone_mapping::CloneMappingState;
use crate::replace_host::{
    Calculation, FieldKey, FieldValue, ItemFacts, ReplaceScope, RowDecision,
};

pub(crate) const OP_REPLACE_HOST: &str = "replace_host";
pub(crate) const OP_REPLACE_HOST_DECISION: &str = "replace_host_decision";
pub(crate) const OP_REPLACE_HOST_CONFIRM_KEPT: &str = "replace_host_confirm_kept";
pub(crate) const OP_CLONE_MAPPING_SET: &str = "clone_mapping_set";
pub(crate) const OP_VERSION_1: u32 = 1;

/// `replace_host` v1.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReplaceHostEntry {
    pub seq: u64,
    pub op: String,
    pub op_version: u32,
    pub recorded_at_ms: u64,
    pub source_hostid: String,
    pub target_hostid: String,
    pub scope: ReplaceScope,
    pub calculation: Calculation,
}

/// `replace_host_decision` v1.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionEntry {
    pub seq: u64,
    pub op: String,
    pub op_version: u32,
    pub recorded_at_ms: u64,
    pub replace_seq: u64,
    pub key: FieldKey,
    /// Field value the decision was made for.
    pub source: FieldValue,
    pub decision: RowDecision,
    /// Evidence of the chosen item, copied from the offered candidates, so
    /// Local is materialized without the server.
    pub chosen: Option<ItemFacts>,
}

/// `replace_host_confirm_kept` v1.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfirmKeptEntry {
    pub seq: u64,
    pub op: String,
    pub op_version: u32,
    pub recorded_at_ms: u64,
    pub replace_seq: u64,
    /// SHA-256 (hex) of the confirmed list.
    pub kept_digest: String,
    pub kept_count: u32,
}

/// `clone_mapping_set` v1.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CloneMappingEntry {
    pub seq: u64,
    pub op: String,
    pub op_version: u32,
    pub recorded_at_ms: u64,
    pub state: CloneMappingState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Operation {
    ReplaceHost(ReplaceHostEntry),
    Decision(DecisionEntry),
    ConfirmKept(ConfirmKeptEntry),
    CloneMapping(CloneMappingEntry),
}

impl Operation {
    pub(crate) const fn seq(&self) -> u64 {
        match self {
            Self::ReplaceHost(entry) => entry.seq,
            Self::Decision(entry) => entry.seq,
            Self::ConfirmKept(entry) => entry.seq,
            Self::CloneMapping(entry) => entry.seq,
        }
    }

    pub(crate) const fn name(&self) -> &'static str {
        match self {
            Self::ReplaceHost(_) => OP_REPLACE_HOST,
            Self::Decision(_) => OP_REPLACE_HOST_DECISION,
            Self::ConfirmKept(_) => OP_REPLACE_HOST_CONFIRM_KEPT,
            Self::CloneMapping(_) => OP_CLONE_MAPPING_SET,
        }
    }

    pub(crate) const fn recorded_at_ms(&self) -> u64 {
        match self {
            Self::ReplaceHost(entry) => entry.recorded_at_ms,
            Self::Decision(entry) => entry.recorded_at_ms,
            Self::ConfirmKept(entry) => entry.recorded_at_ms,
            Self::CloneMapping(entry) => entry.recorded_at_ms,
        }
    }

    /// The `replace_host` entry this entry depends on.
    pub(crate) const fn replace_seq(&self) -> Option<u64> {
        match self {
            Self::ReplaceHost(_) => None,
            Self::Decision(entry) => Some(entry.replace_seq),
            Self::ConfirmKept(entry) => Some(entry.replace_seq),
            Self::CloneMapping(_) => None,
        }
    }

    pub(crate) fn to_value(&self) -> Result<Value, WorkcopyError> {
        let value = match self {
            Self::ReplaceHost(entry) => serde_json::to_value(entry),
            Self::Decision(entry) => serde_json::to_value(entry),
            Self::ConfirmKept(entry) => serde_json::to_value(entry),
            Self::CloneMapping(entry) => serde_json::to_value(entry),
        };
        value.map_err(|_| WorkcopyError::new("workcopy_corrupt"))
    }
}

fn unsupported(entry: &Value) -> WorkcopyError {
    let name = entry
        .get("op")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(32)
        .collect::<String>();
    let version = entry
        .get("op_version")
        .and_then(Value::as_u64)
        .map(|version| version.to_string())
        .unwrap_or_default();
    WorkcopyError::new("workcopy_unsupported_operation")
        .with("op", name)
        .with("op_version", version)
}

fn corrupt(seq: u64) -> WorkcopyError {
    WorkcopyError::new("workcopy_corrupt").with("seq", seq.to_string())
}

fn parse_entry(entry: &Value) -> Result<Operation, WorkcopyError> {
    let op = entry.get("op").and_then(Value::as_str);
    let version = entry.get("op_version").and_then(Value::as_u64);
    let seq = entry.get("seq").and_then(Value::as_u64).unwrap_or(0);
    let parsed = match (op, version) {
        (Some(OP_REPLACE_HOST), Some(1)) => serde_json::from_value(entry.clone())
            .map(Operation::ReplaceHost)
            .ok(),
        (Some(OP_REPLACE_HOST_DECISION), Some(1)) => serde_json::from_value(entry.clone())
            .map(Operation::Decision)
            .ok(),
        (Some(OP_REPLACE_HOST_CONFIRM_KEPT), Some(1)) => serde_json::from_value(entry.clone())
            .map(Operation::ConfirmKept)
            .ok(),
        (Some(OP_CLONE_MAPPING_SET), Some(1)) => serde_json::from_value(entry.clone())
            .map(Operation::CloneMapping)
            .ok(),
        _ => return Err(unsupported(entry)),
    };
    parsed.ok_or_else(|| corrupt(seq))
}

/// Parses and checks the whole log against Base. Pure; never repairs.
///
/// Rules: `seq` ≥ 1 and strictly increasing; at most one `replace_host`
/// (P2-03A v1); its calculation was made on this Base and names the same
/// hosts; decisions and confirmations follow the `replace_host` entry they
/// reference.
pub(crate) fn parse_log(
    entries: &[Value],
    base_fingerprint: &str,
) -> Result<Vec<Operation>, WorkcopyError> {
    let mut operations: Vec<Operation> = Vec::with_capacity(entries.len());
    for entry in entries {
        operations.push(parse_entry(entry)?);
    }
    let mut previous = 0u64;
    let mut replace_seq: Option<u64> = None;
    let mut mapping_revision = 0u64;
    for operation in &operations {
        let seq = operation.seq();
        if seq <= previous {
            return Err(corrupt(seq));
        }
        previous = seq;
        let well_formed = match operation {
            Operation::ReplaceHost(entry) => {
                let first = replace_seq.is_none();
                replace_seq = Some(seq);
                first
                    && is_zabbix_id(&entry.source_hostid)
                    && is_zabbix_id(&entry.target_hostid)
                    && entry.source_hostid != entry.target_hostid
                    && entry.scope.is_well_formed()
                    && entry.calculation.base_fingerprint == base_fingerprint
                    && entry
                        .calculation
                        .is_well_formed(&entry.source_hostid, &entry.target_hostid)
            }
            Operation::Decision(entry) => {
                replace_seq == Some(entry.replace_seq)
                    && entry.decision.itemid().is_none_or(is_zabbix_id)
                    && entry.chosen.as_ref().map(|chosen| chosen.itemid.as_str())
                        == entry.decision.itemid()
            }
            Operation::ConfirmKept(entry) => {
                replace_seq == Some(entry.replace_seq) && is_fingerprint(&entry.kept_digest)
            }
            Operation::CloneMapping(entry) => {
                let valid = entry.op == OP_CLONE_MAPPING_SET
                    && entry.op_version == OP_VERSION_1
                    && entry.state.mapping_revision > mapping_revision
                    && entry.state.is_well_formed();
                if valid {
                    mapping_revision = entry.state.mapping_revision;
                }
                valid
            }
        };
        if !well_formed {
            return Err(corrupt(seq));
        }
    }
    Ok(operations)
}

/// Serializes typed operations back into log entries.
pub(crate) fn to_log(operations: &[Operation]) -> Result<Vec<Value>, WorkcopyError> {
    operations.iter().map(Operation::to_value).collect()
}
