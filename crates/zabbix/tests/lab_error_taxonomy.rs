// SPDX-License-Identifier: MIT OR Apache-2.0

//! Opt-in design-input probe P1-IN-01: which connection and authentication
//! failures the current transport stack can tell apart. It prints one line per
//! case. Real credentials are never sent: version detection is unauthenticated
//! and the token case uses a fixed dummy value.

use std::env;
use std::error::Error as StdError;
use std::time::Duration;

use secrecy::SecretString;
use zdashstitch_zabbix::{Authentication, MethodOutcome, ZabbixClient, ZabbixEndpoint};

#[tokio::test]
#[ignore = "requires an authorized laboratory Zabbix 7.4 and local synthetic TLS fixtures"]
async fn transport_and_auth_failures_are_classified() {
    assert_eq!(
        required("ZDASHSTITCH_LAB_ACK"),
        "read-only-authorized-7.4",
        "explicit read-only laboratory acknowledgement is required"
    );
    // Base URL of the laboratory front end, without `api_jsonrpc.php`.
    let lab = required("ZDASHSTITCH_TAXONOMY_LAB_BASE");
    let tls = env::var("ZDASHSTITCH_TAXONOMY_TLS_HOST").unwrap_or_else(|_| "localhost".to_owned());

    let cases = [
        ("control_ok", format!("{lab}/api_jsonrpc.php")),
        (
            "dns_unresolvable",
            "https://zdashstitch-taxonomy.invalid/api_jsonrpc.php".to_owned(),
        ),
        (
            "connection_refused",
            "http://127.0.0.1:9/api_jsonrpc.php".to_owned(),
        ),
        (
            "wrong_base_path",
            format!("{lab}/wrong-path/api_jsonrpc.php"),
        ),
        ("html_instead_of_jsonrpc", format!("{lab}/index.php")),
        ("redirect_base_without_slash", lab.clone()),
        (
            "tls_untrusted_ca",
            format!("https://{tls}:8441/api_jsonrpc.php"),
        ),
        (
            "tls_self_signed",
            format!("https://{tls}:8445/api_jsonrpc.php"),
        ),
        (
            "plain_http_to_tls_port",
            format!("http://{tls}:8441/api_jsonrpc.php"),
        ),
        (
            "timeout_unroutable",
            "http://10.255.255.1/api_jsonrpc.php".to_owned(),
        ),
    ];
    for (label, url) in cases {
        let line = match ZabbixEndpoint::parse(&url) {
            Err(error) => format!("endpoint rejected: {}", chain(&error)),
            Ok(endpoint) => match ZabbixClient::new(endpoint) {
                Err(error) => format!("client error: {}", chain(&error)),
                Ok(client) => match client.detect_version().await {
                    Ok(version) => format!("ok version={version}"),
                    Err(error) => chain(&error),
                },
            },
        };
        println!("CASE {label}: {line}");
    }

    if let Ok(ca_path) = env::var("ZDASHSTITCH_TAXONOMY_TLS_CA") {
        tls_with_custom_root(&tls, &ca_path).await;
    }

    // Authentication: an obviously invalid token. Version detection succeeds,
    // so the outcome shows how a bad token surfaces on the first real call.
    let endpoint = ZabbixEndpoint::parse(&format!("{lab}/api_jsonrpc.php"))
        .unwrap_or_else(|error| panic!("invalid laboratory endpoint: {error}"));
    let client = ZabbixClient::new(endpoint)
        .unwrap_or_else(|error| panic!("cannot construct laboratory client: {error}"));
    match client
        .authenticate(Authentication::ApiToken(SecretString::from("0".repeat(64))))
        .await
    {
        Err(error) => println!("CASE auth_invalid_token_authenticate: {}", chain(&error)),
        Ok(authenticated) => {
            println!("CASE auth_invalid_token_authenticate: accepted without a server check");
            match authenticated.inventory().await {
                Err(error) => println!("CASE auth_invalid_token_first_call: {}", chain(&error)),
                Ok(inventory) => {
                    let outcome = |outcome: &MethodOutcome| match outcome {
                        MethodOutcome::Available(_) => "available",
                        MethodOutcome::PermissionDenied { .. } => "permission_denied",
                    };
                    println!(
                        "CASE auth_invalid_token_first_call: inventory template={}, dashboard={}",
                        outcome(&inventory.template),
                        outcome(&inventory.dashboard)
                    );
                }
            }
        }
    }
}

async fn tls_with_custom_root(host: &str, ca_path: &str) {
    let pem = std::fs::read(ca_path).unwrap_or_else(|error| panic!("cannot read test CA: {error}"));
    let certificate = reqwest::Certificate::from_pem(&pem)
        .unwrap_or_else(|error| panic!("invalid test CA: {error}"));
    for (mode, only) in [("merge", false), ("only", true)] {
        let builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10));
        let builder = if only {
            builder.tls_certs_only([certificate.clone()])
        } else {
            builder.tls_certs_merge([certificate.clone()])
        };
        let client = match builder.build() {
            Ok(client) => client,
            Err(error) => {
                println!("CASE tls_custom_root_{mode}_build: {}", chain(&error));
                continue;
            }
        };
        for (label, port) in [
            ("valid", 8441),
            ("name_mismatch", 8442),
            ("expired", 8443),
            ("not_yet_valid", 8444),
            ("self_signed", 8445),
        ] {
            let result = client
                .post(format!("https://{host}:{port}/api_jsonrpc.php"))
                .body(r#"{"jsonrpc":"2.0","method":"apiinfo.version","params":[],"id":1}"#)
                .send()
                .await;
            let line = match result {
                Ok(response) => format!("ok status={}", response.status()),
                Err(error) => chain(&error),
            };
            println!("CASE tls_custom_root_{mode}_{label}: {line}");
        }
    }
}

/// Error chain as `Debug` strings, with `reqwest` classification flags.
fn chain(error: &(dyn StdError + 'static)) -> String {
    let mut parts = Vec::new();
    let mut current = Some(error);
    while let Some(error) = current {
        let mut text = format!("{error:?}");
        if let Some(http) = error.downcast_ref::<reqwest::Error>() {
            text = format!(
                "reqwest[connect={},timeout={},redirect={},decode={},status={:?}]",
                http.is_connect(),
                http.is_timeout(),
                http.is_redirect(),
                http.is_decode(),
                http.status()
            );
        }
        // OS messages may be localized; cut on character boundaries.
        parts.push(text.chars().take(220).collect::<String>());
        current = error.source();
    }
    parts.join(" <- ")
}

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("required environment variable is absent: {name}"))
}
