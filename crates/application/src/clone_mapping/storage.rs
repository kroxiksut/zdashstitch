// SPDX-License-Identifier: MIT OR Apache-2.0

//! Atomic local-only persistence of P3 clone mappings (P3-02D).

use serde::{Deserialize, Serialize};

use super::{
    ClonePatternDecision, ClonePatternLookup, CloneRevalidationContext, SourceHostMapping,
    revalidate_clone_mapping_from_source,
};
use crate::resolver::ResolverLookup;
use crate::workcopy::operations::{
    CloneMappingEntry, OP_CLONE_MAPPING_SET, OP_VERSION_1, Operation,
};
use crate::workcopy::{WorkcopyError, WorkcopyService, is_connection_id};

/// Complete local binding state. It contains server IDs and therefore never
/// enters a portable project or diagnostic export.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CloneMappingState {
    pub mapping_revision: u64,
    pub connection_id: String,
    pub server_identity_fingerprint: String,
    pub server_version: String,
    /// Canonical Base fingerprint at the time of this mapping revision.
    #[serde(default)]
    pub base_fingerprint: Option<String>,
    /// Canonical materialized Local fingerprint supplied by the caller that
    /// built this complete mapping revision.
    #[serde(default)]
    pub local_fingerprint: Option<String>,
    pub mappings: Vec<SourceHostMapping>,
    #[serde(default)]
    pub pattern_decisions: Vec<ClonePatternDecision>,
}

impl CloneMappingState {
    pub(crate) fn is_well_formed(&self) -> bool {
        let mut pattern_fields = std::collections::BTreeSet::new();
        self.mapping_revision > 0
            && is_connection_id(&self.connection_id)
            && crate::workcopy::is_fingerprint(&self.server_identity_fingerprint)
            && self
                .server_version
                .parse::<zdashstitch_zabbix::ZabbixVersion>()
                .is_ok_and(|version| version.is_supported_7_4())
            && self
                .base_fingerprint
                .as_ref()
                .is_none_or(|value| crate::workcopy::is_fingerprint(value))
            && self
                .local_fingerprint
                .as_ref()
                .is_none_or(|value| crate::workcopy::is_fingerprint(value))
            && !self.mappings.is_empty()
            && self.mappings.iter().all(SourceHostMapping::is_well_formed)
            && self.pattern_decisions.iter().all(|decision| {
                decision.is_well_formed() && pattern_fields.insert(&decision.requirement.field)
            })
    }
}

/// Input for one atomic mapping revision. The service assigns the next
/// `mapping_revision`; callers cannot reuse an old one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveCloneMappingRequest {
    pub expected_workcopy_revision: u64,
    pub connection_id: String,
    pub server_identity_fingerprint: String,
    pub server_version: String,
    pub local_fingerprint: Option<String>,
    pub mappings: Vec<SourceHostMapping>,
    pub pattern_decisions: Vec<ClonePatternDecision>,
}

impl SaveCloneMappingRequest {
    /// Reapply a state returned by undo. The service assigns a fresh
    /// operation sequence and the next mapping revision.
    #[must_use]
    pub fn redo(expected_workcopy_revision: u64, removed: &CloneMappingState) -> Self {
        Self {
            expected_workcopy_revision,
            connection_id: removed.connection_id.clone(),
            server_identity_fingerprint: removed.server_identity_fingerprint.clone(),
            server_version: removed.server_version.clone(),
            local_fingerprint: removed.local_fingerprint.clone(),
            mappings: removed.mappings.clone(),
            pattern_decisions: removed.pattern_decisions.clone(),
        }
    }
}

/// Latest stored mapping and the workcopy revision that contains it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloneMappingRecord {
    pub operation_seq: u64,
    pub workcopy_revision: u64,
    pub recorded_at_ms: u64,
    pub state: CloneMappingState,
}

/// State removed by undo. [`SaveCloneMappingRequest::redo`] reapplies it as
/// a new operation and therefore invalidates any older plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemovedCloneMapping {
    pub removed: CloneMappingState,
    pub workcopy_revision: u64,
    pub current: Option<CloneMappingRecord>,
}

fn latest(operations: &[Operation], workcopy_revision: u64) -> Option<CloneMappingRecord> {
    operations
        .iter()
        .rev()
        .find_map(|operation| match operation {
            Operation::CloneMapping(entry) => Some(CloneMappingRecord {
                operation_seq: entry.seq,
                workcopy_revision,
                recorded_at_ms: entry.recorded_at_ms,
                state: entry.state.clone(),
            }),
            _ => None,
        })
}

