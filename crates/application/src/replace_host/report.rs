// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-03C: blockers of a `replace_host` operation as a
//! [`CompatibilityReport`]. Unresolved, unverified, stale rows, unproven
//! write contracts, a missing connection, `read_only` mode and an
//! unconfirmed kept list block **publication only**; the local diff and
//! preview are always produced. References kept on the source host (O-003)
//! are informational, and only the unconfirmed list blocks.
//!
//! The report is data for the screens and for P2-04; it is not write
//! authority. A plan still needs a `ReadWriteGateway` publication context.

use zdashstitch_zabbix::{AccessMode, CapabilityState, WriteContract, ZabbixCapabilities};

use super::model::FieldKey;
use super::state::{
    ChangeState, DecisionState, FieldChange, KeptList, ReplaceHostLog, RowCheck, RowView,
};
use crate::publisher::{
    CapabilityCheck, CompatibilityFinding, CompatibilityReport, FindingCode, FindingGroup,
    FindingSeverity, FindingSubject,
};
use crate::resolver::{MatchStatus, ReasonCode};

/// Laboratory contract for `svggraph` `ds.N.itemids.N` (P2-04A), not proven;
/// its state comes from the session capabilities.
pub const CONTRACT_SVGGRAPH_ITEMIDS: &str = WriteContract::SvggraphDatasetItemids.as_str();
/// Laboratory contract for `svggraph` `ds.N.hosts.N` (P2-04A), not proven.
pub const CONTRACT_SVGGRAPH_HOSTS: &str = WriteContract::SvggraphDatasetHosts.as_str();
/// One verified update changing several pages (P2-04B), not proven.
pub const CONTRACT_SEVERAL_PAGES: &str = WriteContract::UpdateSeveralPages.as_str();

/// Stable code of a capability state, for the HTTP layer and the UI.
#[must_use]
pub const fn capability_state_str(state: CapabilityState) -> &'static str {
    match state {
        CapabilityState::Supported => "supported",
        CapabilityState::Experimental => "experimental",
        CapabilityState::Unsupported => "unsupported",
    }
}

/// What the caller knows about the current session. It only shapes the
/// report; it grants nothing.
#[derive(Clone, Copy, Debug)]
pub enum SessionContext<'a> {
    /// No active connection: the preview works offline.
    Offline,
    Connected {
        access_mode: AccessMode,
        capabilities: &'a ZabbixCapabilities,
    },
}

impl SessionContext<'_> {
    pub(crate) fn server_version(&self) -> Option<String> {
        match self {
            Self::Offline => None,
            Self::Connected { capabilities, .. } => Some(capabilities.version.to_string()),
        }
    }
}

pub(crate) struct ReportInput<'a> {
    pub log: &'a ReplaceHostLog<'a>,
    pub rows: &'a [RowView],
    pub kept: &'a KeptList,
    pub changes: &'a [FieldChange],
    /// Server version of Base.
    pub base_version: &'a str,
    pub session: SessionContext<'a>,
    /// References a fresh check found that the calculation lacks.
    pub added: &'a [FieldKey],
}

fn state_of(supported: bool) -> CapabilityState {
    if supported {
        CapabilityState::Supported
    } else {
        CapabilityState::Unsupported
    }
}

fn subject(key: &FieldKey, widgetid: Option<&String>) -> FindingSubject {
    FindingSubject {
        page_index: key.page_index,
        widget_index: key.widget_index,
        widgetid: widgetid.cloned(),
        field: Some(key.field.clone()),
    }
}

const fn blocking(code: FindingCode, group: FindingGroup) -> CompatibilityFinding {
    CompatibilityFinding::new(code, FindingSeverity::BlocksPublication, group)
}

const fn informational(code: FindingCode, group: FindingGroup) -> CompatibilityFinding {
    CompatibilityFinding::new(code, FindingSeverity::Informational, group)
}

/// The reason that explains why a row is unresolved.
fn primary_reason(row: &RowView) -> Option<ReasonCode> {
    const PRIORITY: [ReasonCode; 9] = [
        ReasonCode::NotFoundOrNoAccess,
        ReasonCode::LldInstanceMissing,
        ReasonCode::KeyNotFound,
        ReasonCode::SeveralTargetCandidates,
        ReasonCode::ValueTypeIncompatible,
        ReasonCode::TargetNameIsMask,
        ReasonCode::ItemPatternsWithoutTargetItems,
        ReasonCode::GraphNameNotFound,
        ReasonCode::PrototypeOwnerUnknown,
    ];
    PRIORITY
        .into_iter()
        .find(|reason| row.reasons.contains(reason))
        .or_else(|| row.reasons.first().copied())
}

