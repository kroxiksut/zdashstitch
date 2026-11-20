// SPDX-License-Identifier: MIT OR Apache-2.0

//! Opt-in 8.0.0 API contract evidence. The test-only observer forwards only
//! reads and writes to fixtures made in this run. The application's gateway
//! is exercised separately and must keep its 8.0 write gates closed.

use std::{
    collections::BTreeSet,
    env,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
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
use zdashstitch_zabbix::{
    Authentication, CreateRejectionCode, RoleApiAccess, WidgetFieldReplacement, WriteRejectionCode,
    ZabbixClient, ZabbixEndpoint, dashboard_write_allowlist, read_only_allowlist,
    verify_dashboard_create, verify_widget_field_replacements,
};

const VERSION: &str = "8.0.0";
const ACK: &str = "dashboard-only-authorized-8.0.0";
const PREFIX: &str = "ZDashstitch Z80-P5-03 lab";

#[derive(Default)]
struct Observations {
    methods: Vec<String>,
    writes_forwarded: Vec<String>,
    blocked: usize,
    fixture_ids: BTreeSet<String>,
    discard_next_write_response: bool,
}

#[derive(Clone)]
struct Observer {
    upstream: Url,
    client: Client,
    observations: Arc<Mutex<Observations>>,
    fixture_prefix: String,
}

async fn observe(
    State(observer): State<Observer>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let method = body["method"].as_str().unwrap_or_default();
    let params = &body["params"];
    let discard_response = {
        let mut state = observer
            .observations
            .lock()
            .unwrap_or_else(|_| panic!("observer state"));
        state.methods.push(method.to_owned());
        let allows_params = |keys: &[&str]| {
            params
                .as_object()
                .is_some_and(|object| object.keys().all(|key| keys.contains(&key.as_str())))
        };
        let new_name_in_scope = params["name"]
            .as_str()
            .is_some_and(|name| name.starts_with(&observer.fixture_prefix));
        let permitted = match method {
            "dashboard.create" => new_name_in_scope && allows_params(&["name", "pages"]),
            "dashboard.update" => {
                allows_params(&["dashboardid", "name"])
                    && new_name_in_scope
                    && params["dashboardid"]
                        .as_str()
                        .is_some_and(|id| state.fixture_ids.contains(id))
            }
            method => read_only_allowlist().contains(&method),
        };
        if !permitted {
            state.blocked += 1;
            return StatusCode::FORBIDDEN.into_response();
        }
        if dashboard_write_allowlist().contains(&method) {
            state.writes_forwarded.push(method.to_owned());
            std::mem::take(&mut state.discard_next_write_response)
        } else {
            false
        }
    };

    let mut request = observer.client.post(observer.upstream).json(&body);
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        request = request.header(header::AUTHORIZATION, value);
    }
    match request.send().await {
        Ok(response) => match response.json::<Value>().await {
            Ok(_) if discard_response => StatusCode::BAD_GATEWAY.into_response(),
            Ok(value) => Json(value).into_response(),
            Err(_) => StatusCode::BAD_GATEWAY.into_response(),
        },
        Err(_) => StatusCode::BAD_GATEWAY.into_response(),
    }
}

struct Rpc {
    client: Client,
    endpoint: String,
    session: Option<String>,
}

impl Rpc {
    async fn request(&self, method: &str, params: Value) -> reqwest::Response {
        let mut request = self.client.post(&self.endpoint).json(&json!({
            "jsonrpc": "2.0", "id": 1, "method": method, "params": params
        }));
        if let Some(session) = &self.session {
            request = request.bearer_auth(session);
        }
        request
            .send()
            .await
            .unwrap_or_else(|_| panic!("{method} HTTP request failed"))
    }

    async fn envelope(&self, method: &str, params: Value) -> Value {
        self.request(method, params)
            .await
            .error_for_status()
            .unwrap_or_else(|_| panic!("{method} HTTP response failed"))
            .json()
            .await
            .unwrap_or_else(|_| panic!("{method} response is not JSON"))
    }

    async fn call(&self, method: &str, params: Value) -> Value {
        let response = self.envelope(method, params).await;
        response.get("result").cloned().unwrap_or_else(|| {
            panic!(
                "{method} failed with JSON-RPC code {:?}",
                response["error"]["code"].as_i64()
            )
        })
    }

    async fn login(client: Client, endpoint: String, label: &str) -> Self {
        let mut rpc = Self {
            client,
            endpoint,
            session: None,
        };
        assert_eq!(rpc.call("apiinfo.version", json!([])).await, VERSION);
        let username = required(&format!("ZDASHSTITCH_LAB_Z80_{label}_USERNAME"));
        let password = required(&format!("ZDASHSTITCH_LAB_Z80_{label}_PASSWORD"));
        let result = rpc
            .call(
                "user.login",
                json!({"username": username, "password": password}),
            )
            .await;
        rpc.session = Some(
            result
                .as_str()
                .unwrap_or_else(|| panic!("login session string"))
                .to_owned(),
        );
        rpc
    }

