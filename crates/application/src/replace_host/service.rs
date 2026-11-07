// SPDX-License-Identifier: MIT OR Apache-2.0

//! `WorkcopyService` use cases of ReplaceHost: calculate (read-only),
//! revalidate (read-only, no write), decide, confirm the kept list, undo,
//! preview and the materialized Local.

use std::collections::BTreeSet;

use serde_json::Value;
use zdashstitch_zabbix::{VersionedDashboardSnapshot, is_zabbix_id};

use super::diff::{StructuralDiff, structural_diff};
use super::model::{Calculation, FieldKey, HostRef, ReplaceScope, RowDecision};
use super::report::{ReportInput, SessionContext, report};
use super::state::{
    FieldChange, Freshness, KeptList, ReplaceHostLog, RowView, added_references, apply_changes,
    changes, decision_error, kept_list, rows,
};
use crate::publisher::CompatibilityReport;
use crate::resolver::{
    ReplaceHostRequest, ResolveScope, ResolverLookup, resolve_replace_host_with, source_model,
};
use crate::workcopy::formats::WorkcopyFile;
use crate::workcopy::operations::{
    ConfirmKeptEntry, DecisionEntry, OP_REPLACE_HOST, OP_REPLACE_HOST_CONFIRM_KEPT,
    OP_REPLACE_HOST_DECISION, OP_VERSION_1, Operation, ReplaceHostEntry, parse_log, to_log,
};
use crate::workcopy::{DraftKind, WorkcopyError, WorkcopyService, is_workcopy_id};

/// The `replace_host` entry as the screens show it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaceHostOperation {
    pub seq: u64,
    pub recorded_at_ms: u64,
    pub source_host: HostRef,
    /// `None`: the target host was missing or hidden at calculation time.
    pub target_host: Option<HostRef>,
    /// Effective scope (a page draft is limited to its page).
    pub scope: ReplaceScope,
    pub calculated_at_ms: u64,
    pub server_version: String,
}

/// Everything P2-03D/E and P2-04 need about the operation of one draft.
/// Built offline from the stored log; `checked_at_ms` is set when the call
/// read fresh facts from the server.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaceHostPreview {
    pub local_id: String,
    /// Workcopy revision the preview was built from; any later change of
    /// the draft makes a plan built from this preview invalid.
    pub revision: u64,
    pub kind: DraftKind,
    pub operation: ReplaceHostOperation,
    /// All references of the source host in scope, Base order (09-03).
    pub rows: Vec<RowView>,
    /// «База → черновик» by page, widget and series (P1-08B).
    pub diff: StructuralDiff,
    /// «Останутся на старом узле» with its confirmation state.
    pub kept_on_source: KeptList,
    /// Field writes of Local, Base order.
    pub changes: Vec<FieldChange>,
    pub base_fingerprint: String,
    /// Fingerprint of the materialized Local.
    pub local_fingerprint: String,
    pub checked_at_ms: Option<u64>,
    /// Blockers and warnings of publication; never blocks this preview.
    pub report: CompatibilityReport,
}

/// One entry of the operation log (history and undo).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationRecord {
    pub seq: u64,
    /// `replace_host`, `replace_host_decision`, `replace_host_confirm_kept`.
    pub op: &'static str,
    pub op_version: u32,
    pub recorded_at_ms: u64,
    /// The `replace_host` entry a decision or confirmation belongs to.
    pub replace_seq: Option<u64>,
}

/// Page selected by a page draft: its index and ID in Base.
pub(super) fn draft_page(
    file: &WorkcopyFile,
    raw: &Value,
) -> Result<Option<(usize, String)>, WorkcopyError> {
    let Some(selector) = &file.source.page else {
        return Ok(None);
    };
    let index = raw
        .get("pages")
        .and_then(Value::as_array)
        .and_then(|pages| {
            pages.iter().position(|page| {
                page.get("dashboard_pageid").and_then(Value::as_str)
                    == Some(selector.dashboard_pageid.as_str())
            })
        })
        .ok_or_else(|| WorkcopyError::new("workcopy_corrupt"))?;
    Ok(Some((index, selector.dashboard_pageid.clone())))
}

