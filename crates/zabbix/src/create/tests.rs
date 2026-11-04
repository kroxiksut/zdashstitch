// SPDX-License-Identifier: MIT OR Apache-2.0

//! Payload, gates, read-back and reconciliation of the closed create path
//! (P2-06B/C/D) on synthetic dashboards and a loopback mock transport. No
//! laboratory or production server is used.

use std::sync::{Arc, Mutex, PoisonError};

use axum::response::{IntoResponse, Response};
use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use secrecy::SecretString;
use serde_json::{Value, json};

use super::*;
use crate::{
    Authentication, CapabilityState, FailureCode, ZabbixCapabilities, ZabbixClient, ZabbixEndpoint,
};

const OWNER: &str = "5";
const TARGET: &str = "Copy of contract";

fn version() -> ZabbixVersion {
    "7.4.16"
        .parse()
        .unwrap_or_else(|error| panic!("valid version: {error}"))
}

fn snapshot(raw: Value) -> VersionedDashboardSnapshot {
    VersionedDashboardSnapshot::from_adapter_raw("mock-connection".to_owned(), version(), raw)
        .unwrap_or_else(|error| panic!("valid snapshot: {error}"))
}

fn capabilities() -> ZabbixCapabilities {
    AdapterRegistry::resolve(version()).unwrap_or_else(|error| panic!("adapter: {error}"))
}

fn proven(unknown: bool) -> DashboardCreateContracts {
    let supported = CapabilityState::Supported;
    DashboardCreateContracts {
        create_private_dashboard: supported,
        copy_read_widgets: supported,
        copy_unknown_widgets: if unknown {
            supported
        } else {
            CapabilityState::Unsupported
        },
        reconcile_create_outcome: supported,
    }
}

fn context(contracts: Option<DashboardCreateContracts>) -> PublicationContext {
    let mut capabilities = capabilities();
    if let Some(contracts) = contracts {
        capabilities.dashboard_create_contracts = contracts;
    }
    PublicationContext::detached_for_testing(capabilities)
}

fn widget(widgetid: &str, widget_type: &str, x: u32, fields: Value) -> Value {
    json!({
        "widgetid": widgetid, "type": widget_type, "name": format!("w{widgetid}"),
        "x": x.to_string(), "y": "0", "width": "12", "height": "4", "view_mode": "0",
        "fields": fields
    })
}

/// Two pages: an `item` that provides a link and an `svggraph` that
/// consumes it, then a `problems` page. Shared with a user.
fn source(extra: Option<Value>) -> Value {
    let mut second = vec![widget(
        "33",
        "problems",
        0,
        json!([{"type": "3", "name": "hostids.0", "value": "11"}]),
    )];
    if let Some(extra) = extra {
        second.push(extra);
    }
    json!({
        "dashboardid": "7", "name": "contract", "userid": "1", "private": "0",
        "display_period": "60", "auto_start": "0",
        "pages": [
            {"dashboard_pageid": "11", "name": "first", "display_period": "0", "widgets": [
                widget("13", "item", 0, json!([
                    {"type": "4", "name": "itemid.0", "value": "101"},
                    {"type": "1", "name": "reference", "value": "ABCDE"}
                ])),
                widget("14", "svggraph", 12, json!([
                    {"type": "1", "name": "ds.0.hosts.0", "value": "db-*"},
                    {"type": 1, "name": "ds.0.override_hostid._reference", "value": "ABCDE._hostid"},
                    {"type": "0", "name": "legend", "value": 1}
                ]))
            ]},
            {"dashboard_pageid": "21", "name": "", "display_period": "30", "widgets": second}
        ],
        "users": [{"userid": "9", "permission": "2"}],
        "userGroups": [{"usrgrpid": "4", "permission": "3"}]
    })
}

fn verified(
    result: std::result::Result<VerifiedDashboardCreate, CreateRejection>,
) -> VerifiedDashboardCreate {
    result
        .unwrap_or_else(|rejection| panic!("verified create: {rejection} {:?}", rejection.params()))
}

fn rejected(
    result: std::result::Result<VerifiedDashboardCreate, CreateRejection>,
) -> CreateRejection {
    match result {
        Ok(_) => panic!("the create must be rejected"),
        Err(rejection) => rejection,
    }
}

