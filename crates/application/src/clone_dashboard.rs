// SPDX-License-Identifier: MIT OR Apache-2.0

//! Typed local intent for CloneDashboard (P3-01A).
//!
//! This module binds the user's requested name and frozen target-area
//! selection to one dashboard workcopy, its current Base/Local fingerprints
//! and one current clone-mapping revision. It builds no create plan, owns no
//! credentials and has no Zabbix transport.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use zdashstitch_zabbix::{
    CreateExpectation, HostListing, ReadBackVerdict, VersionedDashboardSnapshot,
    check_created_dashboard, is_valid_dashboard_name,
};

use crate::clone_mapping::{
    CloneCardinalityTransform, CloneMappingState, FrozenTargetSelection, SourceHostDecision,
    complete_clone_series_colors, materialize_clone,
};
use crate::resolver::{DASHBOARD_REFERENCE, LinkProvider, WidgetAddress, source_model};
use crate::workcopy::operations::Operation;
use crate::workcopy::{DraftKind, WorkcopyError, WorkcopyService};

const CLONE_DRAFTS: &str = "clone-workcopies";
const CLONE_DRAFT_FORMAT: &str = "zdashstitch.clone-workcopy/v0";
pub(crate) const CLONE_OPERATION: &str = "clone_dashboard";

/// Structural outcome of comparing the observed copy with the exact
/// expectation made by the common create planner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloneReadBackVerdict {
    IntentMismatch,
    SourceDashboard,
    Copy(ReadBackVerdict),
}

/// Facts selected by the user. Search filters are deliberately absent: the
/// target is the frozen ordered list returned by P3-02B.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrepareCloneIntentRequest {
    pub source_workcopy_id: String,
    pub target_connection_id: String,
    pub target_name: String,
    pub target_selection: FrozenTargetSelection,
    pub expected_mapping_revision: u64,
}

/// Immutable identity of the source draft and dashboard at intent creation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CloneSourceIdentity {
    pub source_workcopy_id: String,
    pub source_workcopy_revision: u64,
    pub connection_id: String,
    pub dashboardid: String,
    pub base_fingerprint: String,
    pub local_fingerprint: String,
}

/// Frozen target-area selection. `connection_id` is always the source
/// connection; cross-connection Clone cannot construct an intent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CloneTargetAreaSelection {
    pub connection_id: String,
    pub hosts: Vec<HostListing>,
}

/// Validated user intent. This is not a write plan and is intentionally not
/// deserializable: later P3-01 preflight must still re-read and verify every
/// publication capability and binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloneDashboardIntent {
    source: CloneSourceIdentity,
    target: CloneTargetAreaSelection,
    target_name: String,
    mapping_revision: u64,
    server_identity_fingerprint: String,
    server_version: String,
}

/// An isolated, uncommitted Clone branch. Its Base is immutable and the
/// branch is held by its owner until the Clone workflow adopts it.
/// Dropping it discards the branch without deleting a stored workcopy.
#[must_use]
pub struct CloneWorkcopy {
    intent: CloneDashboardIntent,
    base: VersionedDashboardSnapshot,
    mapping: CloneMappingState,
}

/// A committed local branch. Only an uncommitted branch can be cancelled.
pub struct SavedCloneWorkcopy {
    local_id: String,
    draft: CloneWorkcopy,
    local: VersionedDashboardSnapshot,
}

impl SavedCloneWorkcopy {
    #[must_use]
    pub fn local_id(&self) -> &str {
        &self.local_id
    }

    pub const fn draft(&self) -> &CloneWorkcopy {
        &self.draft
    }

