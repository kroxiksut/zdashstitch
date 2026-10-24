// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{LocalBindings, PortableProject, Result, Validate};
use crate::formats::validation::{
    ensure, ensure_decimal_id, ensure_len, ensure_slug, validate_hex, validate_version,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DeploymentFormat {
    #[serde(rename = "zdashstitch.deployment-state/v1")]
    V1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DeploymentStorageScope {
    #[serde(rename = "local_only")]
    LocalOnly,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DeploymentStatus {
    #[serde(rename = "verified")]
    Verified,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentState {
    pub format: DeploymentFormat,
    pub storage_scope: DeploymentStorageScope,
    pub project_id: String,
    pub connection_id: String,
    pub deployment_id: String,
    pub adapter_id: String,
    pub target_server_version: String,
    pub server_identity_fingerprint: String,
    pub status: DeploymentStatus,
    pub last_verified_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_operation_id: Option<String>,
    pub dashboards: Vec<DeployedDashboard>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeployedDashboard {
    pub logical_id: String,
    pub dashboardid: String,
    pub fingerprint: String,
    pub pages: Vec<DeployedPage>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeployedPage {
    pub logical_id: String,
    pub dashboard_pageid: String,
    pub widgets: Vec<DeployedWidget>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeployedWidget {
    pub logical_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot_id: Option<String>,
    pub widgetid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

impl DeploymentState {
    pub fn validate_against(
        &self,
        project: Option<&PortableProject>,
        bindings: Option<&LocalBindings>,
    ) -> Result<()> {
        self.validate()?;
        if let Some(project) = project {
            ensure(
                self.project_id == project.project_id,
                "deployment_project_identity",
                "deployment/project identity mismatch",
            )?;
        }
        if let Some(bindings) = bindings {
            ensure(
                self.project_id == bindings.project_id
                    && self.connection_id == bindings.connection_id
                    && self.deployment_id == bindings.deployment_id,
                "deployment_bindings_identity",
                "deployment/bindings identity mismatch",
            )?;
        }

        let mut logical_dashboards = HashSet::new();
        let mut zabbix_dashboards = HashSet::new();
        let known_slots: HashSet<&str> = bindings
            .map(|bindings| {
                bindings
                    .roles
                    .values()
                    .flatten()
                    .map(|entry| entry.slot_id.as_str())
                    .collect()
            })
            .unwrap_or_default();

        for dashboard in &self.dashboards {
            ensure(
                logical_dashboards.insert(dashboard.logical_id.as_str())
                    && zabbix_dashboards.insert(dashboard.dashboardid.as_str()),
                "duplicate_deployment_dashboard",
                "duplicate deployment dashboard identity",
            )?;
            let project_dashboard = if let Some(project) = project {
                Some(
                    project
                        .dashboards
                        .iter()
                        .find(|entry| entry.id == dashboard.logical_id)
                        .ok_or_else(|| {
                            super::FormatError::validation(
                                "unknown_deployment_dashboard",
                                format!("unknown deployment dashboard {}", dashboard.logical_id),
                            )
                        })?,
                )
            } else {
                None
            };

            let mut logical_pages = HashSet::new();
            let mut zabbix_pages = HashSet::new();
            let mut references = HashSet::new();
            for page in &dashboard.pages {
                ensure(
                    logical_pages.insert(page.logical_id.as_str())
                        && zabbix_pages.insert(page.dashboard_pageid.as_str()),
                    "duplicate_deployment_page",
                    "duplicate deployment page identity",
                )?;
                let project_page = if let Some(project_dashboard) = project_dashboard {
                    Some(
                        project_dashboard
                            .pages
                            .iter()
                            .find(|candidate| candidate.id == page.logical_id)
                            .ok_or_else(|| {
                                super::FormatError::validation(
                                    "unknown_deployment_page",
                                    format!("unknown deployment page {}", page.logical_id),
                                )
                            })?,
                    )
                } else {
                    None
                };

                let mut logical_widgets = HashSet::new();
                let mut zabbix_widgets = HashSet::new();
                for widget in &page.widgets {
                    ensure(
                        logical_widgets
                            .insert((widget.logical_id.as_str(), widget.slot_id.as_deref()))
                            && zabbix_widgets.insert(widget.widgetid.as_str()),
                        "duplicate_deployed_widget",
                        "duplicate deployed widget identity",
                    )?;
                    if let Some(reference) = &widget.reference {
                        ensure(
                            references.insert(reference.as_str()),
                            "duplicate_widget_reference",
                            format!("duplicate widget reference {reference}"),
                        )?;
                    }
                    if let Some(project_page) = project_page {
                        let source = project_page
                            .widgets
                            .iter()
                            .find(|candidate| candidate.id() == widget.logical_id)
                            .ok_or_else(|| {
                                super::FormatError::validation(
                                    "unknown_deployment_widget",
                                    format!("unknown deployment widget {}", widget.logical_id),
                                )
                            })?;
                        ensure(
                            widget.slot_id.is_none()
                                || matches!(
                                    source,
                                    super::Widget::ItemValue {
                                        expansion: super::ItemExpansion::WidgetPerHost,
                                        ..
                                    }
                                ),
                            "unexpected_widget_slot",
                            "slot_id is allowed only for widget_per_host identities",
                        )?;
                    }
                    if let Some(slot) = &widget.slot_id
                        && bindings.is_some()
                    {
                        ensure(
                            known_slots.contains(slot.as_str()),
                            "unknown_deployment_slot",
                            format!("deployment references unknown slot {slot}"),
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl Validate for DeploymentState {
    fn validate(&self) -> Result<()> {
        ensure_slug(&self.project_id, "project id")?;
        ensure_slug(&self.connection_id, "connection id")?;
        ensure_slug(&self.deployment_id, "deployment id")?;
        ensure_slug(&self.adapter_id, "adapter id")?;
        validate_version(&self.target_server_version, "target_server_version")?;
        validate_hex(
            &self.server_identity_fingerprint,
            64,
            true,
            "server_identity_fingerprint",
        )?;
        ensure(
            OffsetDateTime::parse(&self.last_verified_at, &Rfc3339).is_ok(),
            "last_verified_at",
            "last_verified_at must be an RFC 3339 date-time",
        )?;
        if let Some(operation) = &self.last_operation_id {
            ensure_len(operation, 1, 128, "last_operation_id", "last operation id")?;
        }
        ensure(
            self.dashboards.len() <= 16,
            "deployment_dashboard_count",
            "deployment may contain at most 16 dashboards",
        )?;
        for dashboard in &self.dashboards {
            dashboard.validate()?;
        }
        Ok(())
    }
}

impl Validate for DeployedDashboard {
    fn validate(&self) -> Result<()> {
        ensure_slug(&self.logical_id, "logical dashboard id")?;
        ensure_decimal_id(&self.dashboardid, "dashboardid")?;
        validate_hex(&self.fingerprint, 64, true, "dashboard fingerprint")?;
        ensure(
            self.pages.len() <= 32,
            "deployment_page_count",
            "deployed dashboard may contain at most 32 pages",
        )?;
        for page in &self.pages {
            page.validate()?;
        }
        Ok(())
    }
}

impl Validate for DeployedPage {
    fn validate(&self) -> Result<()> {
        ensure_slug(&self.logical_id, "logical page id")?;
        ensure_decimal_id(&self.dashboard_pageid, "dashboard_pageid")?;
        ensure(
            self.widgets.len() <= 256,
            "deployment_widget_count",
            "deployed page may contain at most 256 widgets",
        )?;
        for widget in &self.widgets {
            widget.validate()?;
        }
        Ok(())
    }
}

impl Validate for DeployedWidget {
    fn validate(&self) -> Result<()> {
        ensure_slug(&self.logical_id, "logical widget id")?;
        if let Some(slot) = &self.slot_id {
            ensure_slug(slot, "logical slot id")?;
        }
        ensure_decimal_id(&self.widgetid, "widgetid")?;
        if let Some(reference) = &self.reference {
            ensure_len(reference, 1, 255, "widget_reference", "widget reference")?;
        }
        Ok(())
    }
}