/// What the server stores for `params`: new IDs, owner, strings.
fn stored(params: &Value, dashboardid: &str, owner: &str) -> Value {
    let mut next = 100;
    let mut document = params.clone();
    document["dashboardid"] = json!(dashboardid);
    document["userid"] = json!(owner);
    document["private"] = json!(params["private"].to_string());
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

fn expectation(create: &VerifiedDashboardCreate) -> CreateExpectation {
    CreateExpectation {
        connection_id: create.connection_id().to_owned(),
        server_version: create.server_version().clone(),
        name: create.name().to_owned(),
        content_fingerprint: create.content_fingerprint().to_owned(),
        owner_userid: OWNER.to_owned(),
    }
}

// ---------- payload (P2-06B) ----------

#[test]
fn payload_copies_every_page_and_field_without_server_ids() {
    let local = snapshot(source(None));
    let create = verified(verify_dashboard_create(
        &local,
        &context(Some(proven(false))),
        TARGET,
    ));
    let params = &create.create_params;
    assert_eq!(params["name"], json!(TARGET));
    assert_eq!(params["display_period"], json!("60"));
    assert_eq!(params["auto_start"], json!("0"));
    assert_eq!(params["private"], json!(1));
    assert_eq!(params["users"], json!([]));
    assert_eq!(params["userGroups"], json!([]));
    for key in ["dashboardid", "userid", "uuid"] {
        assert!(params.get(key).is_none(), "{key} must not be sent");
    }
    let read_pages = local.raw_api_document()["pages"]
        .as_array()
        .unwrap_or_else(|| panic!("read pages"));
    let sent_pages = params["pages"]
        .as_array()
        .unwrap_or_else(|| panic!("sent pages"));
    assert_eq!(sent_pages.len(), read_pages.len());
    for (sent, read) in sent_pages.iter().zip(read_pages) {
        assert!(sent.get("dashboard_pageid").is_none());
        assert_eq!(sent["name"], read["name"]);
        assert_eq!(sent["display_period"], read["display_period"]);
        let sent_widgets = sent["widgets"]
            .as_array()
            .unwrap_or_else(|| panic!("sent widgets"));
        let read_widgets = read["widgets"]
            .as_array()
            .unwrap_or_else(|| panic!("read widgets"));
        assert_eq!(sent_widgets.len(), read_widgets.len());
        for (sent, read) in sent_widgets.iter().zip(read_widgets) {
            assert!(sent.get("widgetid").is_none());
            let mut expected = read.clone();
            if let Some(object) = expected.as_object_mut() {
                object.remove("widgetid");
            }
            // Fields, geometry and links exactly as read, JSON kinds kept.
            assert_eq!(sent, &expected);
        }
    }
    assert_eq!(
        params["pages"][0]["widgets"][1]["fields"][1]["value"],
        json!("ABCDE._hostid")
    );
    let requirements = create.requirements();
    assert_eq!(requirements.page_count, 2);
    assert_eq!(requirements.widget_count, 3);
    assert_eq!(requirements.reference_definitions, 1);
    assert_eq!(requirements.reference_consumers, 1);
    assert!(requirements.unknown_widgets.is_empty());
    assert_eq!(
        create
            .required_contracts()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        CreateContract::COPY.to_vec()
    );
    assert_eq!(create.local_fingerprint(), local.fingerprint());
}

#[test]
fn only_the_lab_proven_create_contracts_are_supported() {
    // LAB-02 (7.4.16): private create, copy of read widgets and the
    // reconciliation of an unknown outcome are proven; unknown widgets are
    // not.
    let contracts = capabilities().dashboard_create_contracts;
    assert_eq!(
        contracts,
        DashboardCreateContracts {
            create_private_dashboard: CapabilityState::Supported,
            copy_read_widgets: CapabilityState::Supported,
            copy_unknown_widgets: CapabilityState::Unsupported,
            reconcile_create_outcome: CapabilityState::Supported,
        }
    );
    let other_patch = AdapterRegistry::resolve(
        "7.4.17"
            .parse()
            .unwrap_or_else(|error| panic!("valid version: {error}")),
    )
    .unwrap_or_else(|error| panic!("adapter: {error}"));
    assert_eq!(
        other_patch.dashboard_create_contracts,
        DashboardCreateContracts::unproven()
    );
    // A copy of registered widget types is verified on 7.4.16 ...
    verified(verify_dashboard_create(
        &snapshot(source(None)),
        &context(None),
        TARGET,
    ));
    let requirements = create_requirements(&snapshot(source(None)), TARGET)
        .unwrap_or_else(|rejection| panic!("requirements: {rejection}"));
    assert!(requirements.missing_contracts(&contracts).is_empty());
    assert!(requirements.are_met_by(&contracts));
    // ... a copy of a widget type the registry does not know is not.
    let unknown = snapshot(source(Some(widget("34", "vendor.private", 12, json!([])))));
    let rejection = rejected(verify_dashboard_create(&unknown, &context(None), TARGET));
    assert_eq!(rejection.code(), CreateRejectionCode::WidgetCopyUnproven);
    // Without the reconciliation contract nothing is verified.
    let mut withdrawn = contracts.clone();
    withdrawn.reconcile_create_outcome = CapabilityState::Unsupported;
    let rejection = rejected(verify_dashboard_create(
        &snapshot(source(None)),
        &context(Some(withdrawn.clone())),
        TARGET,
    ));
    assert_eq!(rejection.code(), CreateRejectionCode::ContractUnverified);
    assert_eq!(
        rejection.param("contract"),
        Some("create_reconcile_unknown_outcome")
    );
    assert_eq!(
        requirements.missing_contracts(&withdrawn),
        vec![CreateContract::ReconcileCreateOutcome]
    );
}

#[test]
fn unknown_widget_and_field_types_need_their_own_contract() {
    let unknown_type = widget("34", "vendor.private", 12, json!([]));
    let local = snapshot(source(Some(unknown_type)));
    let requirements = create_requirements(&local, TARGET)
        .unwrap_or_else(|rejection| panic!("requirements: {rejection}"));
    assert!(
        requirements
            .contracts
            .contains(&CreateContract::CopyUnknownWidgets)
    );
    assert_eq!(
        requirements.unknown_widgets,
        vec![UnknownCopyWidget {
            page_index: 1,
            widget_index: 1,
            widget_type: "vendor.private".to_owned(),
            reason: UnknownCopyReason::WidgetType,
        }]
    );
    let rejection = rejected(verify_dashboard_create(
        &local,
        &context(Some(proven(false))),
        TARGET,
    ));
    assert_eq!(rejection.code(), CreateRejectionCode::WidgetCopyUnproven);
    assert_eq!(rejection.param("widget_type"), Some("vendor.private"));
    assert_eq!(rejection.param("reason"), Some("widget_type"));
    assert_eq!(rejection.param("page_index"), Some("1"));
    // Copied as read once the same adapter proves it.
    let create = verified(verify_dashboard_create(
        &local,
        &context(Some(proven(true))),
        TARGET,
    ));
    assert_eq!(
        create.create_params["pages"][1]["widgets"][1]["type"],
        json!("vendor.private")
    );

    let unknown_field = widget(
        "34",
        "clock",
        12,
        json!([{"type": "99", "name": "future", "value": "1"}]),
    );
    let requirements = create_requirements(&snapshot(source(Some(unknown_field))), TARGET)
        .unwrap_or_else(|rejection| panic!("requirements: {rejection}"));
    assert_eq!(
        requirements.unknown_widgets[0].reason,
        UnknownCopyReason::FieldType
    );
}

const FIRST_PAGE: CreateScope<'static> = CreateScope::Page {
    dashboard_pageid: "11",
};
const SECOND_PAGE: CreateScope<'static> = CreateScope::Page {
    dashboard_pageid: "21",
};

