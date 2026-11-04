// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-01A–F, H at the application boundary: profile repository without
//! secrets, credential store choices, the five-step check as data, identity
//! changes and access modes. Uses a local mock JSON-RPC server only.

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
    routing::post,
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use zdashstitch_application::connections::{
    AuthInput, ConnectRequest, ConnectionService, ConnectionState, CredentialState,
    CredentialStore, MemoryCredentialStore, ProfileInput, SecretStorage, StepStatus, TrustInput,
    TrustSetting,
};
use zdashstitch_zabbix::{AccessMode, AuthMethod, CheckStep};

const TOKEN: &str = "profile-token-secret";
const PASSWORD: &str = "profile-password-secret";
const SESSION: &str = "profile-session-secret";
const DEFAULT_USERNAME: &str = "Admin";
const DEFAULT_PASSWORD: &str = "zabbix";

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

#[derive(Default)]
struct ServerState {
    userid: String,
    version: String,
    methods: Vec<String>,
}

type Shared = Arc<Mutex<ServerState>>;

fn lock(state: &Shared) -> std::sync::MutexGuard<'_, ServerState> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

async fn rpc(State(state): State<Shared>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let method = body["method"].as_str().unwrap_or_default().to_owned();
    let (userid, version) = {
        let mut state = lock(&state);
        state.methods.push(method.clone());
        (state.userid.clone(), state.version.clone())
    };
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
        "apiinfo.version" => ok(json!(version)),
        "user.login" if body["params"]["password"] == PASSWORD => ok(json!(SESSION)),
        // The public default account of a fresh Zabbix installation (O-037).
        "user.login"
            if body["params"]["username"] == DEFAULT_USERNAME
                && body["params"]["password"] == DEFAULT_PASSWORD =>
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
            ok(json!({"userid": userid, "username": "editor", "sessionid": SESSION}))
        }
        "user.checkAuthentication" => err(-32602, "Not authorized."),
        _ if !authorized => err(-32602, "Not authorized."),
        "user.logout" => ok(json!(true)),
        _ => ok(json!([])),
    }
    .into_response()
}

async fn server() -> (String, Shared, JoinHandle<()>) {
    let state = Arc::new(Mutex::new(ServerState {
        userid: "7".to_owned(),
        version: "7.4.16".to_owned(),
        methods: Vec::new(),
    }));
    let app = Router::new()
        .route("/zabbix/api_jsonrpc.php", post(rpc))
        .with_state(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("address: {error}"));
    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            panic!("mock failed: {error}");
        }
    });
    (format!("http://{address}/zabbix"), state, task)
}

/// Credential store shared between the test and the service.
#[derive(Clone, Default)]
struct SharedStore(Arc<MemoryCredentialStore>);

impl CredentialStore for SharedStore {
    fn availability(&self) -> Result<(), CredentialState> {
        self.0.availability()
    }
    fn save(&self, reference: &str, secret: &SecretString) -> Result<(), CredentialState> {
        self.0.save(reference, secret)
    }
    fn load(&self, reference: &str) -> Result<SecretString, CredentialState> {
        self.0.load(reference)
    }
    fn delete(&self, reference: &str) -> Result<(), CredentialState> {
        self.0.delete(reference)
    }
}

fn service(dir: &tempfile::TempDir) -> (ConnectionService, SharedStore) {
    let store = SharedStore::default();
    (
        ConnectionService::new(dir.path().join("config"), Box::new(store.clone())),
        store,
    )
}

fn token_input(name: &str, address: &str, store_token: bool) -> ProfileInput {
    ProfileInput {
        name: name.to_owned(),
        address: address.to_owned(),
        auth: AuthInput::ApiToken { store_token },
        trust: TrustInput::System,
        confirm_plain_http_token_storage: true,
    }
}

fn password_input(name: &str, address: &str) -> ProfileInput {
    ProfileInput {
        name: name.to_owned(),
        address: address.to_owned(),
        auth: AuthInput::Password {
            username: "editor".to_owned(),
            store_password: false,
        },
        trust: TrustInput::System,
        confirm_plain_http_token_storage: false,
    }
}

fn stored_password_input(name: &str, address: &str, username: &str) -> ProfileInput {
    ProfileInput {
        name: name.to_owned(),
        address: address.to_owned(),
        auth: AuthInput::Password {
            username: username.to_owned(),
            store_password: true,
        },
        trust: TrustInput::System,
        confirm_plain_http_token_storage: true,
    }
}

fn secret(value: &str) -> Option<SecretString> {
    Some(SecretString::from(value.to_owned()))
}

fn profiles_yaml(dir: &tempfile::TempDir) -> String {
    std::fs::read_to_string(dir.path().join("config").join("connections.yaml"))
        .unwrap_or_else(|error| panic!("profiles file: {error}"))
}

fn credential_ref(dir: &tempfile::TempDir) -> String {
    let yaml = profiles_yaml(dir);
    yaml.lines()
        .find_map(|line| line.trim().strip_prefix("credential_ref: "))
        .map(str::to_owned)
        .unwrap_or_else(|| panic!("credential_ref must be stored"))
}

