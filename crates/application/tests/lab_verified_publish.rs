// SPDX-License-Identifier: MIT OR Apache-2.0

use std::env;

use secrecy::SecretString;
use zdashstitch_application::{JournalState, build_verified_plan, publish_confirmed_plan};
use zdashstitch_zabbix::{
    Authentication, ItemFieldReplacement, VersionedDashboardSnapshot, ZabbixClient, ZabbixEndpoint,
};

const LAB_DASHBOARD_NAME: &str = "ZDashstitch Contract Lab";

#[tokio::test]
#[ignore = "publishes only to the explicitly authorized laboratory contract dashboard"]
async fn verified_plan_publishes_once_and_advances_only_after_read_back() {
    assert_eq!(
        required("ZDASHSTITCH_LAB_PUBLISH_ACK"),
        "verified-plan-authorized-7.4-contract-dashboard"
    );
    let endpoint = ZabbixEndpoint::parse(&required("ZDASHSTITCH_LAB_API_URL"))
        .unwrap_or_else(|error| panic!("invalid laboratory endpoint: {error}"));
    let client = ZabbixClient::new(endpoint)
        .unwrap_or_else(|error| panic!("cannot construct laboratory client: {error}"))
        .authenticate(Authentication::UsernamePassword {
            username: required("ZDASHSTITCH_LAB_USERNAME"),
            password: SecretString::from(required("ZDASHSTITCH_LAB_PASSWORD")),
        })
        .await
        .unwrap_or_else(|error| panic!("laboratory authentication failed: {error}"))
        .enable_writes();
    assert_eq!(client.capabilities().version.to_string(), "7.4.16");

    let source = unique_contract_dashboard(
        client
            .dashboard_snapshots("lab-verified-publish")
            .await
            .unwrap_or_else(|error| panic!("cannot read contract dashboard: {error}")),
    );
    let (page_index, widget_index, field_name, old_itemid) = item_field(&source);
    let target_itemid = client
        .reader()
        .readable_item_ids(1_000)
        .await
        .unwrap_or_else(|error| panic!("cannot resolve a replacement item: {error}"))
        .into_iter()
        .find(|itemid| itemid != &old_itemid)
        .unwrap_or_else(|| panic!("laboratory needs a second readable item"));
    let expected_target = target_itemid.clone();
    let replacement = ItemFieldReplacement {
        page_index,
        widget_index,
        field_name,
        expected_itemid: old_itemid,
        replacement_itemid: target_itemid,
    };
    let plan = build_verified_plan(&source, &client.publication_context(), &[replacement])
        .unwrap_or_else(|error| panic!("verified plan must be created: {error}"));
    let preview = plan.preview();
    let confirmed = plan
        .confirm(&preview.plan_id)
        .unwrap_or_else(|error| panic!("displayed plan confirmation must match: {error}"));
    let report = publish_confirmed_plan(&client, confirmed).await;
    if report.states.last() != Some(&JournalState::Verified) {
        let observed = unique_contract_dashboard(
            client
                .dashboard_snapshots("lab-verified-publish")
                .await
                .unwrap_or_else(|error| panic!("cannot diagnose read-back: {error}")),
        );
        let (_, _, _, observed_itemid) = item_field(&observed);
        println!(
            "verified publisher diagnostic: final_state={:?}, target_applied={}",
            report.states.last(),
            observed_itemid == expected_target
        );
    }
    assert_eq!(report.states.last(), Some(&JournalState::Verified));
    assert!(report.deployment_state_may_advance);
    println!(
        "verified publisher: version=7.4.16, replacements=1, final_state=verified, deployment_state_may_advance=true"
    );
    client
        .logout()
        .await
        .unwrap_or_else(|error| panic!("laboratory logout failed: {error}"));
}

fn unique_contract_dashboard(
    snapshots: Vec<VersionedDashboardSnapshot>,
) -> VersionedDashboardSnapshot {
    let mut matching = snapshots
        .into_iter()
        .filter(|snapshot| snapshot.raw_api_document()["name"] == LAB_DASHBOARD_NAME);
    let dashboard = matching
        .next()
        .unwrap_or_else(|| panic!("laboratory contract dashboard is absent"));
    assert!(
        matching.next().is_none(),
        "contract dashboard name must be unique"
    );
    dashboard
}

fn item_field(snapshot: &VersionedDashboardSnapshot) -> (usize, usize, String, String) {
    for (page_index, page) in snapshot.raw_api_document()["pages"]
        .as_array()
        .unwrap_or_else(|| panic!("dashboard pages must be an array"))
        .iter()
        .enumerate()
    {
        let widgets = page["widgets"]
            .as_array()
            .unwrap_or_else(|| panic!("page widgets must be an array"));
        if !widgets
            .iter()
            .all(|widget| matches!(widget["type"].as_str(), Some("item" | "svggraph")))
        {
            continue;
        }
        for (widget_index, widget) in widgets.iter().enumerate() {
            if widget["type"] != "item" {
                continue;
            }
            for field in widget["fields"]
                .as_array()
                .unwrap_or_else(|| panic!("widget fields must be an array"))
            {
                let field_type = field["type"]
                    .as_i64()
                    .or_else(|| field["type"].as_str().and_then(|value| value.parse().ok()));
                if field_type == Some(4) {
                    return (
                        page_index,
                        widget_index,
                        field["name"]
                            .as_str()
                            .unwrap_or_else(|| panic!("item field name must be a string"))
                            .to_owned(),
                        field["value"]
                            .as_str()
                            .unwrap_or_else(|| panic!("item field value must be a string"))
                            .to_owned(),
                    );
                }
            }
        }
    }
    panic!("no contract-proven item field found on the laboratory dashboard")
}

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("required environment variable is absent: {name}"))
}
