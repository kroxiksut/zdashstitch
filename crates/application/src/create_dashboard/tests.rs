// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-06C/D on a synthetic dashboard imported from a loopback mock
//! transport into a real local store. Plans that need an unproven contract
//! are built with a detached test authority and can never be sent; the
//! adapter's own send path is covered in `zdashstitch-zabbix`. A page draft
//! of known widget types (contracts proven on 7.4.16 by LAB-02) is sent
//! end to end to the mock.

use std::sync::{Arc, Mutex, PoisonError};

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, extract::State, routing::post};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use zdashstitch_zabbix::{
    Authentication, DashboardCreateContracts, ZabbixClient, ZabbixEndpoint, read_only_allowlist,
};

use super::*;
use crate::publish_journal::{
    CloseManuallyRequest, FailKind, JournalHandle, JournalProgress, fail,
};
use crate::workcopy::{ImportRequest, ImportScope};

const CONNECTION: &str = "conn-00000000000000000000000000000006";
const TARGET: &str = "Synthetic copy";
const OWNER: &str = "3";

/// How the mock answers `dashboard.create`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum CreateReply {
    /// An API error; nothing is stored (the stand-in tests never send).
    #[default]
    Refuse,
    /// Stores the dashboard and answers with its ID.
    Accept,
    /// Stores the dashboard, then a proxy answers HTTP 502.
    AcceptThenBadGateway,
}

struct MockData {
    dashboards: Vec<Value>,
    calls: Vec<Value>,
    /// `dashboard.get` answers with an API error.
    fail_reads: bool,
    create_reply: CreateReply,
}

type Shared = Arc<Mutex<MockData>>;

fn lock(data: &Shared) -> std::sync::MutexGuard<'_, MockData> {
    data.lock().unwrap_or_else(PoisonError::into_inner)
}

fn widget(widgetid: &str, widget_type: &str, x: u32, fields: Value) -> Value {
    json!({
        "widgetid": widgetid, "type": widget_type, "name": format!("w{widgetid}"),
        "x": x.to_string(), "y": "0", "width": "12", "height": "4", "view_mode": "0",
        "fields": fields
    })
}

fn source() -> Value {
    json!({
        "dashboardid": "950", "name": "Synthetic source", "userid": "1",
        "private": "0", "display_period": "30", "auto_start": "1",
        "users": [{"userid": "7", "permission": "2"}], "userGroups": [],
        "pages": [
            {"dashboard_pageid": "1500", "name": "Main", "display_period": "0", "widgets": [
                widget("2500", "item", 0, json!([
                    {"type": "4", "name": "itemid.0", "value": "101"},
                    {"type": "1", "name": "reference", "value": "AAAAA"}
                ])),
                widget("2501", "svggraph", 12, json!([
                    {"type": "1", "name": "ds.0.hosts.0", "value": "db-*"},
                    {"type": "1", "name": "ds.0.override_hostid._reference", "value": "AAAAA._hostid"}
                ]))
            ]},
            {"dashboard_pageid": "1501", "name": "Second", "display_period": "60", "widgets": [
                widget("2510", "vendor.private", 0, json!([]))
            ]}
        ]
    })
}

/// What the server would store for a copy of `source` named `name`.
fn created(dashboardid: &str, name: &str) -> Value {
    let mut document = source();
    document["dashboardid"] = json!(dashboardid);
    document["name"] = json!(name);
    document["userid"] = json!(OWNER);
    document["private"] = json!("1");
    document["users"] = json!([]);
    let mut next = 7000;
    if let Some(pages) = document["pages"].as_array_mut() {
        for page in pages {
            next += 1;
            page["dashboard_pageid"] = json!(next.to_string());
            if let Some(widgets) = page["widgets"].as_array_mut() {
                for widget in widgets {
                    next += 1;
                    widget["widgetid"] = json!(next.to_string());
                }
            }
        }
    }
    document
}

/// What the server stores for a `dashboard.create` of `params`.
fn stored(params: &Value, dashboardid: &str) -> Value {
    let mut document = params.clone();
    document["dashboardid"] = json!(dashboardid);
    document["userid"] = json!(OWNER);
    document["private"] = json!("1");
    let mut next = 8000;
    if let Some(pages) = document["pages"].as_array_mut() {
        for page in pages {
            next += 1;
            page["dashboard_pageid"] = json!(next.to_string());
            if let Some(widgets) = page["widgets"].as_array_mut() {
                for widget in widgets {
                    next += 1;
                    widget["widgetid"] = json!(next.to_string());
                }
            }
        }
    }
    document
}

async fn rpc(State(data): State<Shared>, Json(body): Json<Value>) -> Response {
    let id = body.get("id").cloned().unwrap_or(Value::Null);
    let params = body["params"].clone();
    let mut data = lock(&data);
    data.calls.push(body.clone());
    let method = body["method"].as_str().unwrap_or_default();
    if data.fail_reads && method == "dashboard.get" {
        return Json(json!({"jsonrpc": "2.0", "id": id, "error": {
            "code": -32500, "message": "Application error.", "data": "Synthetic read failure"
        }}))
        .into_response();
    }
    if method == "dashboard.create" && data.create_reply != CreateReply::Refuse {
        let dashboardid = (990 + data.dashboards.len()).to_string();
        data.dashboards.push(stored(&params, &dashboardid));
        if data.create_reply == CreateReply::AcceptThenBadGateway {
            return StatusCode::BAD_GATEWAY.into_response();
        }
        return Json(json!({
            "jsonrpc": "2.0", "id": id, "result": {"dashboardids": [dashboardid]}
        }))
        .into_response();
    }
    let result = match method {
        "apiinfo.version" => json!("7.4.16"),
        "user.checkAuthentication" => json!({"userid": OWNER, "username": "synthetic"}),
        "dashboard.get" => {
            let names = params["filter"]["name"].as_array().cloned();
            let ids = params["dashboardids"].as_array().cloned();
            Value::Array(
                data.dashboards
                    .iter()
                    .filter(|dashboard| {
                        names
                            .as_ref()
                            .is_none_or(|names| names.contains(&dashboard["name"]))
                            && ids
                                .as_ref()
                                .is_none_or(|ids| ids.contains(&dashboard["dashboardid"]))
                    })
                    .cloned()
                    .collect(),
            )
        }
        "item.get" => json!([]),
        _ => {
            return Json(json!({"jsonrpc": "2.0", "id": id, "error": {
                "code": -32602, "message": "Invalid params.", "data": "Not expected by the mock"
            }}))
            .into_response();
        }
    };
    Json(json!({"jsonrpc": "2.0", "id": id, "result": result})).into_response()
}

