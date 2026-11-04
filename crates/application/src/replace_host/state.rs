// SPDX-License-Identifier: MIT OR Apache-2.0

//! Pure evaluation of a `replace_host` operation: rows with decisions
//! applied, the list kept on the source host, the field change set and the
//! materialization Local = Base + operations.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::model::{
    Binding, Calculation, DecisionKind, FieldKey, FieldValue, ItemFacts, PatternCount, RowDecision,
};
use crate::resolver::{Applicability, Difference, MatchStatus, ReasonCode};
use crate::workcopy::WorkcopyError;
use crate::workcopy::operations::{ConfirmKeptEntry, DecisionEntry, Operation, ReplaceHostEntry};

/// Effect of the operation on one field, widget, series or page.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ChangeState {
    /// The field gets the target value.
    Changed,
    /// The operation does not touch it.
    Untouched,
    /// It should change but has no applicable target or decision; blocks
    /// publication.
    Unresolved,
    /// It references the source host and stays as is (O-003 or a «keep on
    /// source» decision); listed for confirmation.
    KeptOnSource,
}

impl ChangeState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Changed => "changed",
            Self::Untouched => "untouched",
            Self::Unresolved => "unresolved",
            Self::KeptOnSource => "kept_on_source",
        }
    }
}

/// Freshness of a row (P1-05G `checked/stale/invalid`).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RowCheck {
    /// As calculated, or confirmed by a fresh read.
    Checked,
    /// The facts changed: recalculate; no new choice is made automatically.
    Stale,
    /// The stored target or chosen candidate is no longer visible.
    Invalid,
}

impl RowCheck {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Checked => "checked",
            Self::Stale => "stale",
            Self::Invalid => "invalid",
        }
    }
}

/// Why a row is stale or invalid.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CheckReason {
    /// The connected server version differs from the calculation.
    ServerVersionChanged,
    /// A fresh resolution gives another target, status or evidence.
    ResolutionChanged,
    /// The reference is no longer found by a fresh resolution.
    ReferenceGone,
    /// A fresh resolution finds a reference the calculation does not have.
    ReferenceAdded,
    /// The target host is no longer visible.
    TargetNotVisible,
    /// The chosen candidate is no longer offered.
    CandidateNotVisible,
}

impl CheckReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ServerVersionChanged => "server_version_changed",
            Self::ResolutionChanged => "resolution_changed",
            Self::ReferenceGone => "reference_gone",
            Self::ReferenceAdded => "reference_added",
            Self::TargetNotVisible => "target_not_visible",
            Self::CandidateNotVisible => "candidate_not_visible",
        }
    }
}

/// State of the stored decision of a row.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DecisionState {
    /// No decision recorded.
    None,
    /// The decision applies.
    Applied,
    /// The decision no longer fits the row (recalculated row, other source
    /// value, candidate not offered); it is ignored and blocks publication.
    Invalid,
}

impl DecisionState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Applied => "applied",
            Self::Invalid => "invalid",
        }
    }
}

/// One reference of the source host in scope as the screens show it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RowView {
    pub key: FieldKey,
    pub dashboard_pageid: Option<String>,
    pub widgetid: Option<String>,
    pub widget_type: String,
    pub source: FieldValue,
    /// Effective target: calculated or chosen; `Some` only for `changed`.
    pub target: Option<FieldValue>,
    /// `None` for unverified references.
    pub match_status: Option<MatchStatus>,
    pub applicability: Option<Applicability>,
    pub reasons: Vec<ReasonCode>,
    /// Differences of the effective target from the source.
    pub differences: Vec<Difference>,
    pub source_item: Option<ItemFacts>,
    pub target_item: Option<ItemFacts>,
    pub candidates: Vec<ItemFacts>,
    pub pattern_coverage: Vec<PatternCount>,
    pub state: ChangeState,
    /// Why the row stays on the source host; `Some` for `kept_on_source`.
    pub kept_reason: Option<ReasonCode>,
    pub decision: Option<RowDecision>,
    /// `seq` of the decision entry, for undo.
    pub decision_seq: Option<u64>,
    pub decision_state: DecisionState,
    /// Decisions the row accepts now.
    pub allowed_decisions: Vec<DecisionKind>,
    /// The reference could not be attributed to a host (hidden object or
    /// prototype).
    pub unverified: bool,
    pub check: RowCheck,
    pub check_reason: Option<CheckReason>,
}