fn contract_checks(input: &ReportInput<'_>, checks: &mut Vec<CapabilityCheck>) {
    let contracts = match input.session {
        SessionContext::Connected { capabilities, .. } => {
            Some(&capabilities.dashboard_write_contracts)
        }
        SessionContext::Offline => None,
    };
    let state = |pick: fn(&zdashstitch_zabbix::DashboardWriteContracts) -> CapabilityState| {
        contracts.map_or(CapabilityState::Unsupported, pick)
    };
    let any_type = |widget_type: &str| {
        input
            .changes
            .iter()
            .any(|change| change.widget_type == widget_type)
    };
    if input.changes.is_empty() {
        return;
    }
    checks.extend([
        CapabilityCheck {
            name: "update_one_page_with_full_fields",
            state: state(|contracts| contracts.update_one_page_with_full_fields),
            blocking: true,
        },
        CapabilityCheck {
            name: "preserve_untouched_pages",
            state: state(|contracts| contracts.preserve_untouched_pages),
            blocking: true,
        },
        CapabilityCheck {
            name: "preserve_widget_references",
            state: state(|contracts| contracts.preserve_widget_references),
            blocking: true,
        },
        CapabilityCheck {
            name: "reconcile_unknown_outcome",
            state: state(|contracts| contracts.reconcile_unknown_outcome),
            blocking: true,
        },
    ]);
    if any_type("item") {
        checks.push(CapabilityCheck {
            name: "item_widget_itemid",
            state: state(|contracts| contracts.item_widget_itemid),
            blocking: true,
        });
    }
    if any_type("graph") {
        checks.push(CapabilityCheck {
            name: WriteContract::ClassicGraphGraphid.as_str(),
            state: state(|contracts| contracts.classic_graph_graphid),
            blocking: true,
        });
    }
    let svggraph_field = |part: &str| {
        input.changes.iter().any(|change| {
            change.widget_type == "svggraph" && change.key.field.split('.').nth(2) == Some(part)
        })
    };
    if svggraph_field("itemids") {
        checks.push(CapabilityCheck {
            name: CONTRACT_SVGGRAPH_ITEMIDS,
            state: state(|contracts| contracts.svggraph_dataset_itemids),
            blocking: true,
        });
    }
    if svggraph_field("hosts") {
        checks.push(CapabilityCheck {
            name: CONTRACT_SVGGRAPH_HOSTS,
            state: state(|contracts| contracts.svggraph_dataset_hosts),
            blocking: true,
        });
    }
    let first_page = input.changes.first().map(|change| change.key.page_index);
    if input
        .changes
        .iter()
        .any(|change| Some(change.key.page_index) != first_page)
    {
        checks.push(CapabilityCheck {
            name: CONTRACT_SEVERAL_PAGES,
            state: state(|contracts| contracts.update_several_pages),
            blocking: true,
        });
    }
}

