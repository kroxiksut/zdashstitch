// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-01A/B/C/E: endpoint normalization, sign-in verification, failure
//! codes, trust anchors and the read-only gateway allowlist, against a local
//! mock JSON-RPC server. No real server or credential is involved.

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use zdashstitch_zabbix::{
    AccessMode, Authentication, FailureCode, ReadAccess, ReadMethod, TrustAnchors,
    TrustMaterialError, ZabbixClient, ZabbixEndpoint, ZabbixError, dashboard_write_allowlist,
    read_only_allowlist,
};

/// Synthetic self-signed CA generated once for this test; its private key was
/// discarded at generation. It is not a trust root of any real system.
const SYNTHETIC_CA_PEM: &str = "subject=CN = ZDashstitch synthetic test CA
-----BEGIN CERTIFICATE-----
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
const SYNTHETIC_CA_SHA256: &str =
    "06a4c2cb5a0d29374c2d520a6e8b850a0fb0e5d3f873427069d17878e0295c90";

const ADMIN_TOKEN: &str = "admin-token-secret";
const SESSION: &str = "session-secret-value";

#[derive(Clone, Copy, PartialEq)]
enum Account {
    /// Super admin: every method, including writes, would succeed.
    Admin,
    /// `user.login` succeeds, every authenticated call is terminated (MFA).
    Mfa,
    /// `user.login` succeeds, the role has no API access.
    NoApiRole,
    /// Role without access to `template.get` only.
    NoTemplateRead,
    ExpiredToken,
    UnsupportedVersion,
}

type Calls = Arc<Mutex<Vec<(String, Option<String>, Value)>>>;

#[derive(Clone)]
struct Mock {
    account: Account,
    calls: Calls,
}

fn ok(id: &Value, result: Value) -> Response {
    Json(json!({"jsonrpc": "2.0", "result": result, "id": id})).into_response()
}

fn err(id: &Value, code: i64, message: &str, data: &str) -> Response {
    Json(json!({
        "jsonrpc": "2.0",
        "error": {"code": code, "message": message, "data": data},
        "id": id
    }))
    .into_response()
}

async fn rpc(State(mock): State<Mock>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let method = body["method"].as_str().unwrap_or_default().to_owned();
    mock.calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((method.clone(), authorization.clone(), body.clone()));
    let id = &body["id"];
    let bearer_ok = authorization.as_deref() == Some(&format!("Bearer {ADMIN_TOKEN}"))
        || authorization.as_deref() == Some(&format!("Bearer {SESSION}"));
    match method.as_str() {
        "apiinfo.version" if mock.account == Account::UnsupportedVersion => ok(id, json!("7.0.19")),
        "apiinfo.version" => ok(id, json!("7.4.16")),
        "user.login" => {
            if body["params"]["password"] == "right-password" {
                ok(id, json!(SESSION))
            } else {
                err(
                    id,
                    -32500,
                    "Application error.",
                    "Incorrect user name or password or account is temporarily blocked.",
                )
            }
        }
        "user.checkAuthentication" => {
            if authorization.is_some() {
                return err(
                    id,
                    -32602,
                    "Invalid params.",
                    "The \"user.checkAuthentication\" method must be called without authorization header.",
                );
            }
            if mock.account == Account::ExpiredToken {
                return err(id, -32500, "Application error.", "API token expired.");
            }
            if body["params"]["token"] == ADMIN_TOKEN || body["params"]["sessionid"] == SESSION {
                ok(
                    id,
                    json!({"userid": "1", "username": "Admin", "type": "3",
                           "sessionid": SESSION, "secret": "server-side-secret"}),
                )
            } else {
                err(id, -32602, "Invalid params.", "Not authorized.")
            }
        }
        _ if !bearer_ok => err(id, -32602, "Invalid params.", "Not authorized."),
        "user.logout" => ok(id, json!(true)),
        _ if mock.account == Account::Mfa => err(
            id,
            -32602,
            "Invalid params.",
            "Session terminated, re-login, please.",
        ),
        _ if mock.account == Account::NoApiRole
            || (mock.account == Account::NoTemplateRead && method == "template.get") =>
        {
            err(
                id,
                -32500,
                "Application error.",
                &format!("No permissions to call \"{method}\"."),
            )
        }
        "configuration.export" => ok(id, json!("")),
        _ => ok(id, json!([])),
    }
}