/// `(dashboard_pageid, widgetids)` of every page of Base.
fn base_ids(raw: &Value) -> Vec<(Option<String>, BTreeSet<String>)> {
    raw.get("pages")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|page| {
            let widgets = page
                .get("widgets")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|widget| widget.get("widgetid").and_then(Value::as_str))
                .map(str::to_owned)
                .collect();
            (
                page.get("dashboard_pageid")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                widgets,
            )
        })
        .collect()
}

/// The scope the operation really covers: explicit, existing in Base and,
/// for a page draft, inside the draft page.
fn effective_scope(
    requested: &ResolveScope,
    raw: &Value,
    page: Option<&(usize, String)>,
) -> Result<ResolveScope, WorkcopyError> {
    let invalid = || WorkcopyError::new("replace_host_scope_invalid");
    let outside = || WorkcopyError::new("replace_host_scope_outside_draft");
    let pages = base_ids(raw);
    match requested {
        ResolveScope::Dashboard => Ok(page.map_or(ResolveScope::Dashboard, |(_, pageid)| {
            ResolveScope::Pages(BTreeSet::from([pageid.clone()]))
        })),
        ResolveScope::Pages(ids) => {
            if ids.is_empty() || !ids.iter().all(|id| is_zabbix_id(id)) {
                return Err(invalid());
            }
            for id in ids {
                if !pages.iter().any(|(pageid, _)| pageid.as_ref() == Some(id)) {
                    return Err(invalid());
                }
                if page.is_some_and(|(_, draft)| draft != id) {
                    return Err(outside());
                }
            }
            Ok(requested.clone())
        }
        ResolveScope::Widgets(ids) => {
            if ids.is_empty() || !ids.iter().all(|id| is_zabbix_id(id)) {
                return Err(invalid());
            }
            for id in ids {
                let position = pages
                    .iter()
                    .position(|(_, widgets)| widgets.contains(id))
                    .ok_or_else(invalid)?;
                if page.is_some_and(|(index, _)| *index != position) {
                    return Err(outside());
                }
            }
            Ok(requested.clone())
        }
    }
}

fn check_hosts(request: &ReplaceHostRequest) -> Result<(), WorkcopyError> {
    if !is_zabbix_id(&request.source_hostid) || !is_zabbix_id(&request.target_hostid) {
        return Err(WorkcopyError::new("resolver_hostid_invalid"));
    }
    if request.source_hostid == request.target_hostid {
        return Err(WorkcopyError::new("replace_host_same_host"));
    }
    Ok(())
}

fn next_seq(operations: &[Operation]) -> u64 {
    operations
        .last()
        .map_or(1, |operation| operation.seq().saturating_add(1))
}

fn not_calculated() -> WorkcopyError {
    WorkcopyError::new("replace_host_not_calculated")
}

/// Materializes Local from Base and a parsed log. Pure.
fn materialize(
    base: &VersionedDashboardSnapshot,
    changes: &[FieldChange],
) -> Result<VersionedDashboardSnapshot, WorkcopyError> {
    let raw = apply_changes(base.raw_api_document(), changes)?;
    VersionedDashboardSnapshot::from_adapter_raw(
        base.connection_id().to_owned(),
        base.server_version().clone(),
        raw,
    )
    .map_err(|_| WorkcopyError::new("local_materialization_failed"))
}

struct Fresh<'a> {
    calculation: &'a Calculation,
    checked_at_ms: u64,
}

