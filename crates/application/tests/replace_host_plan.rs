// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-04B end to end against a mock Zabbix JSON-RPC transport on loopback:
//! import → ReplaceHost calculation → `plan_replace_host` → confirmation →
//! `publish_replace_host_plan`, plus the durable journal and recovery
//! (P2-04D) and the Remote re-reads (P2-04C). All data is synthetic; no
//! laboratory or production server is contacted.

use std::sync::{Arc, Mutex, PoisonError};

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, extract::State, routing::post};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use zdashstitch_application::connections::ConnectionService;
use zdashstitch_application::replace_host::{ReplaceHostPreview, SessionContext};
use zdashstitch_application::resolver::{ReplaceHostRequest, ResolveScope};
use zdashstitch_application::workcopy::DraftFreshnessState;
use zdashstitch_application::workcopy::{ImportRequest, ImportScope, WorkcopyService};
use zdashstitch_application::{
    ClonePublicationRequest, CloseManuallyRequest, CreatePlanError, EndSessionRequest, FindingCode,
    JournalProgress, JournalState, LocalSession, PlanError, PlanInvalidReason, PublishJournal,
    ReplaceHostPlanRequest, VerifiedPlan, plan_clone_dashboard, plan_replace_host,
    preflight_clone_publication, publish_clone_dashboard_plan, publish_confirmed_plan,
    publish_replace_host_plan, recheck_replace_host, reconcile_update,
};
use zdashstitch_zabbix::{
    Authentication, ReadWriteGateway, ZabbixClient, ZabbixEndpoint, read_only_allowlist,
};

const CONNECTION: &str = "conn-00000000000000000000000000000002";

struct MockData {
    dashboard: Value,
    extra_host: Option<Value>,
    own_role_rules: Option<Value>,
    returned_roleid: &'static str,
    created_dashboard: Option<Value>,
    editable: bool,
    calls: Vec<Value>,
    /// The next `dashboard.update` is applied, but its answer is not
    /// JSON and every later request fails until reset: the outcome of the
    /// write cannot be known.
    lose_update_answer: bool,
    /// Every request answers HTTP 503.
    unreachable: bool,
    /// `dashboard.get` returns nothing (missing or not visible).
    hidden: bool,
    /// A page sent by `dashboard.update` is stored with each widget's
    /// first field moved to the end and the widgets reversed, as Zabbix
    /// 7.4.16 on PostgreSQL may return it (LAB-02).
    reorder_on_update: bool,
}

type Shared = Arc<Mutex<MockData>>;

fn lock(data: &Shared) -> std::sync::MutexGuard<'_, MockData> {
    data.lock().unwrap_or_else(PoisonError::into_inner)
}

fn hosts() -> Vec<Value> {
    vec![
        json!({"hostid": "11", "host": "db_old_01", "name": "db-old-01"}),
        json!({"hostid": "12", "host": "db_new_01", "name": "db-new-01"}),
    ]
}

fn item(itemid: &str, hostid: &str, key: &str, value_type: &str, templateid: &str) -> Value {
    let host = hosts()
        .into_iter()
        .find(|host| host["hostid"] == hostid)
        .map_or_else(
            || json!({"hostid": hostid, "name": "template"}),
            |host| json!({"hostid": hostid, "name": host["name"]}),
        );
    json!({
        "itemid": itemid, "hostid": hostid, "key_": key, "name": key,
        "value_type": value_type, "units": "", "flags": "0", "templateid": templateid,
        "itemDiscovery": [], "hosts": [host], "valuemapid": "0", "valuemap": []
    })
}

fn items() -> Vec<Value> {
    vec![
        item("101", "11", "pgsql.ping", "3", "901"),
        item("102", "11", "system.cpu.util", "0", "902"),
        item("201", "12", "pgsql.ping", "3", "901"),
        item("202", "12", "system.cpu.util", "0", "902"),
        item("901", "800", "pgsql.ping", "3", "0"),
        item("902", "800", "system.cpu.util", "0", "0"),
    ]
}

fn widget(widgetid: &str, widget_type: &str, x: u32, fields: Value) -> Value {
    json!({
        "widgetid": widgetid, "type": widget_type, "name": format!("w{widgetid}"),
        "x": x.to_string(), "y": "0", "width": "12", "height": "4", "view_mode": "0",
        "fields": fields
    })
}

/// Page 1500: two items of the old host and an untouched `svggraph`; page
/// 1501: one item of the old host; page 1502: `problems` of the old host.
fn dashboard() -> Value {
    json!({
        "dashboardid": "950", "name": "Synthetic plan dashboard", "userid": "3",
        "private": "1", "display_period": "30", "auto_start": "1",
        "users": [{"userid": "7", "permission": "2"}], "userGroups": [],
        "pages": [
            {"dashboard_pageid": "1500", "name": "Main", "display_period": "0", "widgets": [
                widget("2500", "item", 0, json!([
                    {"type": "4", "name": "itemid.0", "value": "101"},
                    {"type": "1", "name": "reference", "value": "AAAAA"},
                    {"type": "0", "name": "show.0", "value": "2"}
                ])),
                widget("2501", "item", 12, json!([
                    {"type": "4", "name": "itemid.0", "value": "102"},
                    {"type": "1", "name": "description", "value": "{ITEM.NAME}"}
                ])),
                widget("2502", "svggraph", 24, json!([
                    {"type": "1", "name": "ds.0.hosts.0", "value": "shared-*"},
                    {"type": "1", "name": "ds.0.items.0", "value": "CPU*"},
                    {"type": "1", "name": "ds.0.color.0", "value": "477BDE"},
                    {"type": "1", "name": "reference", "value": "BBBBB"}
                ]))
            ]},
            {"dashboard_pageid": "1501", "name": "Second", "display_period": "60", "widgets": [
                widget("2510", "item", 0, json!([{"type": "4", "name": "itemid.0", "value": "101"}]))
            ]},
            {"dashboard_pageid": "1502", "name": "Problems", "display_period": "0", "widgets": [
                widget("2520", "problems", 0, json!([{"type": "3", "name": "hostids.0", "value": "11"}]))
            ]}
        ]
    })
}

fn ids(params: &Value, key: &str) -> Option<Vec<String>> {
    params.get(key).and_then(Value::as_array).map(|values| {
        values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    })
}

fn matches(object: &Value, key: &str, wanted: Option<&Vec<String>>) -> bool {
    wanted.is_none_or(|wanted| wanted.iter().any(|id| object[key] == id.as_str()))
}

