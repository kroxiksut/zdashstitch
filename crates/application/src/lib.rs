// SPDX-License-Identifier: MIT OR Apache-2.0

//! Application-use-case boundary.
//!
//! Planning, confirmation and the closed dashboard publication boundary.

mod compatibility;
pub mod connections;
mod create_dashboard;
mod generated_message_ids;
pub mod publication;
pub mod publish_journal;
mod publisher;
pub mod replace_host;
mod replace_host_plan;
pub mod resolver;
mod session;
mod settings;
mod source_check;
pub mod workcopy;

use zdashstitch_core::ProductStage;
use zdashstitch_zabbix::{Result as ZabbixResult, SnapshotSummary, VersionedDashboardSnapshot};

pub use compatibility::{
    CompatibilityKnowledge, CompatibilityNotice, CompatibilityNoticeLevel,
    DraftCompatibilityReport, DraftWidgetField, DraftWidgetUsage, PortableWidgetCreateError,
    analyze_draft_compatibility, analyze_project_compatibility,
    validate_portable_widget_for_create,
};
pub use create_dashboard::{
    ConfirmedCreatePlan, CreatePlanError, CreatePlanPreview, CreatePublishReport, CreateRecovery,
    CreateRefusal, NameCheck, SAVE_AS_COPY_OPERATION, SaveAsCopyRequest, VerifiedCreatePlan,
    plan_save_as_copy, publish_save_as_copy_plan, reconcile_create, reconcile_save_as_copy,
};
pub use generated_message_ids::MessageId;
pub use publish_journal::{
    CloseManuallyRequest, DamagedJournal, JOURNAL_RETENTION_DAYS, JournalEntry, JournalError,
    JournalListing, JournalProgress, JournalSource, JournalTarget, JournalTransition,
    OperationKind, PublishJournal, QuarantineRequest, RecoveryReport, RetentionFailure,
    RetentionReport, StartupReport, TargetKey, TargetRef, UpdateEvidence,
    decide_update_reconciliation, reconcile_update,
};
pub use publisher::{
    CONTRACT_RESEND_WIDGET_TYPE, CapabilityCheck, CompatibilityFinding, CompatibilityReport,
    ConfirmedPlan, FindingCode, FindingGroup, FindingSeverity, FindingSubject, JournalState,
    PlanError, PlanInvalidReason, PlanPreview, PublishReport, VerifiedPlan, build_verified_plan,
    publish_confirmed_plan,
};
pub use replace_host_plan::{ReplaceHostPlanRequest, plan_replace_host, publish_replace_host_plan};
pub use session::{
    EndSessionRequest, LocalSession, SessionEndError, SessionEnded, SessionGeneration, WriteStage,
};
pub use settings::{
    AppSettings, LocalePreference, ReducedMotionPreference, SettingsError, SettingsLoadError,
    SettingsStore, SettingsWarning, ThemePreference, check_storage_root,
};
pub use source_check::{ReplaceHostRecheck, SourceCheck, recheck_replace_host};

/// Non-sensitive status presented by the local HTTP shell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeStatus {
    /// Current implementation stage.
    pub stage: ProductStage,
    /// Whether any Zabbix dashboard write path exists.
    pub zabbix_writes_enabled: bool,
    /// First adapter family planned by the product contract.
    pub target_adapter_family: &'static str,
}

/// Build the current status from lower-level component boundaries.
#[must_use]
pub const fn runtime_status() -> RuntimeStatus {
    RuntimeStatus {
        stage: zdashstitch_template_import::stage(),
        zabbix_writes_enabled: zdashstitch_zabbix::DASHBOARD_WRITES_ENABLED,
        target_adapter_family: zdashstitch_zabbix::TARGET_ADAPTER_FAMILY,
    }
}

/// Build a non-sensitive aggregate report. Raw snapshots remain in the
/// versioned adapter and are never copied into the portable project model.
pub fn build_snapshot_report(
    snapshots: &[VersionedDashboardSnapshot],
) -> ZabbixResult<SnapshotSummary> {
    let analyses = snapshots
        .iter()
        .map(VersionedDashboardSnapshot::analyze)
        .collect::<ZabbixResult<Vec<_>>>()?;
    Ok(SnapshotSummary::from_analyses(&analyses))
}

#[cfg(test)]
mod tests {
    use super::runtime_status;

    #[test]
    fn application_reports_writes_disabled() {
        assert!(!runtime_status().zabbix_writes_enabled);
    }
}