struct Mock {
    data: Shared,
    gateway: ReadWriteGateway,
    service: WorkcopyService,
    journal: PublishJournal,
    session: LocalSession,
    local_id: String,
    _dir: tempfile::TempDir,
    _task: JoinHandle<()>,
}

impl Mock {
    async fn start(scope: ImportScope) -> Self {
        Self::start_with(scope, source(), CreateReply::Refuse).await
    }

    async fn start_with(scope: ImportScope, dashboard: Value, create_reply: CreateReply) -> Self {
        let data: Shared = Arc::new(Mutex::new(MockData {
            dashboards: vec![dashboard],
            calls: Vec::new(),
            fail_reads: false,
            create_reply,
        }));
        let app = Router::new()
            .route("/api_jsonrpc.php", post(rpc))
            .with_state(Arc::clone(&data));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|error| panic!("bind: {error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("address: {error}"));
        let task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, app).await {
                panic!("mock API failed: {error}");
            }
        });
        let endpoint = ZabbixEndpoint::parse(&format!("http://{address}/api_jsonrpc.php"))
            .unwrap_or_else(|error| panic!("endpoint: {error}"));
        let reader = ZabbixClient::new(endpoint)
            .unwrap_or_else(|error| panic!("client: {error}"))
            .authenticate(Authentication::ApiToken(SecretString::from(
                "synthetic-token".to_owned(),
            )))
            .await
            .unwrap_or_else(|error| panic!("sign-in: {error}"));
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"));
        let service = WorkcopyService::new(dir.path().to_path_buf());
        let local_id = service
            .import(
                &reader,
                CONNECTION,
                &ImportRequest {
                    dashboardid: "950".to_owned(),
                    scope,
                    name: None,
                },
            )
            .await
            .unwrap_or_else(|error| panic!("import: {error} {:?}", error.params()))
            .local_id;
        Self {
            data,
            gateway: reader.enable_writes(),
            service,
            journal: PublishJournal::new(dir.path().to_path_buf()),
            session: LocalSession::new(),
            local_id,
            _dir: dir,
            _task: task,
        }
    }

    fn request(&self, name: &str) -> SaveAsCopyRequest {
        let file = self
            .service
            .load(&self.local_id)
            .unwrap_or_else(|error| panic!("load: {error}"));
        let local = self
            .service
            .local_snapshot(&self.local_id)
            .unwrap_or_else(|error| panic!("local: {error}"));
        SaveAsCopyRequest {
            local_id: self.local_id.clone(),
            connection_id: CONNECTION.to_owned(),
            revision: file.revision,
            base_fingerprint: file.base.fingerprint,
            local_fingerprint: local.fingerprint().to_owned(),
            target_name: name.to_owned(),
            session: self.session.current(),
        }
    }

    async fn plan(
        &self,
        request: &SaveAsCopyRequest,
    ) -> Result<VerifiedCreatePlan, CreatePlanError> {
        plan_save_as_copy(
            &self.service,
            &self.journal,
            &self.session,
            &self.gateway,
            request,
        )
        .await
    }

    /// A plan with every create contract marked proven, verified against a
    /// detached authority: it can never be sent.
    fn detached_plan(&self, request: &SaveAsCopyRequest) -> VerifiedCreatePlan {
        let file = self
            .service
            .load(&self.local_id)
            .unwrap_or_else(|error| panic!("load: {error}"));
        let local = self
            .service
            .local_snapshot(&self.local_id)
            .unwrap_or_else(|error| panic!("local: {error}"));
        let mut capabilities = self.gateway.capabilities().clone();
        capabilities.dashboard_create_contracts = proven();
        let context = PublicationContext::detached_for_testing(capabilities);
        verify_save_as_copy_plan(&CreatePlanInputs {
            request,
            kind: file.kind,
            page: file
                .source
                .page
                .as_ref()
                .map(|page| page.dashboard_pageid.as_str()),
            revision: file.revision,
            base_fingerprint: &file.base.fingerprint,
            connection_id: &file.source.connection_id,
            local: &local,
            name_check: NameCheck::NotVisible,
            context: &context,
        })
        .unwrap_or_else(|error| panic!("detached plan: {error}"))
    }

    fn creates(&self) -> usize {
        lock(&self.data)
            .calls
            .iter()
            .filter(|call| call["method"] == "dashboard.create")
            .count()
    }

    fn reads_only(&self) -> bool {
        let allowed = read_only_allowlist();
        lock(&self.data)
            .calls
            .iter()
            .all(|call| allowed.contains(&call["method"].as_str().unwrap_or_default()))
    }
}

fn proven() -> DashboardCreateContracts {
    let supported = CapabilityState::Supported;
    DashboardCreateContracts {
        create_private_dashboard: supported,
        copy_read_widgets: supported,
        copy_unknown_widgets: supported,
        reconcile_create_outcome: supported,
    }
}

fn incompatible(result: Result<VerifiedCreatePlan, CreatePlanError>) -> CompatibilityReport {
    match result {
        Err(CreatePlanError::Incompatible(report)) => report,
        Err(error) => panic!("expected blockers, got {error}"),
        Ok(_) => panic!("expected blockers, got a plan"),
    }
}

fn invalidated(result: Result<VerifiedCreatePlan, CreatePlanError>) -> PlanInvalidReason {
    match result {
        Err(CreatePlanError::Invalidated(reason)) => reason,
        Err(error) => panic!("expected invalidation, got {error}"),
        Ok(_) => panic!("expected invalidation, got a plan"),
    }
}

fn contracts_listed(report: &CompatibilityReport) -> Vec<&str> {
    report
        .with_code(FindingCode::CapabilityNotVerified)
        .filter_map(|finding| finding.param("capability"))
        .collect()
}

#[tokio::test]
async fn no_plan_while_a_create_contract_is_unproven() {
    // 7.4.16 after LAB-02: private create, copy of read widgets and
    // unknown-outcome reconciliation are proven; copying a widget type the
    // registry does not know is not, so this dashboard gets no plan.
    let mock = Mock::start(ImportScope::Dashboard).await;
    let report = incompatible(mock.plan(&mock.request(TARGET)).await);
    let listed = contracts_listed(&report);
    assert!(
        listed.contains(&"create_copy_unknown_widgets"),
        "create_copy_unknown_widgets in {listed:?}"
    );
    for proven in [
        "create_private_dashboard",
        "create_copy_read_widgets",
        "create_reconcile_unknown_outcome",
    ] {
        assert!(!listed.contains(&proven), "{proven} in {listed:?}");
    }
    // The unknown widget is named with its own contract.
    let unknown = report
        .with_code(FindingCode::WriteContractUnverified)
        .find(|finding| finding.param("contract") == Some("create_copy_unknown_widgets"))
        .unwrap_or_else(|| panic!("unknown widget finding"));
    assert_eq!(unknown.param("widget_type"), Some("vendor.private"));
    assert_eq!(
        unknown.subject.as_ref().map(|subject| subject.page_index),
        Some(1)
    );
    assert!(!report.allows_publication());
    assert_eq!(mock.creates(), 0);
    assert!(mock.reads_only());
}

