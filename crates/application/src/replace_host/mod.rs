// SPDX-License-Identifier: MIT OR Apache-2.0

//! ReplaceHost over a workcopy (P2-03A/B/C): intent and calculation stored
//! as workcopy operations, user decisions, Local = Base + operations, the
//! structural diff and the publication blockers. Backend only: no HTTP
//! routes, no prose.
//!
//! Flow:
//!
//! 1. [`crate::workcopy::WorkcopyService::calculate_replace_host`] resolves
//!    the intent (source host, target host, scope) over Base with
//!    read-only lookups (P2-02) and stores the frozen result as a
//!    `replace_host` v1 operation. It works in `read_only` sessions.
//! 2. `decide_replace_host` records decisions (`choose_candidate`,
//!    `choose_lld_instance`, `keep_on_source`) as `replace_host_decision`
//!    operations; `confirm_kept_on_source` confirms the list «останутся на
//!    старом узле» by digest; `remove_operation` undoes any of them.
//! 3. `replace_host_preview` (offline) and `revalidate_replace_host`
//!    (read-only re-resolution, no write, no automatic new choice) return a
//!    [`ReplaceHostPreview`]: rows, [`StructuralDiff`], [`KeptList`], field
//!    changes, Base and Local fingerprints and a
//!    [`crate::CompatibilityReport`].
//! 4. `local_snapshot` materializes Local: Base with only the changed field
//!    values replaced, every other byte preserved (O-003, P1-IN-04).
//!
//! Before the first calculation the wizard reads
//! [`crate::workcopy::WorkcopyService::replace_host_inventory`] (hosts the
//! draft references, 09-01) and
//! [`crate::workcopy::WorkcopyService::search_replace_host_targets`] (new
//! host, 09-02); both are read-only and store nothing.
//!
//! Nothing here builds a [`crate::VerifiedPlan`] or reaches a write
//! gateway: publication needs a separate `read_write` plan (P2-04).

mod diff;
mod inventory;
mod model;
mod report;
mod service;
mod state;

pub use diff::{
    ChangeCounts, DiffSummary, PageDiff, SeriesDiff, StateCounts, StructuralDiff, WidgetDiff,
};
pub use inventory::{
    ExpectedOutcome, HostReference, HostSearchLookup, InventoryHost, InventoryPage,
    InventoryWidget, MAX_TARGET_SEARCH_CHARS, ReferenceKind, ReplaceHostInventory,
    TARGET_SEARCH_LIMIT, TargetHostCandidate, TargetHostSearch, UnattributedReference,
};
pub use model::{
    Binding, Calculation, DecisionKind, FieldKey, FieldValue, HostRef, ItemFacts, LinkCrossing,
    PatternCount, ReplaceScope, RowDecision, UnverifiedReference,
};
pub use report::{
    CONTRACT_SEVERAL_PAGES, CONTRACT_SVGGRAPH_HOSTS, CONTRACT_SVGGRAPH_ITEMIDS, SessionContext,
    capability_state_str,
};
pub use service::{OperationRecord, ReplaceHostOperation, ReplaceHostPreview};
pub use state::{
    ChangeState, CheckReason, DecisionState, FieldChange, KeptEntry, KeptList, RowCheck, RowView,
};

#[cfg(test)]
mod tests;
