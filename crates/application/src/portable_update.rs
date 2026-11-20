// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};
use zdashstitch_core::formats::ProjectRoot;
use zdashstitch_zabbix::{
    DashboardPublishOutcome, LinkedGraphCompositionPage, LinkedGraphPage,
    MAX_PORTABLE_APPEND_WIDGETS, MAX_PORTABLE_GRAPH_PAGES, PortableGraphChange,
    PortableUpdateDifference, PortableUpdateReadback, ReadOnlyGateway, ReadWriteGateway,
    VerifiedPortableUpdate, is_valid_dashboard_name,
};

use crate::portable_deployment;
use crate::publish_journal::{
    Finish, JournalSource, JournalTarget, OperationKind, OperationRecord, PublishJournal,
    TargetKey, is_operation_id,
};
use crate::{
    CreatePlanError, JournalState, LocalSession, PlanInvalidReason, PublishReport,
    SessionGeneration, WriteStage,
    connections::ConnectionService,
    portable_lld_review::{PortableGraphSelection, VerifiedPortableLldEvidence},
    workcopy::{WorkcopyError, WorkcopyService},
};

const MAX_AGE: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PortableUpdateIntent {
    AppendPage {
        dashboardid: String,
        after_pageid: Option<String>,
        source_page_id: Option<String>,
    },
    AppendWidget {
        dashboardid: String,
        pageid: String,
        x: u32,
        y: u32,
    },
    AppendWidgets {
        dashboardid: String,
        pageid: String,
        source_page_id: String,
        widgets: Vec<SelectedWidgetPlacement>,
    },
    UpdateCreated {
        deployment_id: String,
    },
    ChangeAccess {
        deployment_id: String,
        access: zdashstitch_zabbix::DashboardAccessSelection,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedWidgetPlacement {
    widget_id: String,
    x: u32,
    y: u32,
}

pub struct PortableUpdateRequest {
    pub local_id: String,
    pub connection_id: String,
    pub revision: u64,
    pub session: SessionGeneration,
    pub intent: PortableUpdateIntent,
}

pub struct PortableUpdateConfirmation<'a> {
    pub plan_id: &'a str,
    pub acknowledge_deletions: bool,
    pub acknowledge_repeat_append: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PortableUpdateRecord {
    pub readback: PortableUpdateReadback,
    pub deployment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping: Option<portable_deployment::PlannedDeployment>,
}

impl PortableUpdateRecord {
    pub(crate) fn valid_source(&self, intent: &str, id: &str) -> bool {
        let page = id.strip_prefix("pg-").is_some_and(|tail| {
            tail.len() == 32
                && tail
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        });
        let dashboard = crate::workcopy::is_offline_dashboard_id(id);
        if let Some(mapping) = &self.mapping {
            return matches!(&self.readback, PortableUpdateReadback::LinkedComposition { pages }
            if pages.len() == mapping.pages.len() && pages.iter().zip(&mapping.pages).all(|(proof, page)| {
                proof.new_widgets <= page.widgets.len() && (!proof.new_page || proof.new_widgets == page.widgets.len())
            })) && intent == "update_created"
                && dashboard
                && self.deployment_id.as_deref().is_some_and(is_operation_id)
                && mapping.root_id == id
                && !mapping.create_another
                && !mapping.pages.is_empty()
                && mapping.valid_pages();
        }
        match (intent, &self.readback, &self.deployment_id) {
            ("append_page", PortableUpdateReadback::AddedPage { page: 1..=49 }, None) => {
                page || dashboard
            }
            (
                "append_page",
                PortableUpdateReadback::InsertedPage {
                    page: index @ 1..=48,
                    page_count: count @ 3..=50,
                },
                None,
            ) => (page || dashboard) && index + 1 < *count,
            (
                "append_page",
                PortableUpdateReadback::AddedPageWidgets {
                    page: index @ 1..=49,
                    page_count: count @ 2..=50,
                    widgets: 2..=MAX_PORTABLE_APPEND_WIDGETS,
                },
                None,
            ) => (page || dashboard) && index < count,
            (
                "append_widgets",
                PortableUpdateReadback::AddedWidget {
                    page: 0..=49,
                    widget: 0..=4607,
                },
                None,
            ) => page || dashboard,
            (
                "append_widgets",
                PortableUpdateReadback::AddedWidgets {
                    page: 0..=49,
                    first_widget,
                    widgets: count @ 2..=MAX_PORTABLE_APPEND_WIDGETS,
                },
                None,
            ) => {
                (page || dashboard)
                    && first_widget
                        .checked_add(*count)
                        .is_some_and(|total| total <= 4608)
            }
            (
                "update_created",
                PortableUpdateReadback::Exact | PortableUpdateReadback::UpdatedGraph,
                Some(id),
            ) => dashboard && is_operation_id(id),
            _ => false,
        }
    }
}

/// Only the live planner can issue this value, and confirmation consumes it.
pub struct VerifiedPortableUpdatePlan {
    id: String,
    update: VerifiedPortableUpdate,
    evidence: VerifiedPortableLldEvidence,
    metadata: PortableUpdateRecord,
    request: PortableUpdateRequest,
    action: &'static str,
    created: SystemTime,
    source_name: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PortableUpdatePreviewChange {
    AddPage {
        name: String,
        number: usize,
    },
    AddWidget {
        page: String,
        name: String,
        x: u32,
        y: u32,
    },
    RemovePage {
        name: String,
        number: usize,
    },
    RemoveWidget {
        page: String,
        name: String,
        x: u32,
        y: u32,
    },
    MovePage {
        name: String,
        from: usize,
        to: usize,
    },
    Geometry {
        page: String,
        widget: String,
        before: [u32; 4],
        after: [u32; 4],
    },
    Name {
        object: &'static str,
        before: String,
        after: String,
    },
    Field {
        page: String,
        widget: String,
        name: String,
        before: Option<String>,
        after: String,
    },
}

impl VerifiedPortableUpdatePlan {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn action(&self) -> &'static str {
        self.action
    }

    pub fn dashboardid(&self) -> &str {
        self.update.target().dashboard_id()
    }

    pub fn target_name(&self) -> &str {
        self.update.target().raw_api_document()["name"]
            .as_str()
            .unwrap_or_default()
    }

    pub fn source_name(&self) -> &str {
        &self.source_name
    }

    pub fn removes_content(&self) -> bool {
        self.update.differences().iter().any(|change| {
            matches!(
                change,
                PortableUpdateDifference::RemovePage { .. }
                    | PortableUpdateDifference::RemoveWidget { .. }
            )
        })
    }

    pub fn repeats_append(&self) -> bool {
        self.update.repeats_append()
    }

    pub fn access_change(
        &self,
    ) -> Option<(
        &zdashstitch_zabbix::DashboardAccessState,
        &zdashstitch_zabbix::DashboardAccessState,
    )> {
        self.update.access_change()
    }

    pub fn requires_public_acknowledgement(&self) -> bool {
        self.access_change()
            .is_some_and(|(before, after)| before.private && !after.private)
    }

    pub async fn preview_changes(
        &self,
        reader: &ReadOnlyGateway,
    ) -> Result<Vec<PortableUpdatePreviewChange>, CreatePlanError> {
        let mut id_set = BTreeSet::new();
        for difference in self.update.differences() {
            if let PortableUpdateDifference::Field {
                name,
                before,
                after,
                ..
            } = difference
                && name.starts_with("ds.0.itemids.")
            {
                id_set.insert(after.clone());
                id_set.extend(before.iter().cloned());
            }
        }
        let ids: Vec<_> = id_set.into_iter().collect();
        let items = reader
            .items_by_ids(&ids)
            .await
            .map_err(|error| CreatePlanError::Read(error.failure()))?;
        let item_name = |id: &str| {
            items
                .iter()
                .find(|item| item.itemid == id)
                .map(|item| item.name.clone())
                .ok_or_else(|| {
                    CreatePlanError::Workcopy(WorkcopyError::new("portable_diff_item_not_visible"))
                })
        };
        use PortableUpdateDifference::*;
        self.update
            .differences()
            .iter()
            .map(|difference| {
                Ok(match difference {
                    AddPage { name, number } => PortableUpdatePreviewChange::AddPage {
                        name: name.clone(),
                        number: *number,
                    },
                    AddWidget { page, name, x, y } => PortableUpdatePreviewChange::AddWidget {
                        page: page.clone(),
                        name: name.clone(),
                        x: *x,
                        y: *y,
                    },
                    RemovePage { name, number } => PortableUpdatePreviewChange::RemovePage {
                        name: name.clone(),
                        number: *number,
                    },
                    RemoveWidget { page, name, x, y } => {
                        PortableUpdatePreviewChange::RemoveWidget {
                            page: page.clone(),
                            name: name.clone(),
                            x: *x,
                            y: *y,
                        }
                    }
                    MovePage { name, from, to } => PortableUpdatePreviewChange::MovePage {
                        name: name.clone(),
                        from: *from,
                        to: *to,
                    },
                    Geometry {
                        page,
                        widget,
                        before,
                        after,
                    } => PortableUpdatePreviewChange::Geometry {
                        page: page.clone(),
                        widget: widget.clone(),
                        before: *before,
                        after: *after,
                    },
                    Name {
                        object,
                        before,
                        after,
                    } => PortableUpdatePreviewChange::Name {
                        object,
                        before: before.clone(),
                        after: after.clone(),
                    },
                    Field {
                        page,
                        widget,
                        name,
                        before,
                        after,
                    } if name.starts_with("ds.0.itemids.") => PortableUpdatePreviewChange::Field {
                        page: page.clone(),
                        widget: widget.clone(),
                        name: name.clone(),
                        before: before.as_deref().map(item_name).transpose()?,
                        after: item_name(after)?,
                    },
                    Field {
                        page,
                        widget,
                        name,
                        before,
                        after,
                    } => PortableUpdatePreviewChange::Field {
                        page: page.clone(),
                        widget: widget.clone(),
                        name: name.clone(),
                        before: before.clone(),
                        after: after.clone(),
                    },
                })
            })
            .collect()
    }

    pub async fn confirm(
        self,
        confirmation: PortableUpdateConfirmation<'_>,
        service: &WorkcopyService,
        client: &ReadWriteGateway,
        journal: &PublishJournal,
        stage: &WriteStage,
    ) -> Result<(PublishReport, Option<WorkcopyError>), CreatePlanError> {
        let now = SystemTime::now();
        let id = confirmation.plan_id;
        if id != self.id
            || self.removes_content() != confirmation.acknowledge_deletions
            || self.repeats_append() != confirmation.acknowledge_repeat_append
        {
            return Err(CreatePlanError::ConfirmationMismatch);
        }
        if stage.generation() != self.request.session
            || now
                .duration_since(self.created)
                .map_or(true, |age| age >= MAX_AGE)
        {
            return Err(CreatePlanError::Invalidated(
                PlanInvalidReason::SessionEnded,
            ));
        }
        let reader = client.reader();
        let target = self.update.target();
        let endpoint = reader.identity().endpoint_fingerprint();
        let authority = reader.identity().fingerprint();
        service
            .revalidate_portable_lld_for_plan(reader, &self.evidence)
            .await
            .map_err(CreatePlanError::Workcopy)?;
        let record = OperationRecord {
            kind: OperationKind::Update,
            intent: self.action,
            connection_id: self.request.connection_id.clone(),
            server_version: target.server_version().to_string(),
            endpoint_fingerprint: endpoint.to_owned(),
            server_identity: authority.to_owned(),
            target: JournalTarget::ExistingDashboard {
                dashboardid: target.dashboard_id().to_owned(),
                name: None,
            },
            source: Some(JournalSource {
                local_id: self.request.local_id.clone(),
                revision: self.request.revision,
            }),
            planned_deployment: None,
            portable_update: Some(self.metadata.clone()),
            source_fingerprint: target.fingerprint().to_owned(),
            expected_fingerprint: self.update.expected_fingerprint().to_owned(),
            owner_userid: None,
            plan_created_at_ms: crate::create_dashboard::millis(self.created),
            confirmed_at_ms: crate::create_dashboard::millis(now),
        };
        let mut handle = journal
            .begin(id, &record)
            .map_err(CreatePlanError::Journal)?;
        let outcome = client
            .publish_verified_portable_update(&self.update, || {
                service.portable_draft_matches(
                    &self.request.local_id,
                    self.request.revision,
                    self.evidence.draft_fingerprint(),
                ) && service.portable_lld_binding_state_matches(&self.evidence)
                    && handle.admit_send()
            })
            .await;
        use DashboardPublishOutcome::*;
        let state = match outcome {
            Conflict => JournalState::Conflict,
            Verified => JournalState::Verified,
            FailedBeforeSend => JournalState::FailedBeforeSend,
            FailedAfterResponse => JournalState::FailedAfterResponse,
            UnknownOutcome => JournalState::UnknownOutcome,
            ReconciledVerified => JournalState::ReconciledVerified,
            ReconciledFailed => JournalState::ReconciledFailed,
        };
        let verified = matches!(outcome, Verified | ReconciledVerified);
        let journal_error = match handle.take_submit_error() {
            Some(error) => Some(error),
            None => handle
                .finish_with(Finish {
                    observed_fingerprint: verified.then(|| record.expected_fingerprint.clone()),
                    evidence: match outcome {
                        Conflict => Some("source_changed"),
                        Verified | ReconciledVerified => Some("expected_found"),
                        _ => None,
                    },
                    ..Finish::state(state)
                })
                .err(),
        };
        let entry = journal.entry(id).ok();
        let record_error = if verified && journal_error.is_none() {
            match &entry {
                Some(entry) => service.project_portable_update(reader, entry).await.err(),
                None => Some(WorkcopyError::new("deployment_read_back_unverified")),
            }
        } else {
            None
        };
        let report = PublishReport {
            plan_id: self.id,
            states: entry
                .map(|entry| {
                    entry
                        .transitions
                        .into_iter()
                        .map(|line| line.state)
                        .collect()
                })
                .unwrap_or_default(),
            deployment_state_may_advance: verified
                && journal_error.is_none()
                && record_error.is_none(),
            invalidated: (outcome == Conflict).then_some(PlanInvalidReason::TargetChanged),
            journal_error,
        };
        Ok((report, record_error))
    }
}

pub async fn plan_portable_update(
    service: &WorkcopyService,
    journal: &PublishJournal,
    session: &LocalSession,
    connections: &ConnectionService,
    request: PortableUpdateRequest,
) -> Result<VerifiedPortableUpdatePlan, CreatePlanError> {
    let stale = || CreatePlanError::Invalidated(PlanInvalidReason::TargetChanged);
    if !session.is_current(request.session) {
        return Err(CreatePlanError::Invalidated(
            PlanInvalidReason::SessionEnded,
        ));
    }
    let client = connections
        .portable_writer()
        .filter(|gateway| {
            gateway.reader().capabilities().version.to_string() == "8.0.0"
                && connections
                    .active()
                    .is_some_and(|(id, _, _)| id == request.connection_id)
        })
        .ok_or(CreatePlanError::Invalidated(
            PlanInvalidReason::ConnectionChanged,
        ))?;
    let reader = client.reader();
    let (revision, bundle) = service
        .load_portable_source_draft(&request.local_id)
        .map_err(CreatePlanError::Workcopy)?;
    let source_name = match &bundle.project.root {
        ProjectRoot::Dashboard(dashboard) => dashboard.name.clone(),
        ProjectRoot::Page(page) => page.name.clone(),
    };
    if revision != request.revision
        || !bundle
            .project
            .declared_compatibility
            .contains(&zdashstitch_core::Version::new(8, 0, 0))
    {
        return Err(CreatePlanError::Invalidated(
            PlanInvalidReason::DraftChanged,
        ));
    }
    let source_pages = match &bundle.project.root {
        ProjectRoot::Dashboard(dashboard) => dashboard.pages.as_slice(),
        ProjectRoot::Page(page) => std::slice::from_ref(page),
    };
    let choose_page = |id: Option<&str>| -> Result<_, CreatePlanError> {
        match id {
            Some(id) => source_pages
                .iter()
                .enumerate()
                .find(|(_, page)| page.id == id)
                .ok_or_else(stale),
            None if matches!(&bundle.project.root, ProjectRoot::Page(_)) => {
                source_pages.first().map(|page| (0, page)).ok_or_else(stale)
            }
            None => Err(stale()),
        }
    };
    let mut source_name = source_name;
    let mut deployment_mapping = None;
    let mut scope = None;
    let (action, dashboardid, deployment_id, change) = match &request.intent {
        PortableUpdateIntent::AppendPage {
            dashboardid,
            after_pageid,
            source_page_id,
        } => {
            let (page_index, page) = choose_page(source_page_id.as_deref())?;
            if !(1..=MAX_PORTABLE_APPEND_WIDGETS).contains(&page.widgets.len()) {
                return Err(stale());
            }
            scope = Some(PortableGraphSelection {
                page_index,
                widgets: (0..page.widgets.len()).collect(),
            });
            source_name.clone_from(&page.name);
            (
                "append_page",
                dashboardid.clone(),
                None,
                PortableGraphChange::AppendPage {
                    after_pageid: after_pageid.clone(),
                },
            )
        }
        PortableUpdateIntent::AppendWidget {
            dashboardid,
            pageid,
            x,
            y,
        } => (
            "append_widgets",
            dashboardid.clone(),
            None,
            PortableGraphChange::AppendWidget {
                pageid: pageid.clone(),
                x: *x,
                y: *y,
            },
        ),
        PortableUpdateIntent::AppendWidgets {
            dashboardid,
            pageid,
            source_page_id,
            widgets,
        } => {
            let (page_index, page) = choose_page(Some(source_page_id))?;
            if !(1..=MAX_PORTABLE_APPEND_WIDGETS).contains(&widgets.len()) {
                return Err(stale());
            }
            let mut positions = BTreeMap::new();
            for placement in widgets {
                let index = page
                    .widgets
                    .iter()
                    .position(|widget| widget.id() == placement.widget_id)
                    .ok_or_else(stale)?;
                if positions
                    .insert(index, (placement.x, placement.y))
                    .is_some()
                {
                    return Err(stale());
                }
            }
            scope = Some(PortableGraphSelection {
                page_index,
                widgets: positions.keys().copied().collect(),
            });
            source_name.clone_from(&page.name);
            (
                "append_widgets",
                dashboardid.clone(),
                None,
                PortableGraphChange::AppendWidgets {
                    pageid: pageid.clone(),
                    placements: positions.into_values().collect(),
                },
            )
        }
        PortableUpdateIntent::UpdateCreated { deployment_id }
        | PortableUpdateIntent::ChangeAccess { deployment_id, .. } => {
            let mapping =
                portable_deployment::PlannedDeployment::from_project(&bundle.project, false)
                    .map_err(CreatePlanError::Workcopy)?;
            let entry = portable_deployment::load(&service.store)
                .map_err(CreatePlanError::Workcopy)?
                .into_iter()
                .find(|entry| entry.state.deployment_id == *deployment_id)
                .ok_or_else(stale)?;
            let state = &entry.state;
            if state.project_id != request.local_id
                || state.source_revision > request.revision
                || mapping.root_id != state.root_id
                || state.connection_id != request.connection_id
                || state.target_server_version != "8.0.0"
                || state.server_identity_fingerprint != reader.identity().fingerprint()
            {
                return Err(stale());
            }
            let linked_pages = &state.dashboard.pages;
            let same_structure = mapping.pages.len() == linked_pages.len()
                && mapping
                    .pages
                    .iter()
                    .zip(linked_pages)
                    .all(|(source, linked)| {
                        source.page_id == linked.logical_id
                            && source
                                .widgets
                                .iter()
                                .map(|widget| &widget.widget_id)
                                .eq(linked.widgets.iter().map(|widget| &widget.logical_id))
                    });
            let change = if let PortableUpdateIntent::ChangeAccess { access, .. } = &request.intent
            {
                if state.source_revision != request.revision || !same_structure {
                    return Err(stale());
                }
                PortableGraphChange::ChangeAccess(access.clone())
            } else if same_structure
                && let [page] = linked_pages.as_slice()
                && let [widget] = page.widgets.as_slice()
            {
                PortableGraphChange::UpdateCreated {
                    pageid: page.dashboard_pageid.clone(),
                    widgetid: widget.widgetid.clone(),
                }
            } else if entry.covers_graph_composition() {
                let pages = mapping
                    .pages
                    .iter()
                    .map(|source| {
                        let linked = linked_pages
                            .iter()
                            .find(|page| page.logical_id == source.page_id);
                        LinkedGraphCompositionPage {
                            pageid: linked.map(|page| page.dashboard_pageid.clone()),
                            widgetids: source
                                .widgets
                                .iter()
                                .map(|widget| {
                                    linked?
                                        .widgets
                                        .iter()
                                        .find(|deployed| deployed.logical_id == widget.widget_id)
                                        .map(|deployed| deployed.widgetid.clone())
                                })
                                .collect(),
                        }
                    })
                    .collect();
                deployment_mapping = Some(mapping);
                PortableGraphChange::UpdateGraphComposition { pages }
            } else if same_structure {
                let pages = linked_pages
                    .iter()
                    .map(|linked| LinkedGraphPage {
                        pageid: linked.dashboard_pageid.clone(),
                        widgetids: linked
                            .widgets
                            .iter()
                            .map(|widget| widget.widgetid.clone())
                            .collect(),
                    })
                    .collect();
                PortableGraphChange::UpdateGraphLabels { pages }
            } else {
                return Err(stale());
            };
            (
                "update_created",
                state.dashboard.dashboardid.clone(),
                Some((deployment_id.clone(), state.dashboard.fingerprint.clone())),
                change,
            )
        }
    };
    journal
        .check_target(&TargetKey::dashboard(
            &request.connection_id,
            Some(reader.identity().endpoint_fingerprint().to_owned()),
            &dashboardid,
        ))
        .map_err(CreatePlanError::Journal)?;
    let target = reader
        .dashboard_snapshot(&request.connection_id, &dashboardid)
        .await
        .map_err(|error| CreatePlanError::Read(error.failure()))?
        .ok_or_else(stale)?;
    if deployment_id
        .as_ref()
        .is_some_and(|(_, fingerprint)| fingerprint != target.fingerprint())
    {
        return Err(stale());
    }
    let target_pages = target.raw_api_document()["pages"]
        .as_array()
        .ok_or_else(stale)?;
    match &change {
        PortableGraphChange::AppendPage { .. }
            if target_pages.len() >= MAX_PORTABLE_GRAPH_PAGES =>
        {
            return Err(stale());
        }
        PortableGraphChange::AppendWidget { pageid, .. }
        | PortableGraphChange::AppendWidgets { pageid, .. } => {
            let additions = match &change {
                PortableGraphChange::AppendWidgets { placements, .. } => placements.len(),
                _ => 1,
            };
            let used = target_pages
                .iter()
                .find(|page| page["dashboard_pageid"] == pageid.as_str())
                .and_then(|page| page["widgets"].as_array())
                .map(Vec::len)
                .ok_or_else(stale)?;
            if used.saturating_add(additions) > MAX_PORTABLE_APPEND_WIDGETS {
                return Err(stale());
            }
        }
        _ => {}
    }
    let full_page = matches!(&change, PortableGraphChange::AppendPage { .. })
        && scope
            .as_ref()
            .is_some_and(|scope| scope.widgets.len() == MAX_PORTABLE_APPEND_WIDGETS);
    let evidence = service
        .verify_portable_lld_for_selection(reader, &request.local_id, request.revision, scope)
        .await
        .map_err(CreatePlanError::Workcopy)?;
    let documents = if matches!(&change, PortableGraphChange::ChangeAccess(_)) {
        Ok(Vec::new())
    } else if full_page
        || matches!(
            &change,
            PortableGraphChange::UpdateGraphLabels { .. }
                | PortableGraphChange::UpdateGraphComposition { .. }
        )
    {
        service
            .materialize_portable_graph_8_0(&evidence)
            .map(|document| vec![document])
    } else {
        service.materialize_portable_graphs_8_0(&evidence)
    }
    .map_err(CreatePlanError::Workcopy)?;
    let mut graphs = Vec::with_capacity(documents.len());
    for document in documents {
        let name = document.as_document()["name"]
            .as_str()
            .filter(|name| is_valid_dashboard_name(name))
            .ok_or_else(stale)?;
        graphs.push(
            client
                .verify_eight_zero_portable_graph_create(
                    document.as_document(),
                    &request.connection_id,
                    evidence.draft_fingerprint(),
                    name,
                )
                .await
                .map_err(CreatePlanError::Adapter)?,
        );
    }
    let update = client
        .verify_eight_zero_portable_update(graphs, target, change)
        .await
        .map_err(CreatePlanError::Adapter)?;
    let metadata = PortableUpdateRecord {
        readback: update.readback().clone(),
        deployment_id: deployment_id.map(|(id, _)| id),
        mapping: deployment_mapping,
    };
    if !metadata.valid_source(action, &request.local_id) {
        return Err(stale());
    }
    let created = SystemTime::now();
    let nonce = crate::publisher::plan_nonce(created);
    let id = format!("zds-portable-update-{nonce:016x}");
    Ok(VerifiedPortableUpdatePlan {
        id,
        update,
        evidence,
        metadata,
        request,
        action,
        created,
        source_name,
    })
}