#[tokio::test]
async fn names_are_checked_as_a_hint_and_invalid_names_block() {
    let mock = Mock::start(ImportScope::Dashboard).await;
    lock(&mock.data).dashboards.push(created("951", TARGET));
    let report = incompatible(mock.plan(&mock.request(TARGET)).await);
    assert_eq!(report.with_code(FindingCode::DashboardNameTaken).count(), 1);

    let report = incompatible(mock.plan(&mock.request(" padded")).await);
    assert_eq!(
        report.with_code(FindingCode::DashboardNameInvalid).count(),
        1
    );
    // An invalid name is never looked up.
    assert!(
        !lock(&mock.data)
            .calls
            .iter()
            .any(|call| call["params"]["filter"]["name"] == json!([" padded"]))
    );
    assert_eq!(mock.creates(), 0);
}

fn page(dashboard_pageid: &str) -> ImportScope {
    ImportScope::Page {
        dashboard_pageid: dashboard_pageid.to_owned(),
    }
}

/// Plans, confirms and sends a Save as copy of the mock's draft through the
/// live gateway (the contracts of its page are proven on 7.4.16).
async fn publish(mock: &Mock, name: &str) -> (CreatePlanPreview, CreatePublishReport) {
    let plan = mock
        .plan(&mock.request(name))
        .await
        .unwrap_or_else(|error| panic!("plan: {error}"));
    let preview = plan.preview();
    let confirmed = plan
        .confirm(&preview.plan_id)
        .unwrap_or_else(|error| panic!("confirm: {error}"));
    let stage = mock
        .session
        .begin_write(confirmed.session())
        .unwrap_or_else(|_| panic!("write stage"));
    let report = publish_save_as_copy_plan(
        &mock.service,
        &mock.gateway,
        &mock.journal,
        confirmed,
        &stage,
    )
    .await;
    (preview, report)
}

/// Owner decision 28.10.2026, item 2: a page draft is saved as a new
/// dashboard of exactly its page, through the same plan, journal and
/// read-back as a dashboard draft.
#[tokio::test]
async fn a_page_draft_is_saved_as_a_dashboard_of_its_page() {
    let mock = Mock::start_with(page("1500"), source(), CreateReply::Accept).await;
    let (preview, report) = publish(&mock, TARGET).await;
    assert_eq!(preview.page_count, 1);
    assert_eq!(preview.widget_count, 2);
    assert_eq!(preview.reference_definitions, 1);
    assert_eq!(preview.reference_consumers, 1);
    assert_eq!(preview.unknown_widgets, 0);
    assert_eq!(preview.source_dashboardid, "950");
    assert!(preview.private);
    assert!(preview.compatibility.allows_publication());

    assert!(report.sent());
    assert_eq!(report.journal_error, None);
    assert_eq!(report.journal_state, Some(JournalState::Verified));
    assert_eq!(mock.creates(), 1);
    let calls = lock(&mock.data).calls.clone();
    let sent = calls
        .iter()
        .find(|call| call["method"] == "dashboard.create")
        .unwrap_or_else(|| panic!("create call"));
    let pages = sent["params"]["pages"]
        .as_array()
        .unwrap_or_else(|| panic!("pages"));
    let [only] = pages.as_slice() else {
        panic!("one page: {pages:?}");
    };
    assert_eq!(only["name"], json!("Main"));
    assert!(only.get("dashboard_pageid").is_none());
    assert_eq!(sent["params"]["display_period"], json!("30"));
    assert_eq!(sent["params"]["auto_start"], json!("1"));
    assert_eq!(sent["params"]["private"], json!(1));

    let entry = mock.entry(&report.plan_id);
    assert_eq!(entry.state, JournalState::Verified);
    assert_eq!(entry.intent, SAVE_AS_COPY_OPERATION);
    assert_eq!(
        entry.source.as_ref().map(|source| source.local_id.as_str()),
        Some(mock.local_id.as_str())
    );
    let (dashboardid, _) = report
        .outcome
        .as_ref()
        .and_then(DashboardCreateOutcome::confirmed_dashboard)
        .unwrap_or_else(|| panic!("confirmed dashboard"));
    assert_eq!(entry.created_dashboardid.as_deref(), Some(dashboardid));
    assert_eq!(mock.created_ids(), [dashboardid]);
    // The source dashboard is never written.
    assert_eq!(lock(&mock.data).dashboards[0], source());
}

/// An unknown outcome of a page copy is reconciled by reads only, by exact
/// name and the content fingerprint of the one page.
#[tokio::test]
async fn an_unknown_outcome_of_a_page_copy_is_reconciled_by_reads() {
    let mock = Mock::start_with(page("1500"), source(), CreateReply::AcceptThenBadGateway).await;
    let (_, report) = publish(&mock, TARGET).await;
    assert!(report.sent());
    assert!(matches!(
        report.outcome,
        Some(DashboardCreateOutcome::ReconciledVerified { .. })
    ));
    assert_eq!(report.journal_state, Some(JournalState::ReconciledVerified));
    assert_eq!(mock.creates(), 1, "never retried");
    assert_eq!(mock.created_ids().len(), 1);
}

/// The page draft keeps the other blockers: an unknown widget on its page
/// (Local position) and a widget link whose provider is on another page.
#[tokio::test]
async fn a_page_draft_is_blocked_by_its_unknown_widgets_and_links_to_other_pages() {
    let mock = Mock::start(page("1501")).await;
    let report = incompatible(mock.plan(&mock.request(TARGET)).await);
    assert_eq!(
        report.with_code(FindingCode::DraftKindUnsupported).count(),
        0
    );
    let unknown = report
        .with_code(FindingCode::WriteContractUnverified)
        .next()
        .unwrap_or_else(|| panic!("unknown widget finding"));
    assert_eq!(unknown.param("widget_type"), Some("vendor.private"));
    assert_eq!(
        unknown.subject.as_ref().map(|subject| subject.page_index),
        Some(1)
    );
    assert_eq!(report.with_code(FindingCode::ScopeLinkCrossing).count(), 0);

    let mut crossing = source();
    crossing["pages"][1]["widgets"] = json!([widget(
        "2511",
        "svggraph",
        0,
        json!([
            {"type": "1", "name": "ds.0.hosts.0", "value": "db-*"},
            {"type": "1", "name": "ds.0.override_hostid._reference", "value": "AAAAA._hostid"}
        ])
    )]);
    let mock = Mock::start_with(page("1501"), crossing, CreateReply::Accept).await;
    let report = incompatible(mock.plan(&mock.request(TARGET)).await);
    let link = report
        .with_code(FindingCode::ScopeLinkCrossing)
        .next()
        .unwrap_or_else(|| panic!("link finding"));
    assert_eq!(link.severity, FindingSeverity::BlocksPublication);
    assert_eq!(link.param("side"), Some("provider_outside"));
    let subject = link
        .subject
        .as_ref()
        .unwrap_or_else(|| panic!("link subject"));
    assert_eq!((subject.page_index, subject.widget_index), (1, 0));
    assert_eq!(
        subject.field.as_deref(),
        Some("ds.0.override_hostid._reference")
    );
    assert_eq!(mock.creates(), 0);
    assert!(mock.reads_only());
}