/// One entry of «останутся на старом узле» (O-003), the list publication
/// asks to confirm.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct KeptEntry {
    pub page_index: usize,
    pub widget_index: usize,
    pub widgetid: Option<String>,
    pub field: String,
    pub widget_type: String,
    pub source: FieldValue,
    /// Resolver reason; `None` when kept only by a user decision.
    pub reason: Option<ReasonCode>,
    pub by_decision: bool,
}

/// The list kept on the source host with its confirmation state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeptList {
    pub entries: Vec<KeptEntry>,
    /// SHA-256 (hex) of the list bound to the operation; pass it back to
    /// confirm exactly this list.
    pub digest: String,
    /// A confirmation of exactly this list is recorded.
    pub confirmed: bool,
    /// `seq` of the latest confirmation entry, also when it is stale.
    pub confirmation_seq: Option<u64>,
}

/// One field write of Local: Base value → new value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldChange {
    pub key: FieldKey,
    pub widgetid: Option<String>,
    pub widget_type: String,
    pub source: FieldValue,
    pub target: FieldValue,
}

/// The `replace_host` entry with its decisions and confirmation.
pub(crate) struct ReplaceHostLog<'a> {
    pub entry: &'a ReplaceHostEntry,
    /// Latest decision per row.
    pub decisions: BTreeMap<FieldKey, &'a DecisionEntry>,
    pub confirmation: Option<&'a ConfirmKeptEntry>,
}

impl<'a> ReplaceHostLog<'a> {
    pub(crate) fn of(operations: &'a [Operation]) -> Option<Self> {
        let entry = operations.iter().find_map(|operation| match operation {
            Operation::ReplaceHost(entry) => Some(entry),
            _ => None,
        })?;
        let mut decisions = BTreeMap::new();
        let mut confirmation = None;
        for operation in operations {
            match operation {
                Operation::Decision(decision) if decision.replace_seq == entry.seq => {
                    decisions.insert(decision.key.clone(), decision);
                }
                Operation::ConfirmKept(confirm) if confirm.replace_seq == entry.seq => {
                    confirmation = Some(confirm);
                }
                _ => {}
            }
        }
        Some(Self {
            entry,
            decisions,
            confirmation,
        })
    }
}

/// Fresh facts for revalidation: a new calculation plus the connected
/// server version.
pub(crate) struct Freshness<'a> {
    pub server_version: Option<&'a str>,
    pub fresh: Option<&'a Calculation>,
}

/// Why `decision` cannot be recorded for `binding`; `None` when it can.
pub(crate) fn decision_error(binding: &Binding, decision: &RowDecision) -> Option<&'static str> {
    if !binding.is_editable() {
        return Some("decision_not_applicable");
    }
    let (status, itemid) = match decision {
        RowDecision::KeepOnSource => return None,
        RowDecision::ChooseCandidate { itemid } => (MatchStatus::Ambiguous, itemid),
        RowDecision::ChooseLldInstance { itemid } => (MatchStatus::UnresolvedLld, itemid),
    };
    if binding.match_status != status || !matches!(binding.source, FieldValue::Item { .. }) {
        return Some("decision_not_applicable");
    }
    let Some(candidate) = binding
        .candidates
        .iter()
        .find(|candidate| &candidate.itemid == itemid)
    else {
        return Some("candidate_not_visible");
    };
    if binding.widget_type == "svggraph" && !candidate.is_numeric() {
        return Some("candidate_value_type_incompatible");
    }
    None
}

fn allowed_decisions(binding: &Binding) -> Vec<DecisionKind> {
    if !binding.is_editable() {
        return Vec::new();
    }
    let mut allowed = Vec::new();
    let has_candidates = !binding.candidates.is_empty();
    match binding.match_status {
        MatchStatus::Ambiguous if has_candidates => allowed.push(DecisionKind::ChooseCandidate),
        MatchStatus::UnresolvedLld if has_candidates => {
            allowed.push(DecisionKind::ChooseLldInstance);
        }
        _ => {}
    }
    allowed.push(DecisionKind::KeepOnSource);
    allowed
}

