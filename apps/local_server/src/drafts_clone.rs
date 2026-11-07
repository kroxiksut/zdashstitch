// SPDX-License-Identifier: MIT OR Apache-2.0

use axum::{extract::Query, http::Uri};
use zdashstitch_application::{
    CloneLldRow, ClonePatternReviewRequest, ClonePublicationRequest, CloneSeriesOption,
    CloneSetupRequest, SavedCloneWorkcopy, clone_mapping::ClonePatternRequirement,
    plan_clone_dashboard,
};

use super::*;

pub(super) fn routes() -> Router<LocalSecurity> {
    Router::new()
        .route("/drafts/{local_id}/clone", get(setup).post(save))
        .route("/drafts/{local_id}/clone/hosts", get(hosts))
        .route(
            "/drafts/{local_id}/clone/review-bindings",
            post(review_bindings),
        )
        .route(
            "/drafts/{local_id}/clone/preview-pattern",
            post(preview_pattern),
        )
        .route(
            "/drafts/{local_id}/clone/preview-growth",
            post(preview_growth),
        )
        .route("/drafts/{local_id}/clone/{clone_id}", get(open))
        .route(
            "/drafts/{local_id}/clone/{clone_id}/comparison",
            get(comparison),
        )
        .route("/drafts/{local_id}/clone/{clone_id}/plan", post(plan_clone))
}

#[derive(Serialize)]
struct BindingReview {
    lld_rows: Vec<CloneLldRow>,
}

async fn review_bindings(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(local_id): PathParams<String>,
    body: Bytes,
) -> Response {
    let request: CloneSetupRequest = match parse(&body) {
        Ok(request) => request,
        Err(error) => return error.into_response(),
    };
    respond(
        run(
            connections,
            drafts,
            move |service, _, connections| async move {
                let connection_id = service
                    .draft_connection_id(&local_id)
                    .map_err(workcopy_failure)?;
                let connections = connections.ok_or_else(|| failure(not_connected()))?;
                let guard = connections.lock().await;
                ensure_draft_connection(&guard, &connection_id).map_err(failure)?;
                let reader = guard.reader().ok_or_else(|| failure(not_connected()))?;
                let lld_rows = service
                    .clone_lld_candidates(reader, &connection_id, &local_id, request)
                    .await
                    .map_err(workcopy_failure)?;
                Ok(BindingReview { lld_rows })
            },
        )
        .await,
    )
}

#[derive(Serialize)]
struct Host {
    hostid: String,
    name: String,
}

#[derive(Serialize)]
struct SetupBody {
    revision: u64,
    source_fingerprint: String,
    hosts: Vec<Host>,
    patterns: Vec<ClonePatternRequirement>,
    unattributed: usize,
    growth_samples: Vec<CloneSeriesOption>,
}

async fn setup(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(local_id): PathParams<String>,
) -> Response {
    respond(
        run(
            connections,
            drafts,
            move |service, _, connections| async move {
                let connection_id = service
                    .draft_connection_id(&local_id)
                    .map_err(workcopy_failure)?;
                let connections = connections.ok_or_else(|| failure(not_connected()))?;
                let guard = connections.lock().await;
                ensure_draft_connection(&guard, &connection_id).map_err(failure)?;
                let reader = guard.reader().ok_or_else(|| failure(not_connected()))?;
                let setup = service
                    .clone_setup(reader, &connection_id, &local_id)
                    .await
                    .map_err(workcopy_failure)?;
                Ok(SetupBody {
                    revision: setup.revision,
                    source_fingerprint: setup.source_fingerprint,
                    hosts: setup
                        .inventory
                        .hosts
                        .into_iter()
                        .map(|host| Host {
                            hostid: host.hostid,
                            name: host.name,
                        })
                        .collect(),
                    patterns: setup.patterns,
                    unattributed: setup.inventory.unattributed.len(),
                    growth_samples: setup.growth_samples,
                })
            },
        )
        .await,
    )
}

async fn preview_pattern(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(local_id): PathParams<String>,
    body: Bytes,
) -> Response {
    let request: ClonePatternReviewRequest = match parse(&body) {
        Ok(request) => request,
        Err(error) => return error.into_response(),
    };
    respond(
        run(
            connections,
            drafts,
            move |service, _, connections| async move {
                let connection_id = service
                    .draft_connection_id(&local_id)
                    .map_err(workcopy_failure)?;
                let connections = connections.ok_or_else(|| failure(not_connected()))?;
                let guard = connections.lock().await;
                ensure_draft_connection(&guard, &connection_id).map_err(failure)?;
                let reader = guard.reader().ok_or_else(|| failure(not_connected()))?;
                service
                    .preview_clone_setup_pattern(reader, &connection_id, &local_id, request)
                    .await
                    .map_err(workcopy_failure)
            },
        )
        .await,
    )
}

