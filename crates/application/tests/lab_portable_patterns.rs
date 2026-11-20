// SPDX-License-Identifier: MIT OR Apache-2.0
//! Opt-in, read-only laboratory check for the portable host-pattern inventory.
use secrecy::SecretString;
use std::env;
use zdashstitch_application::{
    clone_mapping::{TargetSlot, TargetSlotId},
    offline_pages::OfflinePageStore,
    portable_pattern_review::{
        PortablePatternConfirmRequest, PortablePatternReopenRequest, PortablePatternReviewRequest,
    },
    workcopy::WorkcopyService,
};
use zdashstitch_core::formats::{ProjectRoot, Widget, ZabbixWidgetFieldValue};
use zdashstitch_zabbix::{
    Authentication, HostDetails, MAX_HOST_SEARCH, ZabbixClient, ZabbixEndpoint,
};

#[tokio::test]
#[ignore = "requires explicitly authorized read-only laboratory Zabbix"]
async fn pattern_review_includes_visible_unselected_hosts() {
    assert_eq!(
        env::var("ZDASHSTITCH_LAB_ACK").ok().as_deref(),
        Some("read-only-authorized-7.4")
    );
    let required =
        |name| env::var(name).unwrap_or_else(|_| panic!("required laboratory setting absent"));
    let endpoint = ZabbixEndpoint::parse(&required("ZDASHSTITCH_LAB_API_URL"))
        .unwrap_or_else(|_| panic!("invalid lab endpoint"));
    let reader = ZabbixClient::new(endpoint)
        .unwrap_or_else(|_| panic!("client"))
        .authenticate(Authentication::UsernamePassword {
            username: required("ZDASHSTITCH_LAB_USERNAME"),
            password: SecretString::from(required("ZDASHSTITCH_LAB_PASSWORD")),
        })
        .await
        .unwrap_or_else(|_| panic!("lab authentication"));
    assert_eq!(reader.capabilities().version.to_string(), "7.4.16");
    let hosts = reader
        .search_hosts("", MAX_HOST_SEARCH)
        .await
        .unwrap_or_else(|_| panic!("host inventory"));
    assert!(
        !hosts.truncated && !hosts.objects.is_empty(),
        "complete nonempty laboratory inventory required"
    );
    let first = &hosts.objects[0];
    let targets = vec![TargetSlot {
        slot_id: TargetSlotId("first".into()),
        host: HostDetails {
            hostid: first.hostid.clone(),
            host: first.host.clone(),
            name: first.name.clone(),
        },
    }];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| panic!("root"));
    let mut bundle = zdashstitch_core::formats::load_typed_project_file(
        &root.join("fixtures/formats/v2/positive/dynamic-pattern.yaml"),
    )
    .unwrap_or_else(|_| panic!("fixture"));
    let ProjectRoot::Page(page) = &mut bundle.project.root else {
        panic!("page fixture")
    };
    let Widget::Zabbix { fields, .. } = &mut page.widgets[0] else {
        panic!("widget fixture")
    };
    let ZabbixWidgetFieldValue::DynamicPattern { value, .. } = &mut fields[0].value else {
        panic!("pattern fixture")
    };
    *value = "*".into();
    let temp = tempfile::tempdir().unwrap_or_else(|_| panic!("temporary store"));
    let page = OfflinePageStore::new(temp.path().to_path_buf())
        .save_imported(bundle)
        .unwrap_or_else(|_| panic!("save fixture"));
    let service = WorkcopyService::new(temp.path().to_path_buf());
    let request = PortablePatternReviewRequest {
        revision: page.revision,
        page_index: 0,
        widget_index: 0,
        field_index: 0,
        targets,
    };
    let review = service
        .preview_saved_portable_pattern(&reader, &page.local_id, &request)
        .await
        .unwrap_or_else(|_| panic!("pattern preview"));
    assert_eq!(review.visible_matches.len(), hosts.objects.len());
    assert_eq!(
        review
            .visible_matches
            .iter()
            .filter(|entry| entry.selected)
            .count(),
        1
    );
    let confirm = PortablePatternConfirmRequest {
        review: request,
        fingerprint: review.fingerprint.clone(),
        acknowledge_visible_outside: true,
    };
    service
        .confirm_saved_portable_pattern(&reader, &page.local_id, &confirm)
        .await
        .unwrap_or_else(|_| panic!("pattern confirmation"));
    let reopened = service
        .reopen_saved_portable_pattern(
            &reader,
            &page.local_id,
            &PortablePatternReopenRequest {
                revision: page.revision,
                page_index: 0,
                widget_index: 0,
                field_index: 0,
            },
        )
        .await
        .unwrap_or_else(|_| panic!("pattern reopen"));
    assert_eq!(reopened.fingerprint, review.fingerprint);
    reader
        .logout()
        .await
        .unwrap_or_else(|_| panic!("lab logout"));
}