    #[must_use]
    pub const fn local(&self) -> &VersionedDashboardSnapshot {
        &self.local
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CloneWorkcopyFile {
    format: String,
    local_id: String,
    source: CloneSourceIdentity,
    target: CloneTargetAreaSelection,
    target_name: String,
    base_storage_fingerprint: String,
    local_storage_fingerprint: String,
    mapping: CloneMappingState,
}

impl CloneWorkcopy {
    #[must_use]
    pub const fn intent(&self) -> &CloneDashboardIntent {
        &self.intent
    }

    #[must_use]
    pub const fn base(&self) -> &VersionedDashboardSnapshot {
        &self.base
    }

    #[must_use]
    pub const fn mapping(&self) -> &CloneMappingState {
        &self.mapping
    }

    /// Discards the branch and returns the selection so the user can retry.
    #[must_use]
    pub fn cancel(self) -> CloneDashboardIntent {
        self.intent
    }
}

impl CloneDashboardIntent {
    #[must_use]
    pub const fn source(&self) -> &CloneSourceIdentity {
        &self.source
    }

    #[must_use]
    pub const fn target(&self) -> &CloneTargetAreaSelection {
        &self.target
    }

    #[must_use]
    pub fn target_name(&self) -> &str {
        &self.target_name
    }

    #[must_use]
    pub const fn mapping_revision(&self) -> u64 {
        self.mapping_revision
    }

    #[must_use]
    pub fn server_identity_fingerprint(&self) -> &str {
        &self.server_identity_fingerprint
    }

    #[must_use]
    pub fn server_version(&self) -> &str {
        &self.server_version
    }

    /// The adapter ignores assigned IDs and normalizes field/widget order
    /// while retaining the full expected content and access checks.
    #[must_use]
    pub fn compare_read_back(
        &self,
        expectation: &CreateExpectation,
        observed: &VersionedDashboardSnapshot,
    ) -> CloneReadBackVerdict {
        if expectation.connection_id != self.source.connection_id
            || expectation.name != self.target_name
            || expectation.server_version.to_string() != self.server_version
        {
            return CloneReadBackVerdict::IntentMismatch;
        }
        let verdict = check_created_dashboard(observed, expectation);
        if verdict != ReadBackVerdict::ConnectionDiffers
            && observed.dashboard_id() == self.source.dashboardid
        {
            return CloneReadBackVerdict::SourceDashboard;
        }
        CloneReadBackVerdict::Copy(verdict)
    }
}

fn selection_matches_mapping(
    selected: &[HostListing],
    mappings: &[crate::clone_mapping::SourceHostMapping],
) -> bool {
    let selected_by_id = selected
        .iter()
        .enumerate()
        .map(|(position, host)| (host.hostid.as_str(), (position, host)))
        .collect::<BTreeMap<_, _>>();
    let mut used = BTreeSet::new();
    for mapping in mappings {
        let SourceHostDecision::TargetSlots(slots) = &mapping.decision else {
            continue;
        };
        let mut previous = None;
        for slot in slots {
            let Some((position, selected_host)) = selected_by_id.get(slot.host.hostid.as_str())
            else {
                return false;
            };
            if selected_host.host != slot.host.host
                || selected_host.name != slot.host.name
                || previous.is_some_and(|previous| previous >= *position)
            {
                return false;
            }
            previous = Some(*position);
            used.insert(slot.host.hostid.as_str());
        }
    }
    !used.is_empty() && used.len() == selected.len()
}

/// Checks the reference graph of the full dashboard before it can be
/// copied. Reference values remain local to the copy; the create codec
/// discards the source widget IDs separately.
pub fn validate_clone_references(local: &VersionedDashboardSnapshot) -> Result<(), WorkcopyError> {
    let registry = zdashstitch_zabbix::AdapterRegistry::widget_capabilities(local.server_version())
        .map_err(|_| WorkcopyError::new("clone_reference_adapter_missing"))?;
    let model =
        source_model(local, &registry).map_err(|error| WorkcopyError::from_resolver(&error))?;
    let links = &model.links;
    if !links.duplicate_definitions().is_empty()
        || links
            .definitions
            .iter()
            .any(|definition| definition.reference == DASHBOARD_REFERENCE)
    {
        return Err(WorkcopyError::new("clone_reference_duplicate"));
    }
    let widget_ids = model
        .widgets
        .iter()
        .filter_map(|widget| widget.widget.widgetid.as_deref())
        .collect::<BTreeSet<_>>();
    let mut inbound = BTreeMap::<&WidgetAddress, usize>::new();
    let mut consumers = BTreeMap::<&WidgetAddress, Vec<&WidgetAddress>>::new();
    for link in &links.links {
        if widget_ids.contains(link.reference.as_str()) {
            return Err(WorkcopyError::new("clone_reference_source_widget_id"));
        }
        match &link.provider {
            LinkProvider::Missing => return Err(WorkcopyError::new("clone_reference_missing")),
            LinkProvider::Duplicate(_) => {
                return Err(WorkcopyError::new("clone_reference_duplicate"));
            }
            LinkProvider::Dashboard => {}
            LinkProvider::Widget(provider) => {
                inbound.entry(provider).or_default();
                *inbound.entry(&link.consumer).or_default() += 1;
                consumers.entry(provider).or_default().push(&link.consumer);
            }
        }
    }
    // Kahn's algorithm keeps stack usage independent of dashboard size.
    let mut ready = inbound
        .iter()
        .filter_map(|(&widget, &degree)| (degree == 0).then_some(widget))
        .collect::<Vec<_>>();
    let mut visited = 0;
    while let Some(provider) = ready.pop() {
        visited += 1;
        for consumer in consumers.get(provider).into_iter().flatten() {
            if let Some(degree) = inbound.get_mut(consumer) {
                *degree -= 1;
                if *degree == 0 {
                    ready.push(consumer);
                }
            }
        }
    }
    if visited != inbound.len() {
        return Err(WorkcopyError::new("clone_reference_cycle"));
    }
    Ok(())
}

impl WorkcopyService {
    /// Atomically pins the branch in a separate local-only namespace.
    pub fn save_clone_workcopy(
        &self,
        draft: CloneWorkcopy,
        cardinalities: &[CloneCardinalityTransform],
    ) -> Result<SavedCloneWorkcopy, WorkcopyError> {
        // Reject intents that changed while the transient branch was open.
        let source = self.local_snapshot(&draft.intent.source().source_workcopy_id)?;
        let current = self.begin_clone_workcopy(draft.intent.clone())?;
        if current.mapping != draft.mapping {
            return Err(WorkcopyError::new("clone_mapping_changed"));
        }
        let mut cardinalities = cardinalities.to_vec();
        complete_clone_series_colors(&source, &mut cardinalities, &[])
            .map_err(|error| WorkcopyError::new(error.code()))?;
        let local = materialize_clone(
            &source,
            &draft.mapping.mappings,
            &cardinalities,
            &draft.mapping.pattern_decisions,
        )
        .map_err(|error| WorkcopyError::new(error.code()))?;
        if local.fingerprint() != draft.intent.source().local_fingerprint {
            return Err(WorkcopyError::new("clone_local_changed"));
        }
        let local_id = crate::workcopy::new_workcopy_id()?;
        let record = CloneWorkcopyFile {
            format: CLONE_DRAFT_FORMAT.to_owned(),
            local_id: local_id.clone(),
            source: draft.intent.source.clone(),
            target: draft.intent.target.clone(),
            target_name: draft.intent.target_name.clone(),
            base_storage_fingerprint: draft.base.storage_fingerprint().to_owned(),
            local_storage_fingerprint: local.storage_fingerprint().to_owned(),
            mapping: draft.mapping.clone(),
        };
        let raw = serde_json::to_vec(&record)
            .map_err(|_| WorkcopyError::new("clone_workcopy_corrupt"))?;
        let local_raw = serde_json::to_vec(local.raw_api_document())
            .map_err(|_| WorkcopyError::new("clone_workcopy_corrupt"))?;
        self.store
            .create_local_draft(CLONE_DRAFTS, &local_id, &raw, Some(&local_raw))?;
        Ok(SavedCloneWorkcopy {
            local_id,
            draft,
            local,
        })
    }

    /// Reopens the saved branch against its pinned Base, even if the source
    /// workcopy has since changed. Publication must check freshness separately.
    pub fn open_clone_workcopy(&self, local_id: &str) -> Result<SavedCloneWorkcopy, WorkcopyError> {
        use crate::workcopy::{is_connection_id, is_fingerprint, is_workcopy_id};
        let corrupt = || WorkcopyError::new("clone_workcopy_corrupt");
        let raw = self.store.load_local_draft(CLONE_DRAFTS, local_id)?;
        crate::workcopy::formats::check_format(
            &raw,
            CLONE_DRAFT_FORMAT,
            "zdashstitch.clone-workcopy/",
            "clone_workcopy_unsupported_version",
            "clone_workcopy_corrupt",
        )?;
        let record: CloneWorkcopyFile = serde_json::from_slice(&raw).map_err(|_| corrupt())?;
        let source = record.source;
        let target = record.target;
        let mapping = record.mapping;
        if record.local_id != local_id
            || !is_workcopy_id(&source.source_workcopy_id)
            || source.source_workcopy_id == local_id
            || !is_connection_id(&source.connection_id)
            || !zdashstitch_zabbix::is_zabbix_id(&source.dashboardid)
            || !is_fingerprint(&record.base_storage_fingerprint)
            || !is_fingerprint(&record.local_storage_fingerprint)
            || !is_valid_dashboard_name(&record.target_name)
            || !mapping.is_well_formed()
            || mapping.connection_id != source.connection_id
            || target.connection_id != source.connection_id
            || mapping.mapping_revision != source.source_workcopy_revision
            || mapping.base_fingerprint.as_deref() != Some(&source.base_fingerprint)
            || mapping.local_fingerprint.as_deref() != Some(&source.local_fingerprint)
            || FrozenTargetSelection::new(target.hosts.clone()).is_err()
            || !selection_matches_mapping(&target.hosts, &mapping.mappings)
        {
            return Err(corrupt());
        }
        let (_, base) = self.store.load_snapshot(
            &source.connection_id,
            &source.dashboardid,
            &record.base_storage_fingerprint,
        )?;
        if base.fingerprint() != source.base_fingerprint
            || base.server_version().to_string() != mapping.server_version
        {
            return Err(corrupt());
        }
        let local_raw = self.store.load_draft_snapshot(CLONE_DRAFTS, local_id)?;
        let local = VersionedDashboardSnapshot::from_adapter_raw(
            source.connection_id.clone(),
            base.server_version().clone(),
            serde_json::from_slice(&local_raw).map_err(|_| corrupt())?,
        )
        .map_err(|_| corrupt())?;
        if local.dashboard_id() != source.dashboardid
            || local.fingerprint() != source.local_fingerprint
            || local.storage_fingerprint() != record.local_storage_fingerprint
        {
            return Err(corrupt());
        }
        Ok(SavedCloneWorkcopy {
            local_id: local_id.to_owned(),
            local,
            draft: CloneWorkcopy {
                intent: CloneDashboardIntent {
                    source,
                    target,
                    target_name: record.target_name,
                    mapping_revision: mapping.mapping_revision,
                    server_identity_fingerprint: mapping.server_identity_fingerprint.clone(),
                    server_version: mapping.server_version.clone(),
                },
                base,
                mapping,
            },
        })
    }

    /// Forks the exact source revision and mapping from a single local read.
    pub fn begin_clone_workcopy(
        &self,
        intent: CloneDashboardIntent,
    ) -> Result<CloneWorkcopy, WorkcopyError> {
        let source = intent.source();
        let (file, base, mut operations) = self.load_log(&source.source_workcopy_id)?;
        if file.revision != source.source_workcopy_revision
            || file.kind != DraftKind::Dashboard
            || file.source.connection_id != source.connection_id
            || file.source.dashboardid != source.dashboardid
            || base.fingerprint() != source.base_fingerprint
        {
            return Err(WorkcopyError::new("clone_intent_source_changed"));
        }
        let Some(Operation::CloneMapping(entry)) = operations.pop() else {
            return Err(WorkcopyError::new("clone_mapping_changed"));
        };
        let mapping = entry.state;
        if mapping.mapping_revision != intent.mapping_revision()
            || mapping.base_fingerprint.as_deref() != Some(&source.base_fingerprint)
            || mapping.local_fingerprint.as_deref() != Some(&source.local_fingerprint)
            || mapping.connection_id != source.connection_id
            || mapping.server_identity_fingerprint != intent.server_identity_fingerprint()
            || mapping.server_version != intent.server_version()
            || !selection_matches_mapping(&intent.target.hosts, &mapping.mappings)
        {
            return Err(WorkcopyError::new("clone_mapping_changed"));
        }
        Ok(CloneWorkcopy {
            intent,
            base,
            mapping,
        })
    }

    /// Build a local Clone intent from the current workcopy and its latest
    /// mapping. The method performs no server read and creates no plan.
    pub fn prepare_clone_intent(
        &self,
        request: PrepareCloneIntentRequest,
    ) -> Result<CloneDashboardIntent, WorkcopyError> {
        let (file, base, operations) = self.load_log(&request.source_workcopy_id)?;
        if file.kind != DraftKind::Dashboard {
            return Err(WorkcopyError::new("clone_intent_source_kind_invalid"));
        }
        if request.target_connection_id != file.source.connection_id {
            return Err(WorkcopyError::new("clone_intent_connection_changed"));
        }
        if !is_valid_dashboard_name(&request.target_name) {
            return Err(WorkcopyError::new("clone_intent_target_name_invalid"));
        }
        let mapping = operations
            .iter()
            .rev()
            .find_map(|operation| match operation {
                Operation::CloneMapping(entry) => Some(entry),
                _ => None,
            })
            .ok_or_else(|| WorkcopyError::new("clone_mapping_not_found"))?;
        if operations.last().map(Operation::seq) != Some(mapping.seq)
            || mapping.state.mapping_revision != request.expected_mapping_revision
            || mapping.state.mapping_revision != file.revision
        {
            return Err(WorkcopyError::new("clone_mapping_changed"));
        }
        if mapping.state.connection_id != file.source.connection_id {
            return Err(WorkcopyError::new("clone_intent_connection_changed"));
        }
        if mapping.state.server_version != file.source.server_version
            || mapping.state.server_version != base.server_version().to_string()
        {
            return Err(WorkcopyError::new("clone_intent_server_changed"));
        }
        if mapping.state.base_fingerprint.as_deref() != Some(base.fingerprint()) {
            return Err(WorkcopyError::new("clone_intent_base_changed"));
        }
        let local_fingerprint = mapping
            .state
            .local_fingerprint
            .clone()
            .ok_or_else(|| WorkcopyError::new("clone_intent_local_missing"))?;
        if !selection_matches_mapping(request.target_selection.hosts(), &mapping.state.mappings) {
            return Err(WorkcopyError::new("clone_intent_target_selection_changed"));
        }

        Ok(CloneDashboardIntent {
            source: CloneSourceIdentity {
                source_workcopy_id: file.workcopy_id,
                source_workcopy_revision: file.revision,
                connection_id: file.source.connection_id.clone(),
                dashboardid: file.source.dashboardid,
                base_fingerprint: base.fingerprint().to_owned(),
                local_fingerprint,
            },
            target: CloneTargetAreaSelection {
                connection_id: file.source.connection_id,
                hosts: request.target_selection.hosts().to_vec(),
            },
            target_name: request.target_name,
            mapping_revision: mapping.state.mapping_revision,
            server_identity_fingerprint: mapping.state.server_identity_fingerprint.clone(),
            server_version: mapping.state.server_version.clone(),
        })
    }
}
