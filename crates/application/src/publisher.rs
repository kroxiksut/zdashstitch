// SPDX-License-Identifier: MIT OR Apache-2.0

use std::time::SystemTime;

use thiserror::Error;
use zdashstitch_zabbix::{
    AuthenticatedClient, CapabilityState, DashboardPublishOutcome, ItemFieldReplacement,
    PublicationContext, VerifiedDashboardUpdate, VersionedDashboardSnapshot,
    verify_item_field_replacements,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityCheck {
    pub name: &'static str,
    pub state: CapabilityState,
    pub blocking: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompatibilityReport {
    pub checks: Vec<CapabilityCheck>,
    pub blockers: Vec<String>,
}

impl CompatibilityReport {
    pub fn allows_publication(&self) -> bool {
        self.blockers.is_empty()
            && self
                .checks
                .iter()
                .filter(|check| check.blocking)
                .all(|check| check.state == CapabilityState::Supported)
    }
}

/// Trusted planner output. The constructor and payload are private, the type
/// is not deserializable, and the value is consumed by confirmation.
pub struct VerifiedPlan {
    plan_id: String,
    created_at: SystemTime,
    update: VerifiedDashboardUpdate,
    compatibility: CompatibilityReport,
}

pub struct ConfirmedPlan {
    plan_id: String,
    created_at: SystemTime,
    confirmed_at: SystemTime,
    update: VerifiedDashboardUpdate,
    compatibility: CompatibilityReport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanPreview {
    pub plan_id: String,
    pub operation: &'static str,
    pub dashboard_id: String,
    pub replacement_count: usize,
    pub compatibility: CompatibilityReport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalState {
    Planned,
    Previewed,
    Confirmed,
    Submitting,
    Verified,
    Conflict,
    FailedBeforeSend,
    FailedAfterResponse,
    UnknownOutcome,
    ReconciledVerified,
    ReconciledFailed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishReport {
    pub plan_id: String,
    pub states: Vec<JournalState>,
    pub deployment_state_may_advance: bool,
}

#[derive(Debug, Error)]
pub enum PlanError {
    #[error("compatibility checks block publication")]
    Incompatible(CompatibilityReport),
    #[error("adapter validation rejected the plan: {0}")]
    Adapter(String),
    #[error("confirmation does not match the prepared plan")]
    ConfirmationMismatch,
}

pub fn build_verified_plan(
    snapshot: &VersionedDashboardSnapshot,
    context: &PublicationContext,
    replacements: &[ItemFieldReplacement],
) -> Result<VerifiedPlan, PlanError> {
    let compatibility = compatibility_report(snapshot, context, replacements);
    if !compatibility.allows_publication() {
        return Err(PlanError::Incompatible(compatibility));
    }
    let update = verify_item_field_replacements(snapshot, context, replacements)
        .map_err(|error| PlanError::Adapter(error.to_string()))?;
    let plan_id = format!(
        "zds-{}-{}-{}",
        &update.source_fingerprint()[..12],
        &update.expected_fingerprint()[..12],
        update.replacement_count()
    );
    Ok(VerifiedPlan {
        plan_id,
        created_at: SystemTime::now(),
        update,
        compatibility,
    })
}

impl VerifiedPlan {
    pub fn preview(&self) -> PlanPreview {
        PlanPreview {
            plan_id: self.plan_id.clone(),
            operation: "replace_item_fields",
            dashboard_id: self.update.dashboard_id().to_owned(),
            replacement_count: self.update.replacement_count(),
            compatibility: self.compatibility.clone(),
        }
    }

    pub fn confirm(self, displayed_plan_id: &str) -> Result<ConfirmedPlan, PlanError> {
        if displayed_plan_id != self.plan_id {
            return Err(PlanError::ConfirmationMismatch);
        }
        Ok(ConfirmedPlan {
            plan_id: self.plan_id,
            created_at: self.created_at,
            confirmed_at: SystemTime::now(),
            update: self.update,
            compatibility: self.compatibility,
        })
    }
}

pub async fn publish_confirmed_plan(
    client: &AuthenticatedClient,
    plan: ConfirmedPlan,
) -> PublishReport {
    let ConfirmedPlan {
        plan_id,
        created_at,
        confirmed_at,
        update,
        compatibility,
    } = plan;
    let mut states = vec![
        JournalState::Planned,
        JournalState::Previewed,
        JournalState::Confirmed,
    ];
    if confirmed_at < created_at || !compatibility.allows_publication() {
        states.push(JournalState::FailedBeforeSend);
        return PublishReport {
            plan_id,
            states,
            deployment_state_may_advance: false,
        };
    }
    let outcome = client.publish_verified_dashboard_update(&update).await;
    finish_report(plan_id, states, outcome)
}

fn finish_report(
    plan_id: String,
    mut states: Vec<JournalState>,
    outcome: DashboardPublishOutcome,
) -> PublishReport {
    let (state, sent, deployment_state_may_advance) = match outcome {
        DashboardPublishOutcome::Conflict => (JournalState::Conflict, false, false),
        DashboardPublishOutcome::Verified => (JournalState::Verified, true, true),
        DashboardPublishOutcome::FailedBeforeSend => (JournalState::FailedBeforeSend, false, false),
        DashboardPublishOutcome::FailedAfterResponse => {
            (JournalState::FailedAfterResponse, true, false)
        }
        DashboardPublishOutcome::UnknownOutcome => (JournalState::UnknownOutcome, true, false),
        DashboardPublishOutcome::ReconciledVerified => {
            (JournalState::ReconciledVerified, true, true)
        }
        DashboardPublishOutcome::ReconciledFailed => (JournalState::ReconciledFailed, true, false),
    };
    if sent {
        states.push(JournalState::Submitting);
    }
    states.push(state);
    PublishReport {
        plan_id,
        states,
        deployment_state_may_advance,
    }
}

fn compatibility_report(
    snapshot: &VersionedDashboardSnapshot,
    context: &PublicationContext,
    replacements: &[ItemFieldReplacement],
) -> CompatibilityReport {
    let capabilities = context.capabilities();
    let contracts = &capabilities.dashboard_write_contracts;
    let checks = vec![
        CapabilityCheck {
            name: "exact_server_version",
            state: if capabilities.version == *snapshot.server_version() {
                CapabilityState::Supported
            } else {
                CapabilityState::Unsupported
            },
            blocking: true,
        },
        CapabilityCheck {
            name: "authenticated_connection_and_principal_binding",
            state: if context.is_live_authenticated() {
                CapabilityState::Supported
            } else {
                CapabilityState::Unsupported
            },
            blocking: true,
        },
        CapabilityCheck {
            name: "update_one_page_with_full_fields",
            state: contracts.update_one_page_with_full_fields,
            blocking: true,
        },
        CapabilityCheck {
            name: "preserve_untouched_pages",
            state: contracts.preserve_untouched_pages,
            blocking: true,
        },
        CapabilityCheck {
            name: "item_widget_itemid",
            state: contracts.item_widget_itemid,
            blocking: true,
        },
        CapabilityCheck {
            name: "preserve_widget_references",
            state: contracts.preserve_widget_references,
            blocking: true,
        },
        CapabilityCheck {
            name: "reconcile_unknown_outcome",
            state: contracts.reconcile_unknown_outcome,
            blocking: true,
        },
    ];
    let mut blockers = Vec::new();
    if replacements.is_empty() {
        blockers.push("the plan contains no changes".to_owned());
    }
    for check in &checks {
        if check.blocking && check.state != CapabilityState::Supported {
            blockers.push(format!(
                "required capability is not verified: {}",
                check.name
            ));
        }
    }
    CompatibilityReport { checks, blockers }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use zdashstitch_zabbix::{AdapterRegistry, ZabbixVersion};

    use super::*;

    fn snapshot(version: &str, widget_type: &str) -> VersionedDashboardSnapshot {
        let version: ZabbixVersion = version
            .parse()
            .unwrap_or_else(|error| panic!("valid version: {error}"));
        let raw: Value = json!({
            "dashboardid": "7", "name": "contract",
            "pages": [{
                "dashboard_pageid": "11", "name": "page", "display_period": "0",
                "widgets": [{
                    "widgetid": "13", "type": widget_type, "name": "source",
                    "x": "0", "y": "0", "width": "12", "height": "3", "view_mode": "0",
                    "fields": [{"type": "4", "name": "itemid.0", "value": "101"}]
                }]
            }], "users": [], "userGroups": []
        });
        VersionedDashboardSnapshot::from_adapter_raw("test".to_owned(), version, raw)
            .unwrap_or_else(|error| panic!("valid snapshot: {error}"))
    }

    fn replacement() -> ItemFieldReplacement {
        ItemFieldReplacement {
            page_index: 0,
            widget_index: 0,
            field_name: "itemid.0".to_owned(),
            expected_itemid: "101".to_owned(),
            replacement_itemid: "202".to_owned(),
        }
    }

    #[test]
    fn verified_plan_is_previewed_and_consumed_by_matching_confirmation() {
        let source = snapshot("7.4.16", "item");
        let capabilities = AdapterRegistry::resolve(source.server_version().clone())
            .unwrap_or_else(|error| panic!("supported adapter: {error}"));
        let context = PublicationContext::detached_for_testing(capabilities);
        let plan = build_verified_plan(&source, &context, &[replacement()])
            .unwrap_or_else(|error| panic!("plan must verify: {error}"));
        let preview = plan.preview();
        assert_eq!(preview.operation, "replace_item_fields");
        assert_eq!(preview.replacement_count, 1);
        let confirmed = plan
            .confirm(&preview.plan_id)
            .unwrap_or_else(|error| panic!("matching confirmation: {error}"));
        assert_eq!(confirmed.plan_id, preview.plan_id);
    }

    #[test]
    fn unproven_patch_and_unknown_widget_cannot_create_a_plan() {
        let unproven = snapshot("7.4.17", "item");
        let capabilities = AdapterRegistry::resolve(unproven.server_version().clone())
            .unwrap_or_else(|error| panic!("read adapter: {error}"));
        assert!(matches!(
            build_verified_plan(
                &unproven,
                &PublicationContext::detached_for_testing(capabilities),
                &[replacement()]
            ),
            Err(PlanError::Incompatible(_))
        ));

        let unknown = snapshot("7.4.16", "vendor.private");
        let capabilities = AdapterRegistry::resolve(unknown.server_version().clone())
            .unwrap_or_else(|error| panic!("write contracts: {error}"));
        assert!(matches!(
            build_verified_plan(
                &unknown,
                &PublicationContext::detached_for_testing(capabilities),
                &[replacement()]
            ),
            Err(PlanError::Adapter(_))
        ));
    }

    #[test]
    fn journal_distinguishes_conflict_unknown_and_reconciled_success() {
        let initial = vec![
            JournalState::Planned,
            JournalState::Previewed,
            JournalState::Confirmed,
        ];
        let conflict = finish_report(
            "conflict".to_owned(),
            initial.clone(),
            DashboardPublishOutcome::Conflict,
        );
        assert_eq!(conflict.states.last(), Some(&JournalState::Conflict));
        assert!(!conflict.states.contains(&JournalState::Submitting));
        assert!(!conflict.deployment_state_may_advance);

        let unknown = finish_report(
            "unknown".to_owned(),
            initial.clone(),
            DashboardPublishOutcome::UnknownOutcome,
        );
        assert_eq!(unknown.states.last(), Some(&JournalState::UnknownOutcome));
        assert!(unknown.states.contains(&JournalState::Submitting));
        assert!(!unknown.deployment_state_may_advance);

        let reconciled = finish_report(
            "reconciled".to_owned(),
            initial,
            DashboardPublishOutcome::ReconciledVerified,
        );
        assert_eq!(
            reconciled.states.last(),
            Some(&JournalState::ReconciledVerified)
        );
        assert!(reconciled.deployment_state_may_advance);
    }
}