/// Differences of a chosen item from the source item (P1-IN-03).
fn item_differences(source: Option<&ItemFacts>, target: &ItemFacts) -> Vec<Difference> {
    let Some(source) = source else {
        return Vec::new();
    };
    let mut differences = Vec::new();
    if source.value_type != target.value_type {
        differences.push(Difference::ValueType);
    }
    if source.units != target.units {
        differences.push(Difference::Units);
    }
    let same_origin = match (&source.template_itemid, &target.template_itemid) {
        (Some(left), Some(right)) => left == right,
        (None, None) => source.prototype_key == target.prototype_key,
        _ => false,
    };
    if !same_origin {
        differences.push(Difference::Origin);
    }
    differences
}

fn binding_row(binding: &Binding, decision: Option<&DecisionEntry>) -> RowView {
    let mut row = RowView {
        key: binding.key(),
        dashboard_pageid: binding.dashboard_pageid.clone(),
        widgetid: binding.widgetid.clone(),
        widget_type: binding.widget_type.clone(),
        source: binding.source.clone(),
        target: None,
        match_status: Some(binding.match_status),
        applicability: Some(binding.applicability),
        reasons: binding.reasons.clone(),
        differences: binding.differences.clone(),
        source_item: binding.source_item.clone(),
        target_item: binding.target_item.clone(),
        candidates: binding.candidates.clone(),
        pattern_coverage: binding.pattern_coverage.clone(),
        state: ChangeState::Unresolved,
        kept_reason: None,
        decision: decision.map(|entry| entry.decision.clone()),
        decision_seq: decision.map(|entry| entry.seq),
        decision_state: DecisionState::None,
        allowed_decisions: allowed_decisions(binding),
        unverified: false,
        check: RowCheck::Checked,
        check_reason: None,
    };
    if !binding.is_editable() {
        row.state = ChangeState::KeptOnSource;
        row.kept_reason = binding
            .kept_reason
            .or(Some(ReasonCode::FieldNotReplaceable));
        if decision.is_some() {
            row.decision_state = DecisionState::Invalid;
        }
        return row;
    }
    if let Some(entry) = decision {
        let valid =
            entry.source == binding.source && decision_error(binding, &entry.decision).is_none();
        if valid {
            row.decision_state = DecisionState::Applied;
            match &entry.decision {
                RowDecision::KeepOnSource => {
                    row.state = ChangeState::KeptOnSource;
                    row.kept_reason = None;
                    row.target_item = None;
                    row.differences = Vec::new();
                }
                RowDecision::ChooseCandidate { itemid }
                | RowDecision::ChooseLldInstance { itemid } => {
                    row.state = ChangeState::Changed;
                    row.target = Some(FieldValue::Item {
                        itemid: itemid.clone(),
                    });
                    row.target_item.clone_from(&entry.chosen);
                    row.differences = entry.chosen.as_ref().map_or_else(Vec::new, |chosen| {
                        item_differences(binding.source_item.as_ref(), chosen)
                    });
                }
            }
            return row;
        }
        row.decision_state = DecisionState::Invalid;
    }
    if binding.will_change() {
        row.state = ChangeState::Changed;
        row.target.clone_from(&binding.target);
    }
    row
}

fn fresh_matches(stored: &Binding, fresh: &Binding) -> bool {
    let facts = |item: Option<&ItemFacts>| {
        item.map(|item| (item.key.clone(), item.value_type, item.units.clone()))
    };
    stored.source == fresh.source
        && stored.target == fresh.target
        && stored.match_status == fresh.match_status
        && stored.applicability == fresh.applicability
        && stored.reasons == fresh.reasons
        && facts(stored.target_item.as_ref()) == facts(fresh.target_item.as_ref())
}

