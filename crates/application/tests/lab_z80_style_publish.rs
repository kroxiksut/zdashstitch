// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::expect_used)]

//! Opt-in style publication against one designated private 8.0.0 fixture.

use std::env;

use secrecy::SecretString;
use serde_json::{Value, json};
use zdashstitch_application::{
    JournalState, LocalSession, PublishJournal, ReplaceHostPlanRequest, plan_workcopy_style,
    publish_replace_host_plan,
    reusable_library::{ReusableDocument, ReusableLibrary},
    workcopy::{CopyStyleRequest, ImportRequest, ImportScope, WorkcopyService},
};
use zdashstitch_zabbix::{Authentication, ZabbixClient, ZabbixEndpoint};

const CONNECTION: &str = "conn-0123456789abcdef0123456789abcdef";

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("missing {name}"))
}

#[tokio::test]
#[ignore = "requires the designated authorized private 8.0.0 fixture via a loopback tunnel"]
async fn lab_z80_item_background_and_reset_through_verified_plan() {
    check_style_fixture(false).await;
}

#[tokio::test]
#[ignore = "requires the designated authorized private 8.0.0 Graph fixture via a loopback tunnel"]
async fn lab_z80_graph_series_colors_through_verified_plan() {
    check_style_fixture(true).await;
}

