// SPDX-License-Identifier: MIT OR Apache-2.0

//! Server-side preparation of a Clone from explicit GUI choices.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zdashstitch_zabbix::{
    AdapterRegistry, HostListing, HostObjects, HostSearchFilter, ReadOnlyGateway,
    VersionedDashboardSnapshot, is_valid_dashboard_name, is_zabbix_id,
};

use crate::clone_dashboard::{
    PrepareCloneIntentRequest, SavedCloneWorkcopy, validate_clone_references,
};
use crate::clone_mapping::{
    CloneBindingCheck, CloneCardinalityTransform, ClonePatternDecision, ClonePatternRequirement,
    FrozenTargetSelection, ReferenceMapping, SaveCloneMappingRequest, SourceHostDecision,
    SourceHostMapping, SourceRoleId, TargetMetricResolution, TargetSlot, TargetSlotId,
    complete_clone_series_colors, materialize_clone, pattern_requirements,
};
use crate::resolver::{
    Applicability, HostInventory, ReplaceHostRequest, ResolutionRow, ResolveScope, SourceModel,
    collect_source_facts, collect_target_facts, host_inventory, resolve_replace_host, source_model,
};
use crate::workcopy::{DraftKind, WorkcopyError, WorkcopyService};

mod growth;
mod lld;
mod patterns;
pub use growth::{CloneGrowthPreview, CloneSeriesOption};
use growth::{CloneGrowthRequest, Growth, GrowthSelection};
use lld::CloneLldChoice;
pub use lld::CloneLldRow;
use patterns::ClonePatternChoice;
pub use patterns::{ClonePatternReview, ClonePatternReviewRequest};