impl WorkcopyService {
    fn build_preview(
        &self,
        file: &WorkcopyFile,
        base: &VersionedDashboardSnapshot,
        operations: &[Operation],
        session: SessionContext<'_>,
        fresh: Option<Fresh<'_>>,
    ) -> Result<ReplaceHostPreview, WorkcopyError> {
        let log = ReplaceHostLog::of(operations).ok_or_else(not_calculated)?;
        let raw = base.raw_api_document();
        let page = draft_page(file, raw)?;
        let session_version = session.server_version();
        let freshness = Freshness {
            server_version: session_version.as_deref(),
            fresh: fresh.as_ref().map(|fresh| fresh.calculation),
        };
        let rows = rows(&log, &freshness);
        let kept = kept_list(&log, &rows)?;
        let changes = changes(&rows);
        let local = materialize(base, &changes)?;
        let model = source_model(base, &self.registry)
            .map_err(|error| WorkcopyError::from_resolver(&error))?;
        let scope = log.entry.scope.to_resolve();
        let diff = structural_diff(&model, raw, &rows, &scope, page.as_ref().map(|page| page.0));
        let added = fresh
            .as_ref()
            .map(|fresh| added_references(&log.entry.calculation, fresh.calculation))
            .unwrap_or_default();
        let base_version = base.server_version().to_string();
        let report = report(&ReportInput {
            log: &log,
            rows: &rows,
            kept: &kept,
            changes: &changes,
            base_version: &base_version,
            session,
            added: &added,
        });
        let entry = log.entry;
        Ok(ReplaceHostPreview {
            local_id: file.workcopy_id.clone(),
            revision: file.revision,
            kind: file.kind,
            operation: ReplaceHostOperation {
                seq: entry.seq,
                recorded_at_ms: entry.recorded_at_ms,
                source_host: entry.calculation.source_host.clone(),
                target_host: entry.calculation.target_host.clone(),
                scope: entry.scope.clone(),
                calculated_at_ms: entry.calculation.calculated_at_ms,
                server_version: entry.calculation.server_version.clone(),
            },
            rows,
            diff,
            kept_on_source: kept,
            changes,
            base_fingerprint: base.fingerprint().to_owned(),
            local_fingerprint: local.fingerprint().to_owned(),
            checked_at_ms: fresh.map(|fresh| fresh.checked_at_ms),
            report,
        })
    }

    /// Writes a new operation list after checking that the draft did not
    /// change since `revision` was read.
    pub(crate) fn commit_operations(
        &self,
        local_id: &str,
        revision: u64,
        operations: &[Operation],
    ) -> Result<WorkcopyFile, WorkcopyError> {
        let mut file = self.load(local_id)?;
        if file.revision != revision {
            return Err(WorkcopyError::new("workcopy_changed_concurrently"));
        }
        file.operations = to_log(operations)?;
        parse_log(&file.operations, &file.base.fingerprint)?;
        file.revision = file.revision.saturating_add(1);
        file.updated_at_ms = crate::workcopy::now_ms().max(file.updated_at_ms);
        self.store.replace_workcopy(&file)?;
        Ok(file)
    }

    pub(crate) fn load_log(
        &self,
        local_id: &str,
    ) -> Result<(WorkcopyFile, VersionedDashboardSnapshot, Vec<Operation>), WorkcopyError> {
        if !is_workcopy_id(local_id) {
            return Err(WorkcopyError::new("workcopy_id_invalid"));
        }
        let (file, base) = self.load_with_base(local_id)?;
        let operations = parse_log(&file.operations, &file.base.fingerprint)?;
        Ok((file, base, operations))
    }

    async fn calculate<L: ResolverLookup>(
        &self,
        lookup: &L,
        base: &VersionedDashboardSnapshot,
        request: &ReplaceHostRequest,
    ) -> Result<Calculation, WorkcopyError> {
        let model = source_model(base, &self.registry)
            .map_err(|error| WorkcopyError::from_resolver(&error))?;
        let resolution = resolve_replace_host_with(lookup, &model, request)
            .await
            .map_err(|error| WorkcopyError::from_resolver(&error))?;
        Ok(Calculation::from_resolution(
            &resolution,
            &base.server_version().to_string(),
            crate::workcopy::now_ms(),
        ))
    }

