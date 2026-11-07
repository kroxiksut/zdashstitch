// SPDX-License-Identifier: MIT OR Apache-2.0

//! Local preparation and read-only revalidation for a pinned Clone branch.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use zdashstitch_zabbix::{
    AdapterRegistry, CreateScope, ReadOnlyGateway, VersionedDashboardSnapshot,
    create_requirements_for,
};

use crate::clone_dashboard::{SavedCloneWorkcopy, validate_clone_references};
use crate::clone_mapping::{
    ClonePatternAction, CloneRevalidationContext, SourceHostDecision,
    revalidate_clone_mapping_from_source,
};
use crate::resolver::{
    Applicability, FieldAddress, SourceKind, collect_source_facts, host_inventory, source_model,
};
use crate::workcopy::{WorkcopyError, WorkcopyService};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CloneKeptSource {
    pub field: FieldAddress,
    pub explicit_keep: bool,
}

/// Local preview evidence. A preview carries no write authority.
#[must_use]
pub struct CloneLocalPreview {
    local: VersionedDashboardSnapshot,
    kept: Vec<CloneKeptSource>,
    kept_digest: Option<String>,
    missing_contracts: Vec<&'static str>,
}

impl CloneLocalPreview {
    #[must_use]
    pub const fn local(&self) -> &VersionedDashboardSnapshot {
        &self.local
    }

    #[must_use]
    pub fn kept_on_source(&self) -> &[CloneKeptSource] {
        &self.kept
    }

    #[must_use]
    pub fn kept_digest(&self) -> Option<&str> {
        self.kept_digest.as_deref()
    }

    #[must_use]
    pub fn missing_contracts(&self) -> &[&'static str] {
        &self.missing_contracts
    }

    /// Checks the acknowledgement of this exact list and the local codec
    /// requirements. Live write permission is checked separately.
    pub fn check_confirmation(&self, kept_digest: Option<&str>) -> Result<(), WorkcopyError> {
        if let Some(contract) = self.missing_contracts.first() {
            return Err(WorkcopyError::new("write_contract_unverified").with("contract", *contract));
        }
        if self.kept_digest() != kept_digest {
            return Err(WorkcopyError::new("clone_kept_confirmation_required"));
        }
        Ok(())
    }
}

fn number(value: &Value) -> Option<usize> {
    value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .or_else(|| value.as_str()?.parse().ok())
}

fn check_layout(local: &VersionedDashboardSnapshot) -> Result<(), WorkcopyError> {
    const COLUMNS: usize = 72;
    const ROWS: usize = 64;
    let invalid = || WorkcopyError::new("clone_layout_invalid");
    let pages = local.raw_api_document()["pages"]
        .as_array()
        .ok_or_else(invalid)?;
    for page in pages {
        let mut occupied = [0_u128; ROWS];
        for widget in page["widgets"].as_array().ok_or_else(invalid)? {
            let x = number(&widget["x"]).ok_or_else(invalid)?;
            let y = number(&widget["y"]).ok_or_else(invalid)?;
            let width = number(&widget["width"]).ok_or_else(invalid)?;
            let height = number(&widget["height"]).ok_or_else(invalid)?;
            if width == 0
                || height == 0
                || x.saturating_add(width) > COLUMNS
                || y.saturating_add(height) > ROWS
            {
                return Err(invalid());
            }
            let span = ((1_u128 << width) - 1) << x;
            for row in &mut occupied[y..y + height] {
                if *row & span != 0 {
                    return Err(WorkcopyError::new("clone_layout_overlap"));
                }
                *row |= span;
            }
        }
    }
    Ok(())
}

