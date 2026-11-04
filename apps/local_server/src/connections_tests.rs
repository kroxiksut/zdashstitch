// SPDX-License-Identifier: MIT OR Apache-2.0

//! Connection routes over a mock Zabbix JSON-RPC server: the `/api`
//! security boundary, profile commands without secrets, and that no
//! response ever echoes a password, API token or Zabbix session.

use std::{
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use zdashstitch_application::connections::{
    ConnectRequest, ConnectionService, MemoryCredentialStore, SecretString,
};

use super::{ConnectionsHandle, MAX_SECRET_BYTES};
use crate::{CSRF_HEADER, LocalSecurity, build_router_with_connections};

const PORT: u16 = 43177;
const TOKEN: &str = "p201g-token-secret-value";
const PASSWORD: &str = "p201g-password-secret-value";
const SESSION: &str = "p201g-zabbix-session-value";
const SYNTHETIC_CA_PEM: &str = "-----BEGIN CERTIFICATE-----
MIIBpjCCAU2gAwIBAgIUHYjz75z1xUJ+aJ8xgQ/1eYTpzA4wCgYIKoZIzj0EAwIw
KDEmMCQGA1UEAwwdWkRhc2hzdGl0Y2ggc3ludGhldGljIHRlc3QgQ0EwIBcNMjYx
MDI1MTUwMjMyWhgPMjEyNjEwMDExNTAyMzJaMCgxJjAkBgNVBAMMHVpEYXNoc3Rp
dGNoIHN5bnRoZXRpYyB0ZXN0IENBMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE
o1ht3+sAD3r5lH0BJox5MTTuvPpN6rItwrCr0ADRQL9wbEwzy/BAXYmsQWz947NY
+kya6Cd8VRMvgScQ0HwolKNTMFEwHQYDVR0OBBYEFDbpViS+hDiJlkt0V6fm3HLN
s1DKMB8GA1UdIwQYMBaAFDbpViS+hDiJlkt0V6fm3HLNs1DKMA8GA1UdEwEB/wQF
MAMBAf8wCgYIKoZIzj0EAwIDRwAwRAIgBMgOJN5PUkdKRHZ42wIs9cCulS5/b7Ak
cj6JS3lkPVACIFKhh7tp4Y4bKC73af44iPc/ZnC2Kx5yZM2KgxIYmYyM
-----END CERTIFICATE-----
";

type Calls = Arc<StdMutex<Vec<String>>>;

#[derive(Clone)]
struct Mock {
    calls: Calls,
    /// Delay of the `user.logout` answer: a slow or unreachable server.
    logout_delay: Duration,
}

async fn rpc(State(mock): State<Mock>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let calls = mock.calls;
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let method = body["method"].as_str().unwrap_or_default().to_owned();
    calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(method.clone());
    if method == "user.logout" {
        tokio::time::sleep(mock.logout_delay).await;
    }
    let id = &body["id"];
    let ok = |result: Value| Json(json!({"jsonrpc": "2.0", "result": result, "id": id}));
    let err = |code: i64, data: &str| {
        Json(
            json!({"jsonrpc": "2.0", "error": {"code": code, "message": "Error.", "data": data}, "id": id}),
        )
    };
    let authorized = bearer.as_deref() == Some(&format!("Bearer {TOKEN}"))
        || bearer.as_deref() == Some(&format!("Bearer {SESSION}"));
    match method.as_str() {
        "apiinfo.version" => ok(json!("7.4.16")),
        "user.login" if body["params"]["password"] == PASSWORD => ok(json!(SESSION)),
        // The public default account of a fresh Zabbix installation (O-037).
        "user.login"
            if body["params"]["username"] == "Admin" && body["params"]["password"] == "zabbix" =>
        {
            ok(json!(SESSION))
        }
        "user.login" => err(
            -32500,
            "Incorrect user name or password or account is temporarily blocked.",
        ),
        "user.checkAuthentication"
            if body["params"]["token"] == TOKEN || body["params"]["sessionid"] == SESSION =>
        {
            ok(json!({"userid": "7", "username": "editor", "sessionid": SESSION}))
        }
        "user.checkAuthentication" => err(-32602, "Not authorized."),
        _ if !authorized => err(-32602, "Not authorized."),
        "user.logout" => ok(json!(true)),
        _ => ok(json!([])),
    }
    .into_response()
}

async fn zabbix() -> (String, Calls) {
    zabbix_with_logout_delay(Duration::ZERO).await
}

async fn zabbix_with_logout_delay(logout_delay: Duration) -> (String, Calls) {
    let calls = Calls::default();
    let app = Router::new()
        .route("/zabbix/api_jsonrpc.php", post(rpc))
        .with_state(Mock {
            calls: Arc::clone(&calls),
            logout_delay,
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("address: {error}"));
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            panic!("mock failed: {error}");
        }
    });
    (format!("http://{address}/zabbix"), calls)
}