#[test]
fn profile_store_normalizes_address_and_keeps_no_secret() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let (mut connections, _) = service(&dir);
    let view = connections
        .create_profile(&token_input(
            "Prod",
            "HTTPS://Zabbix.Example.Invalid:443/zabbix/api_jsonrpc.php",
            false,
        ))
        .unwrap_or_else(|error| panic!("create: {error}"));
    assert_eq!(
        view.endpoint_display,
        "https://zabbix.example.invalid/zabbix/"
    );
    assert_eq!(
        view.api_url_display,
        "https://zabbix.example.invalid/zabbix/api_jsonrpc.php"
    );
    assert_eq!(view.auth_method, AuthMethod::ApiToken);
    assert_eq!(view.secret_storage, SecretStorage::None);
    assert_eq!(view.state, ConnectionState::NotConnected);
    assert_eq!(view.mode, None);
    assert!(view.local_id.starts_with("conn-"));

    let yaml = profiles_yaml(&dir);
    assert!(yaml.contains("format: zdashstitch-connections/v0"));
    assert!(yaml.contains("mode_on_connect: read_only"));
    for forbidden in ["password", "token:", "secret:"] {
        assert!(!yaml.contains(forbidden), "{forbidden} in {yaml}");
    }

    // P1-02B: a token for plain http needs an explicit confirmation.
    let mut unconfirmed = token_input("Lab", "http://192.0.2.10/zabbix", true);
    unconfirmed.confirm_plain_http_token_storage = false;
    let error = connections
        .create_profile(&unconfirmed)
        .err()
        .unwrap_or_else(|| panic!("http token storage must be confirmed"));
    assert_eq!(error.code(), "plain_http_token_storage_unconfirmed");

    let error = connections
        .create_profile(&token_input(
            "Bad",
            "https://user@zabbix.example.invalid",
            false,
        ))
        .err()
        .unwrap_or_else(|| panic!("userinfo must be rejected"));
    assert_eq!(error.code(), "invalid_address");
    assert_eq!(error.param("reason"), Some("userinfo"));
    let error = connections
        .create_profile(&token_input(" ", "https://zabbix.example.invalid", false))
        .err()
        .unwrap_or_else(|| panic!("blank name must be rejected"));
    assert_eq!(error.code(), "profile_name_invalid");

    // Reload from disk: the same single profile.
    let (reloaded, _) = service(&dir);
    let list = reloaded
        .list_profiles()
        .unwrap_or_else(|error| panic!("list: {error}"));
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].local_id, view.local_id);
}

#[test]
fn tampered_profile_file_is_rejected() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let (mut connections, _) = service(&dir);
    connections
        .create_profile(&token_input(
            "Prod",
            "https://zabbix.example.invalid",
            false,
        ))
        .unwrap_or_else(|error| panic!("create: {error}"));
    let path = dir.path().join("config").join("connections.yaml");
    let yaml = profiles_yaml(&dir).replace(
        "  mode_on_connect: read_only\n",
        "  mode_on_connect: read_only\n  api_token: leaked\n",
    );
    std::fs::write(&path, yaml).unwrap_or_else(|error| panic!("{error}"));
    let error = connections
        .list_profiles()
        .err()
        .unwrap_or_else(|| panic!("unknown fields must be rejected"));
    assert_eq!(error.code(), "profile_store_invalid");
}

