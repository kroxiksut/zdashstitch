// SPDX-License-Identifier: MIT OR Apache-2.0

//! Opt-in read-only laboratory check of P2-01: the five-step connection check
//! with a password, a wrong password and an unknown API token. Only step
//! codes are printed; no secret, address or server data leaves the test.
//! Sessions are opened with `user.login` and closed with `user.logout`.

use std::env;

use secrecy::SecretString;
use zdashstitch_application::connections::{
    AuthInput, ConnectRequest, ConnectionCheckReport, ConnectionService, MemoryCredentialStore,
    ProfileInput, TrustInput,
};

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("required environment variable is absent: {name}"))
}

fn summary(report: &ConnectionCheckReport) -> String {
    report
        .steps
        .iter()
        .map(|step| {
            let params: Vec<String> = step
                .params
                .iter()
                .filter(|param| param.name != "version" && param.name != "previous_version")
                .map(|param| format!("{}={}", param.name, param.value))
                .collect();
            format!(
                "{}:{}{}{}",
                step.step.as_str(),
                step.status.as_str(),
                step.code.map(|code| format!(":{code}")).unwrap_or_default(),
                if params.is_empty() {
                    String::new()
                } else {
                    format!("[{}]", params.join(","))
                }
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[tokio::test]
#[ignore = "requires an authorized laboratory Zabbix 7.4"]
async fn lab_connection_check_reports_codes() {
    assert_eq!(
        required("ZDASHSTITCH_LAB_ACK"),
        "read-only-authorized-7.4",
        "explicit read-only laboratory acknowledgement is required"
    );
    let address = required("ZDASHSTITCH_LAB_API_URL");
    let username = required("ZDASHSTITCH_LAB_USERNAME");
    let password = required("ZDASHSTITCH_LAB_PASSWORD");
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let mut connections = ConnectionService::new(
        directory.path().to_path_buf(),
        Box::new(MemoryCredentialStore::default()),
    );
    let password_profile = connections
        .create_profile(&ProfileInput {
            name: "Lab".to_owned(),
            address: address.clone(),
            auth: AuthInput::Password {
                username,
                store_password: false,
            },
            trust: TrustInput::System,
            confirm_plain_http_token_storage: false,
        })
        .unwrap_or_else(|error| panic!("profile: {error}"));

    let report = connections
        .test_connection(
            &password_profile.local_id,
            Some(SecretString::from(password.clone())),
        )
        .await
        .unwrap_or_else(|error| panic!("test: {error}"));
    println!("CASE password_test: {}", summary(&report));
    assert!(report.passed(), "{}", summary(&report));
    println!(
        "CASE password_test_read_access: {}",
        report
            .read_access
            .iter()
            .map(|(method, access)| format!("{}={}", method.api_method(), access.as_str()))
            .collect::<Vec<_>>()
            .join(",")
    );

    let outcome = connections
        .connect(
            &password_profile.local_id,
            ConnectRequest {
                secret: Some(SecretString::from(password)),
                accept_new_identity: false,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("connect: {error}"));
    println!(
        "CASE password_connect: connected={} {}",
        outcome.connected,
        summary(&outcome.report)
    );
    assert!(outcome.connected);
    let version = connections
        .reader()
        .map(|reader| reader.capabilities().version.to_string())
        .unwrap_or_default();
    println!("CASE server_version: {version}");
    let disconnected = connections
        .disconnect()
        .await
        .unwrap_or_else(|error| panic!("disconnect: {error}"));
    println!(
        "CASE password_disconnect: logout_error={:?}",
        disconnected.logout
    );
    assert_eq!(disconnected.logout, None);

    let wrong = connections
        .test_connection(
            &password_profile.local_id,
            Some(SecretString::from("zdashstitch-wrong-password".to_owned())),
        )
        .await
        .unwrap_or_else(|error| panic!("test: {error}"));
    println!("CASE wrong_password: {}", summary(&wrong));
    assert_eq!(wrong.result_code(), Some("auth_failed"));

    let token_profile = connections
        .create_profile(&ProfileInput {
            name: "Lab token".to_owned(),
            address,
            auth: AuthInput::ApiToken { store_token: false },
            trust: TrustInput::System,
            confirm_plain_http_token_storage: false,
        })
        .unwrap_or_else(|error| panic!("profile: {error}"));
    let unknown = connections
        .test_connection(
            &token_profile.local_id,
            Some(SecretString::from("0".repeat(64))),
        )
        .await
        .unwrap_or_else(|error| panic!("test: {error}"));
    println!("CASE unknown_token: {}", summary(&unknown));
    assert_eq!(unknown.result_code(), Some("token_not_authorized"));
}
