// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::BTreeSet;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use zdashstitch_core::formats::{
    DeployedDashboard, DeployedPage, DeployedWidget, DeploymentFormatV2, DeploymentStateV2,
    DeploymentStatus, DeploymentStorageScope, Layout, ProjectRoot, TypedProject, Validate,
};
use zdashstitch_zabbix::{
    ReadBackVerdict, ReadOnlyGateway, VersionedDashboardSnapshot, check_created_dashboard,
};

use crate::offline_dashboards::OfflineDashboardStore;
use crate::publish_journal::{JournalEntry, JournalState, is_operation_id};
use crate::workcopy::{
    WorkcopyError, WorkcopyService,
    store::{LocalStore, read_bounded, write_atomic},
};

const PATH: &[&str] = &["deployments"];
const FILE: &str = "typed-v2.json";
const MAX_BYTES: usize = 8 * 1024 * 1024;
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlannedDeployment {
    pub root_id: String,
    pub page_id: String,
    pub widget_id: String,
    pub create_another: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<PlannedPage>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlannedPage {
    pub page_id: String,
    pub widgets: Vec<PlannedWidget>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlannedWidget {
    pub widget_id: String,
    pub geometry: [u16; 4],
}

impl PlannedDeployment {
    pub(crate) fn from_project(
        project: &TypedProject,
        create_another: bool,
    ) -> Result<Self, WorkcopyError> {
        let ProjectRoot::Dashboard(root) = &project.root else {
            return Err(unverified());
        };
        let page = root.pages.first().ok_or_else(unverified)?;
        let widget = page.widgets.first().ok_or_else(unverified)?;
        if !(1..=zdashstitch_zabbix::MAX_PORTABLE_GRAPH_PAGES).contains(&root.pages.len())
            || root.pages.iter().any(|page| {
                !(1..=zdashstitch_zabbix::MAX_PORTABLE_APPEND_WIDGETS).contains(&page.widgets.len())
            })
        {
            return Err(unverified());
        }
        let pages = root
            .pages
            .iter()
            .map(|page| PlannedPage {
                page_id: page.id.clone(),
                widgets: page
                    .widgets
                    .iter()
                    .map(|widget| {
                        let geometry = match *widget.layout() {
                            Layout::Fixed { x, y, w, h } => [x, y, w, h],
                            Layout::Auto { w, h } => [0, 0, w, h],
                        };
                        PlannedWidget {
                            widget_id: widget.id().to_owned(),
                            geometry,
                        }
                    })
                    .collect(),
            })
            .collect();
        Ok(Self {
            root_id: root.id.clone(),
            page_id: page.id.clone(),
            widget_id: widget.id().to_owned(),
            create_another,
            pages,
        })
    }

    pub(crate) fn valid_pages(&self) -> bool {
        if self.pages.is_empty() {
            return true;
        }
        let mut ids = BTreeSet::new();
        if self.pages.len() > zdashstitch_zabbix::MAX_PORTABLE_GRAPH_PAGES
            || self.pages[0].page_id != self.page_id
            || self.pages[0]
                .widgets
                .first()
                .is_none_or(|widget| widget.widget_id != self.widget_id)
        {
            return false;
        }
        self.pages.iter().all(|page| {
            let mut widget_ids = BTreeSet::new();
            let mut geometry = BTreeSet::new();
            valid_logical_id(&page.page_id)
                && ids.insert(&page.page_id)
                && (1..=zdashstitch_zabbix::MAX_PORTABLE_APPEND_WIDGETS)
                    .contains(&page.widgets.len())
                && page.widgets.iter().all(|widget| {
                    let [x, y, w, h] = widget.geometry;
                    valid_logical_id(&widget.widget_id)
                        && widget_ids.insert(&widget.widget_id)
                        && w > 0
                        && h > 0
                        && x.checked_add(w).is_some_and(|edge| edge <= 72)
                        && y.checked_add(h).is_some_and(|edge| edge <= 64)
                        && geometry.insert(widget.geometry)
                })
        })
    }
}

pub(crate) fn valid_logical_id(id: &str) -> bool {
    (2..=64).contains(&id.len())
        && id.as_bytes()[0].is_ascii_lowercase()
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

fn remote_id(value: &Value) -> Result<String, WorkcopyError> {
    value
        .as_str()
        .filter(|id| zdashstitch_zabbix::is_zabbix_id(id))
        .map(str::to_owned)
        .ok_or_else(unverified)
}

fn mapped_widget(logical_id: &str, remote: &Value) -> Result<DeployedWidget, WorkcopyError> {
    Ok(DeployedWidget {
        logical_id: logical_id.into(),
        slot_id: None,
        widgetid: remote_id(&remote["widgetid"])?,
        reference: None,
    })
}

fn mapped_pages(
    mapping: &PlannedDeployment,
    raw: &Value,
) -> Result<Vec<DeployedPage>, WorkcopyError> {
    let pages = raw["pages"].as_array().ok_or_else(unverified)?;
    if mapping.pages.is_empty() {
        let [page] = pages.as_slice() else {
            return Err(unverified());
        };
        let [widget] = page["widgets"]
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(unverified)?
        else {
            return Err(unverified());
        };
        return Ok(vec![DeployedPage {
            logical_id: mapping.page_id.clone(),
            dashboard_pageid: remote_id(&page["dashboard_pageid"])?,
            widgets: vec![mapped_widget(&mapping.widget_id, widget)?],
        }]);
    }
    if pages.len() != mapping.pages.len() {
        return Err(unverified());
    }
    pages
        .iter()
        .zip(&mapping.pages)
        .map(|(page, intended)| {
            let widgets = page["widgets"].as_array().ok_or_else(unverified)?;
            if widgets.len() != intended.widgets.len() {
                return Err(unverified());
            }
            let widgets = intended
                .widgets
                .iter()
                .map(|expected| {
                    let mut candidates = widgets.iter().filter(|widget| {
                        ["x", "y", "width", "height"]
                            .into_iter()
                            .zip(expected.geometry)
                            .all(|(key, coordinate)| {
                                widget[key].as_str().and_then(|value| value.parse().ok())
                                    == Some(coordinate)
                            })
                    });
                    let Some(widget) = candidates.next() else {
                        return Err(unverified());
                    };
                    if candidates.next().is_some() {
                        return Err(unverified());
                    }
                    mapped_widget(&expected.widget_id, widget)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(DeployedPage {
                logical_id: intended.page_id.clone(),
                dashboard_pageid: remote_id(&page["dashboard_pageid"])?,
                widgets,
            })
        })
        .collect()
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoredDeployment {
    pub state: DeploymentStateV2,
    pub endpoint_fingerprint: String,
    pub raw_snapshot: Value,
}

impl StoredDeployment {
    pub(crate) fn covers_graph_composition(&self) -> bool {
        covers_graph_composition(&self.state.dashboard.pages, &self.raw_snapshot)
    }

    pub(crate) fn snapshot(&self) -> Result<VersionedDashboardSnapshot, WorkcopyError> {
        let version = self
            .state
            .target_server_version
            .parse()
            .map_err(|_| corrupt())?;
        let snapshot = VersionedDashboardSnapshot::from_adapter_raw(
            self.state.connection_id.clone(),
            version,
            self.raw_snapshot.clone(),
        )
        .map_err(|_| corrupt())?;
        if snapshot.fingerprint() != self.state.dashboard.fingerprint
            || snapshot.adapter_id() != self.state.adapter_id
            || snapshot.raw_api_document()["name"] != self.state.dashboard_name
            || snapshot.raw_api_document()["dashboardid"]
                != self.state.dashboard.dashboardid.as_str()
        {
            return Err(corrupt());
        }
        let pages = snapshot.raw_api_document()["pages"]
            .as_array()
            .ok_or_else(corrupt)?;
        for mapped in &self.state.dashboard.pages {
            let page = pages
                .iter()
                .find(|page| page["dashboard_pageid"] == mapped.dashboard_pageid)
                .ok_or_else(corrupt)?;
            let widgets = page["widgets"].as_array().ok_or_else(corrupt)?;
            if mapped.widgets.iter().any(|mapped| {
                widgets
                    .iter()
                    .filter(|widget| widget["widgetid"] == mapped.widgetid)
                    .count()
                    != 1
            }) {
                return Err(corrupt());
            }
        }
        Ok(snapshot)
    }
}

fn covers_graph_composition(mapped: &[DeployedPage], raw: &Value) -> bool {
    raw["pages"].as_array().is_some_and(|pages| {
        pages.len() == mapped.len()
            && pages.iter().zip(mapped).all(|(page, linked)| {
                page["dashboard_pageid"] == linked.dashboard_pageid
                    && page["widgets"].as_array().is_some_and(|widgets| {
                        widgets.len() == linked.widgets.len()
                            && widgets.iter().all(|widget| widget["type"] == "svggraph")
                            && linked
                                .widgets
                                .iter()
                                .all(|entry| entry.slot_id.is_none() && entry.reference.is_none())
                    })
            })
    })
}

fn corrupt() -> WorkcopyError {
    WorkcopyError::new("deployment_state_corrupt")
}

fn unverified() -> WorkcopyError {
    WorkcopyError::new("deployment_read_back_unverified")
}

pub(crate) fn load(store: &LocalStore) -> Result<Vec<StoredDeployment>, WorkcopyError> {
    let Some(directory) = store.path(PATH, false)? else {
        return Ok(Vec::new());
    };
    let Some(raw) = read_bounded(&directory, FILE, MAX_BYTES)? else {
        return Ok(Vec::new());
    };
    let entries: Vec<StoredDeployment> = serde_json::from_slice(&raw).map_err(|_| corrupt())?;
    let mut ids = BTreeSet::new();
    for entry in &entries {
        entry.state.validate().map_err(|_| corrupt())?;
        if !ids.insert(&entry.state.deployment_id)
            || !is_operation_id(&entry.state.deployment_id)
            || !crate::workcopy::is_fingerprint(&entry.endpoint_fingerprint)
        {
            return Err(corrupt());
        }
        entry.snapshot()?;
    }
    Ok(entries)
}

fn persist(store: &LocalStore, entry: StoredDeployment) -> Result<(), WorkcopyError> {
    let _lock = WRITE_LOCK
        .lock()
        .map_err(|_| WorkcopyError::new("storage_unavailable"))?;
    let mut entries = load(store)?;
    if entries
        .iter()
        .any(|stored| stored.state.deployment_id == entry.state.deployment_id)
    {
        return Ok(());
    }
    entries.push(entry);
    persist_entries(store, &entries)
}

fn persist_entries(store: &LocalStore, entries: &[StoredDeployment]) -> Result<(), WorkcopyError> {
    let raw = serde_json::to_vec(&entries).map_err(|_| corrupt())?;
    if raw.len() > MAX_BYTES {
        return Err(WorkcopyError::new("storage_limit_reached"));
    }
    let directory = store
        .path(PATH, true)?
        .ok_or_else(|| WorkcopyError::new("storage_unavailable"))?;
    write_atomic(&directory, FILE, &raw)
}

fn verified_at(entry: &JournalEntry) -> Result<String, WorkcopyError> {
    let millis = entry.transitions.last().ok_or_else(unverified)?.at_ms;
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(millis) * 1_000_000)
        .map_err(|_| unverified())?
        .format(&Rfc3339)
        .map_err(|_| unverified())
}

impl WorkcopyService {
    pub async fn portable_deployment_status(
        &self,
        reader: &ReadOnlyGateway,
        active: &str,
        local_id: &str,
        deployment_id: &str,
    ) -> Result<(&'static str, Option<crate::publication::RemoteDiff>), crate::CreatePlanError>
    {
        let stored = load(&self.store)
            .map_err(crate::CreatePlanError::Workcopy)?
            .into_iter()
            .find(|stored| {
                stored.state.deployment_id == deployment_id
                    && stored.state.project_id == local_id
                    && stored.state.root_id == local_id
            })
            .ok_or_else(|| {
                crate::CreatePlanError::Workcopy(WorkcopyError::new("workcopy_not_found"))
            })?;
        let deployment = &stored.state;
        if deployment.connection_id != active
            || deployment.server_identity_fingerprint != reader.identity().fingerprint()
            || deployment.target_server_version != reader.capabilities().version.to_string()
            || !reader
                .matches_live_version()
                .await
                .map_err(|error| crate::CreatePlanError::Read(error.failure()))?
        {
            return Ok(("other_server", None));
        }
        let snapshot = reader
            .dashboard_snapshot(active, &deployment.dashboard.dashboardid)
            .await
            .map_err(|error| crate::CreatePlanError::Read(error.failure()))?;
        Ok(match snapshot {
            None => ("missing_or_denied", None),
            Some(snapshot) if snapshot.fingerprint() == deployment.dashboard.fingerprint => {
                ("current", None)
            }
            Some(snapshot) => {
                let baseline = stored
                    .snapshot()
                    .map_err(crate::CreatePlanError::Workcopy)?;
                (
                    "changed",
                    Some(crate::publication::remote_changes(
                        &baseline,
                        Some(&snapshot),
                        &BTreeSet::new(),
                    )),
                )
            }
        })
    }

    /// Lists only durable confirmed dashboard deployments of this local draft.
    pub fn portable_deployments(
        &self,
        local_id: &str,
    ) -> Result<Vec<DeploymentStateV2>, WorkcopyError> {
        if local_id.starts_with("pg-") {
            self.load_portable_source_draft(local_id)?;
            return Ok(Vec::new());
        }
        let draft = OfflineDashboardStore::new(self.store.root_path())
            .get(local_id)?
            .ok_or_else(|| WorkcopyError::new("workcopy_not_found"))?;
        Ok(load(&self.store)?
            .into_iter()
            .filter(|entry| {
                entry.state.project_id == draft.local_id && entry.state.root_id == draft.local_id
            })
            .map(|entry| entry.state)
            .collect())
    }

    pub(crate) async fn project_portable_create(
        &self,
        reader: &ReadOnlyGateway,
        entry: &JournalEntry,
    ) -> Result<(), WorkcopyError> {
        let Some(mapping) = &entry.planned_deployment else {
            return Ok(());
        };
        let source = entry.source.as_ref().ok_or_else(unverified)?;
        let expectation = crate::create_dashboard::portable_create_expectation(entry)
            .map_err(|_| unverified())?;
        let dashboardid = entry
            .created_dashboardid
            .as_deref()
            .ok_or_else(unverified)?;
        if !matches!(
            entry.state,
            JournalState::Verified | JournalState::ReconciledVerified
        ) || entry.sends != 1
            || entry.server_identity != reader.identity().fingerprint()
            || entry.server_version != reader.capabilities().version.to_string()
        {
            return Err(unverified());
        }
        if load(&self.store)?
            .iter()
            .any(|stored| stored.state.deployment_id == entry.operation_id)
        {
            return Ok(());
        }
        if !reader
            .matches_live_version()
            .await
            .map_err(|_| unverified())?
        {
            return Err(unverified());
        }
        let snapshot = reader
            .dashboard_snapshot(&entry.connection_id, dashboardid)
            .await
            .map_err(|_| unverified())?
            .ok_or_else(unverified)?;
        if entry.observed_fingerprint.as_deref() != Some(snapshot.fingerprint())
            || check_created_dashboard(&snapshot, &expectation) != ReadBackVerdict::Matches
        {
            return Err(unverified());
        }
        let raw = snapshot.raw_api_document();
        let state = DeploymentStateV2 {
            format: DeploymentFormatV2::V2,
            storage_scope: DeploymentStorageScope::LocalOnly,
            project_id: source.local_id.clone(),
            root_id: mapping.root_id.clone(),
            source_revision: source.revision,
            connection_id: entry.connection_id.clone(),
            deployment_id: entry.operation_id.clone(),
            adapter_id: snapshot.adapter_id().to_owned(),
            target_server_version: entry.server_version.clone(),
            server_identity_fingerprint: entry.server_identity.clone(),
            status: DeploymentStatus::Verified,
            last_verified_at: verified_at(entry)?,
            last_operation_id: entry.operation_id.clone(),
            dashboard_name: expectation.name,
            dashboard: DeployedDashboard {
                logical_id: mapping.root_id.clone(),
                dashboardid: dashboardid.to_owned(),
                fingerprint: snapshot.fingerprint().to_owned(),
                pages: mapped_pages(mapping, raw)?,
            },
        };
        state.validate().map_err(|_| unverified())?;
        persist(
            &self.store,
            StoredDeployment {
                state,
                endpoint_fingerprint: entry.endpoint_fingerprint.clone(),
                raw_snapshot: raw.clone(),
            },
        )
    }

    /// Advance an existing verified link after an exact, journaled update.
    pub async fn project_portable_update(
        &self,
        reader: &ReadOnlyGateway,
        entry: &JournalEntry,
    ) -> Result<(), WorkcopyError> {
        let Some(update) = &entry.portable_update else {
            return Ok(());
        };
        let Some(deployment_id) = &update.deployment_id else {
            return Ok(());
        };
        let source = entry.source.as_ref().ok_or_else(unverified)?;
        let crate::publish_journal::JournalTarget::ExistingDashboard { dashboardid, .. } =
            &entry.target
        else {
            return Err(unverified());
        };
        if !matches!(
            entry.state,
            JournalState::Verified | JournalState::ReconciledVerified
        ) || entry.sends != 1
            || entry.observed_fingerprint.as_deref() != Some(&entry.expected_fingerprint)
            || entry.server_identity != reader.identity().fingerprint()
            || entry.server_version != reader.capabilities().version.to_string()
        {
            return Err(unverified());
        }
        let load_entry = || -> Result<StoredDeployment, WorkcopyError> {
            load(&self.store)?
                .into_iter()
                .find(|entry| entry.state.deployment_id == *deployment_id)
                .ok_or_else(unverified)
        };
        if load_entry()?.state.last_operation_id == entry.operation_id {
            return Ok(());
        }
        if !reader
            .matches_live_version()
            .await
            .map_err(|_| unverified())?
        {
            return Err(unverified());
        }
        let snapshot = reader
            .dashboard_snapshot(&entry.connection_id, dashboardid)
            .await
            .map_err(|_| unverified())?
            .ok_or_else(unverified)?;
        if update.readback.fingerprint(&snapshot).as_deref() != Some(&entry.expected_fingerprint) {
            return Err(unverified());
        }
        let _lock = WRITE_LOCK
            .lock()
            .map_err(|_| WorkcopyError::new("storage_unavailable"))?;
        let mut entries = load(&self.store)?;
        let stored = entries
            .iter_mut()
            .find(|entry| entry.state.deployment_id == *deployment_id)
            .ok_or_else(unverified)?;
        let state = &mut stored.state;
        if state.last_operation_id == entry.operation_id {
            return Ok(());
        }
        if state.project_id != source.local_id
            || state.source_revision > source.revision
            || state.connection_id != entry.connection_id
            || state.target_server_version != entry.server_version
            || state.server_identity_fingerprint != entry.server_identity
            || state.dashboard.dashboardid != *dashboardid
            || state.dashboard.fingerprint != entry.source_fingerprint
        {
            return Err(unverified());
        }
        state.source_revision = source.revision;
        state.last_operation_id = entry.operation_id.clone();
        state.last_verified_at = verified_at(entry)?;
        state.dashboard.fingerprint = snapshot.fingerprint().to_owned();
        if let Some(mapping) = &update.mapping {
            if mapping.root_id != state.root_id {
                return Err(unverified());
            }
            state.dashboard.pages = mapped_pages(mapping, snapshot.raw_api_document())?;
        }
        state.dashboard_name = snapshot.raw_api_document()["name"]
            .as_str()
            .ok_or_else(unverified)?
            .to_owned();
        stored.raw_snapshot = snapshot.raw_api_document().clone();
        state.validate().map_err(|_| unverified())?;
        persist_entries(&self.store, &entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn composed_link_maps_widget_geometry_and_refuses_ambiguous_readback() {
        let mapping = PlannedDeployment {
            root_id: "dashboard".into(),
            page_id: "page".into(),
            widget_id: "first".into(),
            create_another: false,
            pages: vec![PlannedPage {
                page_id: "page".into(),
                widgets: vec![
                    PlannedWidget {
                        widget_id: "first".into(),
                        geometry: [0, 0, 18, 4],
                    },
                    PlannedWidget {
                        widget_id: "second".into(),
                        geometry: [18, 0, 18, 4],
                    },
                ],
            }],
        };
        let mut raw = json!({"pages":[{"dashboard_pageid":"100","widgets":[
            {"widgetid":"102","type":"svggraph","x":"18","y":"0","width":"18","height":"4"},
            {"widgetid":"101","type":"svggraph","x":"0","y":"0","width":"18","height":"4"}
        ]}]});
        let pages =
            mapped_pages(&mapping, &raw).unwrap_or_else(|_| panic!("mapped by unique geometry"));
        assert_eq!(pages[0].widgets[0].widgetid, "101");
        assert_eq!(pages[0].widgets[1].widgetid, "102");
        assert!(covers_graph_composition(&pages, &raw));
        let mut partial = pages.clone();
        partial[0].widgets.pop();
        assert!(!covers_graph_composition(&partial, &raw));
        raw["pages"][0]["widgets"][1]["x"] = json!("18");
        assert!(mapped_pages(&mapping, &raw).is_err());
    }
}
