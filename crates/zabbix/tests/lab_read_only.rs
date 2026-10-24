// SPDX-License-Identifier: MIT OR Apache-2.0

use std::env;

use secrecy::SecretString;
use zdashstitch_zabbix::{
    Authentication, DependencyKind, MethodOutcome, SnapshotSummary, ZabbixClient, ZabbixEndpoint,
};

/// Opt-in laboratory acceptance for P0-04. It invokes read methods only.
#[tokio::test]
#[ignore = "requires an explicitly authorized laboratory Zabbix 7.4"]
async fn permitted_zabbix_7_4_inventory_is_readable() {
    assert_eq!(
        required("ZDASHSTITCH_LAB_ACK"),
        "read-only-authorized-7.4",
        "explicit read-only laboratory acknowledgement is required"
    );
    let endpoint = ZabbixEndpoint::parse(&required("ZDASHSTITCH_LAB_API_URL"))
        .unwrap_or_else(|error| panic!("invalid laboratory endpoint: {error}"));
    let authentication = match env::var("ZDASHSTITCH_LAB_API_TOKEN") {
        Ok(token) if !token.is_empty() => Authentication::ApiToken(SecretString::from(token)),
        _ => Authentication::UsernamePassword {
            username: required("ZDASHSTITCH_LAB_USERNAME"),
            password: SecretString::from(required("ZDASHSTITCH_LAB_PASSWORD")),
        },
    };
    let client = ZabbixClient::new(endpoint)
        .unwrap_or_else(|error| panic!("cannot construct laboratory client: {error}"))
        .authenticate(authentication)
        .await
        .unwrap_or_else(|error| panic!("laboratory authentication/version check failed: {error}"));
    assert!(client.capabilities().version.is_supported_7_4());
    println!("apiinfo.version: {}", client.capabilities().version);
    let inventory = client
        .inventory()
        .await
        .unwrap_or_else(|error| panic!("laboratory inventory failed: {error}"));

    // Only counts and permission states are emitted; object contents remain private.
    report("template.get", &inventory.template);
    report("host.get", &inventory.host);
    report("item.get", &inventory.item);
    report("dashboard.get", &inventory.dashboard);
    report("graph.get", &inventory.graph);

    let snapshots = client
        .dashboard_snapshots("lab-read-only")
        .await
        .unwrap_or_else(|error| panic!("laboratory dashboard snapshot failed: {error}"));
    let analyses = snapshots
        .iter()
        .map(|snapshot| snapshot.analyze())
        .collect::<zdashstitch_zabbix::Result<Vec<_>>>()
        .unwrap_or_else(|error| panic!("laboratory dependency analysis failed: {error}"));
    let summary = SnapshotSummary::from_analyses(&analyses);
    println!(
        "dashboard snapshot: dashboards={}, pages={}, widgets={}, dependencies={}, items={}, graphs={}, hosts={}, reference_definitions={}, references={}, unknown_widgets={}, unknown_dependencies={}, writable_dependencies={}",
        summary.dashboard_count,
        summary.page_count,
        summary.widget_count,
        summary.dependency_count,
        count(&summary, DependencyKind::Item),
        count(&summary, DependencyKind::Graph),
        count(&summary, DependencyKind::Host),
        count(&summary, DependencyKind::WidgetReferenceDefinition),
        count(&summary, DependencyKind::WidgetReference),
        summary.unknown_widgets,
        summary.unknown_dependencies,
        summary.supported_for_modification,
    );
    assert!(!snapshots.is_empty());
    assert_eq!(summary.supported_for_modification, 0);
    client
        .logout()
        .await
        .unwrap_or_else(|error| panic!("laboratory logout failed: {error}"));
}

fn count(summary: &SnapshotSummary, kind: DependencyKind) -> usize {
    summary
        .dependencies_by_kind
        .get(&kind)
        .copied()
        .unwrap_or(0)
}

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("required environment variable is absent: {name}"))
}

fn report(method: &str, outcome: &MethodOutcome) {
    match outcome {
        MethodOutcome::Available(value) => {
            println!(
                "{method}: available, objects={}",
                value.as_array().map_or(0, Vec::len)
            );
        }
        MethodOutcome::PermissionDenied { .. } => println!("{method}: permission denied"),
    }
}