async fn redirect() -> Response {
    (
        StatusCode::MOVED_PERMANENTLY,
        [(header::LOCATION, "/elsewhere/")],
    )
        .into_response()
}

async fn html() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html")],
        "<html>login</html>",
    )
        .into_response()
}

async fn mock(account: Account) -> (String, Calls, JoinHandle<()>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/zabbix/api_jsonrpc.php", post(rpc))
        .route("/redirect/api_jsonrpc.php", post(redirect))
        .route("/html/api_jsonrpc.php", post(html))
        .with_state(Mock {
            account,
            calls: Arc::clone(&calls),
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("cannot bind mock API: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("cannot read mock address: {error}"));
    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            panic!("mock API failed: {error}");
        }
    });
    (format!("http://{address}"), calls, task)
}

fn client(url: &str) -> ZabbixClient {
    let endpoint =
        ZabbixEndpoint::parse(url).unwrap_or_else(|error| panic!("endpoint {url}: {error}"));
    ZabbixClient::new(endpoint).unwrap_or_else(|error| panic!("client: {error}"))
}

fn token(value: &str) -> Authentication {
    Authentication::ApiToken(SecretString::from(value.to_owned()))
}

fn password(value: &str) -> Authentication {
    Authentication::UsernamePassword {
        username: "Admin".to_owned(),
        password: SecretString::from(value.to_owned()),
    }
}

fn methods(calls: &Calls) -> Vec<String> {
    calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .map(|(method, _, _)| method.clone())
        .collect()
}

#[test]
fn address_variants_normalize_to_one_endpoint() {
    let canonical = "https://zabbix.example.invalid:8443/zabbix/api_jsonrpc.php";
    for input in [
        "https://zabbix.example.invalid:8443/zabbix",
        "https://zabbix.example.invalid:8443/zabbix/",
        "https://zabbix.example.invalid:8443/zabbix//",
        "HTTPS://Zabbix.Example.Invalid:8443/zabbix/api_jsonrpc.php",
        "  https://zabbix.example.invalid:8443/zabbix/api_jsonrpc.php/ ",
    ] {
        let endpoint = ZabbixEndpoint::parse(input)
            .unwrap_or_else(|error| panic!("{input} must be accepted: {error}"));
        assert_eq!(endpoint.api_url().as_str(), canonical, "{input}");
        assert_eq!(
            endpoint.base_url().as_str(),
            "https://zabbix.example.invalid:8443/zabbix/"
        );
    }
    let cases = [
        (
            "https://zabbix.example.invalid",
            "https://zabbix.example.invalid/api_jsonrpc.php",
        ),
        (
            "https://zabbix.example.invalid:443/",
            "https://zabbix.example.invalid/api_jsonrpc.php",
        ),
        (
            "http://192.0.2.10/zabbix",
            "http://192.0.2.10/zabbix/api_jsonrpc.php",
        ),
        (
            "http://192.0.2.10:80/zabbix",
            "http://192.0.2.10/zabbix/api_jsonrpc.php",
        ),
        (
            "http://192.0.2.10:8080",
            "http://192.0.2.10:8080/api_jsonrpc.php",
        ),
        (
            "https://[2001:DB8::10]:8443/",
            "https://[2001:db8::10]:8443/api_jsonrpc.php",
        ),
        (
            "https://[2001:db8::10]/monitoring/zabbix",
            "https://[2001:db8::10]/monitoring/zabbix/api_jsonrpc.php",
        ),
    ];
    for (input, api) in cases {
        let endpoint = ZabbixEndpoint::parse(input)
            .unwrap_or_else(|error| panic!("{input} must be accepted: {error}"));
        assert_eq!(endpoint.api_url().as_str(), api, "{input}");
    }
    let a = ZabbixEndpoint::parse("https://zabbix.example.invalid/zabbix")
        .unwrap_or_else(|error| panic!("{error}"));
    let b = ZabbixEndpoint::parse("https://zabbix.example.invalid/zabbix/api_jsonrpc.php")
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(a.fingerprint(), b.fingerprint());
    assert!(!a.is_plain_http());
    let other = ZabbixEndpoint::parse("https://zabbix.example.invalid:8443/zabbix")
        .unwrap_or_else(|error| panic!("{error}"));
    assert_ne!(a.fingerprint(), other.fingerprint());
}