    /// P2-03A: calculates ReplaceHost over Base with read-only lookups and
    /// stores it as the `replace_host` operation of the draft. Works in a
    /// `read_only` session; builds no write plan.
    ///
    /// An existing operation with the same hosts and scope is recalculated
    /// in place (decisions stay and are re-evaluated, never re-chosen); one
    /// with other hosts or scope must be removed first
    /// (`replace_host_pending`, param `seq`).
    ///
    /// Errors: `workcopy_id_invalid`, `resolver_hostid_invalid`,
    /// `replace_host_same_host`, `replace_host_scope_invalid`,
    /// `replace_host_scope_outside_draft`, resolver and Zabbix read codes,
    /// `workcopy_changed_concurrently`, storage codes.
    ///
    /// Only read-only lookups are accepted:
    ///
    /// ```compile_fail
    /// async fn no_write_gateway(
    ///     service: &zdashstitch_application::workcopy::WorkcopyService,
    ///     writer: &zdashstitch_zabbix::ReadWriteGateway,
    ///     request: &zdashstitch_application::resolver::ReplaceHostRequest,
    /// ) {
    ///     let _ = service
    ///         .calculate_replace_host(
    ///             writer,
    ///             "wc-0",
    ///             request,
    ///             zdashstitch_application::replace_host::SessionContext::Offline,
    ///         )
    ///         .await;
    /// }
    /// ```
    ///
    /// Control: the same call compiles with the read-only gateway.
    ///
    /// ```no_run
    /// async fn read_gateway(
    ///     service: &zdashstitch_application::workcopy::WorkcopyService,
    ///     reader: &zdashstitch_zabbix::ReadOnlyGateway,
    ///     request: &zdashstitch_application::resolver::ReplaceHostRequest,
    /// ) {
    ///     let _ = service
    ///         .calculate_replace_host(
    ///             reader,
    ///             "wc-0",
    ///             request,
    ///             zdashstitch_application::replace_host::SessionContext::Offline,
    ///         )
    ///         .await;
    /// }
    /// ```
    pub async fn calculate_replace_host<L: ResolverLookup>(
        &self,
        lookup: &L,
        local_id: &str,
        request: &ReplaceHostRequest,
        session: SessionContext<'_>,
    ) -> Result<ReplaceHostPreview, WorkcopyError> {
        check_hosts(request)?;
        let (file, base, mut operations) = self.load_log(local_id)?;
        let page = draft_page(&file, base.raw_api_document())?;
        let scope = effective_scope(&request.scope, base.raw_api_document(), page.as_ref())?;
        let stored_scope = ReplaceScope::from_resolve(&scope);
        let existing = operations
            .iter()
            .position(|operation| matches!(operation, Operation::ReplaceHost(_)));
        if let Some(position) = existing
            && let Operation::ReplaceHost(entry) = &operations[position]
            && (entry.source_hostid != request.source_hostid
                || entry.target_hostid != request.target_hostid
                || entry.scope != stored_scope)
        {
            return Err(
                WorkcopyError::new("replace_host_pending").with("seq", entry.seq.to_string())
            );
        }
        let effective = ReplaceHostRequest {
            source_hostid: request.source_hostid.clone(),
            target_hostid: request.target_hostid.clone(),
            scope,
        };
        let calculation = self.calculate(lookup, &base, &effective).await?;
        match existing.map(|position| &mut operations[position]) {
            Some(Operation::ReplaceHost(entry)) => entry.calculation = calculation,
            _ => {
                let seq = next_seq(&operations);
                operations.push(Operation::ReplaceHost(ReplaceHostEntry {
                    seq,
                    op: OP_REPLACE_HOST.to_owned(),
                    op_version: OP_VERSION_1,
                    recorded_at_ms: crate::workcopy::now_ms(),
                    source_hostid: effective.source_hostid,
                    target_hostid: effective.target_hostid,
                    scope: stored_scope,
                    calculation,
                }));
            }
        }
        let file = self.commit_operations(local_id, file.revision, &operations)?;
        self.build_preview(&file, &base, &operations, session, None)
    }