fn apply_freshness(
    row: &mut RowView,
    stored: Option<&Binding>,
    freshness: &Freshness<'_>,
    calculation: &Calculation,
) {
    let mark = |row: &mut RowView, check: RowCheck, reason: CheckReason| {
        if row.check == RowCheck::Checked || check == RowCheck::Invalid {
            row.check = check;
            row.check_reason = Some(reason);
        }
    };
    if freshness
        .server_version
        .is_some_and(|version| version != calculation.server_version)
    {
        mark(row, RowCheck::Stale, CheckReason::ServerVersionChanged);
    }
    let Some(fresh) = freshness.fresh else {
        return;
    };
    if fresh.target_host.is_none() && calculation.target_host.is_some() {
        mark(row, RowCheck::Invalid, CheckReason::TargetNotVisible);
        return;
    }
    let Some(stored) = stored else {
        // Unverified reference: still unverified, or resolved now.
        if !fresh
            .unverified
            .iter()
            .any(|reference| reference.key() == row.key)
        {
            mark(row, RowCheck::Stale, CheckReason::ResolutionChanged);
        }
        return;
    };
    match fresh
        .bindings
        .iter()
        .find(|binding| binding.key() == row.key)
    {
        None => mark(row, RowCheck::Stale, CheckReason::ReferenceGone),
        Some(fresh_binding) => {
            if !fresh_matches(stored, fresh_binding) {
                mark(row, RowCheck::Stale, CheckReason::ResolutionChanged);
            }
            if row.decision_state == DecisionState::Applied
                && let Some(itemid) = row.decision.as_ref().and_then(RowDecision::itemid)
                && !fresh_binding
                    .candidates
                    .iter()
                    .any(|candidate| candidate.itemid == itemid)
            {
                mark(row, RowCheck::Invalid, CheckReason::CandidateNotVisible);
            }
        }
    }
}

/// Rows of the operation in Base order: bindings, then unverified
/// references, with decisions and freshness applied.
pub(crate) fn rows(log: &ReplaceHostLog<'_>, freshness: &Freshness<'_>) -> Vec<RowView> {
    let calculation = &log.entry.calculation;
    let mut rows: Vec<RowView> = Vec::new();
    for binding in &calculation.bindings {
        let decision = log.decisions.get(&binding.key()).copied();
        let mut row = binding_row(binding, decision);
        apply_freshness(&mut row, Some(binding), freshness, calculation);
        rows.push(row);
    }
    for reference in &calculation.unverified {
        let decision = log.decisions.get(&reference.key()).copied();
        let kept = decision.is_some_and(|entry| {
            entry.source == reference.source && entry.decision == RowDecision::KeepOnSource
        });
        let mut row = RowView {
            key: reference.key(),
            dashboard_pageid: reference.dashboard_pageid.clone(),
            widgetid: reference.widgetid.clone(),
            widget_type: reference.widget_type.clone(),
            source: reference.source.clone(),
            target: None,
            match_status: None,
            applicability: None,
            reasons: vec![reference.reason],
            differences: Vec::new(),
            source_item: None,
            target_item: None,
            candidates: Vec::new(),
            pattern_coverage: Vec::new(),
            state: if kept {
                ChangeState::KeptOnSource
            } else {
                ChangeState::Unresolved
            },
            kept_reason: None,
            decision: decision.map(|entry| entry.decision.clone()),
            decision_seq: decision.map(|entry| entry.seq),
            decision_state: match (decision, kept) {
                (None, _) => DecisionState::None,
                (Some(_), true) => DecisionState::Applied,
                (Some(_), false) => DecisionState::Invalid,
            },
            allowed_decisions: vec![DecisionKind::KeepOnSource],
            unverified: true,
            check: RowCheck::Checked,
            check_reason: None,
        };
        apply_freshness(&mut row, None, freshness, calculation);
        rows.push(row);
    }
    rows.sort_by(|left, right| left.key.cmp(&right.key));
    rows
}

/// Keys a fresh calculation has but the stored one does not.
pub(crate) fn added_references(calculation: &Calculation, fresh: &Calculation) -> Vec<FieldKey> {
    let known: std::collections::BTreeSet<FieldKey> = calculation
        .bindings
        .iter()
        .map(Binding::key)
        .chain(
            calculation
                .unverified
                .iter()
                .map(|reference| reference.key()),
        )
        .collect();
    let mut added: Vec<FieldKey> = fresh
        .bindings
        .iter()
        .map(Binding::key)
        .chain(fresh.unverified.iter().map(|reference| reference.key()))
        .filter(|key| !known.contains(key))
        .collect();
    added.sort();
    added.dedup();
    added
}

#[derive(Serialize)]
struct DigestInput<'a> {
    domain: &'static str,
    source_hostid: &'a str,
    target_hostid: &'a str,
    base_fingerprint: &'a str,
    entries: &'a [KeptEntry],
}