/// Builds the report. Pure.
pub(crate) fn report(input: &ReportInput<'_>) -> CompatibilityReport {
    let connected = matches!(input.session, SessionContext::Connected { .. });
    let read_write = matches!(
        input.session,
        SessionContext::Connected {
            access_mode: AccessMode::ReadWrite,
            ..
        }
    );
    let same_version = input
        .session
        .server_version()
        .is_some_and(|version| version == input.base_version);
    let mut checks = vec![
        CapabilityCheck {
            name: "connection",
            state: state_of(connected),
            blocking: true,
        },
        CapabilityCheck {
            name: "write_mode",
            state: state_of(read_write),
            blocking: true,
        },
        CapabilityCheck {
            name: "exact_server_version",
            state: state_of(same_version),
            blocking: true,
        },
    ];
    contract_checks(input, &mut checks);

    let mut findings = Vec::new();
    if !connected {
        findings.push(blocking(
            FindingCode::ConnectionRequired,
            FindingGroup::Connection,
        ));
    } else {
        if !read_write {
            findings.push(blocking(
                FindingCode::WriteModeRequired,
                FindingGroup::Connection,
            ));
        }
        if !same_version {
            findings.push(
                blocking(FindingCode::ServerVersionMismatch, FindingGroup::Connection)
                    .with_param("base_version", input.base_version),
            );
        }
    }
    const SESSION_CHECKS: usize = 3; // connection, write_mode, exact_server_version
    for check in checks.iter().skip(SESSION_CHECKS) {
        // Contracts read from the session are unknown offline (reported as
        // `connection_required`); the unproven ones fail anyway.
        let from_session = !matches!(
            check.name,
            CONTRACT_SVGGRAPH_ITEMIDS | CONTRACT_SVGGRAPH_HOSTS | CONTRACT_SEVERAL_PAGES
        );
        if check.state != CapabilityState::Supported && (connected || !from_session) {
            findings.push(
                blocking(
                    FindingCode::WriteContractUnverified,
                    FindingGroup::Connection,
                )
                .with_param("contract", check.name),
            );
        }
    }

    let calculation = &input.log.entry.calculation;
    if calculation.target_host.is_none() {
        findings.push(blocking(
            FindingCode::TargetHostNotVisible,
            FindingGroup::Bindings,
        ));
    }
    for key in input.added {
        findings.push(
            blocking(FindingCode::BindingStale, FindingGroup::Source)
                .with_subject(subject(key, None))
                .with_param("check_reason", "reference_added"),
        );
    }
    for row in input.rows {
        let row_subject = subject(&row.key, row.widgetid.as_ref());
        match row.state {
            ChangeState::Unresolved if row.unverified => findings.push(
                blocking(FindingCode::SourceUnverified, FindingGroup::Bindings)
                    .with_subject(row_subject.clone())
                    .with_reason(row.reasons.first().copied())
                    .with_param("source_kind", row.source.kind()),
            ),
            ChangeState::Unresolved => findings.push(
                blocking(FindingCode::BindingUnresolved, FindingGroup::Bindings)
                    .with_subject(row_subject.clone())
                    .with_reason(primary_reason(row))
                    .with_param("match", row.match_status.map_or("", MatchStatus::as_str)),
            ),
            ChangeState::Changed if !row.differences.is_empty() => {
                let differences = row
                    .differences
                    .iter()
                    .map(|difference| difference.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                findings.push(
                    informational(FindingCode::MatchDifferences, FindingGroup::Bindings)
                        .with_subject(row_subject.clone())
                        .with_param("differences", differences),
                );
            }
            ChangeState::KeptOnSource => findings.push(
                informational(FindingCode::KeptOnSource, FindingGroup::Widgets)
                    .with_subject(row_subject.clone())
                    .with_reason(row.kept_reason)
                    .with_param(
                        "by_decision",
                        (row.decision_state == DecisionState::Applied).to_string(),
                    ),
            ),
            _ => {}
        }
        if row.decision_state == DecisionState::Invalid {
            findings.push(
                blocking(FindingCode::DecisionInvalid, FindingGroup::Bindings)
                    .with_subject(row_subject.clone())
                    .with_param(
                        "decision",
                        row.decision
                            .as_ref()
                            .map_or("", |decision| decision.kind().as_str()),
                    ),
            );
        }
        let stale_code = match row.check {
            RowCheck::Checked => None,
            RowCheck::Stale => Some(FindingCode::BindingStale),
            RowCheck::Invalid => Some(FindingCode::BindingInvalid),
        };
        if let Some(code) = stale_code {
            findings.push(
                blocking(code, FindingGroup::Source)
                    .with_subject(row_subject)
                    .with_param(
                        "check_reason",
                        row.check_reason.map_or("", |reason| reason.as_str()),
                    ),
            );
        }
    }
    if input.changes.is_empty() {
        findings.push(blocking(FindingCode::NoChanges, FindingGroup::Plan));
    }
    if !input.kept.entries.is_empty() && !input.kept.confirmed {
        let code = if input.kept.confirmation_seq.is_some() {
            FindingCode::KeptOnSourceConfirmationStale
        } else {
            FindingCode::KeptOnSourceUnconfirmed
        };
        findings.push(
            blocking(code, FindingGroup::Widgets)
                .with_param("count", input.kept.entries.len().to_string())
                .with_param("digest", input.kept.digest.clone()),
        );
    }
    if calculation.out_of_scope > 0 {
        findings.push(
            informational(FindingCode::ReferencesOutsideScope, FindingGroup::Widgets)
                .with_param("count", calculation.out_of_scope.to_string()),
        );
    }
    for crossing in &calculation.scope_crossings {
        findings.push(
            informational(FindingCode::ScopeLinkCrossing, FindingGroup::Widgets)
                .with_subject(FindingSubject {
                    page_index: crossing.page_index,
                    widget_index: crossing.widget_index,
                    widgetid: crossing.widgetid.clone(),
                    field: Some(crossing.field.clone()),
                })
                .with_param(
                    "side",
                    match crossing.side {
                        crate::resolver::CrossingSide::ProviderOutside => "provider_outside",
                        crate::resolver::CrossingSide::ConsumerOutside => "consumer_outside",
                    },
                ),
        );
    }
    CompatibilityReport { checks, findings }
}