/// Owner decision 28.10.2026, item 2: a page draft creates a dashboard of
/// exactly its page, with the dashboard properties of Local; positions in
/// requirements stay Local positions.
#[test]
fn a_page_copy_carries_exactly_its_page() {
    let local = snapshot(source(Some(widget("34", "vendor.private", 12, json!([])))));
    let gates = context(Some(proven(true)));
    let whole = verified(verify_dashboard_create(&local, &gates, TARGET));
    let first = verified(verify_dashboard_create_for(
        &local, FIRST_PAGE, &gates, TARGET,
    ));
    let params = &first.create_params;
    assert_eq!(params["name"], json!(TARGET));
    assert_eq!(params["display_period"], json!("60"));
    assert_eq!(params["auto_start"], json!("0"));
    assert_eq!(params["private"], json!(1));
    assert_eq!(params["users"], json!([]));
    assert_eq!(params["userGroups"], json!([]));
    assert_eq!(
        params["pages"],
        json!([whole.create_params["pages"][0].clone()])
    );
    let requirements = first.requirements();
    assert_eq!(requirements.page_count, 1);
    assert_eq!(requirements.widget_count, 2);
    assert_eq!(requirements.reference_definitions, 1);
    assert_eq!(requirements.reference_consumers, 1);
    assert!(requirements.unknown_widgets.is_empty());
    assert!(requirements.links_outside_page.is_empty());
    assert_eq!(
        first
            .required_contracts()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        CreateContract::COPY.to_vec()
    );
    assert_eq!(first.local_fingerprint(), local.fingerprint());
    assert_ne!(first.content_fingerprint(), whole.content_fingerprint());

    // The second page alone: its unknown widget keeps its Local position.
    let requirements = create_requirements_for(&local, SECOND_PAGE, TARGET)
        .unwrap_or_else(|rejection| panic!("requirements: {rejection}"));
    assert_eq!(requirements.page_count, 1);
    assert_eq!(requirements.widget_count, 2);
    assert_eq!(
        requirements.unknown_widgets,
        vec![UnknownCopyWidget {
            page_index: 1,
            widget_index: 1,
            widget_type: "vendor.private".to_owned(),
            reason: UnknownCopyReason::WidgetType,
        }]
    );
    let rejection = rejected(verify_dashboard_create_for(
        &local,
        SECOND_PAGE,
        &context(Some(proven(false))),
        TARGET,
    ));
    assert_eq!(rejection.code(), CreateRejectionCode::WidgetCopyUnproven);
    let second = verified(verify_dashboard_create_for(
        &local,
        SECOND_PAGE,
        &gates,
        TARGET,
    ));
    assert_eq!(
        second.create_params["pages"],
        json!([whole.create_params["pages"][1].clone()])
    );

    // A page that is not exactly once in Local is a shape error.
    for scope in [
        CreateScope::Page {
            dashboard_pageid: "99",
        },
        CreateScope::Page {
            dashboard_pageid: "",
        },
    ] {
        let rejection = rejected(verify_dashboard_create_for(&local, scope, &gates, TARGET));
        assert_eq!(rejection.code(), CreateRejectionCode::SnapshotShapeInvalid);
        assert_eq!(rejection.param("object"), Some("page"));
    }
    let mut twice = source(None);
    twice["pages"][1]["dashboard_pageid"] = json!("11");
    let rejection = rejected(verify_dashboard_create_for(
        &snapshot(twice),
        FIRST_PAGE,
        &gates,
        TARGET,
    ));
    assert_eq!(rejection.param("object"), Some("page"));
}