#[test]
fn rejected_addresses_report_a_reason_without_echoing_input() {
    let cases = [
        ("zabbix.example.invalid", "syntax"),
        ("ftp://zabbix.example.invalid/zabbix", "scheme"),
        ("file:///tmp/api", "scheme"),
        ("https://user@zabbix.example.invalid/zabbix", "userinfo"),
        ("https://user:hunter2@zabbix.example.invalid/", "userinfo"),
        ("https://zabbix.example.invalid/zabbix?x=1", "query"),
        ("https://zabbix.example.invalid/zabbix#main", "fragment"),
    ];
    for (input, reason) in cases {
        let error = ZabbixEndpoint::parse(input)
            .err()
            .unwrap_or_else(|| panic!("{input} must be rejected"));
        let failure = error.failure();
        assert_eq!(failure.code, FailureCode::InvalidAddress, "{input}");
        assert_eq!(failure.param("reason"), Some(reason), "{input}");
        assert!(!error.to_string().contains("hunter2"));
        assert!(!format!("{error:?}").contains("hunter2"));
    }
}

#[tokio::test]
async fn api_token_is_verified_at_sign_in() {
    let (base, calls, task) = mock(Account::Admin).await;
    let gateway = client(&format!("{base}/zabbix"))
        .authenticate(token(ADMIN_TOKEN))
        .await
        .unwrap_or_else(|error| panic!("valid token: {error}"));
    assert_eq!(gateway.mode(), AccessMode::ReadOnly);
    assert_eq!(gateway.identity().version().to_string(), "7.4.16");
    assert_eq!(gateway.identity().fingerprint().len(), 64);
    assert!(!format!("{gateway:?}").contains(ADMIN_TOKEN));
    assert!(!format!("{gateway:?}").contains("server-side-secret"));
    assert_eq!(
        methods(&calls),
        ["apiinfo.version", "user.checkAuthentication"]
    );

    let error = client(&format!("{base}/zabbix"))
        .authenticate(token("wrong-token-secret"))
        .await
        .err()
        .unwrap_or_else(|| panic!("an unknown token must fail at sign-in"));
    let failure = error.failure();
    assert_eq!(failure.code, FailureCode::TokenNotAuthorized);
    assert_eq!(failure.param("auth_method"), Some("api_token"));
    assert!(error.is_authorization_failure());
    assert!(!error.is_permission_denied());
    assert!(!error.to_string().contains("wrong-token-secret"));
    assert!(!format!("{error:?}").contains("wrong-token-secret"));
    task.abort();
}

#[tokio::test]
async fn expired_token_has_its_own_code() {
    let (base, _calls, task) = mock(Account::ExpiredToken).await;
    let error = client(&format!("{base}/zabbix"))
        .authenticate(token(ADMIN_TOKEN))
        .await
        .err()
        .unwrap_or_else(|| panic!("expired token must fail"));
    assert_eq!(error.failure().code, FailureCode::TokenExpired);
    task.abort();
}

#[tokio::test]
async fn password_sign_in_probes_the_session() {
    let (base, calls, task) = mock(Account::Admin).await;
    let gateway = client(&format!("{base}/zabbix"))
        .authenticate(password("right-password"))
        .await
        .unwrap_or_else(|error| panic!("password sign-in: {error}"));
    gateway
        .logout()
        .await
        .unwrap_or_else(|error| panic!("logout: {error}"));
    assert_eq!(
        methods(&calls),
        [
            "apiinfo.version",
            "user.login",
            "user.get",
            "user.checkAuthentication",
            "user.logout"
        ]
    );

    let error = client(&format!("{base}/zabbix"))
        .authenticate(password("wrong-password"))
        .await
        .err()
        .unwrap_or_else(|| panic!("wrong password must fail"));
    let failure = error.failure();
    assert_eq!(failure.code, FailureCode::AuthFailed);
    assert_eq!(failure.param("auth_method"), Some("password"));
    assert!(!format!("{error:?}{error}").contains("wrong-password"));
    task.abort();
}

#[tokio::test]
async fn mfa_account_is_detected_by_the_probe_call() {
    let (base, calls, task) = mock(Account::Mfa).await;
    let error = client(&format!("{base}/zabbix"))
        .authenticate(password("right-password"))
        .await
        .err()
        .unwrap_or_else(|| panic!("MFA account must not sign in by password"));
    assert_eq!(error.failure().code, FailureCode::MfaSessionTerminated);
    assert!(!error.is_permission_denied());
    // The half-open session is closed on a best-effort basis.
    assert_eq!(
        methods(&calls).last().map(String::as_str),
        Some("user.logout")
    );
    task.abort();
}

