// SPDX-License-Identifier: MIT OR Apache-2.0

//! The real application planner, publisher and journal over a fault-injecting
//! loopback observer. Every accepted write goes to the opted-in 8.0.0 fixture.

use std::{
    env,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use reqwest::{Client, Url, redirect::Policy};
use secrecy::SecretString;
use serde_json::{Value, json};
use zdashstitch_application::{
    ConfirmedPlan, JournalState, LocalSession, PublishJournal, PublishReport, build_verified_plan,
    publish_confirmed_plan, reconcile_update,
};
use zdashstitch_zabbix::{
    Authentication, PublicationContext, ReadWriteGateway, VersionedDashboardSnapshot,
    WidgetFieldReplacement, ZabbixClient, ZabbixEndpoint, read_only_allowlist,
};

const CONNECTION: &str = "z80-verified-publish-lab";
const FIXTURE_NAME: &str = "ZDashstitch Z80 Verified App";

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum Fault {
    #[default]
    None,
    VersionChanged,
    RoleChanged,
    NoEditableTarget,
    SourceChanged,
    LostWriteAndReads,
}

#[derive(Default)]
struct Observed {
    fault: Fault,
    completed_write: bool,
    writes: usize,
    blocked: usize,
}

#[derive(Clone)]
struct Observer {
    endpoint: Url,
    client: Client,
    dashboardid: String,
    itemids: [String; 2],
    observed: Arc<Mutex<Observed>>,
}

async fn observe(
    State(observer): State<Observer>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let method = body["method"].as_str().unwrap_or_default();
    let params = &body["params"];
    let fault = {
        let mut observed = observer
            .observed
            .lock()
            .unwrap_or_else(|_| panic!("observer lock"));
        let permitted = match method {
            "dashboard.update" => is_scoped_update(params, &observer),
            "dashboard.get" => params["dashboardids"] == json!([observer.dashboardid]),
            "item.get" => params["itemids"].as_array().is_some_and(|ids| {
                ids.len() == 2
                    && observer
                        .itemids
                        .iter()
                        .all(|id| ids.iter().any(|entry| entry == id))
            }),
            method => read_only_allowlist().contains(&method),
        };
        if !permitted {
            observed.blocked += 1;
            return StatusCode::FORBIDDEN.into_response();
        }
        if observed.fault == Fault::LostWriteAndReads
            && observed.completed_write
            && method == "dashboard.get"
        {
            return StatusCode::BAD_GATEWAY.into_response();
        }
        if method == "dashboard.update" {
            observed.writes += 1;
        }
        observed.fault
    };
    let mut request = observer.client.post(observer.endpoint.clone()).json(&body);
    if let Some(token) = headers.get(header::AUTHORIZATION) {
        request = request.header(header::AUTHORIZATION, token);
    }
    let Ok(upstream) = request.send().await else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    let Ok(mut envelope) = upstream.json::<Value>().await else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    if method == "dashboard.update" {
        let mut observed = observer
            .observed
            .lock()
            .unwrap_or_else(|_| panic!("observer lock"));
        observed.completed_write =
            envelope["result"]["dashboardids"] == json!([observer.dashboardid]);
        if fault == Fault::LostWriteAndReads {
            return StatusCode::BAD_GATEWAY.into_response();
        }
    }
    match (fault, method) {
        (Fault::VersionChanged, "apiinfo.version") => envelope["result"] = json!("8.0.1"),
        (Fault::RoleChanged, "role.get") => {
            envelope["result"][0]["rules"]["api"] = json!(["dashboard.update"])
        }
        (Fault::NoEditableTarget, "dashboard.get") if params["editable"] == true => {
            envelope["result"] = json!([])
        }
        (Fault::SourceChanged, "dashboard.get") if params["editable"] != true => {
            envelope["result"][0]["name"] = json!("ZDashstitch Z80 Verified App - changed source");
        }
        _ => {}
    }
    Json(envelope).into_response()
}

fn is_scoped_update(params: &Value, observer: &Observer) -> bool {
    let Some(pages) = params["pages"].as_array() else {
        return false;
    };
    let [changed, untouched] = pages.as_slice() else {
        return false;
    };
    let Some(widgets) = changed["widgets"].as_array() else {
        return false;
    };
    let [item] = widgets.as_slice() else {
        return false;
    };
    let Some(fields) = item["fields"].as_array() else {
        return false;
    };
    let [field] = fields.as_slice() else {
        return false;
    };
    params.as_object().is_some_and(|params| params.len() == 2)
        && params["dashboardid"] == observer.dashboardid
        && changed.as_object().is_some_and(|page| page.len() == 4)
        && untouched
            .as_object()
            .is_some_and(|page| page.len() == 1 && page.contains_key("dashboard_pageid"))
        && item.as_object().is_some_and(|widget| widget.len() == 9)
        && item["type"] == "item"
        && field.as_object().is_some_and(|field| field.len() == 3)
        && field["name"] == "itemid.0"
        && (field["type"] == "4" || field["type"] == 4)
        && observer.itemids.iter().any(|id| field["value"] == *id)
}

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("missing {name}"))
}