#[tokio::test]
async fn the_plan_is_bound_to_the_previewed_draft_and_session() {
    let mock = Mock::start(ImportScope::Dashboard).await;
    let request = mock.request(TARGET);

    let mut stale = request.clone();
    stale.revision += 1;
    assert_eq!(
        invalidated(mock.plan(&stale).await),
        PlanInvalidReason::DraftChanged
    );
    let mut other_local = request.clone();
    other_local.local_fingerprint = "0".repeat(64);
    assert_eq!(
        invalidated(mock.plan(&other_local).await),
        PlanInvalidReason::DraftChanged
    );
    let mut other_base = request.clone();
    other_base.base_fingerprint = "0".repeat(64);
    assert_eq!(
        invalidated(mock.plan(&other_base).await),
        PlanInvalidReason::DraftChanged
    );
    let mut other_connection = request.clone();
    other_connection.connection_id = "conn-00000000000000000000000000000007".to_owned();
    assert_eq!(
        invalidated(mock.plan(&other_connection).await),
        PlanInvalidReason::ConnectionChanged
    );
    let ended = LocalSession::new();
    let mut admitted = request.clone();
    admitted.session = ended.current();
    ended
        .end(crate::EndSessionRequest { confirmed: true })
        .unwrap_or_else(|error| panic!("end session: {error:?}"));
    assert_eq!(
        invalidated(
            plan_save_as_copy(
                &mock.service,
                &mock.journal,
                &ended,
                &mock.gateway,
                &admitted
            )
            .await
        ),
        PlanInvalidReason::SessionEnded
    );
    assert_eq!(mock.creates(), 0);
}

#[tokio::test]
async fn a_proven_plan_previews_one_private_create_and_is_one_shot() {
    let mock = Mock::start(ImportScope::Dashboard).await;
    let request = mock.request(TARGET);
    let plan = mock.detached_plan(&request);
    let preview = plan.preview();
    assert_eq!(preview.operation, "save_as_copy");
    assert_eq!(preview.api_method, "dashboard.create");
    assert_eq!(preview.write_requests, 1);
    assert!(preview.private);
    assert_eq!(preview.target_name, TARGET);
    assert_eq!(preview.source_dashboardid, "950");
    assert_eq!(preview.page_count, 2);
    assert_eq!(preview.widget_count, 3);
    assert_eq!(preview.reference_definitions, 1);
    assert_eq!(preview.reference_consumers, 1);
    assert_eq!(preview.unknown_widgets, 1);
    assert_eq!(preview.local_fingerprint, request.local_fingerprint);
    assert!(preview.contracts.contains(&"create_copy_unknown_widgets"));
    assert_eq!(preview.name_check, NameCheck::NotVisible);
    assert!(preview.compatibility.allows_publication());

    assert!(matches!(
        plan.confirm("zds-other"),
        Err(CreatePlanError::ConfirmationMismatch)
    ));
    let plan = mock.detached_plan(&request);
    let shown = plan.preview();
    assert_ne!(shown.plan_id, preview.plan_id, "plan IDs are never reused");
    let confirmed = plan
        .confirm(&shown.plan_id)
        .unwrap_or_else(|error| panic!("matching confirmation: {error}"));
    assert_eq!(confirmed.plan_id(), shown.plan_id);
}

#[tokio::test]
async fn sends_are_refused_for_other_sessions_drafts_and_authorities() {
    let mock = Mock::start(ImportScope::Dashboard).await;
    let request = mock.request(TARGET);
    let stage = mock
        .session
        .begin_write(request.session)
        .unwrap_or_else(|_| panic!("write stage"));

    // A plan verified for a detached authority never reaches the server.
    let plan = mock.detached_plan(&request);
    let id = plan.preview().plan_id;
    let confirmed = plan
        .confirm(&id)
        .unwrap_or_else(|error| panic!("confirm: {error}"));
    let report = publish_save_as_copy_plan(
        &mock.service,
        &mock.gateway,
        &mock.journal,
        confirmed,
        &stage,
    )
    .await;
    assert_eq!(
        report.invalidated,
        Some(PlanInvalidReason::ConnectionChanged)
    );
    assert!(report.outcome.is_none());
    assert!(!report.sent());
    // Refused before the journal: no entry is opened.
    assert_eq!(report.journal_state, None);
    assert_eq!(report.journal_error, None);

    // An edit of the draft after confirmation invalidates the plan.
    let plan = mock.detached_plan(&request);
    let id = plan.preview().plan_id;
    let confirmed = plan
        .confirm(&id)
        .unwrap_or_else(|error| panic!("confirm: {error}"));
    mock.service
        .rename_draft(&mock.local_id, "Renamed draft")
        .unwrap_or_else(|error| panic!("rename: {error}"));
    let report = publish_save_as_copy_plan(
        &mock.service,
        &mock.gateway,
        &mock.journal,
        confirmed,
        &stage,
    )
    .await;
    assert_eq!(report.invalidated, Some(PlanInvalidReason::DraftChanged));
    drop(stage);

    // A stage of another session generation.
    let request = mock.request(TARGET);
    let plan = mock.detached_plan(&request);
    let id = plan.preview().plan_id;
    let confirmed = plan
        .confirm(&id)
        .unwrap_or_else(|error| panic!("confirm: {error}"));
    mock.session
        .end(crate::EndSessionRequest { confirmed: true })
        .unwrap_or_else(|error| panic!("end session: {error:?}"));
    let stage = mock
        .session
        .begin_write(mock.session.current())
        .unwrap_or_else(|_| panic!("write stage"));
    let report = publish_save_as_copy_plan(
        &mock.service,
        &mock.gateway,
        &mock.journal,
        confirmed,
        &stage,
    )
    .await;
    assert_eq!(report.invalidated, Some(PlanInvalidReason::SessionEnded));
    drop(stage);

    assert_eq!(mock.creates(), 0);
    assert!(mock.reads_only());
    let listing = mock
        .journal
        .list()
        .unwrap_or_else(|error| panic!("list: {error}"));
    assert!(listing.entries.is_empty() && listing.damaged.is_empty());
}