#[tokio::test]
async fn role_without_api_access_is_a_permission_failure() {
    let (base, _calls, task) = mock(Account::NoApiRole).await;
    let error = client(&format!("{base}/zabbix"))
        .authenticate(password("right-password"))
        .await
        .err()
        .unwrap_or_else(|| panic!("a role without API access must fail"));
    let failure = error.failure();
    assert_eq!(failure.code, FailureCode::ApiMethodDenied);
    assert_eq!(failure.param("method"), Some("user.get"));
    task.abort();
}

#[tokio::test]
async fn unsupported_version_is_refused_before_credentials_are_sent() {
    let (base, calls, task) = mock(Account::UnsupportedVersion).await;
    let error = client(&format!("{base}/zabbix"))
        .authenticate(token(ADMIN_TOKEN))
        .await
        .err()
        .unwrap_or_else(|| panic!("7.0 must be refused"));
    let failure = error.failure();
    assert_eq!(failure.code, FailureCode::UnsupportedVersion);
    assert_eq!(failure.param("version"), Some("7.0.19"));
    assert_eq!(methods(&calls), ["apiinfo.version"]);
    task.abort();
}

#[tokio::test]
async fn reachability_failures_are_classified_by_code() {
    let (base, _calls, task) = mock(Account::Admin).await;
    let cases = [
        (
            format!("{base}/missing"),
            FailureCode::WrongBasePath,
            Some("404"),
        ),
        (
            format!("{base}/redirect"),
            FailureCode::Redirect,
            Some("301"),
        ),
        (format!("{base}/html"), FailureCode::NotJsonrpc, None),
    ];
    for (url, code, status) in cases {
        let error = client(&url)
            .detect_version()
            .await
            .err()
            .unwrap_or_else(|| panic!("{url} must fail"));
        let failure = error.failure();
        assert_eq!(failure.code, code, "{url}");
        assert_eq!(failure.param("http_status"), status, "{url}");
        assert_eq!(failure.code.default_step().as_str(), "api");
    }
    task.abort();

    let closed = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind: {error}"));
    let port = closed
        .local_addr()
        .unwrap_or_else(|error| panic!("address: {error}"))
        .port();
    drop(closed);
    let refused = client(&format!("http://127.0.0.1:{port}/zabbix"))
        .detect_version()
        .await
        .err()
        .unwrap_or_else(|| panic!("closed port must fail"));
    assert_eq!(refused.failure().code, FailureCode::ConnectionRefused);
    assert_eq!(refused.failure().code.default_step().as_str(), "endpoint");

    let dns = client("http://zdashstitch-p2-01.invalid/zabbix")
        .detect_version()
        .await
        .err()
        .unwrap_or_else(|| panic!(".invalid must not resolve"));
    assert_eq!(dns.failure().code, FailureCode::DnsUnresolvable);
}

#[tokio::test]
async fn read_only_gateway_with_admin_token_sends_no_write() {
    let (base, calls, task) = mock(Account::Admin).await;
    let gateway = client(&format!("{base}/zabbix"))
        .authenticate(token(ADMIN_TOKEN))
        .await
        .unwrap_or_else(|error| panic!("admin token: {error}"));
    // Exercise every public read path of the gateway.
    gateway
        .inventory()
        .await
        .unwrap_or_else(|error| panic!("inventory: {error}"));
    gateway
        .readable_item_ids(10)
        .await
        .unwrap_or_else(|error| panic!("item ids: {error}"));
    gateway
        .dashboard_snapshots("mock")
        .await
        .unwrap_or_else(|error| panic!("snapshots: {error}"));
    let access = gateway
        .probe_read_access()
        .await
        .unwrap_or_else(|error| panic!("probe: {error}"));
    assert!(
        access
            .iter()
            .all(|(_, state)| *state == ReadAccess::Available)
    );
    for method in ReadMethod::ALL {
        gateway
            .read(method, json!({"output": "extend"}))
            .await
            .unwrap_or_else(|error| panic!("{}: {error}", method.api_method()));
    }
    // Enabling and dropping writes without a verified plan sends nothing.
    let writer = gateway.enable_writes();
    assert_eq!(writer.mode(), AccessMode::ReadWrite);
    let gateway = writer.into_read_only();
    gateway
        .logout()
        .await
        .unwrap_or_else(|error| panic!("logout: {error}"));

    let allowlist = read_only_allowlist();
    let writes = dashboard_write_allowlist();
    let sent = methods(&calls);
    assert!(sent.len() > 20);
    for method in &sent {
        assert!(
            allowlist.contains(&method.as_str()),
            "{method} not allowlisted"
        );
        assert!(!writes.contains(&method.as_str()), "{method} is a write");
        assert!(!method.ends_with(".create") && !method.ends_with(".update"));
        assert!(!method.ends_with(".delete"));
    }
    task.abort();
}