pub struct CloneSetup {
    pub revision: u64,
    pub source_fingerprint: String,
    pub inventory: HostInventory,
    pub patterns: Vec<ClonePatternRequirement>,
    pub growth_samples: Vec<CloneSeriesOption>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloneSetupRequest {
    pub revision: u64,
    pub source_fingerprint: String,
    pub name: String,
    /// Explicit source host ID to target host ID; null means keep source.
    pub hosts: BTreeMap<String, Option<String>>,
    #[serde(default)]
    pub keep_patterns: bool,
    #[serde(default)]
    pub pattern_choices: Vec<ClonePatternChoice>,
    pub growth: Option<CloneGrowthRequest>,
    pub growth_fingerprint: Option<String>,
    #[serde(default)]
    pub lld_choices: Vec<CloneLldChoice>,
}

struct SetupMappings {
    source: VersionedDashboardSnapshot,
    patterns: Vec<ClonePatternRequirement>,
    mappings: Vec<SourceHostMapping>,
    selected: Vec<HostListing>,
    growth: Option<GrowthSelection>,
}

struct PreparedSetup {
    mappings: Vec<SourceHostMapping>,
    pattern_decisions: Vec<ClonePatternDecision>,
    target_selection: FrozenTargetSelection,
    cardinalities: Vec<CloneCardinalityTransform>,
    local_fingerprint: String,
    growth_preview: Option<CloneGrowthPreview>,
}

fn fingerprint(value: &impl Serialize) -> Result<String, WorkcopyError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| WorkcopyError::new("clone_setup_evidence_invalid"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn target_resolution(slot_id: TargetSlotId, row: &mut ResolutionRow) -> TargetMetricResolution {
    TargetMetricResolution {
        slot_id,
        target: row.target.take(),
        match_status: row.match_status,
        applicability: row.applicability,
        reasons: std::mem::take(&mut row.reasons),
        differences: std::mem::take(&mut row.differences),
        evidence: row.target_evidence.take(),
        candidates: std::mem::take(&mut row.candidates),
        decision: None,
        check: CloneBindingCheck::Checked,
        check_reason: None,
    }
}

impl WorkcopyService {
    pub async fn clone_target_hosts(
        &self,
        reader: &ReadOnlyGateway,
        search: &str,
    ) -> Result<HostObjects<HostListing>, WorkcopyError> {
        reader
            .search_hosts_filtered(
                &HostSearchFilter {
                    name: search.to_owned(),
                    ..HostSearchFilter::default()
                },
                100,
            )
            .await
            .map_err(|error| WorkcopyError::from_zabbix(&error))
    }

    fn clone_setup_source(
        &self,
        local_id: &str,
        connection_id: &str,
    ) -> Result<(u64, VersionedDashboardSnapshot, SourceModel), WorkcopyError> {
        let (file, base, operations) = self.load_log(local_id)?;
        if file.kind != DraftKind::Dashboard {
            return Err(WorkcopyError::new("clone_intent_source_kind_invalid"));
        }
        if file.source.connection_id != connection_id {
            return Err(WorkcopyError::new("workcopy_other_connection"));
        }
        let local = crate::replace_host::materialize_local_operations(base, &operations)?;
        let registry = AdapterRegistry::widget_capabilities(local.server_version())
            .map_err(|_| WorkcopyError::new("clone_preview_adapter_missing"))?;
        let model = source_model(&local, &registry)
            .map_err(|error| WorkcopyError::from_resolver(&error))?;
        Ok((file.revision, local, model))
    }

    pub async fn clone_setup(
        &self,
        reader: &ReadOnlyGateway,
        connection_id: &str,
        local_id: &str,
    ) -> Result<CloneSetup, WorkcopyError> {
        let (revision, local, model) = self.clone_setup_source(local_id, connection_id)?;
        if local.server_version() != &reader.capabilities().version {
            return Err(WorkcopyError::new("clone_intent_server_changed"));
        }
        let facts = collect_source_facts(reader, &model)
            .await
            .map_err(|error| WorkcopyError::from_resolver(&error))?;
        Ok(CloneSetup {
            revision,
            source_fingerprint: local.fingerprint().to_owned(),
            inventory: host_inventory(&model, &facts),
            patterns: pattern_requirements(&model),
            growth_samples: growth::samples(&local, &model, &facts),
        })
    }

    pub async fn preview_clone_growth(
        &self,
        reader: &ReadOnlyGateway,
        connection_id: &str,
        local_id: &str,
        request: CloneSetupRequest,
    ) -> Result<CloneGrowthPreview, WorkcopyError> {
        self.prepare_clone_setup(reader, connection_id, local_id, &request)
            .await?
            .growth_preview
            .ok_or_else(|| WorkcopyError::new("clone_growth_selection_invalid"))
    }

    pub async fn clone_lld_candidates(
        &self,
        reader: &ReadOnlyGateway,
        connection_id: &str,
        local_id: &str,
        request: CloneSetupRequest,
    ) -> Result<Vec<CloneLldRow>, WorkcopyError> {
        let setup = self
            .load_setup_mappings(reader, connection_id, local_id, &request)
            .await?;
        lld::rows(reader, &request, &setup.mappings)
    }

    pub async fn preview_clone_setup_pattern(
        &self,
        reader: &ReadOnlyGateway,
        connection_id: &str,
        local_id: &str,
        request: ClonePatternReviewRequest,
    ) -> Result<ClonePatternReview, WorkcopyError> {
        let setup = self
            .load_setup_mappings(reader, connection_id, local_id, &request.setup)
            .await?;
        patterns::review(reader, &request, &setup).await
    }

    pub async fn save_clone_from_setup(
        &self,
        reader: &ReadOnlyGateway,
        connection_id: &str,
        local_id: &str,
        request: CloneSetupRequest,
    ) -> Result<SavedCloneWorkcopy, WorkcopyError> {
        let setup = self
            .prepare_clone_setup(reader, connection_id, local_id, &request)
            .await?;
        if setup
            .growth_preview
            .as_ref()
            .map(|preview| &preview.fingerprint)
            != request.growth_fingerprint.as_ref()
        {
            return Err(WorkcopyError::new("clone_growth_preview_required"));
        }
        let mapping = self.save_clone_mapping(
            local_id,
            SaveCloneMappingRequest {
                expected_workcopy_revision: request.revision,
                connection_id: connection_id.to_owned(),
                server_identity_fingerprint: reader.identity().fingerprint().to_owned(),
                server_version: reader.capabilities().version.to_string(),
                local_fingerprint: Some(setup.local_fingerprint),
                mappings: setup.mappings,
                pattern_decisions: setup.pattern_decisions,
            },
        )?;
        let intent = self.prepare_clone_intent(PrepareCloneIntentRequest {
            source_workcopy_id: local_id.to_owned(),
            target_connection_id: connection_id.to_owned(),
            target_name: request.name,
            target_selection: setup.target_selection,
            expected_mapping_revision: mapping.state.mapping_revision,
        })?;
        let draft = self.begin_clone_workcopy(intent)?;
        self.save_clone_workcopy(draft, &setup.cardinalities)
    }

    async fn prepare_clone_setup(
        &self,
        reader: &ReadOnlyGateway,
        connection_id: &str,
        local_id: &str,
        request: &CloneSetupRequest,
    ) -> Result<PreparedSetup, WorkcopyError> {
        if !is_valid_dashboard_name(&request.name) {
            return Err(WorkcopyError::new("clone_intent_target_name_invalid"));
        }
        let mut setup = self
            .load_setup_mappings(reader, connection_id, local_id, request)
            .await?;
        lld::apply(reader, request, &mut setup.mappings)?;
        let pattern_decisions = patterns::apply(reader, request, &setup).await?;
        let SetupMappings {
            source,
            mappings,
            selected,
            growth,
            ..
        } = setup;
        let mut cardinalities = Vec::new();
        let mut growth_preview = None;
        if let Some(growth) = growth {
            let Growth { transform, preview } = growth.plan(&mappings)?;
            cardinalities.push(transform);
            growth_preview = Some(preview);
        }
        complete_clone_series_colors(&source, &mut cardinalities, &[])
            .map_err(|error| WorkcopyError::new(error.code()))?;
        let target_selection = FrozenTargetSelection::new(selected)
            .map_err(|error| WorkcopyError::new(error.code()))?;
        let local = materialize_clone(&source, &mappings, &cardinalities, &pattern_decisions)
            .map_err(|error| WorkcopyError::new(error.code()))?;
        validate_clone_references(&local)?;
        if let Some(preview) = &mut growth_preview {
            preview.bind_to(request, local.fingerprint())?;
        }
        Ok(PreparedSetup {
            mappings,
            pattern_decisions,
            target_selection,
            cardinalities,
            local_fingerprint: local.fingerprint().to_owned(),
            growth_preview,
        })
    }

    async fn load_setup_mappings(
        &self,
        reader: &ReadOnlyGateway,
        connection_id: &str,
        local_id: &str,
        request: &CloneSetupRequest,
    ) -> Result<SetupMappings, WorkcopyError> {
        let (revision, source, model) = self.clone_setup_source(local_id, connection_id)?;
        if revision != request.revision || source.fingerprint() != request.source_fingerprint {
            return Err(WorkcopyError::new("clone_intent_source_changed"));
        }
        if source.server_version() != &reader.capabilities().version {
            return Err(WorkcopyError::new("clone_intent_server_changed"));
        }
        let facts = collect_source_facts(reader, &model)
            .await
            .map_err(|error| WorkcopyError::from_resolver(&error))?;
        let inventory = host_inventory(&model, &facts);
        if !inventory.unattributed.is_empty() {
            return Err(WorkcopyError::new("clone_source_dependencies_unverified"));
        }
        if request
            .hosts
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != inventory
                .hosts
                .iter()
                .map(|host| host.hostid.as_str())
                .collect()
        {
            return Err(WorkcopyError::new("clone_mapping_incomplete"));
        }
        let target_ids: Vec<_> = request.hosts.values().flatten().collect();
        if target_ids.is_empty()
            || target_ids.iter().any(|id| !is_zabbix_id(id))
            || target_ids.iter().collect::<BTreeSet<_>>().len() != target_ids.len()
        {
            return Err(WorkcopyError::new("clone_mapping_invalid"));
        }
        let patterns = pattern_requirements(&model);
        let pattern_fields = patterns
            .iter()
            .map(|entry| &entry.field)
            .collect::<BTreeSet<_>>();
        let mut mappings = Vec::new();
        let mut selected = Vec::<HostListing>::new();
        for (source_id, target_id) in &request.hosts {
            // The lookup returns the source rows too. For keep decisions
            // the selected comparison target is used only for that read.
            let comparison = target_id
                .as_ref()
                .or_else(|| target_ids.iter().copied().find(|id| *id != source_id))
                .or_else(|| {
                    inventory
                        .hosts
                        .iter()
                        .find(|host| &host.hostid != source_id)
                        .map(|host| &host.hostid)
                })
                .ok_or_else(|| WorkcopyError::new("clone_mapping_invalid"))?;
            let target = collect_target_facts(reader, &model, source_id, comparison)
                .await
                .map_err(|error| WorkcopyError::from_resolver(&error))?;
            let resolved = resolve_replace_host(
                &model,
                &ReplaceHostRequest {
                    source_hostid: source_id.clone(),
                    target_hostid: comparison.clone(),
                    scope: ResolveScope::Dashboard,
                },
                &facts,
                &target,
            )
            .map_err(|error| WorkcopyError::from_resolver(&error))?;
            let slot_id = TargetSlotId(format!("host-{source_id}"));
            let decision = if target_id.is_some() {
                let target = resolved
                    .target_host
                    .ok_or_else(|| WorkcopyError::new("clone_target_not_visible"))?;
                selected.push(HostListing {
                    hostid: target.hostid.clone(),
                    host: target.host.clone(),
                    name: target.name.clone(),
                    groups: vec![],
                    templates: vec![],
                });
                SourceHostDecision::TargetSlots(vec![TargetSlot {
                    slot_id: slot_id.clone(),
                    host: target,
                }])
            } else {
                SourceHostDecision::KeepSource
            };
            let references = resolved
                .rows
                .into_iter()
                .filter(|row| {
                    row.applicability == Applicability::Editable
                        && !pattern_fields.contains(&row.field)
                })
                .map(|mut row| {
                    let targets = target_id
                        .is_some()
                        .then(|| target_resolution(slot_id.clone(), &mut row))
                        .into_iter()
                        .collect();
                    ReferenceMapping {
                        field: row.field,
                        widget_type: row.widget_type,
                        source: row.source,
                        source_evidence: row.source_evidence,
                        targets,
                    }
                })
                .collect();
            mappings.push(SourceHostMapping {
                role_id: SourceRoleId(format!("host-{source_id}")),
                source_host: resolved.source_host,
                decision,
                references,
            });
        }
        let growth = if let Some(input) = &request.growth {
            let options = growth::samples(&source, &model, &facts);
            Some(
                GrowthSelection::add_slot(
                    input,
                    &options,
                    &model,
                    &facts,
                    reader,
                    &mut mappings,
                    &mut selected,
                )
                .await?,
            )
        } else {
            None
        };
        Ok(SetupMappings {
            source,
            patterns,
            mappings,
            selected,
            growth,
        })
    }
}