async fn read(gateway: &ReadWriteGateway, id: &str) -> VersionedDashboardSnapshot {
    gateway
        .reader()
        .dashboard_snapshot(CONNECTION, id)
        .await
        .unwrap_or_else(|_| panic!("fixture read failed"))
        .unwrap_or_else(|| panic!("fixture must be visible"))
}

async fn scope(
    gateway: &ReadWriteGateway,
    source: &VersionedDashboardSnapshot,
    replacement: &WidgetFieldReplacement,
) -> PublicationContext {
    gateway
        .verified_item_update_context(source, replacement)
        .await
        .unwrap_or_else(|_| panic!("scoped evidence read failed"))
        .unwrap_or_else(|| panic!("scoped evidence must be available"))
}

fn confirmed(
    journal: &PublishJournal,
    local: &LocalSession,
    source: &VersionedDashboardSnapshot,
    context: &PublicationContext,
    replacement: &WidgetFieldReplacement,
) -> (String, ConfirmedPlan) {
    let plan = build_verified_plan(
        journal,
        source,
        context,
        std::slice::from_ref(replacement),
        local.current(),
    )
    .unwrap_or_else(|_| panic!("trusted planner refused fixture"));
    let id = plan.preview().plan_id;
    let plan = plan
        .confirm(&id)
        .unwrap_or_else(|_| panic!("explicit confirmation failed"));
    (id, plan)
}

async fn publish(
    gateway: &ReadWriteGateway,
    journal: &PublishJournal,
    local: &LocalSession,
    plan: ConfirmedPlan,
) -> PublishReport {
    let stage = local
        .begin_write(plan.session())
        .unwrap_or_else(|_| panic!("local write stage"));
    let report = publish_confirmed_plan(gateway, journal, plan, &stage).await;
    assert!(
        report.journal_error.is_none(),
        "publish journal must persist"
    );
    report
}