struct App {
    router: Router,
    handle: ConnectionsHandle,
    security: LocalSecurity,
    cookie: String,
    csrf: String,
    _dir: tempfile::TempDir,
}

fn handle(dir: &tempfile::TempDir) -> ConnectionsHandle {
    ConnectionsHandle::new(ConnectionService::new(
        dir.path().join("config"),
        Box::new(MemoryCredentialStore::default()),
    ))
}

async fn app_with(handle: ConnectionsHandle, dir: tempfile::TempDir) -> App {
    let initial = LocalSecurity::initialize(PORT)
        .unwrap_or_else(|error| panic!("security must initialize: {error}"));
    let security = initial.security.clone();
    let router = build_router_with_connections("missing-ui", initial.security, handle.clone());
    let mut app = App {
        router,
        handle,
        security,
        cookie: String::new(),
        csrf: String::new(),
        _dir: dir,
    };
    app.enter(&initial.code).await;
    app
}

async fn app() -> App {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"));
    let handle = handle(&dir);
    app_with(handle, dir).await
}

impl App {
    /// Exchanges a bootstrap code and takes over its cookie and CSRF token.
    async fn enter(&mut self, code: &str) {
        let response = self
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/?bootstrap={code}"))
                    .header(header::HOST, format!("127.0.0.1:{PORT}"))
                    .body(Body::empty())
                    .unwrap_or_else(|error| panic!("request: {error}")),
            )
            .await
            .unwrap_or_else(|error| panic!("router: {error}"));
        self.cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .unwrap_or_else(|| panic!("bootstrap must set a cookie"))
            .to_owned();
        let (status, session) = self.call("GET", "/api/session", None).await;
        assert_eq!(status, StatusCode::OK);
        self.csrf = session["csrf_token"]
            .as_str()
            .unwrap_or_else(|| panic!("CSRF token"))
            .to_owned();
    }

    fn request(&self, method: &str, uri: &str, body: Option<&str>) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, format!("127.0.0.1:{PORT}"))
            .header(header::ORIGIN, format!("http://127.0.0.1:{PORT}"))
            .header(header::COOKIE, &self.cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .header(CSRF_HEADER, &self.csrf)
            .body(body.map_or_else(Body::empty, |body| Body::from(body.to_owned())))
            .unwrap_or_else(|error| panic!("request: {error}"))
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, String) {
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .unwrap_or_else(|error| panic!("router: {error}"));
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|error| panic!("body: {error}"))
            .to_bytes();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn call(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let raw = body.map(|body| body.to_string());
        let (status, text) = self.send(self.request(method, uri, raw.as_deref())).await;
        assert_no_secret(&text);
        let value = serde_json::from_str(&text).unwrap_or(Value::Null);
        (status, value)
    }
}

fn assert_no_secret(text: &str) {
    for secret in [TOKEN, PASSWORD, SESSION] {
        assert!(!text.contains(secret), "response echoes a secret: {text}");
    }
}

fn token_profile(address: &str, store_token: bool) -> Value {
    json!({
        "name": "Lab",
        "address": address,
        "auth": {"method": "api_token", "store_token": store_token},
        "trust": {"kind": "system"},
        "confirm_plain_http_token_storage": true
    })
}

fn password_profile(address: &str) -> Value {
    json!({
        "name": "Prod",
        "address": address,
        "auth": {"method": "password", "username": "editor"},
        "trust": {"kind": "system"}
    })
}

fn id_of(profile: &Value) -> String {
    profile["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("profile id: {profile}"))
        .to_owned()
}

fn assert_rfc3339_utc(value: &Value) {
    let text = value.as_str().unwrap_or_default();
    assert!(text.ends_with('Z'), "not a UTC timestamp: {value}");
    assert!(
        time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339).is_ok(),
        "not RFC 3339: {value}"
    );
}