#[tokio::test]
async fn reconciliation_reads_only_and_records_one_confirmed_dashboard() {
    let mock = Mock::start(ImportScope::Dashboard).await;
    let request = mock.request(TARGET);
    let preview = mock.detached_plan(&request).preview();
    let expectation = CreateExpectation {
        connection_id: CONNECTION.to_owned(),
        server_version: mock.gateway.capabilities().version.clone(),
        name: TARGET.to_owned(),
        content_fingerprint: preview.content_fingerprint,
        owner_userid: OWNER.to_owned(),
    };
    let reader = mock.gateway.reader();

    // Nothing created: not found, which is not a result.
    let none = reconcile_save_as_copy(reader, &expectation, None).await;
    assert_eq!(none, CreateReconciliation::NotFound);
    assert!(!none.is_confirmed());

    // The server holds exactly the planned dashboard.
    lock(&mock.data).dashboards.push(created("951", TARGET));
    let found = reconcile_save_as_copy(reader, &expectation, None).await;
    let CreateReconciliation::Confirmed {
        dashboardid,
        fingerprint,
    } = &found
    else {
        panic!("expected a confirmed candidate, got {found:?}");
    };
    assert_eq!(dashboardid, "951");
    assert_eq!(
        reconcile_save_as_copy(reader, &expectation, Some("951")).await,
        found
    );

    // Two dashboards with the name: never confirmed.
    lock(&mock.data).dashboards.push(created("952", TARGET));
    assert_eq!(
        reconcile_save_as_copy(reader, &expectation, None).await,
        CreateReconciliation::Ambiguous { candidates: 2 }
    );

    let revision = mock
        .service
        .load(&mock.local_id)
        .unwrap_or_else(|error| panic!("load: {error}"))
        .revision;
    for _ in 0..2 {
        mock.service
            .record_created_dashboard(&mock.local_id, &expectation, dashboardid, fingerprint)
            .unwrap_or_else(|error| panic!("record: {error}"));
    }
    let file = mock
        .service
        .load(&mock.local_id)
        .unwrap_or_else(|error| panic!("load: {error}"));
    assert_eq!(file.revision, revision + 1);
    assert_eq!(file.created_dashboards.len(), 1);
    assert_eq!(file.created_dashboards[0].dashboardid, "951");
    assert_eq!(file.created_dashboards[0].name, TARGET);
    let detail = mock
        .service
        .draft(&mock.local_id)
        .unwrap_or_else(|error| panic!("draft: {error}"));
    assert_eq!(detail.summary.created_dashboards.len(), 1);
    assert!(
        mock.service
            .record_created_dashboard(&mock.local_id, &expectation, "x", fingerprint)
            .is_err()
    );

    assert_eq!(mock.creates(), 0);
    assert!(mock.reads_only());
}

// --- publication journal (P2-04D) -------------------------------------
//
// The adapter's create path stays closed until P2-06A (every create
// contract is `Unsupported`, and the zabbix crate covers its own send
// path), so these tests drive the journal integration through
// `journaling::journaled_create` with a stand-in for the adapter that
// honours the same `before_send` contract: `false` sends nothing.

/// What the stand-in adapter does.
enum Adapter {
    /// Returns before `before_send` (checks before send failed).
    Before(DashboardCreateOutcome),
    /// Runs `before_send`; when admitted, dispatches one `dashboard.create`
    /// (stored on the mock when `stored` is set) and returns the outcome.
    After {
        outcome: DashboardCreateOutcome,
        stored: Vec<Value>,
    },
}

impl Mock {
    fn authority(&self) -> (String, String) {
        let identity = self.gateway.reader().identity();
        (
            identity.endpoint_fingerprint().to_owned(),
            identity.fingerprint().to_owned(),
        )
    }

    /// A confirmed plan past the checks right before send. Built from a
    /// detached plan, so the real adapter would refuse it.
    fn prepared(&self, name: &str) -> PreparedCreateSend {
        let request = self.request(name);
        let plan = self.detached_plan(&request);
        let id = plan.preview().plan_id;
        let plan = plan
            .confirm(&id)
            .unwrap_or_else(|error| panic!("confirm: {error}"));
        let expectation = CreateExpectation {
            connection_id: CONNECTION.to_owned(),
            server_version: self.gateway.capabilities().version.clone(),
            name: name.to_owned(),
            content_fingerprint: plan.create.content_fingerprint().to_owned(),
            owner_userid: OWNER.to_owned(),
        };
        PreparedCreateSend { plan, expectation }
    }

    async fn send(&self, prepared: PreparedCreateSend, adapter: Adapter) -> CreatePublishReport {
        let (endpoint_fingerprint, server_identity) = self.authority();
        let authority = journaling::JournalAuthority {
            endpoint_fingerprint: &endpoint_fingerprint,
            server_identity: &server_identity,
        };
        journaling::journaled_create(
            &self.journal,
            prepared,
            &authority,
            async |prepared, handle: &mut JournalHandle| {
                let outcome = match adapter {
                    Adapter::Before(outcome) => outcome,
                    Adapter::After { outcome, stored } => {
                        if handle.admit_send() {
                            let mut data = lock(&self.data);
                            data.calls.push(json!({"method": "dashboard.create"}));
                            data.dashboards.extend(stored);
                            outcome
                        } else {
                            DashboardCreateOutcome::FailedBeforeSend(
                                zdashstitch_zabbix::CreateSendRefusal::SubmitNotRecorded,
                            )
                        }
                    }
                };
                CreatePublishReport {
                    plan_id: prepared.plan_id().to_owned(),
                    outcome: Some(outcome),
                    invalidated: None,
                    record_error: None,
                    journal_state: None,
                    journal_error: None,
                }
            },
        )
        .await
    }

    fn entry(&self, operation_id: &str) -> crate::JournalEntry {
        self.journal
            .entry(operation_id)
            .unwrap_or_else(|error| panic!("entry: {error}"))
    }

    fn check(&self, name: &str, local_id: &str) -> Option<&'static str> {
        let (endpoint, _) = self.authority();
        self.journal
            .check_target(&create_target_key(CONNECTION, &endpoint, name, local_id))
            .err()
            .map(|error| error.code())
    }

    async fn reconcile(&self, operation_id: &str) -> CreateRecovery {
        reconcile_create(
            &self.journal,
            &self.service,
            self.gateway.reader(),
            operation_id,
        )
        .await
        .unwrap_or_else(|error| panic!("reconcile: {error} {:?}", error.params()))
    }

    fn created_ids(&self) -> Vec<String> {
        self.service
            .load(&self.local_id)
            .unwrap_or_else(|error| panic!("load: {error}"))
            .created_dashboards
            .into_iter()
            .map(|record| record.dashboardid)
            .collect()
    }
}