/// The kept list, its digest and whether exactly this list is confirmed.
pub(crate) fn kept_list(
    log: &ReplaceHostLog<'_>,
    rows: &[RowView],
) -> Result<KeptList, WorkcopyError> {
    let entries: Vec<KeptEntry> = rows
        .iter()
        .filter(|row| row.state == ChangeState::KeptOnSource)
        .map(|row| KeptEntry {
            page_index: row.key.page_index,
            widget_index: row.key.widget_index,
            widgetid: row.widgetid.clone(),
            field: row.key.field.clone(),
            widget_type: row.widget_type.clone(),
            source: row.source.clone(),
            reason: row.kept_reason,
            by_decision: row.decision_state == DecisionState::Applied,
        })
        .collect();
    let input = DigestInput {
        domain: "zdashstitch-kept-on-source-v1",
        source_hostid: &log.entry.source_hostid,
        target_hostid: &log.entry.target_hostid,
        base_fingerprint: &log.entry.calculation.base_fingerprint,
        entries: &entries,
    };
    let raw = serde_json::to_vec(&input).map_err(|_| WorkcopyError::new("workcopy_corrupt"))?;
    let digest = Sha256::digest(&raw);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    let confirmed = !entries.is_empty()
        && log
            .confirmation
            .is_some_and(|confirmation| confirmation.kept_digest == hex);
    Ok(KeptList {
        entries,
        digest: hex,
        confirmed,
        confirmation_seq: log.confirmation.map(|confirmation| confirmation.seq),
    })
}

/// Field writes of Local, in Base order.
pub(crate) fn changes(rows: &[RowView]) -> Vec<FieldChange> {
    rows.iter()
        .filter(|row| row.state == ChangeState::Changed)
        .filter_map(|row| {
            Some(FieldChange {
                key: row.key.clone(),
                widgetid: row.widgetid.clone(),
                widget_type: row.widget_type.clone(),
                source: row.source.clone(),
                target: row.target.clone()?,
            })
        })
        .collect()
}

fn materialization_error(change: &FieldChange) -> WorkcopyError {
    WorkcopyError::new("local_materialization_failed")
        .with("page_index", change.key.page_index.to_string())
        .with("widget_index", change.key.widget_index.to_string())
        .with(
            "field",
            change.key.field.chars().take(64).collect::<String>(),
        )
}

/// Local = Base with `changes` applied. Pure and deterministic: only the
/// `value` of each changed field is replaced, keeping its JSON kind; every
/// other byte of Base is kept. Each change must find exactly one field with
/// its name and the source value, on the widget with the recorded
/// `widgetid`.
pub(crate) fn apply_changes(base: &Value, changes: &[FieldChange]) -> Result<Value, WorkcopyError> {
    let mut local = base.clone();
    for change in changes {
        let fail = || materialization_error(change);
        let widget = local
            .get_mut("pages")
            .and_then(Value::as_array_mut)
            .and_then(|pages| pages.get_mut(change.key.page_index))
            .and_then(|page| page.get_mut("widgets"))
            .and_then(Value::as_array_mut)
            .and_then(|widgets| widgets.get_mut(change.key.widget_index))
            .ok_or_else(fail)?;
        if let Some(widgetid) = &change.widgetid
            && widget.get("widgetid").and_then(Value::as_str) != Some(widgetid.as_str())
        {
            return Err(fail());
        }
        let fields = widget
            .get_mut("fields")
            .and_then(Value::as_array_mut)
            .ok_or_else(fail)?;
        let mut matching = fields.iter_mut().filter(|field| {
            field.get("name").and_then(Value::as_str) == Some(change.key.field.as_str())
        });
        let field = matching.next().ok_or_else(fail)?;
        if matching.next().is_some() {
            return Err(fail());
        }
        let value = field.get_mut("value").ok_or_else(fail)?;
        let source = change.source.raw();
        let target = change.target.raw();
        let replacement = match &*value {
            Value::String(text) if text == source => Value::String(target.to_owned()),
            Value::Number(number) if number.to_string() == source => {
                Value::Number(target.parse().map_err(|_| fail())?)
            }
            _ => return Err(fail()),
        };
        *value = replacement;
    }
    Ok(local)
}
