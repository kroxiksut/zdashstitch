// SPDX-License-Identifier: MIT OR Apache-2.0

//! Opt-in read-only laboratory check of P2-05: list the visible dashboards,
//! copy the first one into a temporary storage root and read the draft back.
//! Only codes and counts are printed: no address, name or server data
//! leaves the test. Every call is a read; the session is closed with
//! `user.logout`.

use std::env;

use secrecy::SecretString;
use zdashstitch_application::workcopy::{
    ImportRequest, ImportScope, ServerDashboardQuery, WorkcopyService,
};
use zdashstitch_zabbix::{Authentication, ZabbixClient, ZabbixEndpoint};

/// The storage path component needs a connection profile ID; the lab check
/// uses a fixed synthetic one.
const CONNECTION: &str = "conn-1abc0000000000000000000000000001";

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("required environment variable is absent: {name}"))
}

#[tokio::test]
#[ignore = "requires an authorized laboratory Zabbix 7.4"]
async fn lab_workcopy_import_is_read_only_and_consistent() {
    assert_eq!(
        required("ZDASHSTITCH_LAB_ACK"),
        "read-only-authorized-7.4",
        "explicit read-only laboratory acknowledgement is required"
    );
    let endpoint = ZabbixEndpoint::parse(&required("ZDASHSTITCH_LAB_API_URL"))
        .unwrap_or_else(|error| panic!("endpoint: {error}"));
    let reader = ZabbixClient::new(endpoint)
        .unwrap_or_else(|error| panic!("client: {error}"))
        .authenticate(Authentication::UsernamePassword {
            username: required("ZDASHSTITCH_LAB_USERNAME"),
            password: SecretString::from(required("ZDASHSTITCH_LAB_PASSWORD")),
        })
        .await
        .unwrap_or_else(|error| panic!("sign-in: {error}"));
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let service = WorkcopyService::new(directory.path().to_path_buf());

    let list = service
        .list_server_dashboards(&reader, CONNECTION, &ServerDashboardQuery::default())
        .await
        .unwrap_or_else(|error| panic!("list: {error}"));
    println!(
        "CASE list: total={} truncated={} empty_reason={:?}",
        list.total, list.truncated, list.empty_reason
    );
    let Some(first) = list.rows.first() else {
        let _ = reader.logout().await;
        return;
    };
    let outcome = service
        .import(
            &reader,
            CONNECTION,
            &ImportRequest {
                dashboardid: first.dashboardid.clone(),
                scope: ImportScope::Dashboard,
                name: None,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("import: {error}"));
    let detail = service
        .draft(&outcome.local_id)
        .unwrap_or_else(|error| panic!("draft: {error}"));
    println!(
        "CASE import: attempts={} item_metadata={} pages={} widgets={} editable={}",
        outcome.attempts,
        outcome.item_metadata.as_str(),
        detail.summary.counts.pages,
        detail.summary.counts.widgets,
        detail
            .pages
            .iter()
            .flat_map(|page| page.widgets.iter())
            .filter(|widget| widget.editable)
            .count()
    );
    assert_eq!(detail.summary.counts.pages, first.page_count);
    reader
        .logout()
        .await
        .unwrap_or_else(|error| panic!("logout: {error}"));
}