#[test]
fn unix_seconds_become_rfc3339_utc() {
    assert_eq!(
        super::rfc3339_utc(1_793_000_000).as_deref(),
        Some("2026-10-26T07:33:20Z")
    );
    assert_eq!(
        super::rfc3339_utc(0).as_deref(),
        Some("1970-01-01T00:00:00Z")
    );
    assert_eq!(super::rfc3339_utc(u64::MAX), None);
}

#[tokio::test]
async fn routes_stay_behind_session_origin_csrf_and_content_type() {
    let app = app().await;
    let body = token_profile("https://zabbix.example.invalid/", false).to_string();

    let mut no_cookie = app.request("GET", "/api/connections", None);
    no_cookie.headers_mut().remove(header::COOKIE);
    assert_eq!(app.send(no_cookie).await.0, StatusCode::UNAUTHORIZED);

    let mut foreign = app.request("POST", "/api/connections", Some(&body));
    foreign.headers_mut().insert(
        header::ORIGIN,
        HeaderValue::from_static("http://attacker.example.invalid"),
    );
    assert_eq!(app.send(foreign).await.0, StatusCode::FORBIDDEN);

    let mut no_csrf = app.request("POST", "/api/connections", Some(&body));
    no_csrf.headers_mut().remove(CSRF_HEADER);
    assert_eq!(app.send(no_csrf).await.0, StatusCode::FORBIDDEN);

    let mut form = app.request("POST", "/api/connections", Some(&body));
    form.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-www-form-urlencoded"),
    );
    assert_eq!(app.send(form).await.0, StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let mut connect_without_csrf = app.request(
        "POST",
        "/api/connections/conn-00000000000000000000000000000000/connect",
        Some(&json!({"secret": TOKEN}).to_string()),
    );
    connect_without_csrf.headers_mut().remove(CSRF_HEADER);
    let (status, text) = app.send(connect_without_csrf).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_no_secret(&text);

    // Nothing was stored by the rejected requests.
    let (status, list) = app.call("GET", "/api/connections", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["profiles"], json!([]));
    assert_eq!(list["active"], Value::Null);
}

#[tokio::test]
async fn unavailable_store_answers_with_a_code() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"));
    let app = app_with(ConnectionsHandle::unavailable(), dir).await;
    let (status, body) = app.call("GET", "/api/connections", None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        body,
        json!({"code": "connections_unavailable", "params": {}})
    );
}