#[tokio::test]
#[ignore = "requires scoped fixtures and an authorized 8.0.0 lab through a loopback tunnel"]
async fn lab_z80_verified_plan_and_persistent_journal() {
    assert_eq!(
        required("ZDASHSTITCH_LAB_Z80_VERIFIED_ACK"),
        "verified-plan-authorized-8.0.0"
    );
    let endpoint =
        Url::parse(&required("ZDASHSTITCH_LAB_API_URL")).unwrap_or_else(|_| panic!("lab URL"));
    assert!(endpoint.host_str().is_some_and(|host| {
        host.parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.is_loopback())
    }));
    assert!(endpoint.username().is_empty() && endpoint.password().is_none());
    assert_eq!(endpoint.path().rsplit('/').next(), Some("api_jsonrpc.php"));
    let client = Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap_or_else(|_| panic!("observer client"));
    let dashboardid = required("ZDASHSTITCH_LAB_Z80_VERIFIED_DASHBOARDID");
    let before_item = required("ZDASHSTITCH_LAB_Z80_VERIFIED_SOURCE_ITEMID");
    let after_item = required("ZDASHSTITCH_LAB_Z80_VERIFIED_TARGET_ITEMID");
    let observed = Arc::new(Mutex::new(Observed::default()));
    let router = Router::new()
        .route("/api_jsonrpc.php", post(observe))
        .with_state(Observer {
            endpoint,
            client,
            dashboardid: dashboardid.clone(),
            itemids: [before_item.clone(), after_item.clone()],
            observed: observed.clone(),
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|_| panic!("observer listener"));
    let base = format!(
        "http://{}",
        listener
            .local_addr()
            .unwrap_or_else(|_| panic!("observer address"))
    );
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let gateway = ZabbixClient::new(
        ZabbixEndpoint::parse(&base).unwrap_or_else(|_| panic!("observer endpoint")),
    )
    .unwrap_or_else(|_| panic!("product client"))
    .authenticate(Authentication::UsernamePassword {
        username: required("ZDASHSTITCH_LAB_Z80_VERIFIED_USERNAME"),
        password: SecretString::from(required("ZDASHSTITCH_LAB_Z80_VERIFIED_PASSWORD")),
    })
    .await
    .unwrap_or_else(|_| panic!("product authentication failed"))
    .enable_writes();
    assert_eq!(gateway.capabilities().version.to_string(), "8.0.0");
    let source = read(&gateway, &dashboardid).await;
    let raw = source.raw_api_document();
    assert!(
        raw["name"]
            .as_str()
            .is_some_and(|name| name.starts_with(FIXTURE_NAME))
    );
    let replacement = WidgetFieldReplacement {
        page_index: 0,
        widgetid: raw["pages"][0]["widgets"][0]["widgetid"]
            .as_str()
            .unwrap_or_else(|| panic!("fixture widget"))
            .to_owned(),
        field_name: "itemid.0".to_owned(),
        expected_value: before_item,
        replacement_value: after_item,
    };
    let local = LocalSession::new();
    let storage = tempfile::tempdir().unwrap_or_else(|_| panic!("journal temp dir"));
    let journal = PublishJournal::new(storage.path().to_path_buf());
    let context = scope(&gateway, &source, &replacement).await;
    assert!(
        build_verified_plan(
            &journal,
            &source,
            &gateway.publication_context(),
            std::slice::from_ref(&replacement),
            local.current()
        )
        .is_err()
    );
    let mut foreign_field = replacement.clone();
    foreign_field.field_name = "reference".to_owned();
    assert!(
        build_verified_plan(
            &journal,
            &source,
            &context,
            &[foreign_field],
            local.current()
        )
        .is_err()
    );
    let mut other_dashboard = raw.clone();
    let other_id = if dashboardid == "9000000000" {
        "9000000001"
    } else {
        "9000000000"
    };
    other_dashboard["dashboardid"] = json!(other_id);
    let other_dashboard = VersionedDashboardSnapshot::from_adapter_raw(
        CONNECTION.to_owned(),
        source.server_version().clone(),
        other_dashboard,
    )
    .unwrap_or_else(|_| panic!("synthetic wrong dashboard"));
    assert!(
        build_verified_plan(
            &journal,
            &other_dashboard,
            &context,
            std::slice::from_ref(&replacement),
            local.current()
        )
        .is_err()
    );

    for (fault, state) in [
        (Fault::VersionChanged, JournalState::FailedBeforeSend),
        (Fault::RoleChanged, JournalState::FailedBeforeSend),
        (Fault::NoEditableTarget, JournalState::FailedBeforeSend),
        (Fault::SourceChanged, JournalState::Conflict),
    ] {
        let (id, plan) = confirmed(&journal, &local, &source, &context, &replacement);
        observed
            .lock()
            .unwrap_or_else(|_| panic!("observer lock"))
            .fault = fault;
        let report = publish(&gateway, &journal, &local, plan).await;
        let entry = journal
            .entry(&id)
            .unwrap_or_else(|_| panic!("negative journal entry"));
        assert_eq!(report.states.last(), Some(&state));
        assert_eq!(entry.state, state);
        assert_eq!(entry.sends, 0);
        assert_eq!(
            observed
                .lock()
                .unwrap_or_else(|_| panic!("observer lock"))
                .writes,
            0
        );
    }
    observed
        .lock()
        .unwrap_or_else(|_| panic!("observer lock"))
        .fault = Fault::None;
    let (id, plan) = confirmed(&journal, &local, &source, &context, &replacement);
    let report = publish(&gateway, &journal, &local, plan).await;
    assert_eq!(report.states.last(), Some(&JournalState::Verified));
    assert!(report.deployment_state_may_advance);
    let entry = journal
        .entry(&id)
        .unwrap_or_else(|_| panic!("positive journal entry"));
    assert_eq!(entry.sends, 1);
    let current = read(&gateway, &dashboardid).await;
    assert_eq!(current.fingerprint(), entry.expected_fingerprint);

    let reverse = WidgetFieldReplacement {
        expected_value: replacement.replacement_value,
        replacement_value: replacement.expected_value,
        ..replacement
    };
    let context = scope(&gateway, &current, &reverse).await;
    let (id, plan) = confirmed(&journal, &local, &current, &context, &reverse);
    {
        let mut observed = observed.lock().unwrap_or_else(|_| panic!("observer lock"));
        observed.fault = Fault::LostWriteAndReads;
        observed.completed_write = false;
    }
    let report = publish(&gateway, &journal, &local, plan).await;
    assert_eq!(report.states.last(), Some(&JournalState::UnknownOutcome));
    assert!(!report.deployment_state_may_advance);
    let reopened = PublishJournal::new(storage.path().to_path_buf());
    let pending = reopened
        .entry(&id)
        .unwrap_or_else(|_| panic!("persisted unknown outcome"));
    assert_eq!(pending.state, JournalState::UnknownOutcome);
    assert_eq!(pending.sends, 1);
    assert!(
        build_verified_plan(&reopened, &current, &context, &[reverse], local.current()).is_err()
    );
    observed
        .lock()
        .unwrap_or_else(|_| panic!("observer lock"))
        .fault = Fault::VersionChanged;
    assert!(
        reconcile_update(&reopened, gateway.reader(), &id)
            .await
            .is_err()
    );
    let still_pending = reopened
        .entry(&id)
        .unwrap_or_else(|_| panic!("unknown outcome after version mismatch"));
    assert_eq!(still_pending.state, JournalState::UnknownOutcome);
    assert_eq!(still_pending.sends, 1);
    observed
        .lock()
        .unwrap_or_else(|_| panic!("observer lock"))
        .fault = Fault::None;
    let recovered = reconcile_update(&reopened, gateway.reader(), &id)
        .await
        .unwrap_or_else(|_| panic!("read-only reconciliation failed"));
    assert_eq!(recovered.state, JournalState::ReconciledVerified);
    assert_eq!(recovered.sends, 1);
    assert_eq!(
        read(&gateway, &dashboardid).await.fingerprint(),
        source.fingerprint()
    );
    gateway
        .logout()
        .await
        .unwrap_or_else(|_| panic!("lab logout failed"));
    let observed = observed.lock().unwrap_or_else(|_| panic!("observer lock"));
    assert_eq!(observed.writes, 2);
    assert_eq!(observed.blocked, 0);
    println!(
        "8.0.0 VerifiedPlan: persisted verified and unknown outcomes, read-only reconcile with fresh version check, two single sends, four stale checks with zero sends"
    );
    server.abort();
}