impl WorkcopyService {
    /// Atomically appends a complete mapping revision. Errors include
    /// `workcopy_changed_concurrently` and `clone_mapping_invalid`.
    pub fn save_clone_mapping(
        &self,
        local_id: &str,
        request: SaveCloneMappingRequest,
    ) -> Result<CloneMappingRecord, WorkcopyError> {
        let (file, base, mut operations) = self.load_log(local_id)?;
        if file.revision != request.expected_workcopy_revision {
            return Err(WorkcopyError::new("workcopy_changed_concurrently"));
        }
        // The workcopy revision never goes backwards, including after undo,
        // so a mapping revision is never reused by a later redo.
        let mapping_revision = file.revision.saturating_add(1);
        let state = CloneMappingState {
            mapping_revision,
            connection_id: request.connection_id,
            server_identity_fingerprint: request.server_identity_fingerprint,
            server_version: request.server_version,
            base_fingerprint: Some(base.fingerprint().to_owned()),
            local_fingerprint: request.local_fingerprint,
            mappings: request.mappings,
            pattern_decisions: request.pattern_decisions,
        };
        if !state.is_well_formed() {
            return Err(WorkcopyError::new("clone_mapping_invalid"));
        }
        let seq = operations
            .last()
            .map_or(1, |operation| operation.seq().saturating_add(1));
        let recorded_at_ms = crate::workcopy::now_ms();
        operations.push(Operation::CloneMapping(CloneMappingEntry {
            seq,
            op: OP_CLONE_MAPPING_SET.to_owned(),
            op_version: OP_VERSION_1,
            recorded_at_ms,
            state: state.clone(),
        }));
        let committed = self.commit_operations(local_id, file.revision, &operations)?;
        Ok(CloneMappingRecord {
            operation_seq: seq,
            workcopy_revision: committed.revision,
            recorded_at_ms,
            state,
        })
    }

    /// Latest mapping after reopening the workcopy.
    pub fn clone_mapping(
        &self,
        local_id: &str,
    ) -> Result<Option<CloneMappingRecord>, WorkcopyError> {
        let (file, _, operations) = self.load_log(local_id)?;
        Ok(latest(&operations, file.revision))
    }

    /// P3-02G: read fresh server facts, retain every stored choice, mark
    /// changed bindings stale/invalid and atomically append a new mapping
    /// revision. This accepts read-only lookups and creates no write plan.
    pub async fn revalidate_clone_mapping<L: ResolverLookup + ClonePatternLookup>(
        &self,
        lookup: &L,
        local_id: &str,
        context: &CloneRevalidationContext,
    ) -> Result<CloneMappingRecord, WorkcopyError> {
        let (file, base, operations) = self.load_log(local_id)?;
        let stored = latest(&operations, file.revision)
            .ok_or_else(|| WorkcopyError::new("clone_mapping_not_found"))?;
        let source = crate::replace_host::materialize_local_operations(base.clone(), &operations)?;
        let fresh =
            revalidate_clone_mapping_from_source(lookup, &base, &source, &stored.state, context)
                .await
                .map_err(|error| WorkcopyError::new(error.code()))?;
        self.save_clone_mapping(
            local_id,
            SaveCloneMappingRequest {
                expected_workcopy_revision: file.revision,
                connection_id: fresh.connection_id,
                server_identity_fingerprint: fresh.server_identity_fingerprint,
                server_version: fresh.server_version,
                local_fingerprint: Some(context.local_fingerprint.clone()),
                mappings: fresh.mappings,
                pattern_decisions: fresh.pattern_decisions,
            },
        )
    }

    /// Removes only the latest mapping revision. Older mapping revisions and
    /// unrelated operations stay in order, so the previous state is restored.
    pub fn undo_clone_mapping(
        &self,
        local_id: &str,
        expected_workcopy_revision: u64,
    ) -> Result<RemovedCloneMapping, WorkcopyError> {
        let (file, _, mut operations) = self.load_log(local_id)?;
        if file.revision != expected_workcopy_revision {
            return Err(WorkcopyError::new("workcopy_changed_concurrently"));
        }
        let position = operations
            .iter()
            .rposition(|operation| matches!(operation, Operation::CloneMapping(_)))
            .ok_or_else(|| WorkcopyError::new("clone_mapping_not_found"))?;
        let Operation::CloneMapping(removed) = operations.remove(position) else {
            unreachable!("position selected a clone mapping")
        };
        let committed = self.commit_operations(local_id, file.revision, &operations)?;
        Ok(RemovedCloneMapping {
            removed: removed.state,
            workcopy_revision: committed.revision,
            current: latest(&operations, committed.revision),
        })
    }
}
