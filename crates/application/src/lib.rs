// SPDX-License-Identifier: MIT OR Apache-2.0

//! Application-use-case boundary.
//!
//! Planning, confirmation and the closed dashboard publication boundary.

mod compatibility;
pub mod connections;
mod generated_message_ids;
mod publisher;
mod settings;

use zdashstitch_core::ProductStage;
use zdashstitch_zabbix::{Result as ZabbixResult, SnapshotSummary, VersionedDashboardSnapshot};

pub use compatibility::{
    CompatibilityKnowledge, CompatibilityNotice, CompatibilityNoticeLevel,
    DraftCompatibilityReport, DraftWidgetField, DraftWidgetUsage, PortableWidgetCreateError,
    analyze_draft_compatibility, analyze_project_compatibility,
    validate_portable_widget_for_create,
};
pub use generated_message_ids::MessageId;
pub use publisher::{
    CapabilityCheck, CompatibilityReport, ConfirmedPlan, JournalState, PlanError, PlanPreview,
    PublishReport, VerifiedPlan, build_verified_plan, publish_confirmed_plan,
};
pub use settings::{
    AppSettings, LocalePreference, ReducedMotionPreference, SettingsError, SettingsLoadError,
    SettingsStore, SettingsWarning, ThemePreference, check_storage_root,
};

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