    /// P2-03 revalidation («Проверить все»): reads fresh facts with the
    /// stored intent and marks rows `stale`/`invalid`. Writes nothing and
    /// never picks a new target or candidate; recalculating is a separate,
    /// explicit [`Self::calculate_replace_host`].
    pub async fn revalidate_replace_host<L: ResolverLookup>(
        &self,
        lookup: &L,
        local_id: &str,
        session: SessionContext<'_>,
    ) -> Result<ReplaceHostPreview, WorkcopyError> {
        let (file, base, operations) = self.load_log(local_id)?;
        let entry = ReplaceHostLog::of(&operations)
            .ok_or_else(not_calculated)?
            .entry;
        let request = ReplaceHostRequest {
            source_hostid: entry.source_hostid.clone(),
            target_hostid: entry.target_hostid.clone(),
            scope: entry.scope.to_resolve(),
        };
        let fresh = self.calculate(lookup, &base, &request).await?;
        self.build_preview(
            &file,
            &base,
            &operations,
            session,
            Some(Fresh {
                checked_at_ms: fresh.calculated_at_ms,
                calculation: &fresh,
            }),
        )
    }

    /// Preview of the stored operation, offline. Errors:
    /// `replace_host_not_calculated` when the draft has no operation.
    pub fn replace_host_preview(
        &self,
        local_id: &str,
        session: SessionContext<'_>,
    ) -> Result<ReplaceHostPreview, WorkcopyError> {
        let (file, base, operations) = self.load_log(local_id)?;
        self.build_preview(&file, &base, &operations, session, None)
    }

    /// P1-05H `bindings.decide`: records a decision on one row as a
    /// `replace_host_decision` operation; the latest decision per row wins
    /// and removing it (undo) restores the previous one.
    ///
    /// Errors: `replace_host_not_calculated`, `binding_not_found`,
    /// `decision_not_applicable` (row not editable, wrong status, or a
    /// choice on an unverified reference), `candidate_not_visible` (item not
    /// among the offered candidates), `candidate_value_type_incompatible`
    /// (non-numeric item for `svggraph`).
    pub fn decide_replace_host(
        &self,
        local_id: &str,
        key: &FieldKey,
        decision: &RowDecision,
        session: SessionContext<'_>,
    ) -> Result<ReplaceHostPreview, WorkcopyError> {
        let (file, base, mut operations) = self.load_log(local_id)?;
        let entry = ReplaceHostLog::of(&operations)
            .ok_or_else(not_calculated)?
            .entry
            .clone();
        let calculation = &entry.calculation;
        let (source, chosen) = if let Some(binding) = calculation
            .bindings
            .iter()
            .find(|binding| &binding.key() == key)
        {
            if let Some(code) = decision_error(binding, decision) {
                return Err(WorkcopyError::new(code));
            }
            let chosen = decision.itemid().and_then(|itemid| {
                binding
                    .candidates
                    .iter()
                    .find(|candidate| candidate.itemid == itemid)
                    .cloned()
            });
            (binding.source.clone(), chosen)
        } else if let Some(reference) = calculation
            .unverified
            .iter()
            .find(|reference| &reference.key() == key)
        {
            if *decision != RowDecision::KeepOnSource {
                return Err(WorkcopyError::new("decision_not_applicable"));
            }
            (reference.source.clone(), None)
        } else {
            return Err(WorkcopyError::new("binding_not_found"));
        };
        operations.push(Operation::Decision(DecisionEntry {
            seq: next_seq(&operations),
            op: OP_REPLACE_HOST_DECISION.to_owned(),
            op_version: OP_VERSION_1,
            recorded_at_ms: crate::workcopy::now_ms(),
            replace_seq: entry.seq,
            key: key.clone(),
            source,
            decision: decision.clone(),
            chosen,
        }));
        let file = self.commit_operations(local_id, file.revision, &operations)?;
        self.build_preview(&file, &base, &operations, session, None)
    }