pub(crate) fn kept_sources(
    source: &VersionedDashboardSnapshot,
    branch: &SavedCloneWorkcopy,
) -> Result<Vec<CloneKeptSource>, WorkcopyError> {
    let mapping = branch.draft().mapping();
    let registry = AdapterRegistry::widget_capabilities(source.server_version())
        .map_err(|_| WorkcopyError::new("clone_preview_adapter_missing"))?;
    let model =
        source_model(source, &registry).map_err(|error| WorkcopyError::from_resolver(&error))?;
    let patterns = mapping
        .pattern_decisions
        .iter()
        .map(|decision| (&decision.requirement.field, &decision.action))
        .collect::<BTreeMap<_, _>>();
    let mut mapped = BTreeMap::new();
    for host in &mapping.mappings {
        let keep = host.decision == SourceHostDecision::KeepSource;
        for row in &host.references {
            if !patterns.contains_key(&row.field) && mapped.insert(&row.field, keep).is_some() {
                return Err(WorkcopyError::new("clone_field_mapped_twice"));
            }
        }
    }
    let mut kept = Vec::new();
    for (_, source) in model.sources() {
        let field = &source.field;
        if let Some(action) = patterns.get(field) {
            if **action == ClonePatternAction::KeepAsIs {
                kept.push(CloneKeptSource {
                    field: field.clone(),
                    explicit_keep: false,
                });
            }
            continue;
        }
        let decision = mapped.remove(field);
        if decision == Some(true) {
            kept.push(CloneKeptSource {
                field: field.clone(),
                explicit_keep: true,
            });
            continue;
        }
        if matches!(
            source.kind,
            SourceKind::Item { .. }
                | SourceKind::Graph { .. }
                | SourceKind::Host { .. }
                | SourceKind::ItemPrototype { .. }
                | SourceKind::GraphPrototype { .. }
                | SourceKind::HostPattern { .. }
        ) {
            if source.applicability == Applicability::Editable {
                if decision != Some(false) {
                    return Err(WorkcopyError::new("clone_mapping_incomplete"));
                }
            } else {
                if decision.is_some() {
                    return Err(WorkcopyError::new("clone_mapping_unsupported_field"));
                }
                kept.push(CloneKeptSource {
                    field: field.clone(),
                    explicit_keep: false,
                });
            }
        } else if decision.is_some() {
            return Err(WorkcopyError::new("clone_mapping_unsupported_field"));
        }
    }
    if !mapped.is_empty() {
        return Err(WorkcopyError::new("clone_mapping_incomplete"));
    }
    kept.sort_by(|left, right| left.field.cmp(&right.field));
    Ok(kept)
}

impl WorkcopyService {
    /// Checks the pinned Local against the current source and codec requirements.
    pub fn preview_clone_local(&self, local_id: &str) -> Result<CloneLocalPreview, WorkcopyError> {
        let branch = self.open_clone_workcopy(local_id)?;
        let source = self.local_snapshot(&branch.draft().intent().source().source_workcopy_id)?;
        self.preview_saved_clone(&branch, &source)
    }

    fn preview_saved_clone(
        &self,
        branch: &SavedCloneWorkcopy,
        source: &VersionedDashboardSnapshot,
    ) -> Result<CloneLocalPreview, WorkcopyError> {
        let intent = branch.draft().intent();
        let mapping = branch.draft().mapping();
        let current = self.begin_clone_workcopy(intent.clone())?;
        if current.mapping() != mapping {
            return Err(WorkcopyError::new("clone_mapping_changed"));
        }
        let kept = kept_sources(source, branch)?;
        let local = branch.local();
        validate_clone_references(local)?;
        check_layout(local)?;
        let capabilities = AdapterRegistry::resolve(local.server_version().clone())
            .map_err(|_| WorkcopyError::new("clone_preview_adapter_missing"))?;
        let requirements =
            create_requirements_for(local, CreateScope::Dashboard, intent.target_name())
                .map_err(|error| WorkcopyError::new(error.code().as_str()))?;
        let missing_contracts = requirements
            .missing_contracts(&capabilities.dashboard_create_contracts)
            .into_iter()
            .map(|contract| contract.as_str())
            .collect();
        let kept_digest = if kept.iter().any(|entry| !entry.explicit_keep) {
            let encoded = serde_json::to_vec(&(
                branch.local_id(),
                intent.mapping_revision(),
                local.fingerprint(),
                &kept,
            ))
            .map_err(|_| WorkcopyError::new("clone_kept_digest_failed"))?;
            Some(format!("{:x}", Sha256::digest(encoded)))
        } else {
            None
        };
        Ok(CloneLocalPreview {
            local: local.clone(),
            kept,
            kept_digest,
            missing_contracts,
        })
    }

    /// Rechecks source freshness and binding evidence through read methods.
    pub async fn revalidate_clone_preview(
        &self,
        reader: &ReadOnlyGateway,
        local_id: &str,
    ) -> Result<CloneLocalPreview, WorkcopyError> {
        let branch = self.open_clone_workcopy(local_id)?;
        self.revalidate_saved_clone(reader, &branch).await
    }