async fn preview_growth(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(local_id): PathParams<String>,
    body: Bytes,
) -> Response {
    let request: CloneSetupRequest = match parse(&body) {
        Ok(request) => request,
        Err(error) => return error.into_response(),
    };
    respond(
        run(
            connections,
            drafts,
            move |service, _, connections| async move {
                let connection_id = service
                    .draft_connection_id(&local_id)
                    .map_err(workcopy_failure)?;
                let connections = connections.ok_or_else(|| failure(not_connected()))?;
                let guard = connections.lock().await;
                ensure_draft_connection(&guard, &connection_id).map_err(failure)?;
                let reader = guard.reader().ok_or_else(|| failure(not_connected()))?;
                service
                    .preview_clone_growth(reader, &connection_id, &local_id, request)
                    .await
                    .map_err(workcopy_failure)
            },
        )
        .await,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HostQuery {
    #[serde(default)]
    search: String,
}

#[derive(Serialize)]
struct HostList {
    hosts: Vec<Host>,
    truncated: bool,
}

async fn hosts(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(local_id): PathParams<String>,
    uri: Uri,
) -> Response {
    let Ok(Query(query)) = Query::<HostQuery>::try_from_uri(&uri) else {
        return code(StatusCode::BAD_REQUEST, "request_invalid").into_response();
    };
    if query.search.chars().count() > 128 {
        return code(StatusCode::BAD_REQUEST, "request_invalid").into_response();
    }
    respond(
        run(
            connections,
            drafts,
            move |service, _, connections| async move {
                let connection_id = service
                    .draft_connection_id(&local_id)
                    .map_err(workcopy_failure)?;
                let connections = connections.ok_or_else(|| failure(not_connected()))?;
                let guard = connections.lock().await;
                ensure_draft_connection(&guard, &connection_id).map_err(failure)?;
                let reader = guard.reader().ok_or_else(|| failure(not_connected()))?;
                let found = service
                    .clone_target_hosts(reader, &query.search)
                    .await
                    .map_err(workcopy_failure)?;
                Ok(HostList {
                    hosts: found
                        .objects
                        .into_iter()
                        .map(|host| Host {
                            hostid: host.hostid,
                            name: host.name,
                        })
                        .collect(),
                    truncated: found.truncated,
                })
            },
        )
        .await,
    )
}

#[derive(Serialize)]
struct DraftBody {
    clone_id: String,
    name: String,
    pages: usize,
    widgets: usize,
    changed: usize,
    targets: Vec<String>,
    kept_fields: Vec<String>,
    kept_digest: Option<String>,
    missing_contracts: Vec<&'static str>,
    blocked: Option<&'static str>,
    last_outcome: Option<OutcomeBody>,
}

fn draft_body(
    service: &WorkcopyService,
    journal: &PublishJournal,
    branch: &SavedCloneWorkcopy,
) -> Result<DraftBody, Refusal> {
    let intent = branch.draft().intent();
    let analysis = branch
        .local()
        .analyze()
        .map_err(|_| code(StatusCode::CONFLICT, "clone_workcopy_corrupt"))?;
    let (kept_fields, kept_digest, missing_contracts, blocked) =
        match service.preview_clone_local(branch.local_id()) {
            Ok(preview) => (
                preview
                    .kept_on_source()
                    .iter()
                    .map(|kept| kept.field.field_name.clone())
                    .collect(),
                preview.kept_digest().map(str::to_owned),
                preview.missing_contracts().to_vec(),
                None,
            ),
            Err(error) => (vec![], None, vec![], Some(error.code())),
        };
    let changed = branch
        .draft()
        .mapping()
        .mappings
        .iter()
        .flat_map(|mapping| &mapping.references)
        .filter(|reference| !reference.targets.is_empty())
        .count();
    let last_outcome = journal
        .list()
        .map_err(|error| journal_failure(&error))?
        .entries
        .iter()
        .filter(|entry| {
            entry.intent == "clone_dashboard"
                && entry.source.as_ref().is_some_and(|source| {
                    source.local_id == branch.local_id()
                        && source.revision == intent.mapping_revision()
                })
        })
        .max_by_key(|entry| entry.confirmed_at_ms)
        .map(OutcomeBody::from_entry);
    Ok(DraftBody {
        clone_id: branch.local_id().to_owned(),
        name: intent.target_name().to_owned(),
        pages: analysis.page_count,
        widgets: analysis.widget_count,
        changed,
        targets: intent
            .target()
            .hosts
            .iter()
            .map(|host| host.name.clone())
            .collect(),
        kept_fields,
        kept_digest,
        missing_contracts,
        blocked,
        last_outcome,
    })
}

fn load_branch(
    service: &WorkcopyService,
    local_id: &str,
    clone_id: &str,
) -> Result<SavedCloneWorkcopy, Refusal> {
    let branch = service
        .open_clone_workcopy(clone_id)
        .map_err(workcopy_failure)?;
    if branch.draft().intent().source().source_workcopy_id != local_id {
        return Err(code(StatusCode::NOT_FOUND, "workcopy_not_found"));
    }
    Ok(branch)
}

async fn save(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams(local_id): PathParams<String>,
    body: Bytes,
) -> Response {
    let request: CloneSetupRequest = match parse(&body) {
        Ok(request) => request,
        Err(error) => return error.into_response(),
    };
    respond(
        run(
            connections,
            drafts,
            move |service, journal, connections| async move {
                let connection_id = service
                    .draft_connection_id(&local_id)
                    .map_err(workcopy_failure)?;
                let connections = connections.ok_or_else(|| failure(not_connected()))?;
                let guard = connections.lock().await;
                ensure_draft_connection(&guard, &connection_id).map_err(failure)?;
                let reader = guard.reader().ok_or_else(|| failure(not_connected()))?;
                let branch = service
                    .save_clone_from_setup(reader, &connection_id, &local_id, request)
                    .await
                    .map_err(workcopy_failure)?;
                draft_body(&service, &journal, &branch)
            },
        )
        .await,
    )
}

async fn open(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams((local_id, clone_id)): PathParams<(String, String)>,
) -> Response {
    respond(
        run(connections, drafts, move |service, journal, _| async move {
            let branch = load_branch(&service, &local_id, &clone_id)?;
            draft_body(&service, &journal, &branch)
        })
        .await,
    )
}

async fn comparison(
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    PathParams((local_id, clone_id)): PathParams<(String, String)>,
) -> Response {
    respond(
        run(
            connections,
            drafts,
            move |service, journal, connections| async move {
                let branch = load_branch(&service, &local_id, &clone_id)?;
                let entries = journal
                    .list()
                    .map_err(|error| journal_failure(&error))?
                    .entries;
                let guard = match &connections {
                    Some(connections) => Some(connections.lock().await),
                    None => None,
                };
                let connection_id = &branch.draft().intent().source().connection_id;
                let reader = guard.as_ref().and_then(|guard| {
                    ensure_draft_connection(guard, connection_id).ok()?;
                    guard.reader()
                });
                branch
                    .comparison(&entries, reader)
                    .await
                    .map_err(workcopy_failure)
            },
        )
        .await,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClonePlanRequest {
    kept_digest: Option<String>,
}

#[derive(Serialize)]
struct ClonePlan {
    handle: String,
    plan_id: String,
    name: String,
    pages: usize,
    widgets: usize,
}

async fn plan_clone(
    State(security): State<LocalSecurity>,
    Extension(admitted): Extension<SessionGeneration>,
    Extension(connections): Extension<ConnectionsHandle>,
    Extension(drafts): Extension<DraftsHandle>,
    Extension(plans): Extension<Arc<PlanRegistry>>,
    PathParams((local_id, clone_id)): PathParams<(String, String)>,
    body: Bytes,
) -> Response {
    let input: ClonePlanRequest = match parse(&body) {
        Ok(input) => input,
        Err(error) => return error.into_response(),
    };
    let session = security.local_session().clone();
    respond(
        run(
            connections,
            drafts,
            move |service, journal, connections| async move {
                let branch = load_branch(&service, &local_id, &clone_id)?;
                let intent = branch.draft().intent();
                let source = intent.source();
                let connections = connections.ok_or_else(|| failure(not_connected()))?;
                let guard = connections.lock().await;
                ensure_draft_connection(&guard, &source.connection_id).map_err(failure)?;
                let context_generation = guard.active().map_or(0, |(_, generation, _)| generation);
                let request = ClonePublicationRequest {
                    local_id: clone_id,
                    connection_id: source.connection_id.clone(),
                    mapping_revision: intent.mapping_revision(),
                    base_fingerprint: source.base_fingerprint.clone(),
                    local_fingerprint: source.local_fingerprint.clone(),
                    target_name: intent.target_name().to_owned(),
                    kept_digest: input.kept_digest,
                    session: admitted,
                };
                let plan = plan_clone_dashboard(&service, &journal, &session, &guard, &request)
                    .await
                    .map_err(create_plan_failure)?;
                let preview = plan.preview();
                let handle = new_handle()?;
                plans.insert(StoredPlan {
                    handle: handle.clone(),
                    local_id,
                    generation: admitted,
                    connection_id: request.connection_id,
                    context_generation,
                    plan: HeldPlan::Create {
                        plan: Box::new(plan),
                        name: preview.target_name.clone(),
                        clone: true,
                        kept_digest: request.kept_digest,
                    },
                });
                Ok(ClonePlan {
                    handle,
                    plan_id: preview.plan_id,
                    name: preview.target_name,
                    pages: preview.page_count,
                    widgets: preview.widget_count,
                })
            },
        )
        .await,
    )
}