    /// P1-05H `bindings.confirm_kept` / P1-08C «Подтвердить список»:
    /// confirms exactly the current kept list, identified by the digest the
    /// preview returned. A later change of the list makes the confirmation
    /// stale. Errors: `kept_on_source_empty`, `kept_on_source_list_changed`.
    pub fn confirm_kept_on_source(
        &self,
        local_id: &str,
        kept_digest: &str,
        session: SessionContext<'_>,
    ) -> Result<ReplaceHostPreview, WorkcopyError> {
        let (file, base, mut operations) = self.load_log(local_id)?;
        let log = ReplaceHostLog::of(&operations).ok_or_else(not_calculated)?;
        let current = rows(
            &log,
            &Freshness {
                server_version: None,
                fresh: None,
            },
        );
        let kept = kept_list(&log, &current)?;
        if kept.entries.is_empty() {
            return Err(WorkcopyError::new("kept_on_source_empty"));
        }
        if kept.digest != kept_digest {
            return Err(WorkcopyError::new("kept_on_source_list_changed"));
        }
        if kept.confirmed {
            return self.build_preview(&file, &base, &operations, session, None);
        }
        let replace_seq = log.entry.seq;
        operations.push(Operation::ConfirmKept(ConfirmKeptEntry {
            seq: next_seq(&operations),
            op: OP_REPLACE_HOST_CONFIRM_KEPT.to_owned(),
            op_version: OP_VERSION_1,
            recorded_at_ms: crate::workcopy::now_ms(),
            replace_seq,
            kept_digest: kept.digest,
            kept_count: u32::try_from(kept.entries.len()).unwrap_or(u32::MAX),
        }));
        let file = self.commit_operations(local_id, file.revision, &operations)?;
        self.build_preview(&file, &base, &operations, session, None)
    }

    /// Undo: removes the operation `seq`. Removing `replace_host` also
    /// removes its decisions and confirmations («Сбросить все привязки»).
    /// Returns the removed `seq`s. Errors: `operation_not_found`.
    pub fn remove_operation(&self, local_id: &str, seq: u64) -> Result<Vec<u64>, WorkcopyError> {
        let (file, _, operations) = self.load_log(local_id)?;
        let target = operations
            .iter()
            .find(|operation| operation.seq() == seq)
            .ok_or_else(|| {
                WorkcopyError::new("operation_not_found").with("seq", seq.to_string())
            })?;
        let cascade = matches!(target, Operation::ReplaceHost(_));
        let (removed, kept): (Vec<Operation>, Vec<Operation>) =
            operations.into_iter().partition(|operation| {
                operation.seq() == seq || (cascade && operation.replace_seq() == Some(seq))
            });
        self.commit_operations(local_id, file.revision, &kept)?;
        Ok(removed.iter().map(Operation::seq).collect())
    }

    /// The operation log of a draft, oldest first.
    pub fn operation_history(&self, local_id: &str) -> Result<Vec<OperationRecord>, WorkcopyError> {
        let (_, _, operations) = self.load_log(local_id)?;
        Ok(operations
            .iter()
            .map(|operation| OperationRecord {
                seq: operation.seq(),
                op: operation.name(),
                op_version: OP_VERSION_1,
                recorded_at_ms: operation.recorded_at_ms(),
                replace_seq: operation.replace_seq(),
            })
            .collect())
    }

    /// Local = Base + operations as a snapshot of the whole dashboard (a page
    /// draft changes only its page). Deterministic and offline; the
    /// fingerprint equals `ReplaceHostPreview::local_fingerprint`. Input for
    /// P2-04 (update plan) and P2-06 (create from Local).
    pub fn local_snapshot(
        &self,
        local_id: &str,
    ) -> Result<VersionedDashboardSnapshot, WorkcopyError> {
        let (_, base, operations) = self.load_log(local_id)?;
        materialize_local_operations(base, &operations)
    }
}

pub(crate) fn materialize_local_operations(
    base: VersionedDashboardSnapshot,
    operations: &[Operation],
) -> Result<VersionedDashboardSnapshot, WorkcopyError> {
    let Some(log) = ReplaceHostLog::of(operations) else {
        return Ok(base);
    };
    let current = rows(
        &log,
        &Freshness {
            server_version: None,
            fresh: None,
        },
    );
    materialize(&base, &changes(&current))
}

#[cfg(test)]
pub(crate) fn materialize_for_tests(
    base: &VersionedDashboardSnapshot,
    operations: &[Value],
) -> Result<VersionedDashboardSnapshot, WorkcopyError> {
    let operations = parse_log(operations, base.storage_fingerprint())?;
    materialize_local_operations(base.clone(), &operations)
}
