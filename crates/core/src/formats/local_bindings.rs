// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{MetricCatalog, PortableProject, Result, Validate};
use crate::formats::validation::{ensure, ensure_decimal_id, ensure_slug, ensure_unique};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LocalBindingsFormat {
    #[serde(rename = "zdashstitch.local-bindings/v1")]
    V1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum StorageScope {
    #[serde(rename = "local_only")]
    LocalOnly,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalBindings {
    pub format: LocalBindingsFormat,
    pub storage_scope: StorageScope,
    pub project_id: String,
    pub connection_id: String,
    pub deployment_id: String,
    pub roles: BTreeMap<String, Vec<BindingEntry>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metric_overrides: Vec<MetricOverride>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BindingEntry {
    pub slot_id: String,
    pub hostid: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MetricOverride {
    pub catalog: String,
    pub metric: String,
    pub slot_id: String,
    pub itemid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

impl LocalBindings {
    pub fn validate_against(
        &self,
        project: Option<&PortableProject>,
        catalogs: Option<&BTreeMap<String, MetricCatalog>>,
    ) -> Result<()> {
        self.validate()?;
        if let Some(project) = project {
            ensure(
                self.project_id == project.project_id,
                "bindings_project_identity",
                "bindings/project identity mismatch",
            )?;
        }

        let mut all_slots = HashSet::new();
        let mut slots_by_role: HashMap<&str, HashSet<&str>> = HashMap::new();
        for (role, entries) in &self.roles {
            if let Some(project) = project {
                let project_role = project.roles.get(role).ok_or_else(|| {
                    super::FormatError::validation(
                        "unknown_bindings_role",
                        format!("unknown bindings role {role}"),
                    )
                })?;
                if project_role.cardinality == super::Cardinality::One {
                    ensure(
                        entries.len() == 1,
                        "singleton_binding_count",
                        format!("singleton role {role} needs exactly one host binding"),
                    )?;
                }
            }
            let mut local = HashSet::new();
            for entry in entries {
                ensure(
                    local.insert(entry.slot_id.as_str())
                        && all_slots.insert(entry.slot_id.as_str()),
                    "duplicate_slot",
                    format!("duplicate slot_id {}", entry.slot_id),
                )?;
            }
            slots_by_role.insert(role, local);

            if let Some(project) = project
                && let Some(required) = &project.roles[role].slots
            {
                ensure(
                    required.iter().all(|slot| {
                        slots_by_role
                            .get(role.as_str())
                            .is_some_and(|slots| slots.contains(slot.as_str()))
                    }),
                    "missing_role_slot",
                    format!("role {role} is missing a required slot"),
                )?;
            }
        }

        let catalog_metrics: BTreeMap<&str, HashSet<&str>> = catalogs
            .map(|entries| {
                entries
                    .iter()
                    .map(|(id, catalog)| {
                        (
                            id.as_str(),
                            catalog
                                .metrics
                                .iter()
                                .map(|metric| metric.id.as_str())
                                .collect(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut overrides = HashSet::new();
        for item in &self.metric_overrides {
            ensure(
                overrides.insert((
                    item.catalog.as_str(),
                    item.metric.as_str(),
                    item.slot_id.as_str(),
                )),
                "duplicate_metric_override",
                "duplicate metric override",
            )?;
            ensure(
                all_slots.contains(item.slot_id.as_str()),
                "unknown_override_slot",
                format!("override references unknown slot {}", item.slot_id),
            )?;
            if catalogs.is_some() {
                ensure(
                    catalog_metrics
                        .get(item.catalog.as_str())
                        .is_some_and(|metrics| metrics.contains(item.metric.as_str())),
                    "unknown_override_metric",
                    format!(
                        "override references unknown metric {}/{}",
                        item.catalog, item.metric
                    ),
                )?;
            }
            if let Some(role) = &item.role {
                ensure(
                    slots_by_role
                        .get(role.as_str())
                        .is_some_and(|slots| slots.contains(item.slot_id.as_str())),
                    "override_role_slot",
                    "override role/slot mismatch",
                )?;
            }
        }
        Ok(())
    }
}

impl Validate for LocalBindings {
    fn validate(&self) -> Result<()> {
        ensure_slug(&self.project_id, "project id")?;
        ensure_slug(&self.connection_id, "connection id")?;
        ensure_slug(&self.deployment_id, "deployment id")?;
        ensure(
            !self.roles.is_empty(),
            "binding_roles",
            "bindings require roles",
        )?;
        for (role, entries) in &self.roles {
            ensure_slug(role, "binding role")?;
            ensure(
                (1..=256).contains(&entries.len()),
                "binding_count",
                "each role requires 1..=256 bindings",
            )?;
            for entry in entries {
                ensure_slug(&entry.slot_id, "binding slot")?;
                ensure_decimal_id(&entry.hostid, "hostid")?;
            }
        }
        for item in &self.metric_overrides {
            ensure_slug(&item.catalog, "override catalog")?;
            ensure_slug(&item.metric, "override metric")?;
            ensure_slug(&item.slot_id, "override slot")?;
            ensure_decimal_id(&item.itemid, "itemid")?;
            if let Some(role) = &item.role {
                ensure_slug(role, "override role")?;
            }
        }
        ensure_unique(
            self.metric_overrides.iter().map(|item| {
                (
                    item.catalog.as_str(),
                    item.metric.as_str(),
                    item.slot_id.as_str(),
                )
            }),
            "duplicate_metric_override",
            "metric override",
        )
    }
}