fn unknown(reconciliation: CreateReconciliation) -> Adapter {
    Adapter::After {
        outcome: DashboardCreateOutcome::UnknownOutcome { reconciliation },
        stored: Vec::new(),
    }
}

fn plan_journal_code(result: Result<VerifiedCreatePlan, CreatePlanError>) -> Option<&'static str> {
    match result {
        Err(CreatePlanError::Journal(error)) => Some(error.code()),
        _ => None,
    }
}

/// Injects `kind` for journal appends of `state` while alive (this thread).
struct Failing;

impl Failing {
    fn at(state: JournalState, kind: FailKind) -> Self {
        fail::AT.with(|at: &std::cell::Cell<_>| at.set(Some((state, kind))));
        Self
    }
}

impl Drop for Failing {
    fn drop(&mut self) {
        fail::AT.with(|at: &std::cell::Cell<_>| at.set(None));
    }
}

#[tokio::test]
async fn the_journal_records_the_reconciliation_key_and_a_unique_operation_id() {
    let mock = Mock::start(ImportScope::Dashboard).await;
    let prepared = mock.prepared(TARGET);
    let operation_id = prepared.plan_id().to_owned();
    assert!(crate::publish_journal::is_operation_id(&operation_id));
    let content = prepared.expectation().content_fingerprint.clone();
    let local = prepared.plan.binding.local_fingerprint.clone();
    let report = mock
        .send(
            prepared,
            Adapter::After {
                outcome: DashboardCreateOutcome::Verified {
                    dashboardid: "951".to_owned(),
                    fingerprint: "e".repeat(64),
                },
                stored: vec![created("951", TARGET)],
            },
        )
        .await;
    assert_eq!(report.journal_state, Some(JournalState::Verified));
    assert_eq!(report.journal_error, None);
    let entry = mock.entry(&operation_id);
    assert_eq!(entry.kind, crate::OperationKind::Create);
    assert_eq!(entry.intent, SAVE_AS_COPY_OPERATION);
    assert_eq!(entry.connection_id, CONNECTION);
    assert_eq!(entry.server_version, "7.4.16");
    assert_eq!(
        entry.target,
        crate::JournalTarget::NewDashboard {
            name: TARGET.to_owned()
        }
    );
    assert_eq!(
        entry.source.as_ref().map(|source| source.local_id.as_str()),
        Some(mock.local_id.as_str())
    );
    assert_eq!(entry.source_fingerprint, local);
    assert_eq!(entry.expected_fingerprint, content);
    assert_eq!(entry.owner_userid.as_deref(), Some(OWNER));
    assert_eq!(entry.created_dashboardid.as_deref(), Some("951"));
    assert_eq!(entry.evidence.as_deref(), Some("read_back_matches"));
    assert_eq!(entry.sends, 1);
    assert_eq!(entry.progress, JournalProgress::Finished);
    assert_eq!(mock.check(TARGET, &mock.local_id), None);
    // Plan IDs differ even with the same inputs and sequence position
    // across restarts (random part).
    let a = mock.prepared(TARGET).plan_id().to_owned();
    let b = mock.prepared(TARGET).plan_id().to_owned();
    assert_ne!(a, b);
    assert_eq!(a.split('-').count(), 6, "{a}");
}