    pub(crate) async fn revalidate_saved_clone(
        &self,
        reader: &ReadOnlyGateway,
        branch: &SavedCloneWorkcopy,
    ) -> Result<CloneLocalPreview, WorkcopyError> {
        let intent = branch.draft().intent();
        let source = intent.source();
        if reader.identity().fingerprint() != intent.server_identity_fingerprint()
            || reader.capabilities().version.to_string() != intent.server_version()
        {
            return Err(WorkcopyError::new("clone_intent_server_changed"));
        }
        let source_local = self.local_snapshot(&source.source_workcopy_id)?;
        let preview = self.preview_saved_clone(branch, &source_local)?;
        if !crate::source_check::check_snapshot_source(
            reader,
            branch.local_id(),
            branch.draft().base(),
        )
        .await?
        .is_current()
        {
            return Err(WorkcopyError::new("clone_remote_changed"));
        }
        let mapping = branch.draft().mapping();
        let selected = &intent.target().hosts;
        let selected_ids = selected
            .iter()
            .map(|host| host.hostid.clone())
            .collect::<Vec<_>>();
        let visible = reader
            .hosts_by_ids(&selected_ids)
            .await
            .map_err(|error| WorkcopyError::from_zabbix(&error))?;
        let mut visible_by_id = BTreeMap::new();
        for host in &visible {
            if visible_by_id.insert(host.hostid.as_str(), host).is_some() {
                return Err(WorkcopyError::new("clone_target_selection_changed"));
            }
        }
        for host in selected {
            let observed = visible_by_id
                .remove(host.hostid.as_str())
                .ok_or_else(|| WorkcopyError::new("clone_target_not_visible"))?;
            if observed.host != host.host || observed.name != host.name {
                return Err(WorkcopyError::new("clone_target_selection_changed"));
            }
        }
        if !visible_by_id.is_empty() {
            return Err(WorkcopyError::new("clone_target_selection_changed"));
        }
        let registry = AdapterRegistry::widget_capabilities(preview.local.server_version())
            .map_err(|_| WorkcopyError::new("clone_preview_adapter_missing"))?;
        let model = source_model(&source_local, &registry)
            .map_err(|error| WorkcopyError::from_resolver(&error))?;
        let facts = collect_source_facts(reader, &model)
            .await
            .map_err(|error| WorkcopyError::from_resolver(&error))?;
        let inventory = host_inventory(&model, &facts);
        if !inventory.unattributed.is_empty() {
            return Err(WorkcopyError::new("clone_source_dependencies_unverified"));
        }
        for host in &mapping.mappings {
            if host.decision == SourceHostDecision::KeepSource
                && facts.hosts.get(&host.source_host.hostid) != Some(&host.source_host)
            {
                return Err(WorkcopyError::new("clone_mapping_changed"));
            }
        }
        let fresh = revalidate_clone_mapping_from_source(
            reader,
            branch.draft().base(),
            &source_local,
            mapping,
            &CloneRevalidationContext {
                connection_id: source.connection_id.clone(),
                server_identity_fingerprint: intent.server_identity_fingerprint().to_owned(),
                server_version: intent.server_version().to_owned(),
                local_fingerprint: preview.local.fingerprint().to_owned(),
            },
        )
        .await
        .map_err(|error| WorkcopyError::new(error.code()))?;
        if &fresh != mapping {
            return Err(WorkcopyError::new("clone_mapping_changed"));
        }
        // This can detect a visible collision; the create method remains
        // the authority for an invisible dashboard with the same name.
        let named = reader
            .dashboards_named(&source.connection_id, intent.target_name())
            .await
            .map_err(|error| WorkcopyError::from_zabbix(&error))?;
        if !named.dashboards.is_empty() {
            return Err(WorkcopyError::new("dashboard_name_taken"));
        }
        if named.truncated {
            return Err(WorkcopyError::new("clone_name_visibility_incomplete"));
        }
        let current = self.begin_clone_workcopy(intent.clone())?;
        if current.mapping() != mapping {
            return Err(WorkcopyError::new("clone_mapping_changed"));
        }
        Ok(preview)
    }
}