#[tokio::test]
async fn address_preview_normalizes_and_classifies() {
    let app = app().await;
    let (status, body) = app
        .call(
            "POST",
            "/api/connections/address-preview",
            Some(json!({"address": "HTTPS://Zabbix.Example.Invalid:443/zabbix"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["base_url"], "https://zabbix.example.invalid/zabbix/");
    assert_eq!(
        body["api_url"],
        "https://zabbix.example.invalid/zabbix/api_jsonrpc.php"
    );
    assert_eq!(body["plain_http"], false);

    let (status, body) = app
        .call(
            "POST",
            "/api/connections/address-preview",
            Some(json!({"address": "https://user@zabbix.example.invalid/"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "invalid_address");
    assert_eq!(body["params"]["reason"], "userinfo");
}

#[tokio::test]
async fn profile_commands_never_accept_or_return_secrets() {
    let app = app().await;
    // An unknown secret field in a profile is refused before anything is
    // stored; only `secret` with storage chosen is accepted (O-037).
    let mut with_secret = password_profile("https://zabbix.example.invalid/");
    with_secret["password"] = json!(PASSWORD);
    let (status, body) = app
        .call("POST", "/api/connections", Some(with_secret))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, json!({"code": "request_invalid", "params": {}}));

    // A type error must not quote the offending value.
    let (status, text) = app
        .send(app.request(
            "POST",
            "/api/connections/conn-00000000000000000000000000000000/connect",
            Some(&json!({"accept_new_identity": PASSWORD}).to_string()),
        ))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_no_secret(&text);

    let (status, created) = app
        .call(
            "POST",
            "/api/connections",
            Some(password_profile(
                "https://zabbix.example.invalid:8443/zabbix",
            )),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = id_of(&created);
    assert_eq!(
        created["endpoint_display"],
        "https://zabbix.example.invalid:8443/zabbix/"
    );
    assert_eq!(created["auth_method"], "password");
    assert_eq!(created["secret_storage"], "none");
    assert_eq!(created["username"], "editor");
    assert_eq!(created["state"], "not_connected");
    assert!(created.get("credential_ref").is_none());

    let (status, invalid) = app
        .call(
            "POST",
            "/api/connections",
            Some(json!({
                "name": "  ",
                "address": "https://zabbix.example.invalid/",
                "auth": {"method": "api_token", "store_token": false},
                "trust": {"kind": "system"}
            })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(invalid["code"], "profile_name_invalid");
    assert_eq!(invalid["params"]["max_chars"], "128");

    let mut renamed = password_profile("https://zabbix.example.invalid:8443/zabbix");
    renamed["name"] = json!("Prod renamed");
    let (status, updated) = app
        .call("PUT", &format!("/api/connections/{id}"), Some(renamed))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["context_changed"], false);
    assert_eq!(updated["profile"]["name"], "Prod renamed");

    let (status, moved) = app
        .call(
            "PUT",
            &format!("/api/connections/{id}"),
            Some(password_profile("https://zabbix-new.example.invalid/")),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(moved["context_changed"], true);

    let (status, copy) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/duplicate"),
            Some(json!({"name": "Prod copy"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let copy_id = id_of(&copy);
    assert_ne!(copy_id, id);

    let (status, missing_secret) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/connect"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(missing_secret["code"], "secret_required");
    assert_eq!(missing_secret["params"]["auth_method"], "password");

    let (status, deleted) = app
        .call("DELETE", &format!("/api/connections/{copy_id}"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(deleted["local_credential"], Value::Null);
    let (status, gone) = app
        .call("GET", &format!("/api/connections/{copy_id}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(gone["code"], "profile_not_found");

    let (_, list) = app.call("GET", "/api/connections", None).await;
    assert_eq!(list["profiles"].as_array().map(Vec::len), Some(1));
}

#[tokio::test]
async fn token_connection_modes_and_disconnect_without_echoing_the_token() {
    let (address, calls) = zabbix().await;
    let app = app().await;
    let (_, created) = app
        .call(
            "POST",
            "/api/connections",
            Some(token_profile(&address, true)),
        )
        .await;
    let id = id_of(&created);
    assert_eq!(created["plain_http"], true);
    assert_eq!(created["secret_storage"], "os_store");

    // No stored token yet: the UI asks for it (sign-in dialog).
    let (status, missing) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/connect"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(missing["code"], "credential_missing");

    let (status, test) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/test"),
            Some(json!({"secret": TOKEN})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(test["report"]["passed"], true);
    assert_eq!(test["report"]["steps"].as_array().map(Vec::len), Some(5));
    assert_eq!(test["report"]["steps"][1]["params"]["version"], "7.4.16");
    assert_eq!(test["profile"]["state"], "not_connected");
    // Timestamps travel as RFC 3339 UTC strings (O-035), never as formatted dates.
    assert_rfc3339_utc(&test["report"]["checked_at"]);
    assert_rfc3339_utc(&test["profile"]["last_check"]["at"]);

    let (status, connected) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/connect"),
            Some(json!({"secret": TOKEN})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(connected["connected"], true);
    assert_eq!(connected["credential_save"], Value::Null);
    assert_eq!(connected["profile"]["state"], "connected");
    assert_eq!(connected["profile"]["mode"], "read_only");

    let (_, list) = app.call("GET", "/api/connections", None).await;
    assert_eq!(list["active"]["local_id"], id.as_str());
    assert_eq!(list["active"]["mode"], "read_only");

    let (status, refused) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/enable-writes"),
            Some(json!({"confirmed": false})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused["code"], "writes_not_confirmed");
    let (status, writes) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/enable-writes"),
            Some(json!({"confirmed": true})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(writes["mode"], "read_write");
    let (_, read_only) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/read-only"),
            Some(json!({})),
        )
        .await;
    assert_eq!(read_only["mode"], "read_only");

    let (status, disconnected) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/disconnect"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(disconnected["profile"]["state"], "not_connected");
    let (status, again) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/disconnect"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(again["code"], "not_connected");

    // The token was stored on connect: no dialog the second time.
    let (status, reconnected) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/connect"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reconnected["connected"], true);
    let (status, forgotten) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/forget-secret"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(forgotten["done"], true);

    let methods = calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert!(
        methods
            .iter()
            .all(|method| !method.ends_with(".create") && !method.ends_with(".update"))
    );
}

#[tokio::test]
async fn rejected_password_counts_failures_and_is_not_echoed() {
    let (address, _) = zabbix().await;
    let app = app().await;
    let (_, created) = app
        .call("POST", "/api/connections", Some(password_profile(&address)))
        .await;
    let id = id_of(&created);
    for attempt in 1..=2 {
        let (status, outcome) = app
            .call(
                "POST",
                &format!("/api/connections/{id}/connect"),
                Some(json!({"secret": "wrong-password-value"})),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(outcome["connected"], false);
        let auth = &outcome["report"]["steps"][2];
        assert_eq!(auth["step"], "auth");
        assert_eq!(auth["status"], "failed");
        assert_eq!(auth["code"], "auth_failed");
        assert_eq!(auth["params"]["consecutive_failures"], attempt.to_string());
        assert_eq!(outcome["report"]["steps"][3]["status"], "not_checked");
        assert_eq!(
            outcome["report"]["steps"][3]["code"],
            "previous_step_failed"
        );
        assert_eq!(outcome["profile"]["state"], "needs_sign_in");
    }
    let (status, outcome) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/connect"),
            Some(json!({"secret": PASSWORD})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outcome["connected"], true);
    assert_eq!(outcome["report"]["identity"], "identity_first_seen");

    let oversized = "x".repeat(MAX_SECRET_BYTES + 1);
    let (status, too_long) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/test"),
            Some(json!({"secret": oversized})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(too_long["code"], "secret_invalid");
}

#[tokio::test]
async fn trusted_ca_is_imported_from_one_uploaded_file() {
    let app = app().await;
    let (status, set) = app.call("POST", "/api/upload-sets", Some(json!({}))).await;
    assert_eq!(status, StatusCode::CREATED);
    let set_id = set["id"].as_str().unwrap_or_default().to_owned();
    let mut upload = app.request(
        "POST",
        &format!("/api/uploads/{set_id}/files/ca.pem"),
        Some(SYNTHETIC_CA_PEM),
    );
    upload.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    assert_eq!(app.send(upload).await.0, StatusCode::CREATED);

    let (status, trust) = app
        .call(
            "POST",
            "/api/connections/trusted-ca",
            Some(json!({"upload_set_id": set_id})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let ca_ref = trust["ca_ref"].as_str().unwrap_or_default().to_owned();
    assert!(ca_ref.starts_with("sha256-"));
    assert_eq!(
        trust["certificate_fingerprints"].as_array().map(Vec::len),
        Some(1)
    );

    let mut profile = password_profile("https://zabbix.example.invalid/");
    profile["trust"] = json!({"kind": "custom_ca", "ca_ref": ca_ref});
    let (status, created) = app.call("POST", "/api/connections", Some(profile)).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["trust"], "custom_ca");
    assert_eq!(created["ca_fingerprints"].as_array().map(Vec::len), Some(1));

    // The upload set was consumed.
    let (status, _) = app
        .call(
            "POST",
            "/api/connections/trusted-ca",
            Some(json!({"upload_set_id": set_id})),
        )
        .await;
    assert_ne!(status, StatusCode::CREATED);
}

const END: &str = "/api/session/end";

impl App {
    fn service(&self) -> Arc<tokio::sync::Mutex<ConnectionService>> {
        self.handle
            .service()
            .unwrap_or_else(|| panic!("the connection service must be available"))
    }

    /// Creates a profile and connects it with `secret`.
    async fn connected(&self, profile: Value, secret: &str) -> String {
        let (status, created) = self.call("POST", "/api/connections", Some(profile)).await;
        assert_eq!(status, StatusCode::CREATED);
        let id = id_of(&created);
        let (status, outcome) = self
            .call(
                "POST",
                &format!("/api/connections/{id}/connect"),
                Some(json!({"secret": secret})),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(outcome["connected"], true);
        assert!(self.service().lock().await.holds_session_secrets());
        id
    }

    /// Waits until the erase task of `session.end` emptied the service.
    async fn secrets_erased(&self) {
        let service = self.service();
        tokio::time::timeout(Duration::from_secs(5), async {
            while service.lock().await.holds_session_secrets() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("the session secrets must be erased"));
    }
}

/// Finding: the end-session handler awaited the service mutex (held by a
/// connection command across network calls) before starting the erase, so
/// a client that disconnected meanwhile cancelled the erase and a secret
/// stored by the finishing command stayed in memory of a dead session.
#[tokio::test]
async fn end_session_erases_even_when_the_request_is_cancelled_while_the_service_is_busy() {
    let (address, _) = zabbix().await;
    let app = app().await;
    let id = app.connected(token_profile(&address, false), TOKEN).await;
    let service = app.service();

    // A connection command holds the service.
    let mut busy = service.lock().await;
    // The browser sends «End session» and drops the request.
    let end = app
        .router
        .clone()
        .oneshot(app.request("POST", END, Some(r#"{"confirmed":true}"#)));
    let _ = tokio::time::timeout(Duration::from_millis(100), end).await;
    // The command finishes after the end and stores a secret.
    let outcome = busy
        .connect(
            &id,
            ConnectRequest {
                secret: Some(SecretString::from(TOKEN.to_owned())),
                accept_new_identity: false,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("connect: {error}"));
    assert!(outcome.connected);
    assert!(busy.holds_session_secrets());
    drop(busy);

    app.secrets_erased().await;
    let (status, body) = app.call("GET", "/api/connections", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "http.security.session_invalid");
}

/// Finding: the end-session response waited for `user.logout` (up to the
/// transport timeout). The secret now leaves the service at once and the
/// logout is a bounded background attempt.
#[tokio::test]
async fn end_session_answers_promptly_while_the_logout_target_hangs() {
    let (address, calls) = zabbix_with_logout_delay(Duration::from_secs(120)).await;
    let app = app().await;
    let _id = app.connected(password_profile(&address), PASSWORD).await;

    let (status, _) = tokio::time::timeout(
        Duration::from_secs(3),
        app.call("POST", END, Some(json!({"confirmed": true}))),
    )
    .await
    .unwrap_or_else(|_| panic!("end session must not wait for user.logout"));
    assert_eq!(status, StatusCode::NO_CONTENT);
    app.secrets_erased().await;

    // The best-effort logout was attempted in the background.
    tokio::time::timeout(Duration::from_secs(5), async {
        while !calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .any(|method| method == "user.logout")
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("user.logout must be attempted"));
}

/// O-037 over HTTP: the password from the profile form is accepted only
/// together with «store», is never echoed, connects without a dialog and
/// can be forgotten; the default-password warning is a flag in the report
/// and «do not show again» is a non-secret profile flag.
#[tokio::test]
async fn stored_password_from_the_form_and_default_password_warning() {
    let (address, _) = zabbix().await;
    let app = app().await;
    // «Store» off: the secret is kept in process memory only and connects
    // without the dialog; «End session» erases it.
    let mut unsaved = password_profile(&address);
    unsaved["secret"] = json!(PASSWORD);
    let (status, memory) = app.call("POST", "/api/connections", Some(unsaved)).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(memory["secret_storage"], "none");
    assert_eq!(memory["secret_in_memory"], true);
    let memory_id = id_of(&memory);
    let (status, connected) = app
        .call(
            "POST",
            &format!("/api/connections/{memory_id}/connect"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(connected["connected"], true);
    let (status, _) = app
        .call(
            "POST",
            &format!("/api/connections/{memory_id}/disconnect"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, after) = app
        .call("GET", &format!("/api/connections/{memory_id}"), None)
        .await;
    assert_eq!(after["secret_in_memory"], false);

    let mut unconfirmed = password_profile(&address);
    unconfirmed["auth"]["store_password"] = json!(true);
    unconfirmed["secret"] = json!(PASSWORD);
    let (status, refused) = app
        .call("POST", "/api/connections", Some(unconfirmed.clone()))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused["code"], "plain_http_token_storage_unconfirmed");

    let mut stored = unconfirmed;
    stored["confirm_plain_http_token_storage"] = json!(true);
    let (status, created) = app.call("POST", "/api/connections", Some(stored)).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["auth_method"], "password");
    assert_eq!(created["secret_storage"], "os_store");
    assert_eq!(created["default_password_warning_hidden"], false);
    let id = id_of(&created);

    let (status, connected) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/connect"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(connected["connected"], true);
    assert_eq!(connected["report"]["default_password_warning"], false);
    let (status, _) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/disconnect"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, forgotten) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/forget-secret"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(forgotten["done"], true);
    let (status, missing) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/connect"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(missing["code"], "credential_missing");

    // Sign-in with Admin / zabbix: only a flag travels back, never a value.
    let mut default_account = password_profile(&address);
    default_account["auth"]["username"] = json!("Admin");
    let (_, local) = app
        .call("POST", "/api/connections", Some(default_account))
        .await;
    let local_id = id_of(&local);
    let (status, tested) = app
        .call(
            "POST",
            &format!("/api/connections/{local_id}/test"),
            Some(json!({"secret": "zabbix"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(tested["report"]["passed"], true);
    assert_eq!(tested["report"]["default_password_warning"], true);

    // «Do not show again for this connection» is a flag in the profile.
    let (status, hidden) = app
        .call(
            "POST",
            &format!("/api/connections/{id}/hide-default-password-warning"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hidden["default_password_warning_hidden"], true);
    assert_eq!(hidden["local_id"], id.as_str());
}

/// Finding: a path segment that is not valid UTF-8 got axum's plain-text
/// rejection instead of the routes' JSON code.
#[tokio::test]
async fn invalid_profile_id_segment_answers_with_a_json_code() {
    let app = app().await;
    for (method, uri) in [
        ("GET", "/api/connections/%FF"),
        ("PUT", "/api/connections/%FF"),
        ("DELETE", "/api/connections/%FF"),
        ("POST", "/api/connections/%FF/connect"),
        ("POST", "/api/connections/%FF/forget-secret"),
        ("POST", "/api/connections/%FF/hide-default-password-warning"),
    ] {
        let body = (method != "GET").then(|| json!({}));
        let (status, value) = app.call(method, uri, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {uri}");
        assert_eq!(
            value,
            json!({"code": "request_invalid", "params": {}}),
            "{method} {uri}"
        );
    }
}

/// P2-SEC-01F acceptance: after «End session» a repeated launch opens a new
/// session in the same process over the single-instance channel; the old
/// cookie stays rejected and the new session sees no secret of the old one.
#[tokio::test]
async fn a_repeated_launch_after_end_session_sees_no_previous_secret() {
    use crate::instance::{self, Acquired, InstanceConfig};

    let (address, _) = zabbix().await;
    let mut app = app().await;
    let _id = app.connected(token_profile(&address, false), TOKEN).await;
    let (status, list) = app.call("GET", "/api/connections", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(list["active"].is_object());

    let dir = InstanceConfig::private_temp_dir();
    let config = InstanceConfig::for_test(dir.path());
    let Ok(Acquired::Primary(primary)) = instance::acquire(&config) else {
        panic!("the test directory must give the primary lock");
    };
    let _server = primary
        .serve(app.security.clone(), &config)
        .unwrap_or_else(|error| panic!("serve: {error}"));
    // A second launch in the same directory does not become a server.
    assert!(matches!(instance::acquire(&config), Ok(Acquired::Running)));

    let (status, _) = app
        .call("POST", END, Some(json!({"confirmed": true})))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let old_cookie = app.cookie.clone();

    let url = instance::request_bootstrap(&config)
        .await
        .unwrap_or_else(|error| panic!("bootstrap: {error}"));
    // The code is issued only after the erase: nothing is left to see.
    assert!(!app.service().lock().await.holds_session_secrets());
    let code = url
        .split_once("?bootstrap=")
        .map(|(_, code)| code.to_owned())
        .unwrap_or_else(|| panic!("bootstrap URL"));
    app.enter(&code).await;
    assert_ne!(app.cookie, old_cookie);

    let (status, list) = app.call("GET", "/api/connections", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(list["active"].is_null(), "{list}");
    assert_eq!(list["profiles"].as_array().map(Vec::len), Some(1));

    let mut stale = app.request("GET", "/api/connections", None);
    stale.headers_mut().insert(
        header::COOKIE,
        HeaderValue::from_str(&old_cookie).unwrap_or_else(|error| panic!("cookie: {error}")),
    );
    let (status, body) = app.send(stale).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("http.security.session_invalid"));
}