    async fn identity(&self) -> Value {
        Self {
            client: self.client.clone(),
            endpoint: self.endpoint.clone(),
            session: None,
        }
        .call(
            "user.checkAuthentication",
            json!({"sessionid": self.session}),
        )
        .await
    }

    async fn dashboard(&self, id: &str, editable: bool) -> Option<Value> {
        let result = self
            .call(
                "dashboard.get",
                json!({
                    "dashboardids": [id], "editable": editable,
                    "output": ["dashboardid", "userid", "name", "private"],
                    "selectPages": "extend", "selectUsers": "extend", "selectUserGroups": "extend"
                }),
            )
            .await;
        let rows = result
            .as_array()
            .unwrap_or_else(|| panic!("dashboard rows"));
        assert!(rows.len() <= 1, "dashboard read must be scoped to one ID");
        rows.first().map(|row| {
            assert_eq!(
                row["dashboardid"].as_str(),
                Some(id),
                "unexpected dashboard ID"
            );
            row.clone()
        })
    }
}

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("missing {name}"))
}

#[tokio::test]
#[ignore = "requires the authorized 8.0.0 laboratory and three dedicated laboratory accounts"]
async fn lab_z80_p5_03_dashboard_only_contract() {
    assert_eq!(required("ZDASHSTITCH_LAB_Z80_P5_03_ACK"), ACK);
    let upstream = Url::parse(&required("ZDASHSTITCH_LAB_API_URL"))
        .unwrap_or_else(|_| panic!("valid lab endpoint"));
    assert!(
        matches!(upstream.host(), Some(url::Host::Ipv4(ip)) if ip.is_loopback())
            || matches!(upstream.host(), Some(url::Host::Ipv6(ip)) if ip.is_loopback()),
        "use a loopback tunnel to the explicitly authorized laboratory"
    );
    assert!(upstream.username().is_empty() && upstream.password().is_none());
    assert_eq!(upstream.path().rsplit('/').next(), Some("api_jsonrpc.php"));
    assert_eq!(
        dashboard_write_allowlist(),
        ["dashboard.update", "dashboard.create"]
    );
    let client = Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| panic!("test HTTP client"));
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| panic!("clock"))
        .as_nanos();
    let fixture_prefix = format!("{PREFIX} {stamp} ");
    let observations = Arc::new(Mutex::new(Observations::default()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|_| panic!("observer listener"));
    let base = format!(
        "http://{}",
        listener
            .local_addr()
            .unwrap_or_else(|_| panic!("observer address"))
    );
    let router = Router::new()
        .route("/api_jsonrpc.php", post(observe))
        .with_state(Observer {
            upstream,
            client: client.clone(),
            observations: observations.clone(),
            fixture_prefix: fixture_prefix.clone(),
        });
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let endpoint = format!("{base}/api_jsonrpc.php");
    let admin = Rpc::login(client.clone(), endpoint.clone(), "ADMIN").await;
    let method_denied = Rpc::login(client.clone(), endpoint.clone(), "METHOD_DENIED").await;
    let outsider = Rpc::login(client, endpoint, "OUTSIDER").await;
    let identity = admin.identity().await;
    assert!(
        matches!(identity["type"].as_i64(), Some(2 | 3))
            || matches!(identity["type"].as_str(), Some("2" | "3"))
    );
    let outsider_identity = outsider.identity().await;
    let denied_identity = method_denied.identity().await;
    assert_ne!(identity["userid"], outsider_identity["userid"]);
    assert_ne!(identity["userid"], denied_identity["userid"]);
    assert_ne!(outsider_identity["userid"], denied_identity["userid"]);
    let role = admin
        .call(
            "role.get",
            json!({"current": true, "output": ["roleid", "type"], "selectRules": "extend"}),
        )
        .await;
    assert_eq!(
        role.as_array()
            .unwrap_or_else(|| panic!("current role rows"))
            .len(),
        1
    );
    assert_eq!(role[0]["roleid"], identity["roleid"]);
    let api_access = &role[0]["rules"]["api.access"];
    assert!(api_access == "1" || api_access == 1);
    assert!(
        role[0]["rules"]["actions"]
            .as_array()
            .unwrap_or_else(|| panic!("role actions"))
            .iter()
            .any(|action| {
                action["name"] == "edit_dashboards"
                    && (action["status"] == "1" || action["status"] == 1)
            })
    );

    let initial = format!("{fixture_prefix}created");
    let create = json!({"name": initial, "pages": [
        {"name": "First", "widgets": [{"type": "clock", "name": "Clock", "x": 0, "y": 0, "width": 8, "height": 4, "view_mode": 0}]},
        {"name": "Second"}
    ]});
    let denied_create = method_denied
        .envelope("dashboard.create", create.clone())
        .await;
    assert!(
        denied_create["error"]["code"].is_i64(),
        "role must deny create"
    );
    let created = admin.call("dashboard.create", create.clone()).await;
    let id = created["dashboardids"][0]
        .as_str()
        .unwrap_or_else(|| panic!("created dashboard ID"));
    observations
        .lock()
        .unwrap_or_else(|_| panic!("observer state"))
        .fixture_ids
        .insert(id.to_owned());
    let before = admin
        .dashboard(id, false)
        .await
        .unwrap_or_else(|| panic!("created dashboard read-back"));
    assert_eq!(before["name"], initial);
    assert_eq!(before["userid"], identity["userid"]);
    assert!(before["private"] == "1" || before["private"] == 1);
    assert_eq!(before["users"], json!([]));
    assert_eq!(before["userGroups"], json!([]));
    assert_eq!(
        before["pages"]
            .as_array()
            .unwrap_or_else(|| panic!("created pages"))
            .len(),
        2
    );
    assert_eq!(before["pages"][0]["widgets"][0]["type"], "clock");
    assert!(
        admin.dashboard(id, true).await.is_some(),
        "owner can edit fixture"
    );
    assert!(
        outsider.dashboard(id, true).await.is_none(),
        "outsider cannot edit fixture"
    );

    let update_name = format!("{fixture_prefix}updated");
    let update = json!({"dashboardid": id, "name": update_name});
    let denied_update = method_denied
        .envelope("dashboard.update", update.clone())
        .await;
    let foreign_update = outsider.envelope("dashboard.update", update.clone()).await;
    assert!(
        denied_update["error"]["code"].is_i64(),
        "role must deny update"
    );
    assert!(
        foreign_update["error"]["code"].is_i64(),
        "foreign owner must deny update"
    );
    let untouched = admin
        .dashboard(id, false)
        .await
        .unwrap_or_else(|| panic!("unchanged fixture"));
    assert!(untouched == before, "denied updates changed the fixture");
    for method in [
        "host.update",
        "item.create",
        "template.update",
        "trigger.delete",
        "user.update",
        "configuration.import",
        "dashboard.delete",
        "templatedashboard.update",
    ] {
        assert_eq!(
            admin.request(method, json!({})).await.status(),
            StatusCode::FORBIDDEN,
            "{method} must never reach the laboratory"
        );
    }

    let confirmed = admin.call("dashboard.update", update).await;
    assert_eq!(confirmed["dashboardids"][0].as_str(), Some(id));
    let after = admin
        .dashboard(id, false)
        .await
        .unwrap_or_else(|| panic!("update read-back"));
    let mut expected = before;
    expected["name"] = json!(update_name);
    assert!(
        after == expected,
        "update changed data outside its intended field"
    );

    let uncertain_name = format!("{fixture_prefix}reconciled");
    let unknown_update_start = {
        let mut state = observations
            .lock()
            .unwrap_or_else(|_| panic!("observer state"));
        state.discard_next_write_response = true;
        state.methods.len()
    };
    let response = admin
        .request(
            "dashboard.update",
            json!({"dashboardid": id, "name": uncertain_name}),
        )
        .await;
    assert_eq!(
        response.status(),
        StatusCode::BAD_GATEWAY,
        "write reply must be unavailable"
    );
    let reconciled = admin
        .dashboard(id, false)
        .await
        .unwrap_or_else(|| panic!("read-only reconciliation"));
    expected["name"] = json!(uncertain_name);
    assert!(
        reconciled == expected,
        "read-only reconciliation did not find the intended state"
    );
    assert_eq!(
        observations
            .lock()
            .unwrap_or_else(|_| panic!("observer state"))
            .methods[unknown_update_start..],
        ["dashboard.update", "dashboard.get"],
        "ambiguous update must be sent only once and reconciled by reading"
    );

    let unknown_create_start = {
        let mut state = observations
            .lock()
            .unwrap_or_else(|_| panic!("observer state"));
        state.discard_next_write_response = true;
        state.methods.len()
    };
    let unknown_create_name = format!("{fixture_prefix}create without reply");
    let mut unknown_create = create;
    unknown_create["name"] = json!(unknown_create_name);
    assert_eq!(
        admin
            .request("dashboard.create", unknown_create)
            .await
            .status(),
        StatusCode::BAD_GATEWAY
    );
    let rows = admin
        .call(
            "dashboard.get",
            json!({
                "output": ["dashboardid", "name", "userid", "private"],
                "filter": {"name": [unknown_create_name]}, "selectPages": "extend", "limit": 2
            }),
        )
        .await;
    let [candidate] = rows
        .as_array()
        .unwrap_or_else(|| panic!("candidate rows"))
        .as_slice()
    else {
        panic!("ambiguous create must leave exactly one visible candidate");
    };
    assert_eq!(candidate["name"], unknown_create_name);
    assert_eq!(candidate["userid"], identity["userid"]);
    assert!(candidate["private"] == "1" || candidate["private"] == 1);
    let unconfirmed_id = candidate["dashboardid"]
        .as_str()
        .unwrap_or_else(|| panic!("unconfirmed dashboard ID"));
    assert_ne!(unconfirmed_id, id);
    assert_eq!(
        candidate["pages"]
            .as_array()
            .unwrap_or_else(|| panic!("created pages"))
            .len(),
        2
    );
    assert_eq!(candidate["pages"][0]["widgets"][0]["type"], "clock");
    assert_eq!(
        observations
            .lock()
            .unwrap_or_else(|_| panic!("observer state"))
            .methods[unknown_create_start..],
        ["dashboard.create", "dashboard.get"],
        "ambiguous create must be sent only once and reconciled by reading"
    );
    assert_eq!(
        admin
            .request(
                "dashboard.update",
                json!({"dashboardid": unconfirmed_id, "name": format!("{fixture_prefix}outside scope")})
            )
            .await
            .status(),
        StatusCode::FORBIDDEN,
        "an owned dashboard without update scope must not be written"
    );

    let application_start = observations
        .lock()
        .unwrap_or_else(|_| panic!("observer state"))
        .methods
        .len();
    let writer = ZabbixClient::new(
        ZabbixEndpoint::parse(&base).unwrap_or_else(|_| panic!("observer endpoint")),
    )
    .unwrap_or_else(|_| panic!("application client"))
    .authenticate(Authentication::UsernamePassword {
        username: required("ZDASHSTITCH_LAB_Z80_ADMIN_USERNAME"),
        password: SecretString::from(required("ZDASHSTITCH_LAB_Z80_ADMIN_PASSWORD")),
    })
    .await
    .unwrap_or_else(|_| panic!("application authentication failed"))
    .enable_writes();
    assert_eq!(writer.capabilities().version.to_string(), VERSION);
    assert_eq!(
        writer
            .reader()
            .probe_role_api_access()
            .await
            .unwrap_or_else(|_| panic!("application role probe")),
        RoleApiAccess::Enabled
    );
    assert!(
        !writer
            .reader()
            .dashboard_create_permission()
            .await
            .unwrap_or_else(|_| panic!("application create gate"))
    );
    let snapshot = writer
        .reader()
        .dashboard_snapshot("z80-p5-03-lab", id)
        .await
        .unwrap_or_else(|_| panic!("application read"))
        .unwrap_or_else(|| panic!("application fixture snapshot"));
    let result = verify_dashboard_create(
        &snapshot,
        &writer.publication_context(),
        &format!("{fixture_prefix}application"),
    );
    assert!(
        matches!(result, Err(ref error) if error.code() == CreateRejectionCode::AdapterUnsupported),
        "application must refuse 8.0 create: {:?}",
        result.err().map(|error| error.code())
    );
    let replacement = WidgetFieldReplacement {
        page_index: 0,
        widgetid: reconciled["pages"][0]["widgets"][0]["widgetid"]
            .as_str()
            .unwrap_or_else(|| panic!("fixture widget ID"))
            .to_owned(),
        field_name: "time_type".to_owned(),
        expected_value: "0".to_owned(),
        replacement_value: "1".to_owned(),
    };
    let result =
        verify_widget_field_replacements(&snapshot, &writer.publication_context(), &[replacement]);
    assert!(
        matches!(result, Err(ref error) if error.code() == WriteRejectionCode::ContractUnverified)
    );
    writer
        .logout()
        .await
        .unwrap_or_else(|_| panic!("application logout failed"));
    for session in [admin, method_denied, outsider] {
        assert_eq!(session.call("user.logout", json!([])).await, true);
    }
    let state = observations
        .lock()
        .unwrap_or_else(|_| panic!("observer state"));
    assert!(
        state.methods[application_start..]
            .iter()
            .all(|method| read_only_allowlist().contains(&method.as_str()))
    );
    assert_eq!(
        state.writes_forwarded,
        [
            "dashboard.create",
            "dashboard.create",
            "dashboard.update",
            "dashboard.update",
            "dashboard.update",
            "dashboard.update",
            "dashboard.create",
        ]
    );
    assert_eq!(state.blocked, 9);
    println!(
        "Z80-P5-03: version={VERSION}; role, editable, private create, scoped update, read-back, single-send reconciliation, non-dashboard dispatch and closed application writes verified"
    );
    server.abort();
}