/// A widget of the copied page that listens to a widget on another page
/// would point at nothing in the new dashboard: the create is refused. The
/// dashboard selector and a link already orphaned in Local are copied as
/// read, as in a whole-dashboard copy.
#[test]
fn a_page_copy_refuses_links_to_other_pages() {
    let consumer = |value: &str| {
        widget(
            "34",
            "svggraph",
            12,
            json!([{"type": "1", "name": "ds.0.override_hostid._reference", "value": value}]),
        )
    };
    let gates = context(Some(proven(false)));
    let crossing = snapshot(source(Some(consumer("ABCDE._hostid"))));
    let requirements = create_requirements_for(&crossing, SECOND_PAGE, TARGET)
        .unwrap_or_else(|rejection| panic!("requirements: {rejection}"));
    assert_eq!(
        requirements.links_outside_page,
        vec![LinkOutsidePage {
            page_index: 1,
            widget_index: 1,
            field: "ds.0.override_hostid._reference".to_owned(),
        }]
    );
    let rejection = rejected(verify_dashboard_create_for(
        &crossing,
        SECOND_PAGE,
        &gates,
        TARGET,
    ));
    assert_eq!(rejection.code(), CreateRejectionCode::LinkOutsidePage);
    assert_eq!(rejection.code().as_str(), "create_link_outside_page");
    assert_eq!(rejection.param("page_index"), Some("1"));
    assert_eq!(rejection.param("widget_index"), Some("1"));
    assert_eq!(
        rejection.param("field"),
        Some("ds.0.override_hostid._reference")
    );
    // The whole dashboard and the page that holds both ends are fine.
    verified(verify_dashboard_create(&crossing, &gates, TARGET));
    verified(verify_dashboard_create_for(
        &crossing, FIRST_PAGE, &gates, TARGET,
    ));

    for value in ["DASHBOARD._hostid", "ZZZZZ._hostid", ""] {
        let local = snapshot(source(Some(consumer(value))));
        let requirements = create_requirements_for(&local, SECOND_PAGE, TARGET)
            .unwrap_or_else(|rejection| panic!("requirements: {rejection}"));
        assert!(requirements.links_outside_page.is_empty(), "{value}");
        verified(verify_dashboard_create_for(
            &local,
            SECOND_PAGE,
            &gates,
            TARGET,
        ));
    }
}

#[test]
fn names_shapes_versions_and_authority_fail_closed() {
    let local = snapshot(source(None));
    let gates = context(Some(proven(true)));
    for name in ["", " padded", "padded ", "tab\there", &"x".repeat(256)] {
        assert_eq!(
            rejected(verify_dashboard_create(&local, &gates, name)).code(),
            CreateRejectionCode::NameInvalid,
            "{name:?}"
        );
    }
    assert!(is_valid_dashboard_name(&"я".repeat(255)));

    let mut empty = source(None);
    empty["pages"] = json!([]);
    assert_eq!(
        rejected(verify_dashboard_create(&snapshot(empty), &gates, TARGET)).code(),
        CreateRejectionCode::PagesEmpty
    );
    let mut no_geometry = source(None);
    if let Some(widget) = no_geometry["pages"][0]["widgets"][0].as_object_mut() {
        widget.remove("x");
    }
    let rejection = rejected(verify_dashboard_create(
        &snapshot(no_geometry),
        &gates,
        TARGET,
    ));
    assert_eq!(rejection.code(), CreateRejectionCode::SnapshotShapeInvalid);
    assert_eq!(rejection.param("object"), Some("widget"));

    let older = VersionedDashboardSnapshot::from_adapter_raw(
        "mock-connection".to_owned(),
        "7.4.15"
            .parse()
            .unwrap_or_else(|error| panic!("version: {error}")),
        source(None),
    )
    .unwrap_or_else(|error| panic!("snapshot: {error}"));
    assert_eq!(
        rejected(verify_dashboard_create(&older, &gates, TARGET)).code(),
        CreateRejectionCode::ServerVersionMismatch
    );
    let mut capabilities = capabilities();
    capabilities.dashboard_create_contracts = proven(true);
    assert_eq!(
        rejected(verify_dashboard_create(
            &local,
            &PublicationContext::offline_for_planning(capabilities),
            TARGET
        ))
        .code(),
        CreateRejectionCode::AuthorityOffline
    );
}