#[tokio::test]
async fn capability_probe_reports_denied_methods() {
    let (base, _calls, task) = mock(Account::NoTemplateRead).await;
    let gateway = client(&format!("{base}/zabbix"))
        .authenticate(token(ADMIN_TOKEN))
        .await
        .unwrap_or_else(|error| panic!("token: {error}"));
    let access = gateway
        .probe_read_access()
        .await
        .unwrap_or_else(|error| panic!("probe: {error}"));
    assert_eq!(access.len(), ReadMethod::PROBED.len());
    for (method, state) in access {
        let expected = if method == ReadMethod::TemplateGet {
            ReadAccess::Denied
        } else {
            ReadAccess::Available
        };
        assert_eq!(state, expected, "{}", method.api_method());
    }
    task.abort();
}

#[test]
fn custom_ca_is_added_to_the_system_store_without_a_bypass() {
    let anchors = TrustAnchors::with_pem_bundle(SYNTHETIC_CA_PEM.as_bytes())
        .unwrap_or_else(|error| panic!("synthetic CA must be accepted: {error}"));
    assert!(!anchors.is_system_only());
    assert_eq!(anchors.certificate_fingerprints(), [SYNTHETIC_CA_SHA256]);
    let fingerprint = anchors
        .fingerprint()
        .unwrap_or_else(|| panic!("custom material has a fingerprint"));
    let canonical = TrustAnchors::with_pem_bundle(anchors.to_pem().as_bytes())
        .unwrap_or_else(|error| panic!("canonical PEM must round-trip: {error}"));
    assert_eq!(
        canonical.fingerprint().as_deref(),
        Some(fingerprint.as_str())
    );
    assert_eq!(format!("{anchors:?}"), "TrustAnchors { custom_roots: 1 }");
    assert!(TrustAnchors::system().fingerprint().is_none());

    let endpoint = ZabbixEndpoint::parse("https://zabbix.example.invalid/zabbix")
        .unwrap_or_else(|error| panic!("{error}"));
    ZabbixClient::with_trust(endpoint, &anchors)
        .unwrap_or_else(|error| panic!("client with custom CA: {error}"));

    let with_key =
        format!("{SYNTHETIC_CA_PEM}-----BEGIN PRIVATE KEY-----\nMAA=\n-----END PRIVATE KEY-----\n");
    assert!(matches!(
        TrustAnchors::with_pem_bundle(with_key.as_bytes()),
        Err(ZabbixError::TrustMaterial(TrustMaterialError::PrivateKey))
    ));
    let broken = SYNTHETIC_CA_PEM.replace("MIIBpjCC", "MIIBpjCD");
    let error = TrustAnchors::with_pem_bundle(broken.as_bytes())
        .err()
        .unwrap_or_else(|| panic!("a damaged certificate must be rejected"));
    assert_eq!(error.failure().code, FailureCode::TrustMaterialInvalid);
}

#[test]
fn transport_source_has_no_tls_bypass() {
    let source_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let entries = std::fs::read_dir(&source_dir).unwrap_or_else(|error| panic!("{error}"));
    let mut checked = 0;
    for entry in entries {
        let path = entry.unwrap_or_else(|error| panic!("{error}")).path();
        let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{error}"));
        for forbidden in [
            "danger_accept_invalid",
            "tls_danger",
            "dangerous()",
            "NoCertificateVerification",
        ] {
            assert!(
                !text.contains(forbidden),
                "{} uses {forbidden}",
                path.display()
            );
        }
        checked += 1;
    }
    assert!(checked >= 10);
}

#[test]
fn authentication_debug_is_redacted() {
    let rendered = format!(
        "{:?} {:?}",
        token("debug-token-secret"),
        password("debug-password-secret")
    );
    assert!(!rendered.contains("debug-token-secret"));
    assert!(!rendered.contains("debug-password-secret"));
    assert!(rendered.contains("REDACTED"));
}