async fn rpc(State(data): State<Shared>, Json(body): Json<Value>) -> Response {
    let id = body.get("id").cloned().unwrap_or(Value::Null);
    let params = body["params"].clone();
    let mut data = lock(&data);
    data.calls.push(body.clone());
    if data.unreachable {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if body["method"] == "dashboard.update" && data.lose_update_answer {
        data.lose_update_answer = false;
        data.unreachable = true;
        apply_update(&mut data.dashboard, &params);
        return "connection reset".into_response();
    }
    let result = match body["method"].as_str().unwrap_or_default() {
        "apiinfo.version" => json!("7.4.16"),
        "user.checkAuthentication" => {
            json!({"userid": "3", "username": "synthetic", "roleid": "7", "type": 2})
        }
        "role.get" => {
            assert_eq!(
                params,
                json!({
                    "roleids": ["7"], "output": ["roleid", "type"],
                    "selectRules": ["api.access", "api.mode", "api", "actions"],
                })
            );
            data.own_role_rules.as_ref().map_or_else(
                || json!([]),
                |rules| json!([{"roleid": data.returned_roleid, "type": "2", "rules": rules}]),
            )
        }
        "dashboard.get" if data.hidden => json!([]),
        "dashboard.get" if params.get("editable").is_some() && !data.editable => json!([]),
        "dashboard.get" => {
            let by_id = ids(&params, "dashboardids");
            let names = params["filter"]["name"].as_array();
            Value::Array(
                std::iter::once(&data.dashboard)
                    .chain(&data.created_dashboard)
                    .filter(|dashboard| {
                        matches(dashboard, "dashboardid", by_id.as_ref())
                            && names.is_none_or(|names| names.contains(&dashboard["name"]))
                    })
                    .cloned()
                    .collect(),
            )
        }
        "host.get" => {
            let by_id = ids(&params, "hostids");
            let names = params["search"]["name"].as_array().cloned();
            Value::Array(
                hosts()
                    .into_iter()
                    .chain(data.extra_host.clone())
                    .filter(|host| {
                        matches(host, "hostid", by_id.as_ref())
                            && names.as_ref().is_none_or(|names| {
                                names.iter().any(|name| {
                                    name.as_str().map(str::to_lowercase)
                                        == host["name"].as_str().map(str::to_lowercase)
                                })
                            })
                    })
                    .collect(),
            )
        }
        "item.get" => {
            let by_id = ids(&params, "itemids");
            let by_host = ids(&params, "hostids");
            Value::Array(
                items()
                    .into_iter()
                    .filter(|item| {
                        matches(item, "itemid", by_id.as_ref())
                            && matches(item, "hostid", by_host.as_ref())
                    })
                    .collect(),
            )
        }
        "graph.get" => json!([]),
        "hostgroup.get" | "templategroup.get" | "template.get" | "user.get" | "usergroup.get" => {
            json!([])
        }
        "dashboard.update" => {
            let reorder = data.reorder_on_update;
            apply_update(&mut data.dashboard, &params);
            if reorder {
                reorder_sent_pages(&mut data.dashboard, &params);
            }
            json!({"dashboardids": ["950"]})
        }
        "dashboard.create" => {
            assert!(data.created_dashboard.is_none(), "a second create was sent");
            let mut created = params.clone();
            created["dashboardid"] = json!("951");
            created["userid"] = json!("3");
            created["private"] = json!("1");
            created["pages"][0]["dashboard_pageid"] = json!("1900");
            created["pages"][0]["widgets"][0]["widgetid"] = json!("2900");
            data.created_dashboard = Some(created);
            json!({"dashboardids": ["951"]})
        }
        _ => {
            return Json(json!({"jsonrpc": "2.0", "id": id, "error": {
                "code": -32602, "message": "Invalid params.", "data": "Not expected by the mock"
            }}))
            .into_response();
        }
    };
    Json(json!({"jsonrpc": "2.0", "id": id, "result": result})).into_response()
}

fn apply_update(dashboard: &mut Value, params: &Value) {
    let pages = dashboard["pages"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("pages"));
    for incoming in params["pages"]
        .as_array()
        .unwrap_or_else(|| panic!("sent pages"))
    {
        let current = pages
            .iter_mut()
            .find(|page| page["dashboard_pageid"] == incoming["dashboard_pageid"])
            .unwrap_or_else(|| panic!("update page must exist"));
        if incoming.get("widgets").is_some() {
            *current = incoming.clone();
        }
    }
}

fn reorder_sent_pages(dashboard: &mut Value, params: &Value) {
    let sent = params["pages"]
        .as_array()
        .unwrap_or_else(|| panic!("sent pages"))
        .iter()
        .filter(|page| page.get("widgets").is_some())
        .map(|page| page["dashboard_pageid"].clone())
        .collect::<Vec<_>>();
    for page in dashboard["pages"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("pages"))
    {
        if !sent.contains(&page["dashboard_pageid"]) {
            continue;
        }
        let widgets = page["widgets"]
            .as_array_mut()
            .unwrap_or_else(|| panic!("widgets"));
        widgets.reverse();
        for widget in widgets {
            widget["fields"]
                .as_array_mut()
                .unwrap_or_else(|| panic!("fields"))
                .rotate_left(1);
        }
    }
}

struct Mock {
    data: Shared,
    gateway: Option<ReadWriteGateway>,
    service: WorkcopyService,
    journal: PublishJournal,
    session: LocalSession,
    local_id: String,
    _dir: tempfile::TempDir,
    _task: JoinHandle<()>,
}

impl Mock {
    /// Imports the dashboard read-only, then enables writes.
    async fn start() -> Self {
        Self::start_with(dashboard()).await
    }

    /// [`Self::start`] with another server dashboard.
    async fn start_with(server_dashboard: Value) -> Self {
        let data: Shared = Arc::new(Mutex::new(MockData {
            dashboard: server_dashboard,
            extra_host: None,
            own_role_rules: None,
            returned_roleid: "7",
            created_dashboard: None,
            editable: true,
            calls: Vec::new(),
            lose_update_answer: false,
            unreachable: false,
            hidden: false,
            reorder_on_update: false,
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
                    scope: ImportScope::Dashboard,
                    name: None,
                },
            )
            .await
            .unwrap_or_else(|error| panic!("import: {error} {:?}", error.params()))
            .local_id;
        Self {
            data,
            gateway: Some(reader.enable_writes()),
            service,
            journal: PublishJournal::new(dir.path().to_path_buf()),
            session: LocalSession::new(),
            local_id,
            _dir: dir,
            _task: task,
        }
    }

    fn writer(&self) -> &ReadWriteGateway {
        self.gateway
            .as_ref()
            .unwrap_or_else(|| panic!("write gateway"))
    }

    /// Back to read-only and to read-write again: a new write epoch.
    fn reopen_writes(&mut self) {
        let gateway = self
            .gateway
            .take()
            .unwrap_or_else(|| panic!("write gateway"));
        self.gateway = Some(gateway.into_read_only().enable_writes());
    }

    fn context(&self) -> SessionContext<'_> {
        SessionContext::Connected {
            access_mode: self.writer().mode(),
            capabilities: self.writer().capabilities(),
        }
    }

    async fn calculate(&self, scope: ResolveScope) -> ReplaceHostPreview {
        self.service
            .calculate_replace_host(
                self.writer().reader(),
                &self.local_id,
                &ReplaceHostRequest {
                    source_hostid: "11".to_owned(),
                    target_hostid: "12".to_owned(),
                    scope,
                },
                self.context(),
            )
            .await
            .unwrap_or_else(|error| panic!("calculate: {error} {:?}", error.params()))
    }

    fn request(&self, preview: &ReplaceHostPreview) -> ReplaceHostPlanRequest {
        ReplaceHostPlanRequest {
            local_id: preview.local_id.clone(),
            connection_id: CONNECTION.to_owned(),
            revision: preview.revision,
            base_fingerprint: preview.base_fingerprint.clone(),
            local_fingerprint: preview.local_fingerprint.clone(),
            kept_digest: (!preview.kept_on_source.entries.is_empty())
                .then(|| preview.kept_on_source.digest.clone()),
            session: self.session.current(),
        }
    }

    async fn plan(&self, request: &ReplaceHostPlanRequest) -> Result<VerifiedPlan, PlanError> {
        plan_replace_host(
            &self.service,
            &self.journal,
            &self.session,
            self.writer(),
            request,
        )
        .await
    }

    /// Confirms `plan` and publishes it under a write stage.
    async fn publish(&self, plan: VerifiedPlan) -> zdashstitch_application::PublishReport {
        let shown = plan.preview().plan_id;
        let confirmed = plan
            .confirm(&shown)
            .unwrap_or_else(|error| panic!("confirm: {error}"));
        let stage = self
            .session
            .begin_write(confirmed.session())
            .unwrap_or_else(|_| panic!("write stage"));
        publish_replace_host_plan(
            &self.service,
            self.writer(),
            &self.journal,
            confirmed,
            &stage,
        )
        .await
    }

    fn updates(&self) -> Vec<Value> {
        lock(&self.data)
            .calls
            .iter()
            .filter(|call| call["method"] == "dashboard.update")
            .map(|call| call["params"].clone())
            .collect()
    }

    fn reads_only(&self) -> bool {
        let allowed = read_only_allowlist();
        lock(&self.data)
            .calls
            .iter()
            .all(|call| allowed.contains(&call["method"].as_str().unwrap_or_default()))
    }
}

fn one_page() -> ResolveScope {
    ResolveScope::Pages(["1500".to_owned()].into())
}

fn ok_plan(result: Result<VerifiedPlan, PlanError>) -> VerifiedPlan {
    result.unwrap_or_else(|error| panic!("plan: {error}"))
}

/// Sent widget equals the read widget except the listed field values.
fn assert_only_values_changed(sent: &Value, read: &Value, changed: &[(&str, &str)]) {
    let mut expected = read.clone();
    for field in expected["fields"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("fields"))
    {
        if let Some((_, value)) = changed.iter().find(|(name, _)| field["name"] == *name) {
            field["value"] = json!(value);
        }
    }
    assert_eq!(sent, &expected);
}

#[tokio::test]
async fn item_only_plan_on_one_page_is_sent_once_and_keeps_every_other_byte() {
    let mock = Mock::start().await;
    let preview = mock.calculate(one_page()).await;
    assert_eq!(preview.changes.len(), 2, "{:?}", preview.report);
    assert!(preview.report.allows_publication(), "{:?}", preview.report);
    let plan = ok_plan(mock.plan(&mock.request(&preview)).await);
    assert!(mock.reads_only(), "planning must not write");
    let shown = plan.preview();
    assert_eq!(shown.changed_pages, vec![0]);
    assert_eq!(shown.replacement_count, 2);

    let confirmed = plan
        .confirm(&shown.plan_id)
        .unwrap_or_else(|error| panic!("confirm: {error}"));
    let stage = mock
        .session
        .begin_write(confirmed.session())
        .unwrap_or_else(|_| panic!("write stage"));
    let report = publish_replace_host_plan(
        &mock.service,
        mock.writer(),
        &mock.journal,
        confirmed,
        &stage,
    )
    .await;
    drop(stage);
    assert_eq!(report.states.last(), Some(&JournalState::Verified));
    assert!(report.deployment_state_may_advance);
    assert_eq!(report.invalidated, None);

    let updates = mock.updates();
    assert_eq!(updates.len(), 1, "exactly one write per confirmed plan");
    let sent = &updates[0];
    let read = dashboard();
    assert_eq!(
        sent.as_object().map(|object| object.len()),
        Some(2),
        "only dashboardid and pages"
    );
    assert_eq!(sent["dashboardid"], "950");
    assert_eq!(sent["pages"][1], json!({"dashboard_pageid": "1501"}));
    assert_eq!(sent["pages"][2], json!({"dashboard_pageid": "1502"}));
    let page = &sent["pages"][0];
    assert_eq!(page.as_object().map(|object| object.len()), Some(4));
    for key in ["dashboard_pageid", "name", "display_period"] {
        assert_eq!(page[key], read["pages"][0][key]);
    }
    assert_only_values_changed(
        &page["widgets"][0],
        &read["pages"][0]["widgets"][0],
        &[("itemid.0", "201")],
    );
    assert_only_values_changed(
        &page["widgets"][1],
        &read["pages"][0]["widgets"][1],
        &[("itemid.0", "202")],
    );
    assert_only_values_changed(&page["widgets"][2], &read["pages"][0]["widgets"][2], &[]);

    // The server now holds exactly Local; the old-host widgets out of scope
    // and the dashboard properties are untouched.
    let local = mock
        .service
        .local_snapshot(&mock.local_id)
        .unwrap_or_else(|error| panic!("local: {error}"));
    assert_eq!(&lock(&mock.data).dashboard, local.raw_api_document());
    assert_eq!(lock(&mock.data).dashboard["pages"][1], read["pages"][1]);
    assert_eq!(lock(&mock.data).dashboard["users"], read["users"]);
}

/// LAB-02: on 7.4.16 several pages form one plan, and a read-back in
/// another widget and field order is still verified (canonical
/// fingerprint), with exactly one write.
#[tokio::test]
async fn several_pages_publish_once_and_verify_a_reordered_read_back() {
    let mock = Mock::start().await;
    let preview = mock
        .calculate(ResolveScope::Pages(
            ["1500".to_owned(), "1501".to_owned()].into(),
        ))
        .await;
    assert!(preview.report.allows_publication(), "{:?}", preview.report);
    let plan = ok_plan(mock.plan(&mock.request(&preview)).await);
    let shown = plan.preview();
    assert_eq!(shown.changed_pages, vec![0, 1]);
    assert!(shown.contracts.contains(&"update_several_pages"));
    lock(&mock.data).reorder_on_update = true;
    let report = mock.publish(plan).await;
    assert_eq!(report.states.last(), Some(&JournalState::Verified));
    assert!(report.deployment_state_may_advance);
    assert_eq!(mock.updates().len(), 1, "exactly one write");
    let local = mock
        .service
        .local_snapshot(&mock.local_id)
        .unwrap_or_else(|error| panic!("local: {error}"));
    assert_ne!(
        &lock(&mock.data).dashboard,
        local.raw_api_document(),
        "the mock must return another order"
    );
}

/// `widgetid`s of a page in array order.
fn widget_ids(page: &Value) -> Vec<String> {
    page["widgets"]
        .as_array()
        .unwrap_or_else(|| panic!("widgets"))
        .iter()
        .map(|widget| widget["widgetid"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// LAB-02: Remote may return the widgets (and fields) of a page in another
/// order than Base. Replacements are addressed by `widgetid`, so the plan is
/// built, and the payload carries the widgets in the order of the read made
/// right before send.
#[tokio::test]
async fn a_remote_in_another_widget_order_is_planned_and_sent_by_widgetid() {
    let mock = Mock::start().await;
    let preview = mock.calculate(one_page()).await;
    assert_eq!(preview.changes.len(), 2);
    {
        let mut data = lock(&mock.data);
        let widgets = data.dashboard["pages"][0]["widgets"]
            .as_array_mut()
            .unwrap_or_else(|| panic!("widgets"));
        widgets.reverse();
        for widget in widgets {
            widget["fields"]
                .as_array_mut()
                .unwrap_or_else(|| panic!("fields"))
                .reverse();
        }
    }
    let plan = ok_plan(mock.plan(&mock.request(&preview)).await);
    assert!(mock.reads_only(), "planning must not write");
    // Between plan and send the widgets come back in yet another order.
    lock(&mock.data).dashboard["pages"][0]["widgets"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("widgets"))
        .rotate_left(1);
    let read_before_send = widget_ids(&lock(&mock.data).dashboard["pages"][0]);
    assert_eq!(read_before_send, vec!["2501", "2500", "2502"]);
    let report = mock.publish(plan).await;
    assert_eq!(report.states.last(), Some(&JournalState::Verified));
    let updates = mock.updates();
    assert_eq!(updates.len(), 1, "exactly one write");
    assert_eq!(widget_ids(&updates[0]["pages"][0]), read_before_send);
    let sent = &updates[0]["pages"][0]["widgets"];
    assert_eq!(sent[0]["widgetid"], "2501");
    assert!(
        sent[0]["fields"]
            .as_array()
            .unwrap_or_else(|| panic!("fields"))
            .iter()
            .any(|field| field["name"] == "itemid.0" && field["value"] == "202")
    );
}

/// `users` and `userGroups` are sets: Remote listing them in another order
/// than Base is not a change of the source.
#[tokio::test]
async fn a_remote_with_sharing_in_another_order_is_not_a_source_change() {
    let mut shared = dashboard();
    shared["users"] = json!([
        {"userid": "7", "permission": "2"},
        {"userid": "12", "permission": "3"}
    ]);
    shared["userGroups"] = json!([
        {"usrgrpid": "9", "permission": "2"},
        {"usrgrpid": "14", "permission": "2"}
    ]);
    let mock = Mock::start_with(shared).await;
    let preview = mock.calculate(one_page()).await;
    {
        let mut data = lock(&mock.data);
        for list in ["users", "userGroups"] {
            data.dashboard[list]
                .as_array_mut()
                .unwrap_or_else(|| panic!("{list}"))
                .reverse();
        }
    }
    let plan = ok_plan(mock.plan(&mock.request(&preview)).await);
    let report = mock.publish(plan).await;
    assert_eq!(report.states.last(), Some(&JournalState::Verified));
    assert_eq!(mock.updates().len(), 1);
    // Sharing is never sent.
    assert!(mock.updates()[0].get("users").is_none());
}

#[tokio::test]
async fn remote_changed_since_base_gives_no_plan_and_no_write() {
    let mock = Mock::start().await;
    let preview = mock.calculate(one_page()).await;
    lock(&mock.data).dashboard["pages"][2]["name"] = json!("Concurrent edit");
    let result = mock.plan(&mock.request(&preview)).await;
    assert!(matches!(
        result,
        Err(PlanError::Invalidated(PlanInvalidReason::SourceChanged))
    ));
    assert!(mock.updates().is_empty());
}

#[tokio::test]
async fn remote_changed_after_the_plan_is_a_conflict_without_write() {
    let mock = Mock::start().await;
    let preview = mock.calculate(one_page()).await;
    // A plan ID shown for another plan never confirms this one.
    let other = ok_plan(mock.plan(&mock.request(&preview)).await);
    let plan = ok_plan(mock.plan(&mock.request(&preview)).await);
    assert!(matches!(
        plan.confirm(&other.preview().plan_id),
        Err(PlanError::ConfirmationMismatch)
    ));
    let plan = ok_plan(mock.plan(&mock.request(&preview)).await);
    let shown = plan.preview().plan_id;
    let confirmed = plan
        .confirm(&shown)
        .unwrap_or_else(|error| panic!("confirm: {error}"));
    lock(&mock.data).dashboard["pages"][1]["name"] = json!("Concurrent edit");
    let stage = mock
        .session
        .begin_write(confirmed.session())
        .unwrap_or_else(|_| panic!("write stage"));
    let report = publish_replace_host_plan(
        &mock.service,
        mock.writer(),
        &mock.journal,
        confirmed,
        &stage,
    )
    .await;
    assert_eq!(report.states.last(), Some(&JournalState::Conflict));
    assert!(!report.states.contains(&JournalState::Submitting));
    assert_eq!(report.invalidated, Some(PlanInvalidReason::SourceChanged));
    assert!(mock.updates().is_empty());
}

#[tokio::test]
async fn a_draft_change_after_the_plan_blocks_the_send() {
    let mock = Mock::start().await;
    let preview = mock.calculate(one_page()).await;
    let request = mock.request(&preview);
    let plan = ok_plan(mock.plan(&request).await);
    let shown = plan.preview().plan_id;
    let confirmed = plan
        .confirm(&shown)
        .unwrap_or_else(|error| panic!("confirm: {error}"));
    // Recalculating commits a new revision even with the same result.
    let fresh = mock.calculate(one_page()).await;
    assert_ne!(fresh.revision, preview.revision);
    let stage = mock
        .session
        .begin_write(confirmed.session())
        .unwrap_or_else(|_| panic!("write stage"));
    let report = publish_replace_host_plan(
        &mock.service,
        mock.writer(),
        &mock.journal,
        confirmed,
        &stage,
    )
    .await;
    assert_eq!(report.states.last(), Some(&JournalState::FailedBeforeSend));
    assert_eq!(report.invalidated, Some(PlanInvalidReason::DraftChanged));
    // The request shown before the change no longer plans either.
    assert!(matches!(
        mock.plan(&request).await,
        Err(PlanError::Invalidated(PlanInvalidReason::DraftChanged))
    ));
    assert!(mock.updates().is_empty());
}

#[tokio::test]
async fn ended_session_and_new_write_epoch_block_the_send() {
    let mut mock = Mock::start().await;
    let preview = mock.calculate(one_page()).await;

    // A plan admitted under an ended session cannot reach a write stage.
    let plan = ok_plan(mock.plan(&mock.request(&preview)).await);
    let old = plan.session();
    let shown = plan.preview().plan_id;
    let confirmed = plan
        .confirm(&shown)
        .unwrap_or_else(|error| panic!("confirm: {error}"));
    assert_eq!(
        mock.session.end(EndSessionRequest { confirmed: true }),
        Ok(old)
    );
    assert!(mock.session.begin_write(old).is_err());
    let foreign = mock
        .session
        .begin_write(mock.session.current())
        .unwrap_or_else(|_| panic!("write stage"));
    let report = publish_replace_host_plan(
        &mock.service,
        mock.writer(),
        &mock.journal,
        confirmed,
        &foreign,
    )
    .await;
    drop(foreign);
    assert_eq!(report.invalidated, Some(PlanInvalidReason::SessionEnded));
    let mut stale = mock.request(&preview);
    stale.session = old;
    assert!(matches!(
        mock.plan(&stale).await,
        Err(PlanError::Invalidated(PlanInvalidReason::SessionEnded))
    ));

    // Returning to read-only and enabling writes again opens a new epoch.
    let plan = ok_plan(mock.plan(&mock.request(&preview)).await);
    let shown = plan.preview().plan_id;
    let confirmed = plan
        .confirm(&shown)
        .unwrap_or_else(|error| panic!("confirm: {error}"));
    mock.reopen_writes();
    let stage = mock
        .session
        .begin_write(confirmed.session())
        .unwrap_or_else(|_| panic!("write stage"));
    let report = publish_replace_host_plan(
        &mock.service,
        mock.writer(),
        &mock.journal,
        confirmed,
        &stage,
    )
    .await;
    assert_eq!(report.states.last(), Some(&JournalState::FailedBeforeSend));
    assert_eq!(
        report.invalidated,
        Some(PlanInvalidReason::ConnectionChanged)
    );
    assert!(mock.updates().is_empty());
}

#[tokio::test]
async fn a_workcopy_plan_cannot_bypass_the_draft_check_and_needs_write_permission() {
    let mock = Mock::start().await;
    let preview = mock.calculate(one_page()).await;
    let plan = ok_plan(mock.plan(&mock.request(&preview)).await);
    let shown = plan.preview().plan_id;
    let confirmed = plan
        .confirm(&shown)
        .unwrap_or_else(|error| panic!("confirm: {error}"));
    let stage = mock
        .session
        .begin_write(confirmed.session())
        .unwrap_or_else(|_| panic!("write stage"));
    let report = publish_confirmed_plan(mock.writer(), &mock.journal, confirmed, &stage).await;
    drop(stage);
    assert_eq!(report.states.last(), Some(&JournalState::FailedBeforeSend));

    lock(&mock.data).editable = false;
    let Err(PlanError::Incompatible(report)) = mock.plan(&mock.request(&preview)).await else {
        panic!("a dashboard without write permission must give no plan");
    };
    assert!(
        report
            .with_code(FindingCode::DashboardNotEditable)
            .next()
            .is_some()
    );
    assert!(mock.updates().is_empty());
}

#[tokio::test]
async fn revoked_edit_permission_after_preview_sends_no_update() {
    let mock = Mock::start().await;
    let preview = mock.calculate(one_page()).await;
    let plan = ok_plan(mock.plan(&mock.request(&preview)).await);
    lock(&mock.data).editable = false;

    let report = mock.publish(plan).await;

    assert_eq!(report.states.last(), Some(&JournalState::FailedBeforeSend));
    assert!(mock.updates().is_empty());
    assert!(mock.reads_only());
}

#[tokio::test]
async fn another_server_cannot_send_the_confirmed_plan() {
    let source = Mock::start().await;
    let other = Mock::start().await;
    let preview = source.calculate(one_page()).await;
    let plan = ok_plan(source.plan(&source.request(&preview)).await);
    let plan_id = plan.preview().plan_id;
    let confirmed = plan
        .confirm(&plan_id)
        .unwrap_or_else(|error| panic!("confirm: {error}"));
    let stage = source
        .session
        .begin_write(confirmed.session())
        .unwrap_or_else(|_| panic!("write stage"));
    let report = publish_replace_host_plan(
        &source.service,
        other.writer(),
        &source.journal,
        confirmed,
        &stage,
    )
    .await;
    assert_eq!(
        report.invalidated,
        Some(PlanInvalidReason::ConnectionChanged)
    );
    assert!(source.updates().is_empty());
    assert!(other.updates().is_empty());
}

#[tokio::test]
async fn g2_multi_page_update_preserves_sharing_layout_and_confirmed_kept_fields() {
    let mut before = dashboard();
    before["userGroups"] = json!([{"usrgrpid": "8", "permission": "2"}]);
    for page_index in 0..2 {
        let page = before["pages"][page_index]["widgets"]
            .as_array_mut()
            .unwrap_or_else(|| panic!("widgets"));
        for index in 0..4 {
            let id = (2600 + page_index * 10 + index).to_string();
            page.push(widget(
                &id,
                if index == 3 { "svggraph" } else { "item" },
                36 + (index % 3) as u32 * 12,
                if index == 3 {
                    json!([
                        {"type":"0", "name":"ds.0.dataset_type", "value":"0"},
                        {"type":"4", "name":"ds.0.itemids.0", "value":"101"},
                        {"type":"4", "name":"ds.0.itemids.1", "value":"202"},
                        {"type":"1", "name":"ds.0.color.0", "value":"AA0000"},
                        {"type":"1", "name":"ds.0.color.1", "value":"00AA00"}
                    ])
                } else {
                    json!([{"type":"4", "name":"itemid.0", "value":"101"}])
                },
            ));
            page.last_mut().unwrap_or_else(|| panic!("widget"))["y"] =
                json!(if index == 3 { "8" } else { "4" });
        }
    }
    before["pages"][0]["widgets"][2]["fields"][0]["value"] = json!("db-old-*");
    let mock = Mock::start_with(before.clone()).await;
    let preview = mock.calculate(ResolveScope::Dashboard).await;
    assert_eq!(preview.changes.len(), 11);
    assert!(preview.kept_on_source.entries.len() >= 2);
    assert!(mock.plan(&mock.request(&preview)).await.is_err());
    assert!(mock.updates().is_empty());
    let preview = mock
        .service
        .confirm_kept_on_source(
            &mock.local_id,
            &preview.kept_on_source.digest,
            mock.context(),
        )
        .unwrap_or_else(|error| panic!("confirm kept: {error}"));
    let plan = ok_plan(mock.plan(&mock.request(&preview)).await);
    let report = mock.publish(plan).await;
    assert_eq!(report.states.last(), Some(&JournalState::Verified));
    assert_eq!(mock.updates().len(), 1);
    let mut expected = before;
    for page in expected["pages"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("pages"))
    {
        for widget in page["widgets"]
            .as_array_mut()
            .unwrap_or_else(|| panic!("widgets"))
        {
            for field in widget["fields"]
                .as_array_mut()
                .unwrap_or_else(|| panic!("fields"))
            {
                if field["type"] == "4" {
                    match field["value"].as_str() {
                        Some("101") => field["value"] = json!("201"),
                        Some("102") => field["value"] = json!("202"),
                        _ => {}
                    }
                }
            }
        }
    }
    assert_eq!(lock(&mock.data).dashboard, expected);
}

fn journal_code(error: &PlanError) -> Option<&'static str> {
    match error {
        PlanError::Journal(error) => Some(error.code()),
        _ => None,
    }
}

async fn clone_fixture() -> (Mock, ConnectionService, ClonePublicationRequest) {
    use zdashstitch_application::PrepareCloneIntentRequest;
    use zdashstitch_application::clone_mapping::{
        CloneBindingCheck, FrozenTargetSelection, ReferenceMapping, SaveCloneMappingRequest,
        SourceHostDecision, SourceHostMapping, SourceRoleId, TargetMetricResolution, TargetSlot,
        TargetSlotId, materialize_clone,
    };
    use zdashstitch_application::connections::{
        AuthInput, ConnectRequest, MemoryCredentialStore, ProfileInput, TrustInput,
    };
    use zdashstitch_application::resolver::{resolve_replace_host_with, source_model};
    use zdashstitch_zabbix::{HostDetails, HostListing, WidgetCapabilityRegistry};

    let mut raw = dashboard();
    raw["pages"] = json!([{
        "dashboard_pageid": "1500", "name": "Main", "display_period": "0",
        "widgets": [raw["pages"][0]["widgets"][0].clone()]
    }]);
    let mock = Mock::start_with(raw).await;
    let unused_target = HostDetails {
        hostid: "13".to_owned(),
        host: "synthetic-unused".to_owned(),
        name: "Synthetic unused".to_owned(),
    };
    let extra_host = json!(unused_target);
    lock(&mock.data).extra_host = Some(extra_host.clone());
    let mut connections = ConnectionService::new(
        mock._dir.path().join("connections"),
        Box::new(MemoryCredentialStore::default()),
    );
    let profile = connections
        .create_profile(&ProfileInput {
            name: "Synthetic clone".to_owned(),
            address: mock.writer().reader().endpoint().base_url().to_string(),
            auth: AuthInput::ApiToken { store_token: false },
            trust: TrustInput::System,
            confirm_plain_http_token_storage: false,
        })
        .unwrap_or_else(|error| panic!("profile: {error}"));
    let connection_id = &profile.local_id;
    connections
        .connect(
            connection_id,
            ConnectRequest {
                secret: Some(SecretString::from("synthetic-token".to_owned())),
                accept_new_identity: false,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("connection: {error}"));
    let reader = connections.reader().unwrap_or_else(|| panic!("reader"));
    let source_id = mock
        .service
        .import(
            reader,
            connection_id,
            &ImportRequest {
                dashboardid: "950".to_owned(),
                scope: ImportScope::Dashboard,
                name: None,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("source import: {error}"))
        .local_id;
    let replace = mock
        .service
        .calculate_replace_host(
            reader,
            &source_id,
            &ReplaceHostRequest {
                source_hostid: "11".to_owned(),
                target_hostid: "12".to_owned(),
                scope: ResolveScope::Dashboard,
            },
            SessionContext::Connected {
                access_mode: reader.mode(),
                capabilities: reader.capabilities(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("replace: {error}"));
    let base = mock
        .service
        .base_snapshot(&source_id)
        .unwrap_or_else(|error| panic!("base: {error}"));
    let source_local = mock
        .service
        .local_snapshot(&source_id)
        .unwrap_or_else(|error| panic!("source Local: {error}"));
    assert_ne!(source_local.fingerprint(), base.fingerprint());
    let model = source_model(&source_local, &WidgetCapabilityRegistry::v7_4())
        .unwrap_or_else(|error| panic!("model: {error}"));
    let resolved = resolve_replace_host_with(
        reader,
        &model,
        &ReplaceHostRequest {
            source_hostid: "12".to_owned(),
            target_hostid: "11".to_owned(),
            scope: ResolveScope::Dashboard,
        },
    )
    .await
    .unwrap_or_else(|error| panic!("resolve: {error}"));
    let target = resolved.target_host.unwrap_or_else(|| panic!("target"));
    let slot_id = TargetSlotId("pilot".to_owned());
    let references = resolved
        .rows
        .into_iter()
        .map(|row| ReferenceMapping {
            field: row.field,
            widget_type: row.widget_type,
            source: row.source,
            source_evidence: row.source_evidence,
            targets: vec![TargetMetricResolution {
                slot_id: slot_id.clone(),
                target: row.target,
                match_status: row.match_status,
                applicability: row.applicability,
                reasons: row.reasons,
                differences: row.differences,
                evidence: row.target_evidence,
                candidates: row.candidates,
                decision: None,
                check: CloneBindingCheck::Checked,
                check_reason: None,
            }],
        })
        .collect();
    let mappings = vec![
        SourceHostMapping {
            role_id: SourceRoleId("db".to_owned()),
            source_host: resolved.source_host,
            references,
            decision: SourceHostDecision::TargetSlots(vec![TargetSlot {
                slot_id,
                host: target.clone(),
            }]),
        },
        SourceHostMapping {
            role_id: SourceRoleId("unused".to_owned()),
            source_host: target.clone(),
            references: vec![],
            decision: SourceHostDecision::TargetSlots(vec![TargetSlot {
                slot_id: TargetSlotId("unused".to_owned()),
                host: unused_target.clone(),
            }]),
        },
    ];
    let local = materialize_clone(&source_local, &mappings, &[], &[])
        .unwrap_or_else(|error| panic!("Local: {error}"));
    assert_eq!(local.fingerprint(), base.fingerprint());
    let mapping = mock
        .service
        .save_clone_mapping(
            &source_id,
            SaveCloneMappingRequest {
                expected_workcopy_revision: replace.revision,
                connection_id: connection_id.clone(),
                server_identity_fingerprint: reader.identity().fingerprint().to_owned(),
                server_version: "7.4.16".to_owned(),
                local_fingerprint: Some(local.fingerprint().to_owned()),
                mappings,
                pattern_decisions: vec![],
            },
        )
        .unwrap_or_else(|error| panic!("mapping: {error}"));
    let intent = mock
        .service
        .prepare_clone_intent(PrepareCloneIntentRequest {
            source_workcopy_id: source_id,
            target_connection_id: connection_id.clone(),
            target_name: "Separate copy".to_owned(),
            expected_mapping_revision: mapping.state.mapping_revision,
            target_selection: FrozenTargetSelection::new(
                [target, unused_target]
                    .into_iter()
                    .map(|host| HostListing {
                        hostid: host.hostid,
                        host: host.host,
                        name: host.name,
                        groups: vec![],
                        templates: vec![],
                    })
                    .collect(),
            )
            .unwrap_or_else(|error| panic!("selection: {error}")),
        })
        .unwrap_or_else(|error| panic!("intent: {error}"));
    let branch = mock
        .service
        .begin_clone_workcopy(intent)
        .and_then(|draft| mock.service.save_clone_workcopy(draft, &[]))
        .unwrap_or_else(|error| panic!("branch: {error}"));
    let request = ClonePublicationRequest {
        local_id: branch.local_id().to_owned(),
        connection_id: connection_id.clone(),
        mapping_revision: mapping.state.mapping_revision,
        base_fingerprint: base.fingerprint().to_owned(),
        local_fingerprint: local.fingerprint().to_owned(),
        target_name: "Separate copy".to_owned(),
        kept_digest: None,
        session: mock.session.current(),
    };
    (mock, connections, request)
}

#[tokio::test]
async fn clone_preflight_rechecks_source_local_and_remote_base_using_only_reads() {
    let (mock, mut connections, request) = clone_fixture().await;
    let extra_host = lock(&mock.data)
        .extra_host
        .clone()
        .unwrap_or_else(|| panic!("unused host"));
    let preflight = async |connections: &ConnectionService, request: &ClonePublicationRequest| {
        preflight_clone_publication(
            &mock.service,
            &mock.journal,
            &mock.session,
            connections,
            request,
        )
        .await
    };
    let preview = preflight(&connections, &request)
        .await
        .unwrap_or_else(|error| panic!("read-only preflight: {error}"));
    assert_eq!(
        preview.draft.local().fingerprint(),
        request.local_fingerprint
    );
    assert!(!preview.compatibility.allows_publication());
    assert!(
        preview
            .compatibility
            .with_code(FindingCode::WriteModeRequired)
            .next()
            .is_some()
    );
    connections
        .enable_writes(&request.connection_id, true)
        .unwrap_or_else(|error| panic!("enable writes: {error}"));
    let preview = preflight(&connections, &request)
        .await
        .unwrap_or_else(|error| panic!("read-write preflight: {error}"));
    assert!(!preview.compatibility.allows_publication());
    assert!(
        preview
            .compatibility
            .with_code(FindingCode::WriteModeRequired)
            .next()
            .is_none()
    );
    assert!(
        preview.compatibility.checks.iter().any(|check| {
            check.name == "dashboard_create_permission_verified" && check.blocking
        })
    );
    let stale = ClonePublicationRequest {
        mapping_revision: request.mapping_revision + 1,
        ..request.clone()
    };
    assert!(matches!(
        preflight(&connections, &stale).await,
        Err(CreatePlanError::Invalidated(
            PlanInvalidReason::DraftChanged
        ))
    ));
    let renamed = ClonePublicationRequest {
        target_name: "Different copy".to_owned(),
        ..request.clone()
    };
    assert!(matches!(
        preflight(&connections, &renamed).await,
        Err(CreatePlanError::Invalidated(
            PlanInvalidReason::TargetChanged
        ))
    ));
    let stale_acknowledgement = ClonePublicationRequest {
        kept_digest: Some("old digest".to_owned()),
        ..request.clone()
    };
    let preview = preflight(&connections, &stale_acknowledgement)
        .await
        .unwrap_or_else(|error| panic!("stale acknowledgement preflight: {error}"));
    assert!(
        preview
            .compatibility
            .with_code(FindingCode::KeptOnSourceConfirmationStale)
            .next()
            .is_some()
    );
    lock(&mock.data).extra_host = None;
    assert!(matches!(
        preflight(&connections, &request).await,
        Err(CreatePlanError::Workcopy(error)) if error.code() == "clone_target_not_visible"
    ));
    let mut renamed_host = extra_host.clone();
    renamed_host["name"] = json!("Renamed synthetic host");
    lock(&mock.data).extra_host = Some(renamed_host);
    assert!(matches!(
        preflight(&connections, &request).await,
        Err(CreatePlanError::Workcopy(error)) if error.code() == "clone_target_selection_changed"
    ));
    lock(&mock.data).extra_host = Some(extra_host);
    lock(&mock.data).dashboard["name"] = json!("Concurrent change");
    assert!(matches!(
        preflight(&connections, &request).await,
        Err(CreatePlanError::Workcopy(error)) if error.code() == "clone_remote_changed"
    ));
    mock.session
        .end(EndSessionRequest { confirmed: true })
        .unwrap_or_else(|_| panic!("end session"));
    assert!(matches!(
        preflight(&connections, &request).await,
        Err(CreatePlanError::Invalidated(
            PlanInvalidReason::SessionEnded
        ))
    ));
    assert!(mock.reads_only());
    assert!(
        mock.journal
            .list()
            .unwrap_or_else(|error| panic!("journal: {error}"))
            .entries
            .is_empty()
    );
}

#[tokio::test]
async fn clone_create_requires_permission_at_plan_and_send_and_links_the_verified_copy() {
    use zdashstitch_zabbix::{CreateSendRefusal, DashboardCreateOutcome};

    let (mock, mut connections, request) = clone_fixture().await;
    let source_id = mock
        .service
        .open_clone_workcopy(&request.local_id)
        .unwrap_or_else(|error| panic!("branch: {error}"))
        .draft()
        .intent()
        .source()
        .source_workcopy_id
        .clone();
    let original = lock(&mock.data).dashboard.clone();
    connections
        .enable_writes(&request.connection_id, true)
        .unwrap_or_else(|error| panic!("enable writes: {error}"));
    let plan = || {
        plan_clone_dashboard(
            &mock.service,
            &mock.journal,
            &mock.session,
            &connections,
            &request,
        )
    };
    assert!(matches!(
        plan().await,
        Err(CreatePlanError::Incompatible(_))
    ));
    let allowed = json!({
        "api.access": "1", "api.mode": "0", "api": [],
        "actions": [{"name": "edit_dashboards", "status": "1"}]
    });
    let mut denied = allowed.clone();
    denied["api"] = json!(["dashboard.*"]);
    {
        let mut data = lock(&mock.data);
        data.own_role_rules = Some(allowed.clone());
        data.returned_roleid = "8";
    }
    assert!(matches!(
        plan().await,
        Err(CreatePlanError::Incompatible(_))
    ));
    lock(&mock.data).returned_roleid = "7";
    lock(&mock.data).own_role_rules = Some(denied.clone());
    assert!(matches!(
        plan().await,
        Err(CreatePlanError::Incompatible(_))
    ));
    assert!(mock.reads_only());

    let publish = async |confirmed: zdashstitch_application::ConfirmedCreatePlan| {
        let stage = mock
            .session
            .begin_write(confirmed.session())
            .unwrap_or_else(|_| panic!("write stage"));
        publish_clone_dashboard_plan(
            &mock.service,
            &connections,
            &mock.journal,
            confirmed,
            &stage,
        )
        .await
    };
    let confirm = |plan: zdashstitch_application::VerifiedCreatePlan| {
        let preview = plan.preview();
        assert_eq!(preview.operation, "clone_dashboard");
        plan.confirm(&preview.plan_id)
            .unwrap_or_else(|error| panic!("confirm: {error}"))
    };
    lock(&mock.data).own_role_rules = Some(allowed.clone());
    let ready = confirm(plan().await.unwrap_or_else(|error| panic!("plan: {error}")));
    lock(&mock.data).own_role_rules = Some(denied);
    let refused = publish(ready).await;
    assert_eq!(
        refused.outcome,
        Some(DashboardCreateOutcome::FailedBeforeSend(
            CreateSendRefusal::CreatePermissionUnverified,
        ))
    );
    assert_eq!(refused.journal_state, Some(JournalState::FailedBeforeSend));
    assert!(mock.reads_only());
    assert!(
        mock.service
            .draft(&source_id)
            .unwrap_or_else(|error| panic!("source: {error}"))
            .summary
            .created_dashboards
            .is_empty()
    );

    lock(&mock.data).own_role_rules = Some(allowed);
    let ready = confirm(plan().await.unwrap_or_else(|error| panic!("plan: {error}")));
    let result = publish(ready).await;
    assert_eq!(
        result.journal_state,
        Some(JournalState::Verified),
        "{result:?}"
    );
    assert!(result.record_error.is_none(), "{result:?}");
    assert!(
        result
            .clone_source
            .as_ref()
            .and_then(|source| source.as_ref().ok())
            .is_some_and(|source| source.is_current())
    );
    let entry = mock
        .journal
        .entry(&result.plan_id)
        .unwrap_or_else(|error| panic!("entry: {error}"));
    assert_eq!(entry.intent, "clone_dashboard");
    assert_eq!(entry.sends, 1);
    assert_eq!(entry.created_dashboardid.as_deref(), Some("951"));
    let linked = mock
        .service
        .draft(&source_id)
        .unwrap_or_else(|error| panic!("source: {error}"));
    assert_eq!(linked.summary.created_dashboards.len(), 1);
    assert_eq!(
        linked.summary.created_dashboards[0].name,
        request.target_name
    );
    assert_eq!(
        linked.summary.created_dashboards[0].connection_id,
        request.connection_id
    );
    let data = lock(&mock.data);
    assert_eq!(data.dashboard, original);
    assert_eq!(
        data.calls
            .iter()
            .filter(|call| call["method"] == "dashboard.create")
            .count(),
        1
    );
    assert!(
        data.calls
            .iter()
            .all(|call| call["method"] != "dashboard.update")
    );
}

/// P2-04D: an unknown outcome is on disk, blocks every new plan and every
/// send to the dashboard, and only a read reconciles it.
#[tokio::test]
async fn an_unknown_outcome_blocks_the_target_until_a_read_reconciles_it() {
    let mock = Mock::start().await;
    let preview = mock.calculate(one_page()).await;
    let request = mock.request(&preview);
    let plan = ok_plan(mock.plan(&request).await);
    let operation_id = plan.preview().plan_id;
    // Confirmed before the write: must never be sent afterwards.
    let second = ok_plan(mock.plan(&request).await);

    lock(&mock.data).lose_update_answer = true;
    let report = mock.publish(plan).await;
    assert_eq!(report.states.last(), Some(&JournalState::UnknownOutcome));
    assert_eq!(report.journal_error, None);
    assert_eq!(mock.updates().len(), 1);

    let recovery = mock
        .journal
        .recover()
        .unwrap_or_else(|error| panic!("recover: {error}"));
    assert!(recovery.damaged.is_empty());
    let [entry] = recovery.unfinished.as_slice() else {
        panic!("one unfinished entry: {recovery:?}");
    };
    assert_eq!(entry.operation_id, operation_id);
    assert_eq!(entry.state, JournalState::UnknownOutcome);
    assert_eq!(entry.progress, JournalProgress::NeedsReconciliation);
    assert_eq!(entry.sends, 1);
    assert!(entry.submitted_at_ms().is_some());
    assert_eq!(
        entry.source.as_ref().map(|source| source.local_id.as_str()),
        Some(mock.local_id.as_str())
    );
    let states: Vec<_> = entry.transitions.iter().map(|step| step.state).collect();
    assert_eq!(
        states,
        [
            JournalState::Confirmed,
            JournalState::Submitting,
            JournalState::UnknownOutcome
        ]
    );
    let opened = mock
        .journal
        .recover_for_workcopy(&mock.local_id)
        .unwrap_or_else(|error| panic!("recover for workcopy: {error}"));
    assert_eq!(opened.unfinished.len(), 1);

    lock(&mock.data).unreachable = false;
    let blocked = mock.plan(&request).await.err();
    assert_eq!(
        blocked.as_ref().and_then(journal_code),
        Some("target_blocked")
    );
    let report = mock.publish(second).await;
    assert_eq!(report.states.last(), Some(&JournalState::FailedBeforeSend));
    assert_eq!(
        report.journal_error.as_ref().map(|error| error.code()),
        Some("target_blocked")
    );
    assert_eq!(
        report
            .journal_error
            .as_ref()
            .and_then(|error| error.param("operation_id")),
        Some(operation_id.as_str())
    );
    assert_eq!(
        mock.updates().len(),
        1,
        "an unknown outcome is never resent"
    );

    let reconciled = reconcile_update(&mock.journal, mock.writer().reader(), &operation_id)
        .await
        .unwrap_or_else(|error| panic!("reconcile: {error}"));
    assert_eq!(reconciled.state, JournalState::ReconciledVerified);
    assert_eq!(reconciled.evidence.as_deref(), Some("expected_found"));
    assert_eq!(
        reconciled.observed_fingerprint.as_deref(),
        Some(preview.local_fingerprint.as_str())
    );
    assert_eq!(reconciled.progress, JournalProgress::Finished);
    assert_eq!(mock.updates().len(), 1, "reconciliation only reads");
    assert!(
        mock.journal
            .recover()
            .unwrap_or_else(|error| panic!("recover: {error}"))
            .is_empty()
    );
    let again = reconcile_update(&mock.journal, mock.writer().reader(), &operation_id).await;
    assert_eq!(
        again.err().map(|error| error.code()),
        Some("reconcile_not_needed")
    );
}

/// P2-04D: a process that died between the `submitting` record and the
/// answer leaves an entry that a restarted process finds and only
/// reconciles; afterwards a fresh plan publishes normally.
#[tokio::test]
async fn a_crash_after_submitting_is_found_on_restart_and_reconciled_by_read() {
    let mock = Mock::start().await;
    let preview = mock.calculate(one_page()).await;
    let identity = mock.writer().reader().identity();
    let operation = json!({
        "kind": "update", "intent": "replace_host",
        "connection_id": CONNECTION, "server_version": "7.4.16",
        "endpoint_fingerprint": identity.endpoint_fingerprint(),
        "server_identity": identity.fingerprint(),
        "target": {"kind": "existing_dashboard", "dashboardid": "950"},
        "source": {"local_id": mock.local_id, "revision": preview.revision},
        "source_fingerprint": preview.base_fingerprint,
        "expected_fingerprint": preview.local_fingerprint,
        "plan_created_at_ms": 1, "confirmed_at_ms": 2
    });
    let id = "zds-crashed-0001";
    let line = |seq: u32, state: &str, operation: Option<&Value>| {
        let mut line = json!({
            "format": "zdashstitch.journal/v0", "storage_scope": "local_only",
            "operation_id": id, "seq": seq, "state": state, "at_ms": 3 + u64::from(seq)
        });
        if let Some(operation) = operation {
            line["operation"] = operation.clone();
        }
        format!("{line}\n")
    };
    let journal_dir = mock._dir.path().join("journal");
    std::fs::create_dir_all(&journal_dir).unwrap_or_else(|error| panic!("journal dir: {error}"));
    std::fs::write(
        journal_dir.join(format!("{id}.jsonl")),
        line(0, "confirmed", Some(&operation)) + &line(1, "submitting", None),
    )
    .unwrap_or_else(|error| panic!("journal file: {error}"));

    // A new process: nothing of the old one is running.
    let restarted = PublishJournal::new(mock._dir.path().to_path_buf());
    let recovery = restarted
        .recover()
        .unwrap_or_else(|error| panic!("recover: {error}"));
    let [entry] = recovery.unfinished.as_slice() else {
        panic!("one unfinished entry: {recovery:?}");
    };
    assert_eq!(entry.state, JournalState::Submitting);
    assert_eq!(entry.progress, JournalProgress::NeedsReconciliation);
    assert_eq!(entry.submitted_at_ms(), Some(4));
    let blocked = mock.plan(&mock.request(&preview)).await.err();
    assert_eq!(
        blocked.as_ref().and_then(journal_code),
        Some("target_blocked")
    );

    let reconciled = reconcile_update(&restarted, mock.writer().reader(), id)
        .await
        .unwrap_or_else(|error| panic!("reconcile: {error}"));
    assert_eq!(reconciled.state, JournalState::ReconciledFailed);
    assert_eq!(reconciled.evidence.as_deref(), Some("source_found"));
    assert!(mock.updates().is_empty(), "reconciliation only reads");

    let plan = ok_plan(mock.plan(&mock.request(&preview)).await);
    let report = mock.publish(plan).await;
    assert_eq!(report.states.last(), Some(&JournalState::Verified));
    assert_eq!(report.journal_error, None);
    assert_eq!(mock.updates().len(), 1);
    let entry = mock
        .journal
        .entry(&report.plan_id)
        .unwrap_or_else(|error| panic!("entry: {error}"));
    assert_eq!(entry.state, JournalState::Verified);
    assert_eq!(entry.progress, JournalProgress::Finished);
}

/// P1-01G §7: «Закрыть без сверки» needs the confirmation, unblocks the
/// target, keeps the entry and claims no result; the next publication
/// starts with a fresh read, which sees what the unknown write did.
#[tokio::test]
async fn closing_without_checking_unblocks_the_target_and_claims_nothing() {
    let mock = Mock::start().await;
    let preview = mock.calculate(one_page()).await;
    let plan = ok_plan(mock.plan(&mock.request(&preview)).await);
    let operation_id = plan.preview().plan_id;
    lock(&mock.data).lose_update_answer = true;
    let report = mock.publish(plan).await;
    assert_eq!(report.states.last(), Some(&JournalState::UnknownOutcome));

    // The server stays unreachable: the read fails and records nothing.
    let failed = reconcile_update(&mock.journal, mock.writer().reader(), &operation_id).await;
    assert!(failed.is_err());
    let entry = mock
        .journal
        .entry(&operation_id)
        .unwrap_or_else(|error| panic!("entry: {error}"));
    assert_eq!(entry.state, JournalState::UnknownOutcome);

    let unconfirmed = mock.journal.close_manually(&CloseManuallyRequest {
        operation_id: operation_id.clone(),
        confirmed: false,
    });
    assert_eq!(
        unconfirmed.err().map(|error| error.code()),
        Some("close_not_confirmed")
    );
    let closed = mock
        .journal
        .close_manually(&CloseManuallyRequest {
            operation_id: operation_id.clone(),
            confirmed: true,
        })
        .unwrap_or_else(|error| panic!("close: {error}"));
    assert_eq!(closed.state, JournalState::ClosedManually);
    assert_eq!(closed.progress, JournalProgress::Finished);
    assert_eq!(closed.sends, 1);
    assert_eq!(closed.evidence, None, "no result is claimed");
    assert_eq!(closed.observed_fingerprint, None);
    assert_eq!(
        closed.transitions.len(),
        4,
        "the entry stays in the journal"
    );
    assert!(
        mock.journal
            .recover()
            .unwrap_or_else(|error| panic!("recover: {error}"))
            .is_empty()
    );
    let twice = mock.journal.close_manually(&CloseManuallyRequest {
        operation_id: operation_id.clone(),
        confirmed: true,
    });
    assert_eq!(
        twice.err().map(|error| error.code()),
        Some("reconcile_not_needed")
    );

    // Unblocked, but the fresh read before planning finds Remote != Base.
    lock(&mock.data).unreachable = false;
    assert!(matches!(
        mock.plan(&mock.request(&preview)).await,
        Err(PlanError::Invalidated(PlanInvalidReason::SourceChanged))
    ));
    assert_eq!(mock.updates().len(), 1);
}

/// P2-04C: Remote is re-read on open and on command (`check_source`) and
/// before preview (`recheck_replace_host`); `Remote != Base` gives the
/// typed reason and, at plan time, zero updates.
#[tokio::test]
async fn remote_is_re_read_on_open_on_command_and_before_preview() {
    let mock = Mock::start().await;
    let reader = mock.writer().reader();
    let current = mock
        .service
        .check_source(reader, CONNECTION, &mock.local_id)
        .await
        .unwrap_or_else(|error| panic!("check: {error}"));
    assert_eq!(current.freshness.state, DraftFreshnessState::Current);
    assert!(current.freshness.checked_at_ms.is_some());
    assert_eq!(current.invalidated, None);
    assert_eq!(
        current.remote_fingerprint.as_deref(),
        Some(current.base_fingerprint.as_str())
    );
    let other = mock
        .service
        .check_source(
            reader,
            "conn-ffffffffffffffffffffffffffffffff",
            &mock.local_id,
        )
        .await;
    assert_eq!(
        other.err().map(|error| error.code()),
        Some("workcopy_other_connection")
    );

    let preview = mock.calculate(one_page()).await;
    let recheck = recheck_replace_host(
        &mock.service,
        reader,
        CONNECTION,
        &mock.local_id,
        mock.context(),
    )
    .await
    .unwrap_or_else(|error| panic!("recheck: {error}"));
    assert!(recheck.allows_plan(), "{:?}", recheck.preview.report);

    lock(&mock.data).dashboard["pages"][2]["name"] = json!("Concurrent edit");
    let conflict = recheck_replace_host(
        &mock.service,
        reader,
        CONNECTION,
        &mock.local_id,
        mock.context(),
    )
    .await
    .unwrap_or_else(|error| panic!("recheck: {error}"));
    assert_eq!(
        conflict.source.freshness.state,
        DraftFreshnessState::Conflict
    );
    assert_eq!(
        conflict.source.invalidated,
        Some(PlanInvalidReason::SourceChanged)
    );
    assert!(!conflict.allows_plan());
    assert!(matches!(
        mock.plan(&mock.request(&preview)).await,
        Err(PlanError::Invalidated(PlanInvalidReason::SourceChanged))
    ));

    lock(&mock.data).hidden = true;
    let missing = mock
        .service
        .check_source(reader, CONNECTION, &mock.local_id)
        .await
        .unwrap_or_else(|error| panic!("check: {error}"));
    assert_eq!(missing.freshness.state, DraftFreshnessState::SourceMissing);
    assert_eq!(missing.invalidated, Some(PlanInvalidReason::SourceChanged));
    assert_eq!(missing.remote_fingerprint, None);
    assert!(mock.reads_only(), "re-reading never writes");
}