// ---------- read-back and reconciliation (P2-06D) ----------

#[test]
fn read_back_checks_content_privacy_and_owner() {
    let create = verified(verify_dashboard_create(
        &snapshot(source(None)),
        &context(Some(proven(false))),
        TARGET,
    ));
    let expected = expectation(&create);
    let good = stored(&create.create_params, "70", OWNER);
    assert_eq!(
        check_created_dashboard(&snapshot(good.clone()), &expected),
        ReadBackVerdict::Matches
    );
    // Numbers read back as strings and widgets in another order still match.
    let mut reordered = good.clone();
    if let Some(widgets) = reordered["pages"][0]["widgets"].as_array_mut() {
        widgets.reverse();
    }
    reordered["pages"][0]["widgets"][0]["fields"][1]["type"] = json!("1");
    assert_eq!(
        check_created_dashboard(&snapshot(reordered), &expected),
        ReadBackVerdict::Matches
    );
    let cases: [(&str, Value, ReadBackVerdict); 6] = [
        ("name", json!("Other"), ReadBackVerdict::NameDiffers),
        ("private", json!("0"), ReadBackVerdict::NotPrivate),
        (
            "users",
            json!([{"userid": "9", "permission": "2"}]),
            ReadBackVerdict::Shared,
        ),
        (
            "userGroups",
            json!([{"usrgrpid": "4", "permission": "2"}]),
            ReadBackVerdict::Shared,
        ),
        ("userid", json!("6"), ReadBackVerdict::OwnerDiffers),
        (
            "display_period",
            json!("30"),
            ReadBackVerdict::ContentDiffers,
        ),
    ];
    for (key, value, verdict) in cases {
        let mut changed = good.clone();
        changed[key] = value;
        assert_eq!(
            check_created_dashboard(&snapshot(changed), &expected),
            verdict,
            "{key}"
        );
    }
    let mut field = good.clone();
    field["pages"][0]["widgets"][1]["fields"][1]["value"] = json!("FGHIJ._hostid");
    assert_eq!(
        check_created_dashboard(&snapshot(field), &expected),
        ReadBackVerdict::ContentDiffers
    );
    let mut lost_page = good;
    if let Some(pages) = lost_page["pages"].as_array_mut() {
        pages.pop();
    }
    assert_eq!(
        check_created_dashboard(&snapshot(lost_page), &expected),
        ReadBackVerdict::ContentDiffers
    );
}

#[test]
fn reconciliation_confirms_only_exactly_one_matching_candidate() {
    let create = verified(verify_dashboard_create(
        &snapshot(source(None)),
        &context(Some(proven(false))),
        TARGET,
    ));
    let expected = expectation(&create);
    let ours = snapshot(stored(&create.create_params, "70", OWNER));
    let named = |dashboards: Vec<VersionedDashboardSnapshot>, truncated| NamedDashboards {
        dashboards,
        truncated,
    };

    let found = reconcile_created_dashboard(Ok(&named(vec![ours.clone()], false)), &expected);
    assert_eq!(
        found,
        CreateReconciliation::Confirmed {
            dashboardid: "70".to_owned(),
            fingerprint: ours.fingerprint().to_owned(),
        }
    );
    assert!(found.is_confirmed());
    assert_eq!(found.code(), "reconciled_verified");

    let none = reconcile_created_dashboard(Ok(&named(Vec::new(), false)), &expected);
    assert_eq!(none, CreateReconciliation::NotFound);
    assert!(!none.is_confirmed());

    let twin = snapshot(stored(&create.create_params, "71", OWNER));
    assert_eq!(
        reconcile_created_dashboard(Ok(&named(vec![ours.clone(), twin], false)), &expected),
        CreateReconciliation::Ambiguous { candidates: 2 }
    );

    let mut other_owner = stored(&create.create_params, "72", "8");
    other_owner["private"] = json!("1");
    assert_eq!(
        reconcile_created_dashboard(Ok(&named(vec![snapshot(other_owner)], false)), &expected),
        CreateReconciliation::Mismatch {
            verdict: ReadBackVerdict::OwnerDiffers
        }
    );

    assert_eq!(
        reconcile_created_dashboard(Ok(&named(vec![ours.clone()], true)), &expected),
        CreateReconciliation::PartialVisibility
    );

    let failure = Failure::new(FailureCode::HostUnreachableOrTimeout);
    let failed = reconcile_created_dashboard(Err(&failure), &expected);
    assert_eq!(failed.code(), "reconcile_read_failed");
    assert!(!failed.is_confirmed());

    // A case variant of the name is not a candidate.
    let mut variant = stored(&create.create_params, "73", OWNER);
    variant["name"] = json!(TARGET.to_uppercase());
    assert_eq!(
        reconcile_created_dashboard(Ok(&named(vec![snapshot(variant)], false)), &expected),
        CreateReconciliation::NotFound
    );
}