#[tokio::test]
async fn a_crash_after_submitting_blocks_the_name_and_the_workcopy_until_reconciled() {
    let mock = Mock::start(ImportScope::Dashboard).await;
    let prepared = mock.prepared(TARGET);
    let operation_id = prepared.plan_id().to_owned();
    let (endpoint_fingerprint, server_identity) = mock.authority();
    let record = journaling::operation_record(
        &prepared,
        &journaling::JournalAuthority {
            endpoint_fingerprint: &endpoint_fingerprint,
            server_identity: &server_identity,
        },
    );
    let mut handle = mock
        .journal
        .begin(&operation_id, &record)
        .unwrap_or_else(|error| panic!("begin: {error}"));
    assert!(handle.admit_send());
    // The process dies after `submitting`, before any outcome.
    drop(handle);
    let recovery = mock
        .journal
        .recover()
        .unwrap_or_else(|error| panic!("recover: {error}"));
    assert_eq!(recovery.unfinished.len(), 1);
    assert_eq!(
        recovery.unfinished[0].progress,
        JournalProgress::NeedsReconciliation
    );

    // The same name, and another name from the same workcopy, are blocked
    // for new plans …
    assert_eq!(
        plan_journal_code(mock.plan(&mock.request(TARGET)).await),
        Some("target_blocked")
    );
    assert_eq!(
        plan_journal_code(mock.plan(&mock.request("Another copy")).await),
        Some("target_blocked")
    );
    // … and the same name from another workcopy too.
    let other = mock
        .service
        .import(
            mock.gateway.reader(),
            CONNECTION,
            &ImportRequest {
                dashboardid: "950".to_owned(),
                scope: ImportScope::Dashboard,
                name: Some("Second draft".to_owned()),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("import: {error}"))
        .local_id;
    assert_eq!(mock.check(TARGET, &other), Some("target_blocked"));
    assert_eq!(mock.check("Another copy", &other), None);
    // … and for sends: the journal refuses to open a second entry.
    let report = mock
        .send(
            mock.prepared(TARGET),
            Adapter::After {
                outcome: DashboardCreateOutcome::Verified {
                    dashboardid: "951".to_owned(),
                    fingerprint: "e".repeat(64),
                },
                stored: Vec::new(),
            },
        )
        .await;
    assert_eq!(
        report.journal_error.as_ref().map(JournalError::code),
        Some("target_blocked")
    );
    assert!(report.outcome.is_none());
    assert_eq!(mock.creates(), 0);

    // Nothing visible yet: zero candidates is not a result.
    let first = mock.reconcile(&operation_id).await;
    assert_eq!(first.reconciliation, CreateReconciliation::NotFound);
    assert_eq!(first.entry.state, JournalState::ManualCheckRequired);
    assert_eq!(first.entry.evidence.as_deref(), Some("reconcile_not_found"));
    assert_eq!(mock.check(TARGET, &mock.local_id), Some("target_blocked"));

    // The copy shows up: exactly one confirmed candidate.
    lock(&mock.data).dashboards.push(created("951", TARGET));
    let second = mock.reconcile(&operation_id).await;
    assert!(second.reconciliation.is_confirmed());
    assert_eq!(second.entry.state, JournalState::ReconciledVerified);
    assert_eq!(second.entry.created_dashboardid.as_deref(), Some("951"));
    assert_eq!(second.record_error, None);
    assert_eq!(mock.created_ids(), ["951"]);
    assert_eq!(mock.check(TARGET, &mock.local_id), None);
    assert!(
        mock.journal
            .recover()
            .unwrap_or_else(|error| panic!("recover: {error}"))
            .is_empty()
    );
    assert_eq!(mock.creates(), 0, "recovery never creates");
    assert!(mock.reads_only());
}

#[tokio::test]
async fn a_submit_record_that_is_not_durable_sends_no_create() {
    let mock = Mock::start(ImportScope::Dashboard).await;
    let created_one = || Adapter::After {
        outcome: DashboardCreateOutcome::Verified {
            dashboardid: "951".to_owned(),
            fingerprint: "e".repeat(64),
        },
        stored: vec![created("951", TARGET)],
    };
    for kind in [FailKind::BeforeWrite, FailKind::Torn, FailKind::Sync] {
        let report = {
            let _failing = Failing::at(JournalState::Submitting, kind);
            mock.send(mock.prepared(TARGET), created_one()).await
        };
        assert_eq!(
            report.outcome,
            Some(DashboardCreateOutcome::FailedBeforeSend(
                zdashstitch_zabbix::CreateSendRefusal::SubmitNotRecorded
            )),
            "{kind:?}"
        );
        assert!(!report.sent());
        assert_eq!(
            report.journal_error.as_ref().map(JournalError::code),
            Some("journal_write_failed"),
            "{kind:?}"
        );
        assert_eq!(mock.creates(), 0, "{kind:?}: nothing may be sent");
        let entry = mock.entry(&report.plan_id);
        if kind == FailKind::Sync {
            // The line may be on disk: checked before anything else.
            assert_eq!(entry.progress, JournalProgress::NeedsReconciliation);
            assert_eq!(mock.check(TARGET, &mock.local_id), Some("target_blocked"));
        } else {
            assert_eq!(entry.state, JournalState::Confirmed);
            assert_eq!(entry.progress, JournalProgress::Abandoned);
            assert_eq!(mock.check(TARGET, &mock.local_id), None);
        }
    }
}

#[tokio::test]
async fn a_taken_name_is_final_and_never_retried() {
    let mock = Mock::start(ImportScope::Dashboard).await;
    // `already exists` after send: final, the target is free again.
    let report = mock
        .send(
            mock.prepared(TARGET),
            Adapter::After {
                outcome: DashboardCreateOutcome::NameTaken { sent: true },
                stored: Vec::new(),
            },
        )
        .await;
    assert_eq!(report.journal_state, Some(JournalState::ReconciledFailed));
    assert_eq!(report.journal_error, None);
    let entry = mock.entry(&report.plan_id);
    assert_eq!(entry.evidence.as_deref(), Some("dashboard_name_taken"));
    assert_eq!(entry.sends, 1);
    assert_eq!(entry.progress, JournalProgress::Finished);
    assert_eq!(mock.creates(), 1, "exactly one send, no retry");
    assert_eq!(mock.check(TARGET, &mock.local_id), None);
    assert_eq!(
        code_of(
            reconcile_create(
                &mock.journal,
                &mock.service,
                mock.gateway.reader(),
                &report.plan_id
            )
            .await
        ),
        Some("reconcile_not_needed")
    );

    // Visible before send: nothing sent, final.
    let report = mock
        .send(
            mock.prepared(TARGET),
            Adapter::Before(DashboardCreateOutcome::NameTaken { sent: false }),
        )
        .await;
    assert_eq!(report.journal_state, Some(JournalState::FailedBeforeSend));
    let entry = mock.entry(&report.plan_id);
    assert_eq!(entry.sends, 0);
    assert_eq!(entry.evidence.as_deref(), Some("dashboard_name_taken"));
    assert_eq!(mock.creates(), 1);
    assert_eq!(mock.check(TARGET, &mock.local_id), None);
}

fn code_of<T>(result: Result<T, JournalError>) -> Option<&'static str> {
    result.err().map(|error| error.code())
}

#[tokio::test]
async fn an_unknown_outcome_is_reconciled_by_reads_and_recorded_in_the_workcopy() {
    let mock = Mock::start(ImportScope::Dashboard).await;
    // The server created the copy, but the answer was lost and the reads
    // right after did not see it yet.
    let report = mock
        .send(
            mock.prepared(TARGET),
            Adapter::After {
                outcome: DashboardCreateOutcome::UnknownOutcome {
                    reconciliation: CreateReconciliation::NotFound,
                },
                stored: vec![created("951", TARGET)],
            },
        )
        .await;
    assert_eq!(report.journal_state, Some(JournalState::UnknownOutcome));
    let entry = mock.entry(&report.plan_id);
    assert_eq!(entry.evidence.as_deref(), Some("reconcile_not_found"));
    assert_eq!(entry.progress, JournalProgress::NeedsReconciliation);
    assert_eq!(
        plan_journal_code(mock.plan(&mock.request(TARGET)).await),
        Some("target_blocked")
    );

    let recovery = mock.reconcile(&report.plan_id).await;
    assert_eq!(recovery.entry.state, JournalState::ReconciledVerified);
    assert_eq!(recovery.entry.created_dashboardid.as_deref(), Some("951"));
    assert_eq!(
        recovery.entry.evidence.as_deref(),
        Some("reconcile_confirmed")
    );
    assert!(recovery.entry.observed_fingerprint.is_some());
    // O-022: the confirmed copy is listed in the workcopy.
    assert_eq!(mock.created_ids(), ["951"]);
    assert_eq!(mock.check(TARGET, &mock.local_id), None);
    assert_eq!(mock.creates(), 1, "no second create");
}

#[tokio::test]
async fn a_reported_dashboardid_is_reconciled_by_id() {
    let mock = Mock::start(ImportScope::Dashboard).await;
    // The server reported 951; the read-back failed. Another dashboard with
    // the same name exists, so a lookup by name alone would be ambiguous.
    let report = mock
        .send(
            mock.prepared(TARGET),
            Adapter::After {
                outcome: DashboardCreateOutcome::FailedAfterResponse {
                    reason: zdashstitch_zabbix::CreateFailureReason::ReadBackUnavailable,
                    dashboardid: Some("951".to_owned()),
                },
                stored: vec![created("951", TARGET), created("952", TARGET)],
            },
        )
        .await;
    assert_eq!(
        report.journal_state,
        Some(JournalState::FailedAfterResponse)
    );
    let entry = mock.entry(&report.plan_id);
    assert_eq!(entry.reported_dashboardid.as_deref(), Some("951"));
    assert_eq!(entry.created_dashboardid, None, "reported is not confirmed");
    assert_eq!(entry.evidence.as_deref(), Some("read_back_unavailable"));
    assert_eq!(entry.progress, JournalProgress::NeedsReconciliation);

    let recovery = mock.reconcile(&report.plan_id).await;
    assert_eq!(recovery.entry.state, JournalState::ReconciledVerified);
    assert_eq!(recovery.entry.created_dashboardid.as_deref(), Some("951"));
    assert_eq!(mock.created_ids(), ["951"]);
}