/// P0-11: the profile file uses the strict YAML loader (no duplicate keys,
/// no anchors/aliases), and `trust: {kind: system}` rejects extra fields
/// despite serde-rs/serde#2294.
#[test]
fn profile_file_rejects_duplicate_keys_aliases_and_extra_trust_fields() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let (mut connections, _) = service(&dir);
    connections
        .create_profile(&token_input(
            "Prod",
            "https://zabbix.example.invalid",
            false,
        ))
        .unwrap_or_else(|error| panic!("create: {error}"));
    let path = dir.path().join("config").join("connections.yaml");
    let original = profiles_yaml(&dir);
    let trust_line = original
        .lines()
        .find(|line| line.trim() == "kind: system")
        .unwrap_or_else(|| panic!("trust kind must be stored: {original}"))
        .to_owned();
    let indent = &trust_line[..trust_line.len() - trust_line.trim_start().len()];
    let cases = [
        (
            "duplicate key",
            format!("{original}format: zdashstitch-connections/v0\n"),
        ),
        (
            "alias",
            original.replacen(
                "format: zdashstitch-connections/v0",
                "format: &format zdashstitch-connections/v0",
                1,
            ),
        ),
        (
            "extra trust field",
            original.replacen(
                &format!("{trust_line}\n"),
                &format!("{trust_line}\n{indent}ca_ref: ca-unexpected\n"),
                1,
            ),
        ),
    ];
    for (name, yaml) in cases {
        assert_ne!(yaml, original, "{name} must change the file");
        std::fs::write(&path, &yaml).unwrap_or_else(|error| panic!("{error}"));
        let error = connections
            .list_profiles()
            .err()
            .unwrap_or_else(|| panic!("{name} must be rejected"));
        assert_eq!(error.code(), "profile_store_invalid", "{name}");
    }
    std::fs::write(&path, &original).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        connections
            .list_profiles()
            .unwrap_or_else(|error| panic!("restored file: {error}"))
            .len(),
        1
    );

    assert_eq!(
        serde_json::from_str::<TrustSetting>(r#"{"kind":"system"}"#).ok(),
        Some(TrustSetting::System)
    );
    assert!(serde_json::from_str::<TrustSetting>(r#"{"kind":"system","ca_ref":"x"}"#).is_err());
    assert_eq!(
        serde_json::to_string(&TrustSetting::System).ok().as_deref(),
        Some(r#"{"kind":"system"}"#)
    );
}

#[tokio::test]
async fn token_check_reports_five_steps_and_stores_token_only_by_choice() {
    let (address, state, task) = server().await;
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let (mut connections, store) = service(&dir);
    let memory_only = connections
        .create_profile(&token_input("Memory", &address, false))
        .unwrap_or_else(|error| panic!("create: {error}"));
    let report = connections
        .test_connection(&memory_only.local_id, secret(TOKEN))
        .await
        .unwrap_or_else(|error| panic!("test: {error}"));
    let steps: Vec<(&str, StepStatus)> = report
        .steps
        .iter()
        .map(|step| (step.step.as_str(), step.status))
        .collect();
    assert_eq!(
        steps,
        [
            ("endpoint", StepStatus::Passed),
            ("api", StepStatus::Passed),
            ("auth", StepStatus::Passed),
            ("identity", StepStatus::Notice),
            ("capabilities", StepStatus::Passed),
        ]
    );
    assert!(report.passed());
    assert_eq!(report.server_version.as_deref(), Some("7.4.16"));
    assert_eq!(report.read_access.len(), 9);
    assert!(!format!("{report:?}").contains(TOKEN));
    // Not stored without the user's choice.
    assert!(!store.0.contains(&credential_ref(&dir)));
    // O-037: kept in process memory only, until it is forgotten or the
    // session ends.
    assert!(
        connections
            .profile(&memory_only.local_id)
            .unwrap_or_else(|error| panic!("{error}"))
            .secret_in_memory
    );
    connections
        .forget_secret(&memory_only.local_id)
        .unwrap_or_else(|error| panic!("forget: {error}"));
    let error = connections
        .connect(&memory_only.local_id, ConnectRequest::default())
        .await
        .err()
        .unwrap_or_else(|| panic!("a forgotten memory-only token must be entered again"));
    assert_eq!(error.code(), "secret_required");

    let stored = connections
        .create_profile(&token_input("Stored", &address, true))
        .unwrap_or_else(|error| panic!("create: {error}"));
    let outcome = connections
        .connect(
            &stored.local_id,
            ConnectRequest {
                secret: secret(TOKEN),
                accept_new_identity: false,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("connect: {error}"));
    assert!(outcome.connected);
    assert_eq!(outcome.credential_save, None);
    assert_eq!(
        connections.active().map(|(_, _, mode)| mode),
        Some(AccessMode::ReadOnly)
    );
    connections
        .disconnect()
        .await
        .unwrap_or_else(|error| panic!("disconnect: {error}"));
    // The stored token is used without a dialog.
    let outcome = connections
        .connect(&stored.local_id, ConnectRequest::default())
        .await
        .unwrap_or_else(|error| panic!("connect with stored token: {error}"));
    assert!(outcome.connected);
    assert_eq!(
        outcome.report.identity.map(|m| m.as_str()),
        Some("identity_matches")
    );
    connections
        .disconnect()
        .await
        .unwrap_or_else(|error| panic!("disconnect: {error}"));

    // Store states.
    store.0.fail_with(Some(CredentialState::Locked));
    let error = connections
        .connect(&stored.local_id, ConnectRequest::default())
        .await
        .err()
        .unwrap_or_else(|| panic!("a locked store must be reported"));
    assert_eq!(error.code(), "credential_locked");
    store.0.fail_with(Some(CredentialState::Unavailable));
    let error = connections
        .connect(&stored.local_id, ConnectRequest::default())
        .await
        .err()
        .unwrap_or_else(|| panic!("an unavailable store must be reported"));
    assert_eq!(error.code(), "credential_unavailable");
    store.0.fail_with(None);
    connections
        .forget_secret(&stored.local_id)
        .unwrap_or_else(|error| panic!("forget: {error}"));
    let error = connections
        .connect(&stored.local_id, ConnectRequest::default())
        .await
        .err()
        .unwrap_or_else(|| panic!("a forgotten token must be missing"));
    assert_eq!(error.code(), "credential_missing");

    let yaml = profiles_yaml(&dir);
    assert!(!yaml.contains(TOKEN));
    assert!(!yaml.contains(SESSION));
    assert!(
        lock(&state)
            .methods
            .iter()
            .all(|method| !method.ends_with(".update"))
    );
    task.abort();
}

#[tokio::test]
async fn failed_steps_stop_the_check_and_mark_later_steps_not_checked() {
    let (address, _state, task) = server().await;
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let (mut connections, _) = service(&dir);
    let profile = connections
        .create_profile(&password_input("Pwd", &address))
        .unwrap_or_else(|error| panic!("create: {error}"));
    for attempt in 1..=2 {
        let report = connections
            .test_connection(&profile.local_id, secret("wrong-password-value"))
            .await
            .unwrap_or_else(|error| panic!("test: {error}"));
        assert_eq!(report.failed_step(), Some(CheckStep::Auth));
        assert_eq!(report.result_code(), Some("auth_failed"));
        let auth = &report.steps[2];
        assert!(
            auth.params
                .iter()
                .any(|param| param.name == "auth_method" && param.value == "password")
        );
        assert!(auth.params.iter().any(
            |param| param.name == "consecutive_failures" && param.value == attempt.to_string()
        ));
        assert!(
            report.steps[3..]
                .iter()
                .all(|step| step.status == StepStatus::NotChecked)
        );
        assert!(!format!("{report:?}").contains("wrong-password-value"));
    }
    let view = connections
        .profile(&profile.local_id)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(view.state, ConnectionState::NeedsSignIn);
    assert_eq!(
        view.last_check
            .and_then(|check| check.result_code)
            .as_deref(),
        Some("auth_failed")
    );
    let error = connections
        .connect(&profile.local_id, ConnectRequest::default())
        .await
        .err()
        .unwrap_or_else(|| panic!("a password is always requested"));
    assert_eq!(error.code(), "secret_required");
    assert_eq!(error.param("auth_method"), Some("password"));
    task.abort();

    // Nothing listens any more: the endpoint step fails, the rest is skipped.
    let report = connections
        .test_connection(&profile.local_id, secret(PASSWORD))
        .await
        .unwrap_or_else(|error| panic!("test: {error}"));
    assert_eq!(report.failed_step(), Some(CheckStep::Endpoint));
    assert!(
        report.steps[1..]
            .iter()
            .all(|step| step.status == StepStatus::NotChecked
                && step.code == Some("previous_step_failed"))
    );
    let view = connections
        .profile(&profile.local_id)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(view.state, ConnectionState::LastCheckFailed);
}

#[tokio::test]
async fn identity_change_requires_confirmation_and_raises_generation() {
    let (address, state, task) = server().await;
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let (mut connections, _) = service(&dir);
    let profile = connections
        .create_profile(&password_input("Pwd", &address))
        .unwrap_or_else(|error| panic!("create: {error}"));
    let request = || ConnectRequest {
        secret: secret(PASSWORD),
        accept_new_identity: false,
    };
    let first = connections
        .connect(&profile.local_id, request())
        .await
        .unwrap_or_else(|error| panic!("connect: {error}"));
    assert!(first.connected);
    assert_eq!(
        first.report.identity.map(|m| m.as_str()),
        Some("identity_first_seen")
    );
    let disconnected = connections
        .disconnect()
        .await
        .unwrap_or_else(|error| panic!("disconnect: {error}"));
    assert_eq!(disconnected.logout, None);
    assert_eq!(
        lock(&state).methods.last().map(String::as_str),
        Some("user.logout")
    );

    // Upgrade only: a notice, same generation.
    lock(&state).version = "7.4.17".to_owned();
    let upgraded = connections
        .connect(&profile.local_id, request())
        .await
        .unwrap_or_else(|error| panic!("connect: {error}"));
    assert!(upgraded.connected);
    assert_eq!(
        upgraded.report.identity.map(|m| m.as_str()),
        Some("server_version_changed")
    );
    assert!(!upgraded.context_changed);
    let _ = connections.disconnect().await;

    // Another server answers at the same address.
    lock(&state).userid = "42".to_owned();
    let refused = connections
        .connect(&profile.local_id, request())
        .await
        .unwrap_or_else(|error| panic!("connect: {error}"));
    assert!(!refused.connected);
    assert_eq!(
        refused.report.result_code(),
        Some("server_identity_changed")
    );
    assert_eq!(refused.report.failed_step(), Some(CheckStep::Identity));
    assert!(connections.active().is_none());
    let accepted = connections
        .connect(
            &profile.local_id,
            ConnectRequest {
                secret: secret(PASSWORD),
                accept_new_identity: true,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("connect: {error}"));
    assert!(accepted.connected);
    assert!(accepted.context_changed);
    assert_eq!(
        connections.active().map(|(_, generation, _)| generation),
        Some(1)
    );
    task.abort();
}

#[tokio::test]
async fn writes_need_confirmation_and_return_to_read_only() {
    let (address, state, task) = server().await;
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let (mut connections, _) = service(&dir);
    let profile = connections
        .create_profile(&token_input("Prod", &address, false))
        .unwrap_or_else(|error| panic!("create: {error}"));
    let error = connections
        .enable_writes(&profile.local_id, true)
        .err()
        .unwrap_or_else(|| panic!("writes need a connection"));
    assert_eq!(error.code(), "not_connected");
    connections
        .connect(
            &profile.local_id,
            ConnectRequest {
                secret: secret(TOKEN),
                accept_new_identity: false,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("connect: {error}"));
    assert!(connections.reader().is_some());
    assert!(connections.writer().is_none());
    let error = connections
        .enable_writes(&profile.local_id, false)
        .err()
        .unwrap_or_else(|| panic!("writes need confirmation"));
    assert_eq!(error.code(), "writes_not_confirmed");
    assert_eq!(
        connections
            .enable_writes(&profile.local_id, true)
            .unwrap_or_else(|error| panic!("{error}")),
        AccessMode::ReadWrite
    );
    assert!(connections.writer().is_some());
    let view = connections
        .profile(&profile.local_id)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(view.mode, Some(AccessMode::ReadWrite));
    assert_eq!(view.state, ConnectionState::Connected);
    connections
        .set_read_only(&profile.local_id)
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(connections.writer().is_none());
    assert!(connections.reader().is_some());
    assert!(
        lock(&state)
            .methods
            .iter()
            .all(|method| { zdashstitch_zabbix::read_only_allowlist().contains(&method.as_str()) })
    );
    task.abort();
}

#[tokio::test]
async fn rename_keeps_identity_and_address_change_resets_context() {
    let (address, _state, task) = server().await;
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let (mut connections, store) = service(&dir);
    let profile = connections
        .create_profile(&token_input("Prod", &address, true))
        .unwrap_or_else(|error| panic!("create: {error}"));
    connections
        .connect(
            &profile.local_id,
            ConnectRequest {
                secret: secret(TOKEN),
                accept_new_identity: false,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("connect: {error}"));
    let reference = credential_ref(&dir);
    assert!(store.0.contains(&reference));

    let renamed = connections
        .update_profile(
            &profile.local_id,
            &token_input("Prod renamed", &address, true),
        )
        .await
        .unwrap_or_else(|error| panic!("rename: {error}"));
    assert!(!renamed.context_changed);
    assert_eq!(renamed.view.context_generation, 0);
    assert_eq!(renamed.view.state, ConnectionState::Connected);
    assert!(store.0.contains(&reference));

    // Duplicates copy neither the stored token nor the identity.
    let copy = connections
        .duplicate_profile(&profile.local_id, "Prod copy")
        .unwrap_or_else(|error| panic!("duplicate: {error}"));
    assert_ne!(copy.local_id, profile.local_id);
    assert_eq!(copy.endpoint_display, renamed.view.endpoint_display);
    let error = connections
        .connect(&copy.local_id, ConnectRequest::default())
        .await
        .err()
        .unwrap_or_else(|| panic!("the copy has no stored token"));
    assert_eq!(error.code(), "credential_missing");

    let moved = connections
        .update_profile(
            &profile.local_id,
            &token_input(
                "Prod renamed",
                "https://zabbix.example.invalid/zabbix",
                true,
            ),
        )
        .await
        .unwrap_or_else(|error| panic!("move: {error}"));
    assert!(moved.context_changed);
    assert_eq!(moved.view.context_generation, 1);
    assert_eq!(moved.view.state, ConnectionState::NotConnected);
    assert!(connections.active().is_none());
    // A token of the old server is never sent to the new one.
    assert!(!store.0.contains(&reference));

    let deleted = connections
        .delete_profile(&profile.local_id)
        .await
        .unwrap_or_else(|error| panic!("delete: {error}"));
    assert_eq!(deleted.local_credential, None);
    assert_eq!(
        connections
            .list_profiles()
            .unwrap_or_else(|error| panic!("{error}"))
            .len(),
        1
    );
    task.abort();
}

#[test]
fn trusted_ca_is_stored_by_reference_and_verified_on_load() {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let (mut connections, _) = service(&dir);
    let imported = connections
        .import_trusted_ca(SYNTHETIC_CA_PEM.as_bytes())
        .unwrap_or_else(|error| panic!("import: {error}"));
    assert!(imported.ca_ref.starts_with("sha256-"));
    assert_eq!(
        imported.certificate_fingerprints,
        ["06a4c2cb5a0d29374c2d520a6e8b850a0fb0e5d3f873427069d17878e0295c90"]
    );
    let mut input = token_input("Corp", "https://zabbix.example.invalid", false);
    input.trust = TrustInput::CustomCa {
        ca_ref: imported.ca_ref.clone(),
    };
    let view = connections
        .create_profile(&input)
        .unwrap_or_else(|error| panic!("create: {error}"));
    assert_eq!(view.trust, "custom_ca");
    assert_eq!(view.ca_fingerprints, imported.certificate_fingerprints);
    assert!(!profiles_yaml(&dir).contains("BEGIN CERTIFICATE"));

    let error = connections
        .import_trusted_ca(b"-----BEGIN PRIVATE KEY-----\nMAA=\n-----END PRIVATE KEY-----\n")
        .err()
        .unwrap_or_else(|| panic!("private keys must be rejected"));
    assert_eq!(error.code(), "trust_material_invalid");
    assert_eq!(error.param("reason"), Some("private_key"));

    let mut missing = input.clone();
    missing.trust = TrustInput::CustomCa {
        ca_ref: format!("sha256-{}", "0".repeat(64)),
    };
    let error = connections
        .create_profile(&missing)
        .err()
        .unwrap_or_else(|| panic!("unknown CA reference must be rejected"));
    assert_eq!(error.code(), "trust_material_missing");
}

#[test]
fn requests_and_services_do_not_print_secrets() {
    let request = ConnectRequest {
        secret: secret("debug-secret-value"),
        accept_new_identity: false,
    };
    assert!(!format!("{request:?}").contains("debug-secret-value"));
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let (connections, store) = service(&dir);
    store
        .save(
            "cred-x",
            &SecretString::from("stored-secret-value".to_owned()),
        )
        .unwrap_or_else(|state| panic!("{}", state.code()));
    assert!(!format!("{connections:?}").contains("stored-secret-value"));
    assert!(!format!("{:?}", store.0).contains("stored-secret-value"));
}

/// P2-SEC-01E: «End session» erases the in-memory secrets of the active
/// connection and keeps everything saved: profiles and the stored token.
#[tokio::test]
async fn ending_the_session_erases_memory_secrets_and_keeps_saved_data() {
    let (address, state, task) = server().await;
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let (mut connections, store) = service(&dir);
    let token_profile = connections
        .create_profile(&token_input("Prod", &address, true))
        .unwrap_or_else(|error| panic!("create: {error}"));
    let password_profile = connections
        .create_profile(&password_input("Pilot", &address))
        .unwrap_or_else(|error| panic!("create: {error}"));
    assert!(!connections.holds_session_secrets());

    for (profile, value) in [(&token_profile, TOKEN), (&password_profile, PASSWORD)] {
        let outcome = connections
            .connect(
                &profile.local_id,
                ConnectRequest {
                    secret: secret(value),
                    accept_new_identity: false,
                },
            )
            .await
            .unwrap_or_else(|error| panic!("connect: {error}"));
        assert!(outcome.connected);
        connections
            .enable_writes(&profile.local_id, true)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(connections.holds_session_secrets());
        let saved = profiles_yaml(&dir);

        // The secret leaves the service at once, before any network call.
        let closed = connections
            .erase_session_secrets()
            .unwrap_or_else(|| panic!("the active session must be taken"));
        assert_eq!(closed.connection_id(), profile.local_id);
        assert!(!format!("{closed:?}").contains(value));
        assert!(!connections.holds_session_secrets());
        assert!(connections.reader().is_none());
        assert!(connections.writer().is_none());
        assert!(connections.active().is_none());
        let view = connections
            .profile(&profile.local_id)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(view.mode, None);
        assert_ne!(view.state, ConnectionState::Connected);
        assert_eq!(profiles_yaml(&dir), saved);
        // Erasing again is harmless.
        assert!(connections.erase_session_secrets().is_none());
        // `user.logout` is the caller's best-effort step afterwards.
        assert_eq!(closed.logout().await, None);
    }
    // The token stored by the user's choice stays in the credential store.
    assert!(store.load(&credential_ref(&dir)).is_ok());
    // The password session was closed on the server.
    assert!(
        lock(&state)
            .methods
            .iter()
            .any(|method| method == "user.logout")
    );
    task.abort();
}

/// O-037: a password typed in the profile form is stored only by choice,
/// only in the credential store, and then connects without a dialog;
/// «Forget password» removes it; a profile file written before O-037 reads
/// as «not stored».
#[tokio::test]
async fn password_from_the_profile_form_is_stored_only_by_choice() {
    let (address, _state, task) = server().await;
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let (mut connections, store) = service(&dir);

    // Plain http: storing a password needs the same confirmation as a token.
    let mut unconfirmed = stored_password_input("Pilot", &address, "editor");
    unconfirmed.confirm_plain_http_token_storage = false;
    let error = connections
        .create_profile_with_secret(&unconfirmed, secret(PASSWORD))
        .err()
        .unwrap_or_else(|| panic!("http password storage must be confirmed"));
    assert_eq!(error.code(), "plain_http_token_storage_unconfirmed");

    let view = connections
        .create_profile_with_secret(
            &stored_password_input("Pilot", &address, "editor"),
            secret(PASSWORD),
        )
        .unwrap_or_else(|error| panic!("create: {error}"));
    assert_eq!(view.auth_method, AuthMethod::Password);
    assert_eq!(view.secret_storage, SecretStorage::OsStore);
    assert_eq!(view.username.as_deref(), Some("editor"));
    let reference = credential_ref(&dir);
    assert!(store.0.contains(&reference));
    let yaml = profiles_yaml(&dir);
    assert!(!yaml.contains(PASSWORD), "{yaml}");
    assert!(yaml.contains("secret_storage: os_store"), "{yaml}");
    assert!(!yaml.contains("default_password_warning_hidden"), "{yaml}");

    // The stored password connects without a secret in the request.
    let outcome = connections
        .connect(&view.local_id, ConnectRequest::default())
        .await
        .unwrap_or_else(|error| panic!("connect with stored password: {error}"));
    assert!(outcome.connected);
    assert!(!outcome.report.default_password_warning);
    connections
        .disconnect()
        .await
        .unwrap_or_else(|error| panic!("disconnect: {error}"));

    // Renaming keeps the context; a new password from the form replaces the
    // stored one.
    let update = connections
        .update_profile_with_secret(
            &view.local_id,
            &stored_password_input("Pilot renamed", &address, "editor"),
            secret("replaced-password-value"),
        )
        .await
        .unwrap_or_else(|error| panic!("update: {error}"));
    assert!(!update.context_changed);
    assert_eq!(
        store
            .load(&reference)
            .map(|value| value.expose_secret().to_owned())
            .ok()
            .as_deref(),
        Some("replaced-password-value")
    );

    // «Forget password»: the next connect needs the dialog again, and a
    // password typed there is stored after a successful sign-in.
    connections
        .forget_secret(&view.local_id)
        .unwrap_or_else(|error| panic!("forget: {error}"));
    assert!(!store.0.contains(&reference));
    let error = connections
        .connect(&view.local_id, ConnectRequest::default())
        .await
        .err()
        .unwrap_or_else(|| panic!("a forgotten password must be missing"));
    assert_eq!(error.code(), "credential_missing");
    let outcome = connections
        .connect(
            &view.local_id,
            ConnectRequest {
                secret: secret(PASSWORD),
                accept_new_identity: false,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("connect: {error}"));
    assert!(outcome.connected);
    assert_eq!(outcome.credential_save, None);
    assert!(store.0.contains(&reference));
    connections
        .disconnect()
        .await
        .unwrap_or_else(|error| panic!("disconnect: {error}"));

    // Turning storage off removes the stored password.
    connections
        .update_profile(&view.local_id, &password_input("Pilot", &address))
        .await
        .unwrap_or_else(|error| panic!("update: {error}"));
    assert!(!store.0.contains(&reference));
    let error = connections
        .connect(&view.local_id, ConnectRequest::default())
        .await
        .err()
        .unwrap_or_else(|| panic!("an unsaved password must be entered"));
    assert_eq!(error.code(), "secret_required");

    // A profile written before O-037 has no `secret_storage` for passwords.
    let legacy = profiles_yaml(&dir).replace("    secret_storage: none\n", "");
    assert!(!legacy.contains("secret_storage"), "{legacy}");
    std::fs::write(dir.path().join("config").join("connections.yaml"), legacy)
        .unwrap_or_else(|error| panic!("{error}"));
    let (reloaded, _) = service(&dir);
    let views = reloaded
        .list_profiles()
        .unwrap_or_else(|error| panic!("legacy profile: {error}"));
    assert_eq!(views[0].secret_storage, SecretStorage::None);
    assert!(!views[0].default_password_warning_hidden);
    task.abort();
}

/// O-037: a sign-in with the public default account raises a warning in
/// the report until the profile hides it; another account or a failed
/// sign-in never does, and a server change shows it again.
#[tokio::test]
async fn default_account_warning_until_hidden_for_the_connection() {
    let (address, _state, task) = server().await;
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let (mut connections, _) = service(&dir);
    let mut input = password_input("Local test", &address);
    input.auth = AuthInput::Password {
        username: DEFAULT_USERNAME.to_owned(),
        store_password: false,
    };
    let view = connections
        .create_profile(&input)
        .unwrap_or_else(|error| panic!("create: {error}"));

    let report = connections
        .test_connection(&view.local_id, secret(DEFAULT_PASSWORD))
        .await
        .unwrap_or_else(|error| panic!("test: {error}"));
    assert!(report.passed());
    assert!(report.default_password_warning);
    assert!(!format!("{report:?}").contains(DEFAULT_PASSWORD));

    let rejected = connections
        .test_connection(&view.local_id, secret("not-the-default"))
        .await
        .unwrap_or_else(|error| panic!("test: {error}"));
    assert!(!rejected.passed());
    assert!(!rejected.default_password_warning);

    let outcome = connections
        .connect(
            &view.local_id,
            ConnectRequest {
                secret: secret(DEFAULT_PASSWORD),
                accept_new_identity: false,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("connect: {error}"));
    assert!(outcome.connected);
    assert!(outcome.report.default_password_warning);

    let hidden = connections
        .hide_default_password_warning(&view.local_id)
        .unwrap_or_else(|error| panic!("hide: {error}"));
    assert!(hidden.default_password_warning_hidden);
    assert!(profiles_yaml(&dir).contains("default_password_warning_hidden: true"));
    let outcome = connections
        .connect(
            &view.local_id,
            ConnectRequest {
                secret: secret(DEFAULT_PASSWORD),
                accept_new_identity: false,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("connect: {error}"));
    assert!(outcome.connected);
    assert!(!outcome.report.default_password_warning);

    // Another server at a new address is a new context: the flag is reset.
    let mut moved = input.clone();
    moved.address = address.replace("127.0.0.1", "localhost");
    let update = connections
        .update_profile(&view.local_id, &moved)
        .await
        .unwrap_or_else(|error| panic!("update: {error}"));
    assert!(update.context_changed);
    assert!(!update.view.default_password_warning_hidden);

    // A regular account with a password is never flagged.
    let editor = connections
        .create_profile(&password_input("Editor", &address))
        .unwrap_or_else(|error| panic!("create: {error}"));
    let report = connections
        .test_connection(&editor.local_id, secret(PASSWORD))
        .await
        .unwrap_or_else(|error| panic!("test: {error}"));
    assert!(report.passed());
    assert!(!report.default_password_warning);
    let error = connections
        .hide_default_password_warning("conn-00000000000000000000000000000000")
        .err()
        .unwrap_or_else(|| panic!("unknown profile"));
    assert_eq!(error.code(), "profile_not_found");
    task.abort();
}

/// O-037: a password or token typed with «store» off lives only in process
/// memory, per profile: connect and check use it without the dialog; it is
/// dropped on a rejected sign-in, on disconnect, on a server or account
/// change, on delete and when the local session ends, and never reaches
/// disk or the credential store.
#[tokio::test]
async fn unsaved_secret_is_kept_in_memory_until_session_end_or_context_change() {
    let (address, _state, task) = server().await;
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let (mut connections, store) = service(&dir);
    let view = connections
        .create_profile_with_secret(&password_input("Pilot", &address), secret(PASSWORD))
        .unwrap_or_else(|error| panic!("create: {error}"));
    assert_eq!(view.secret_storage, SecretStorage::None);
    assert!(view.secret_in_memory);
    assert!(connections.holds_session_secrets());
    assert!(!store.0.contains(&credential_ref(&dir)));
    assert!(!profiles_yaml(&dir).contains(PASSWORD));
    assert!(!format!("{connections:?}").contains(PASSWORD));

    // Check and connect without a secret in the request: no dialog.
    let report = connections
        .test_connection(&view.local_id, None)
        .await
        .unwrap_or_else(|error| panic!("test from memory: {error}"));
    assert!(report.passed());
    let outcome = connections
        .connect(&view.local_id, ConnectRequest::default())
        .await
        .unwrap_or_else(|error| panic!("connect from memory: {error}"));
    assert!(outcome.connected);

    // «End session» erases the remembered secret with the session.
    let closed = connections
        .erase_session_secrets()
        .unwrap_or_else(|| panic!("the active session must be taken"));
    drop(closed);
    assert!(!connections.holds_session_secrets());
    let view = connections
        .profile(&view.local_id)
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(!view.secret_in_memory);
    let error = connections
        .connect(&view.local_id, ConnectRequest::default())
        .await
        .err()
        .unwrap_or_else(|| panic!("an erased secret must be entered again"));
    assert_eq!(error.code(), "secret_required");

    // A secret typed in the sign-in dialog is remembered the same way;
    // an explicit disconnect is a sign-out and drops it.
    let outcome = connections
        .connect(
            &view.local_id,
            ConnectRequest {
                secret: secret(PASSWORD),
                accept_new_identity: false,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("connect: {error}"));
    assert!(outcome.connected);
    assert!(
        connections
            .profile(&view.local_id)
            .unwrap_or_else(|error| panic!("{error}"))
            .secret_in_memory
    );
    connections
        .disconnect()
        .await
        .unwrap_or_else(|error| panic!("disconnect: {error}"));
    assert!(!connections.holds_session_secrets());

    // A rejected sign-in drops the remembered secret: back to the dialog.
    connections
        .update_profile_with_secret(
            &view.local_id,
            &password_input("Pilot", &address),
            secret("wrong-password-value"),
        )
        .await
        .unwrap_or_else(|error| panic!("update: {error}"));
    let rejected = connections
        .test_connection(&view.local_id, None)
        .await
        .unwrap_or_else(|error| panic!("test: {error}"));
    assert!(!rejected.passed());
    assert!(!connections.holds_session_secrets());

    // A server change invalidates it like a stored secret.
    connections
        .update_profile_with_secret(
            &view.local_id,
            &password_input("Pilot", &address),
            secret(PASSWORD),
        )
        .await
        .unwrap_or_else(|error| panic!("update: {error}"));
    let renamed = connections
        .update_profile(&view.local_id, &password_input("Pilot renamed", &address))
        .await
        .unwrap_or_else(|error| panic!("rename: {error}"));
    assert!(!renamed.context_changed);
    assert!(renamed.view.secret_in_memory);
    let moved = connections
        .update_profile(
            &view.local_id,
            &password_input("Pilot", &address.replace("127.0.0.1", "localhost")),
        )
        .await
        .unwrap_or_else(|error| panic!("move: {error}"));
    assert!(moved.context_changed);
    assert!(!moved.view.secret_in_memory);

    // A token works the same way; deleting the profile drops its secret.
    let token = connections
        .create_profile_with_secret(&token_input("Token", &address, false), secret(TOKEN))
        .unwrap_or_else(|error| panic!("create: {error}"));
    let outcome = connections
        .connect(&token.local_id, ConnectRequest::default())
        .await
        .unwrap_or_else(|error| panic!("connect: {error}"));
    assert!(outcome.connected);
    connections
        .delete_profile(&token.local_id)
        .await
        .unwrap_or_else(|error| panic!("delete: {error}"));
    assert!(!connections.holds_session_secrets());
    let yaml = profiles_yaml(&dir);
    for value in [PASSWORD, TOKEN, SESSION, "wrong-password-value"] {
        assert!(!yaml.contains(value), "{value} in {yaml}");
    }
    task.abort();
}