// ---------- closed create path on a mock transport (P2-06C) ----------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CreateMode {
    /// Create and answer with the new ID.
    Accept,
    /// `Dashboard "<name>" already exists.`
    NameTaken,
    /// Create, then answer HTTP 502 (a proxy lost the answer).
    CreateThenLoseAnswer,
    /// Answer HTTP 502 without creating.
    LoseRequest,
}

struct MockData {
    dashboards: Vec<Value>,
    creates: Vec<Value>,
    mode: CreateMode,
    /// `filter` by name ignores case, as a case-insensitive collation would.
    filter_ignores_case: bool,
}

type Shared = Arc<Mutex<MockData>>;

fn lock(data: &Shared) -> std::sync::MutexGuard<'_, MockData> {
    data.lock().unwrap_or_else(PoisonError::into_inner)
}

fn reply(id: &Value, result: Value) -> Response {
    Json(json!({"jsonrpc": "2.0", "result": result, "id": id})).into_response()
}

async fn rpc(State(data): State<Shared>, Json(body): Json<Value>) -> Response {
    let id = body["id"].clone();
    let params = body["params"].clone();
    let mut data = lock(&data);
    match body["method"].as_str().unwrap_or_default() {
        "apiinfo.version" => reply(&id, json!("7.4.16")),
        "user.checkAuthentication" => reply(&id, json!({"userid": OWNER, "username": "writer"})),
        "dashboard.get" => {
            let names = params["filter"]["name"].as_array().cloned();
            let ids = params["dashboardids"].as_array().cloned();
            let fold = |value: &Value| {
                let text = value.as_str().unwrap_or_default();
                if data.filter_ignores_case {
                    text.to_lowercase()
                } else {
                    text.to_owned()
                }
            };
            let found = data
                .dashboards
                .iter()
                .filter(|dashboard| {
                    names.as_ref().is_none_or(|names| {
                        names
                            .iter()
                            .any(|name| fold(name) == fold(&dashboard["name"]))
                    }) && ids
                        .as_ref()
                        .is_none_or(|ids| ids.contains(&dashboard["dashboardid"]))
                })
                .cloned()
                .collect::<Vec<_>>();
            reply(&id, Value::Array(found))
        }
        "dashboard.create" => {
            data.creates.push(params.clone());
            let dashboardid = (90 + data.dashboards.len()).to_string();
            match data.mode {
                CreateMode::Accept => {
                    data.dashboards.push(stored(&params, &dashboardid, OWNER));
                    reply(&id, json!({"dashboardids": [dashboardid]}))
                }
                CreateMode::NameTaken => {
                    let text = format!(
                        "Dashboard \"{}\" already exists.",
                        params["name"].as_str().unwrap_or_default()
                    );
                    Json(json!({"jsonrpc": "2.0", "id": id, "error": {
                        "code": -32602, "message": "Invalid params.", "data": text
                    }}))
                    .into_response()
                }
                CreateMode::CreateThenLoseAnswer => {
                    data.dashboards.push(stored(&params, &dashboardid, OWNER));
                    StatusCode::BAD_GATEWAY.into_response()
                }
                CreateMode::LoseRequest => StatusCode::BAD_GATEWAY.into_response(),
            }
        }
        _ => reply(&id, json!([])),
    }
}

async fn writer(data: &Shared) -> (ReadWriteGateway, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route("/api_jsonrpc.php", post(rpc))
        .with_state(Arc::clone(data));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("mock bind: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("mock address: {error}"));
    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            panic!("mock API failed: {error}");
        }
    });
    let endpoint = ZabbixEndpoint::parse(&format!("http://{address}/api_jsonrpc.php"))
        .unwrap_or_else(|error| panic!("mock endpoint: {error}"));
    let gateway = ZabbixClient::new(endpoint)
        .unwrap_or_else(|error| panic!("client: {error}"))
        .authenticate(Authentication::ApiToken(SecretString::from(
            "test-token".to_owned(),
        )))
        .await
        .unwrap_or_else(|error| panic!("authentication: {error}"))
        .enable_writes();
    (gateway, task)
}

async fn start(mode: CreateMode) -> (Shared, ReadWriteGateway, tokio::task::JoinHandle<()>) {
    let data: Shared = Arc::new(Mutex::new(MockData {
        dashboards: vec![source(None)],
        creates: Vec::new(),
        mode,
        filter_ignores_case: false,
    }));
    let (mut gateway, task) = writer(&data).await;
    gateway.set_create_contracts_for_test(proven(false));
    (data, gateway, task)
}

async fn local(gateway: &ReadWriteGateway) -> VersionedDashboardSnapshot {
    gateway
        .reader()
        .dashboard_snapshot("mock-connection", "7")
        .await
        .unwrap_or_else(|error| panic!("read: {error}"))
        .unwrap_or_else(|| panic!("source dashboard"))
}