#[tokio::test]
async fn undetermined_reconciliations_need_a_manual_check_until_closed_manually() {
    let mock = Mock::start(ImportScope::Dashboard).await;
    let report = mock
        .send(
            mock.prepared(TARGET),
            unknown(CreateReconciliation::NotFound),
        )
        .await;
    let operation_id = report.plan_id;

    // Nothing visible: not final.
    let recovery = mock.reconcile(&operation_id).await;
    assert_eq!(recovery.entry.state, JournalState::ManualCheckRequired);
    assert_eq!(
        recovery.entry.evidence.as_deref(),
        Some("reconcile_not_found")
    );
    assert_eq!(mock.check(TARGET, &mock.local_id), Some("target_blocked"));

    // Two candidates: never confirmed.
    lock(&mock.data)
        .dashboards
        .extend([created("951", TARGET), created("952", TARGET)]);
    let recovery = mock.reconcile(&operation_id).await;
    assert_eq!(
        recovery.reconciliation,
        CreateReconciliation::Ambiguous { candidates: 2 }
    );
    assert_eq!(recovery.entry.state, JournalState::ManualCheckRequired);
    assert_eq!(
        recovery.entry.evidence.as_deref(),
        Some("reconcile_ambiguous")
    );
    assert_eq!(mock.check(TARGET, &mock.local_id), Some("target_blocked"));

    // One candidate that is not the planned copy (shared): not confirmed.
    {
        let mut data = lock(&mock.data);
        data.dashboards
            .retain(|dashboard| dashboard["dashboardid"] != "952");
        if let Some(copy) = data
            .dashboards
            .iter_mut()
            .find(|dashboard| dashboard["dashboardid"] == "951")
        {
            copy["private"] = json!("0");
        }
    }
    let recovery = mock.reconcile(&operation_id).await;
    assert!(matches!(
        recovery.reconciliation,
        CreateReconciliation::Mismatch { .. }
    ));
    assert_eq!(recovery.entry.state, JournalState::ManualCheckRequired);
    assert!(mock.created_ids().is_empty());

    let before = mock.entry(&operation_id).transitions.len();

    // «Закрыть без сверки»: final, the entry stays, the target is free.
    let closed = mock
        .journal
        .close_manually(&CloseManuallyRequest {
            operation_id: operation_id.clone(),
            confirmed: true,
        })
        .unwrap_or_else(|error| panic!("close: {error}"));
    assert_eq!(closed.state, JournalState::ClosedManually);
    assert_eq!(closed.transitions.len(), before + 1);
    assert_eq!(closed.created_dashboardid, None);
    assert_eq!(mock.check(TARGET, &mock.local_id), None);
    assert!(
        mock.plan(&mock.request(TARGET))
            .await
            .is_err_and(|error| !matches!(error, CreatePlanError::Journal(_))),
        "the journal no longer blocks; the plan starts from fresh reads"
    );
    assert_eq!(
        code_of(
            reconcile_create(
                &mock.journal,
                &mock.service,
                mock.gateway.reader(),
                &operation_id
            )
            .await
        ),
        Some("reconcile_not_needed")
    );
    assert_eq!(mock.creates(), 1);
}

#[tokio::test]
async fn reconcile_create_refuses_update_entries_and_other_servers() {
    let mock = Mock::start(ImportScope::Dashboard).await;
    let report = mock
        .send(
            mock.prepared(TARGET),
            unknown(CreateReconciliation::NotFound),
        )
        .await;
    // An update reconciliation does not apply to a create entry.
    assert_eq!(
        code_of(
            crate::reconcile_update(&mock.journal, mock.gateway.reader(), &report.plan_id).await
        ),
        Some("reconcile_kind_mismatch")
    );
    // Another server (endpoint and identity): nothing is recorded.
    let before = mock.entry(&report.plan_id);
    let other = Mock::start(ImportScope::Dashboard).await;
    assert_eq!(
        code_of(
            reconcile_create(
                &mock.journal,
                &mock.service,
                other.gateway.reader(),
                &report.plan_id
            )
            .await
        ),
        Some("reconcile_server_changed")
    );
    assert_eq!(mock.entry(&report.plan_id), before);
}

#[tokio::test]
async fn failed_or_truncated_reads_record_nothing_and_can_be_retried() {
    let mock = Mock::start(ImportScope::Dashboard).await;
    let report = mock
        .send(
            mock.prepared(TARGET),
            unknown(CreateReconciliation::NotFound),
        )
        .await;
    let operation_id = report.plan_id;
    let before = mock.entry(&operation_id);
    let attempt = || {
        reconcile_create(
            &mock.journal,
            &mock.service,
            mock.gateway.reader(),
            &operation_id,
        )
    };

    // A failed read is no evidence: the connection code, nothing recorded.
    lock(&mock.data).fail_reads = true;
    let error = attempt()
        .await
        .err()
        .unwrap_or_else(|| panic!("a failed read must not record a result"));
    assert_ne!(error.code(), "reconcile_partial_visibility");
    assert_eq!(mock.entry(&operation_id), before);
    lock(&mock.data).fail_reads = false;

    // A truncated read did not see every candidate: nothing recorded.
    lock(&mock.data).dashboards.extend(
        (0..=zdashstitch_zabbix::MAX_NAMED_DASHBOARDS)
            .map(|index| created(&(951 + index).to_string(), TARGET)),
    );
    assert_eq!(
        code_of(attempt().await),
        Some("reconcile_partial_visibility")
    );
    assert_eq!(mock.entry(&operation_id), before);
    assert_eq!(before.state, JournalState::UnknownOutcome);
    assert_eq!(mock.check(TARGET, &mock.local_id), Some("target_blocked"));

    // Retried once the reads see everything: exactly one candidate.
    lock(&mock.data)
        .dashboards
        .retain(|dashboard| dashboard["dashboardid"] == "950" || dashboard["dashboardid"] == "951");
    let recovery = mock.reconcile(&operation_id).await;
    assert_eq!(recovery.entry.state, JournalState::ReconciledVerified);
    assert_eq!(mock.created_ids(), ["951"]);
    assert_eq!(mock.creates(), 1, "recovery never creates");
}