async fn check_style_fixture(graph: bool) {
    assert_eq!(
        required("ZDASHSTITCH_LAB_STYLE_ACK"),
        "style-update-authorized-8.0.0"
    );
    let url = reqwest::Url::parse(&required("ZDASHSTITCH_LAB_API_URL")).expect("lab URL");
    assert!(url.host_str().is_some_and(|host| {
        host.parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.is_loopback())
    }));
    assert!(url.username().is_empty() && url.password().is_none());
    let fixture = if graph {
        "ZDASHSTITCH_LAB_STYLE_GRAPH"
    } else {
        "ZDASHSTITCH_LAB_STYLE"
    };
    let id = required(&format!("{fixture}_DASHBOARD_ID"));
    let name = required(&format!("{fixture}_DASHBOARD_NAME"));
    assert!(name.starts_with("ZDashstitch"));
    let gateway = ZabbixClient::new(ZabbixEndpoint::parse(url.as_str()).expect("endpoint"))
        .expect("client")
        .authenticate(Authentication::UsernamePassword {
            username: required("ZDASHSTITCH_LAB_USERNAME"),
            password: SecretString::from(required("ZDASHSTITCH_LAB_PASSWORD")),
        })
        .await
        .expect("lab authentication")
        .enable_writes();
    assert_eq!(gateway.capabilities().version.to_string(), "8.0.0");
    let reader = gateway.reader();
    let read = || async {
        reader
            .dashboard_snapshot(CONNECTION, &id)
            .await
            .expect("read")
            .expect("visible fixture")
    };
    let initial = read().await;
    assert!(initial.raw_api_document()["name"].as_str() == Some(name.as_str()));
    let storage = tempfile::tempdir().expect("local temporary storage");
    let service = WorkcopyService::new(storage.path().to_path_buf());
    let library = ReusableLibrary::new(storage.path().to_path_buf());
    let journal = PublishJournal::new(storage.path().to_path_buf());
    let session = LocalSession::new();
    let mut original = initial.raw_api_document().clone();
    let colors = color_fields(&mut original, graph);
    let first = if colors.iter().all(|color| color.as_str() != Some("A1B2C3")) {
        "A1B2C3"
    } else {
        "C3B2A1"
    };
    let palette = [first, ""];
    for (index, &color) in palette.iter().take(if graph { 1 } else { 2 }).enumerate() {
        let before = read().await;
        let mut expected = before.raw_api_document().clone();
        assert!(expected["name"].as_str() == Some(name.as_str()));
        let expected_changes = color_fields(&mut expected, graph)
            .into_iter()
            .map(|field| {
                let changed = *field != color;
                *field = json!(color);
                usize::from(changed)
            })
            .sum::<usize>();
        let draft = service
            .import(
                reader,
                CONNECTION,
                &ImportRequest {
                    dashboardid: id.clone(),
                    scope: ImportScope::Dashboard,
                    name: None,
                },
            )
            .await
            .expect("import designated fixture");
        let section = if graph {
            format!("  svggraph:\n    series_palette: ['{color}']")
        } else {
            format!("  item:\n    widget_fields: {{show: [2], bg_color: '{color}'}}")
        };
        let asset = ReusableDocument::parse_yaml(format!(
            "format: zdashstitch.style-profile/v1\nprofile_id: 7a2f3c10-5b7e-4c8a-9d21-0f6e4b1a2c3{index}\nname: Lab visual proof\nversion: 1\nadapter: '8.0'\nwidget_types:\n{section}\n"
        ).as_bytes()).expect("profile fixture");
        let asset = library.create(asset).expect("save profile");
        let local = service
            .workcopy_style_state(&draft.local_id)
            .expect("current workcopy");
        let request: CopyStyleRequest = serde_json::from_value(json!({
            "expected_revision": local.revision, "profile_id": asset.asset.id(),
            "profile_revision": asset.revision, "page": 0
        }))
        .expect("style request");
        let preview = service
            .workcopy_style(&draft.local_id, &request, false)
            .expect("preview");
        assert!(expected_changes > 0);
        assert_eq!(preview.changes.len(), expected_changes);
        service
            .workcopy_style(&draft.local_id, &request, true)
            .expect("apply in copy");
        let local = service
            .workcopy_style_state(&draft.local_id)
            .expect("publication preview");
        let preview = local.publication.expect("style only");
        let plan = plan_workcopy_style(
            &service,
            &journal,
            &session,
            &gateway,
            &ReplaceHostPlanRequest {
                local_id: draft.local_id.clone(),
                connection_id: CONNECTION.to_owned(),
                revision: local.revision,
                base_fingerprint: preview.base_fingerprint,
                local_fingerprint: preview.local_fingerprint,
                kept_digest: None,
                session: session.current(),
            },
        )
        .await
        .expect("verified style plan");
        let plan_id = plan.preview().plan_id;
        let confirmed = plan.confirm(&plan_id).expect("explicit confirmation");
        let stage = session
            .begin_write(confirmed.session())
            .expect("one write stage");
        let report =
            publish_replace_host_plan(&service, &gateway, &journal, confirmed, &stage).await;
        drop(stage);
        assert_eq!(
            report.states.last(),
            Some(&JournalState::Verified),
            "style publication failed: invalidated={:?}, journal_error={:?}",
            report.invalidated,
            report.journal_error.as_ref().map(|error| error.code())
        );
        assert!(report.journal_error.is_none());
        let entry = journal.entry(&plan_id).expect("journal evidence");
        assert_eq!(entry.sends, 1);
        let after = read().await;
        assert!(
            after.raw_api_document() == &expected,
            "full fixture read-back differed"
        );
    }
}

fn color_fields(raw: &mut Value, graph: bool) -> Vec<&mut Value> {
    assert!(raw["private"] == "1" || raw["private"] == 1);
    let pages = raw["pages"].as_array_mut().expect("fixture pages");
    assert_eq!(pages.len(), 1);
    let widgets = pages[0]["widgets"].as_array_mut().expect("fixture widgets");
    let kind = if graph { "svggraph" } else { "item" };
    let widget = widgets
        .iter_mut()
        .find(|widget| widget["type"] == kind)
        .expect("fixture widget");
    let fields = widget["fields"].as_array_mut().expect("fixture fields");
    let colors: Vec<_> = fields
        .iter_mut()
        .filter(|field| {
            matches!(
                field["name"].as_str(),
                Some("bg_color" | "ds.0.color.0" | "ds.0.color.1")
            )
        })
        .map(|field| &mut field["value"])
        .collect();
    assert_eq!(colors.len(), if graph { 2 } else { 1 });
    colors
}