#[tokio::test]
async fn a_proven_create_is_sent_once_and_verified_by_read_back() {
    let (data, gateway, task) = start(CreateMode::Accept).await;
    let local = local(&gateway).await;
    let create = verified(verify_dashboard_create(
        &local,
        &gateway.publication_context(),
        TARGET,
    ));
    let outcome = gateway.create_verified_dashboard(&create, || true).await;
    let state = lock(&data);
    assert_eq!(state.creates, vec![create.create_params.clone()]);
    let created = state.dashboards.last().cloned();
    drop(state);
    let created = snapshot(created.unwrap_or_else(|| panic!("created dashboard")));
    assert_eq!(
        outcome,
        DashboardCreateOutcome::Verified {
            dashboardid: created.dashboard_id().to_owned(),
            fingerprint: created.fingerprint().to_owned(),
        }
    );
    assert!(outcome.sent());
    assert_eq!(outcome.as_str(), "verified");
    assert!(outcome.confirmed_dashboard().is_some());
    // The source dashboard is untouched.
    assert_eq!(lock(&data).dashboards[0], source(None));
    task.abort();
}

#[tokio::test]
async fn a_page_copy_is_sent_once_and_verified_by_read_back() {
    let (data, gateway, task) = start(CreateMode::Accept).await;
    let local = local(&gateway).await;
    let create = verified(verify_dashboard_create_for(
        &local,
        SECOND_PAGE,
        &gateway.publication_context(),
        TARGET,
    ));
    let outcome = gateway.create_verified_dashboard(&create, || true).await;
    let state = lock(&data);
    assert_eq!(state.creates, vec![create.create_params.clone()]);
    let created = state.dashboards.last().cloned();
    drop(state);
    let created = snapshot(created.unwrap_or_else(|| panic!("created dashboard")));
    assert_eq!(
        created.raw_api_document()["pages"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        outcome,
        DashboardCreateOutcome::Verified {
            dashboardid: created.dashboard_id().to_owned(),
            fingerprint: created.fingerprint().to_owned(),
        }
    );
    assert_eq!(lock(&data).dashboards[0], source(None));
    task.abort();
}

#[tokio::test]
async fn already_exists_is_final_name_taken_without_a_retry() {
    let (data, gateway, task) = start(CreateMode::NameTaken).await;
    let create = verified(verify_dashboard_create(
        &local(&gateway).await,
        &gateway.publication_context(),
        TARGET,
    ));
    let outcome = gateway.create_verified_dashboard(&create, || true).await;
    assert_eq!(outcome, DashboardCreateOutcome::NameTaken { sent: true });
    assert_eq!(outcome.as_str(), "dashboard_name_taken");
    assert_eq!(lock(&data).creates.len(), 1);
    task.abort();
}

#[tokio::test]
async fn a_visible_name_stops_the_create_before_send() {
    let (data, gateway, task) = start(CreateMode::Accept).await;
    let mut visible = source(None);
    visible["dashboardid"] = json!("8");
    visible["name"] = json!(TARGET);
    lock(&data).dashboards.push(visible);
    let create = verified(verify_dashboard_create(
        &local(&gateway).await,
        &gateway.publication_context(),
        TARGET,
    ));
    let outcome = gateway.create_verified_dashboard(&create, || true).await;
    assert_eq!(outcome, DashboardCreateOutcome::NameTaken { sent: false });
    assert!(!outcome.sent());
    assert!(lock(&data).creates.is_empty());
    task.abort();
}

#[tokio::test]
async fn a_case_variant_of_the_name_does_not_stop_the_create() {
    // LAB-02: on PostgreSQL `dashboard.create` accepts a name that differs
    // only in case, so the pre-check compares names exactly even when the
    // server's `filter` ignores case.
    let (data, gateway, task) = start(CreateMode::Accept).await;
    let mut variant = source(None);
    variant["dashboardid"] = json!("8");
    variant["name"] = json!(TARGET.to_uppercase());
    {
        let mut data = lock(&data);
        data.dashboards.push(variant);
        data.filter_ignores_case = true;
    }
    let named = gateway
        .reader()
        .dashboards_named("mock-connection", TARGET)
        .await
        .unwrap_or_else(|error| panic!("named: {error}"));
    assert!(named.dashboards.is_empty() && !named.truncated);
    let create = verified(verify_dashboard_create(
        &local(&gateway).await,
        &gateway.publication_context(),
        TARGET,
    ));
    let outcome = gateway.create_verified_dashboard(&create, || true).await;
    assert!(
        matches!(outcome, DashboardCreateOutcome::Verified { .. }),
        "{outcome:?}"
    );
    assert_eq!(lock(&data).creates.len(), 1);
    task.abort();
}

#[tokio::test]
async fn unproven_contracts_and_other_authorities_send_nothing() {
    let (data, mut gateway, task) = start(CreateMode::Accept).await;
    let local = local(&gateway).await;
    let create = verified(verify_dashboard_create(
        &local,
        &gateway.publication_context(),
        TARGET,
    ));
    // Reconciliation withdrawn right before send: nothing is sent.
    let mut withdrawn = capabilities().dashboard_create_contracts;
    withdrawn.reconcile_create_outcome = CapabilityState::Unsupported;
    gateway.set_create_contracts_for_test(withdrawn);
    assert_eq!(
        gateway.create_verified_dashboard(&create, || true).await,
        DashboardCreateOutcome::FailedBeforeSend(CreateSendRefusal::ContractUnverified(
            CreateContract::ReconcileCreateOutcome
        ))
    );
    // Any other patch: nothing is proven.
    gateway.set_create_contracts_for_test(DashboardCreateContracts::unproven());
    assert_eq!(
        gateway.create_verified_dashboard(&create, || true).await,
        DashboardCreateOutcome::FailedBeforeSend(CreateSendRefusal::ContractUnverified(
            CreateContract::CreatePrivateDashboard
        ))
    );
    // A plan verified for another authority never reaches the server.
    gateway.set_create_contracts_for_test(proven(false));
    let detached = verified(verify_dashboard_create(
        &local,
        &context(Some(proven(false))),
        TARGET,
    ));
    assert_eq!(
        gateway.create_verified_dashboard(&detached, || true).await,
        DashboardCreateOutcome::FailedBeforeSend(CreateSendRefusal::AuthorityChanged)
    );
    // A new write epoch invalidates the authority too.
    let gateway = gateway.into_read_only().enable_writes();
    assert_eq!(
        gateway.create_verified_dashboard(&create, || true).await,
        DashboardCreateOutcome::FailedBeforeSend(CreateSendRefusal::AuthorityChanged)
    );
    assert!(lock(&data).creates.is_empty());
    task.abort();
}

#[tokio::test]
async fn a_lost_answer_is_reconciled_by_reads_without_a_second_create() {
    let (data, gateway, task) = start(CreateMode::CreateThenLoseAnswer).await;
    let create = verified(verify_dashboard_create(
        &local(&gateway).await,
        &gateway.publication_context(),
        TARGET,
    ));
    let outcome = gateway.create_verified_dashboard(&create, || true).await;
    assert!(
        matches!(&outcome, DashboardCreateOutcome::ReconciledVerified { dashboardid, .. } if dashboardid == "91"),
        "{outcome:?}"
    );
    assert_eq!(lock(&data).creates.len(), 1);

    // Restart-time reconciliation by the recorded expectation finds it too.
    let expected = gateway.create_expectation(&create);
    assert_eq!(expected.owner_userid, OWNER);
    let again = gateway
        .reader()
        .reconcile_dashboard_create(&expected, None)
        .await;
    assert!(again.is_confirmed());
    let by_id = gateway
        .reader()
        .reconcile_dashboard_create(&expected, Some("91"))
        .await;
    assert!(by_id.is_confirmed());
    assert_eq!(
        gateway
            .reader()
            .reconcile_dashboard_create(&expected, Some("99"))
            .await,
        CreateReconciliation::NotFound
    );
    assert_eq!(lock(&data).creates.len(), 1);
    task.abort();
}

#[tokio::test]
async fn a_lost_request_stays_unknown_and_is_not_repeated() {
    let (data, gateway, task) = start(CreateMode::LoseRequest).await;
    let create = verified(verify_dashboard_create(
        &local(&gateway).await,
        &gateway.publication_context(),
        TARGET,
    ));
    let outcome = gateway.create_verified_dashboard(&create, || true).await;
    assert_eq!(
        outcome,
        DashboardCreateOutcome::UnknownOutcome {
            reconciliation: CreateReconciliation::NotFound
        }
    );
    assert!(outcome.sent());
    assert!(outcome.confirmed_dashboard().is_none());
    assert_eq!(lock(&data).creates.len(), 1);

    let mut other = gateway.create_expectation(&create);
    other.owner_userid = "6".to_owned();
    assert_eq!(
        gateway
            .reader()
            .reconcile_dashboard_create(&other, None)
            .await,
        CreateReconciliation::ConnectionMismatch
    );
    task.abort();
}

#[tokio::test]
async fn a_refused_submit_record_sends_nothing() {
    let (data, gateway, task) = start(CreateMode::Accept).await;
    let create = verified(verify_dashboard_create(
        &local(&gateway).await,
        &gateway.publication_context(),
        TARGET,
    ));
    let mut calls = 0;
    let outcome = gateway
        .create_verified_dashboard(&create, || {
            calls += 1;
            false
        })
        .await;
    assert_eq!(
        outcome,
        DashboardCreateOutcome::FailedBeforeSend(CreateSendRefusal::SubmitNotRecorded)
    );
    assert_eq!(calls, 1);
    assert!(lock(&data).creates.is_empty());
    task.abort();
}
