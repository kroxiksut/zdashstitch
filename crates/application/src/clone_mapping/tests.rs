// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::future::{Future, ready};
use std::path::Path;
use std::sync::Mutex;

use serde_json::{Value, json};
use zdashstitch_zabbix::{
    ExactHostTag, HostDetails, HostGroupListing, HostListing, HostObjects, HostSearchFilter,
    ItemDetails, ItemFlags, TemplateRef, VersionedDashboardSnapshot, WidgetCapabilityRegistry,
    ZabbixError, ZabbixVersion,
};

use super::*;
use crate::PrepareCloneIntentRequest;
use crate::resolver::{Evidence, FieldAddress, ItemOrigin, SourceValue, WidgetAddress};
use crate::workcopy::WorkcopyService;
use crate::workcopy::formats::{
    BaseReference, DraftKind, ItemMetadataState, LOCAL_ONLY, LocalMetadata, SNAPSHOT_FORMAT,
    SnapshotFile, SnapshotScope, WORKCOPY_FORMAT, WorkcopyFile, WorkcopySource,
};
use crate::workcopy::store::LocalStore;

const CONNECTION: &str = "conn-0123456789abcdef0123456789abcdef";
const LOCAL: &str = "wc-00112233445566778899aabbccddeeff";
const IDENTITY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn host(hostid: &str) -> HostDetails {
    HostDetails {
        hostid: hostid.to_owned(),
        host: format!("technical-{hostid}"),
        name: format!("Visible {hostid}"),
    }
}

fn listing(hostid: &str, templates: &[(&str, &str)]) -> HostListing {
    HostListing {
        hostid: hostid.to_owned(),
        host: format!("technical-{hostid}"),
        name: format!("Visible {hostid}"),
        groups: vec!["Pilot/DB".to_owned()],
        templates: templates
            .iter()
            .map(|(templateid, name)| TemplateRef {
                templateid: (*templateid).to_owned(),
                name: (*name).to_owned(),
            })
            .collect(),
    }
}

fn field(name: &str) -> FieldAddress {
    field_at(0, "20", name)
}

fn field_at(widget_index: usize, widgetid: &str, name: &str) -> FieldAddress {
    FieldAddress {
        widget: WidgetAddress {
            page_index: 0,
            dashboard_pageid: Some("10".to_owned()),
            widget_index,
            widgetid: Some(widgetid.to_owned()),
        },
        field_name: name.to_owned(),
    }
}

fn reference(targets: Vec<TargetMetricResolution>) -> ReferenceMapping {
    ReferenceMapping {
        field: field("itemid.0"),
        widget_type: "item".to_owned(),
        source: SourceValue::Item {
            itemid: "30".to_owned(),
        },
        source_evidence: None,
        targets,
    }
}

fn mapped(slot_ids: &[(&str, &str)]) -> SourceHostMapping {
    let slots = slot_ids
        .iter()
        .map(|(slot_id, hostid)| TargetSlot {
            slot_id: TargetSlotId((*slot_id).to_owned()),
            host: host(hostid),
        })
        .collect::<Vec<_>>();
    let targets = slots
        .iter()
        .map(|slot| TargetMetricResolution {
            slot_id: slot.slot_id.clone(),
            target: Some(TargetValue::Item {
                itemid: format!("9{}", slot.host.hostid),
            }),
            match_status: MatchStatus::Exact,
            applicability: Applicability::Editable,
            reasons: Vec::new(),
            differences: Vec::new(),
            evidence: None,
            candidates: Vec::new(),
            decision: None,
            check: CloneBindingCheck::Checked,
            check_reason: None,
        })
        .collect();
    SourceHostMapping {
        role_id: SourceRoleId("postgres".to_owned()),
        source_host: host("100"),
        decision: SourceHostDecision::TargetSlots(slots),
        references: vec![reference(targets)],
    }
}

fn version() -> ZabbixVersion {
    "7.4.16"
        .parse()
        .unwrap_or_else(|error| panic!("version: {error}"))
}

fn storage_setup() -> (tempfile::TempDir, WorkcopyService) {
    let snapshot = VersionedDashboardSnapshot::from_adapter_raw(
        CONNECTION.to_owned(),
        version(),
        json!({
            "dashboardid": "700", "name": "Synthetic", "private": "1",
            "userid": "1", "display_period": "30", "auto_start": "1",
            "pages": [{"dashboard_pageid": "701", "name": "Page", "display_period": "0", "widgets": []}],
            "users": [], "userGroups": []
        }),
    )
    .unwrap_or_else(|error| panic!("snapshot: {error}"));
    storage_setup_with_snapshot(snapshot)
}

fn storage_setup_with_snapshot(
    snapshot: VersionedDashboardSnapshot,
) -> (tempfile::TempDir, WorkcopyService) {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("temp: {error}"));
    let store = LocalStore::at(directory.path().to_path_buf());
    let dashboardid = snapshot.raw_api_document()["dashboardid"]
        .as_str()
        .unwrap_or_else(|| panic!("dashboardid"))
        .to_owned();
    let name = snapshot.raw_api_document()["name"]
        .as_str()
        .unwrap_or_else(|| panic!("name"))
        .to_owned();
    let page_count = snapshot.raw_api_document()["pages"]
        .as_array()
        .map_or(0, Vec::len) as u32;
    let widget_count = snapshot.raw_api_document()["pages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|page| page["widgets"].as_array())
        .map(Vec::len)
        .sum::<usize>() as u32;
    let fingerprint = snapshot.storage_fingerprint().to_owned();
    store
        .put_snapshot(&SnapshotFile {
            format: SNAPSHOT_FORMAT.to_owned(),
            storage_scope: LOCAL_ONLY.to_owned(),
            connection_id: CONNECTION.to_owned(),
            adapter_id: snapshot.adapter_id().to_owned(),
            server_version: "7.4.16".to_owned(),
            dashboardid: dashboardid.clone(),
            scope: SnapshotScope::Dashboard,
            first_read_at_ms: 1,
            last_read_at_ms: 1,
            first_read_fingerprint: fingerprint.clone(),
            last_read_fingerprint: fingerprint.clone(),
            fingerprint: fingerprint.clone(),
            raw_api_document: snapshot.raw_api_document().clone(),
        })
        .unwrap_or_else(|error| panic!("put snapshot: {error}"));
    store
        .create_workcopy(&WorkcopyFile {
            format: WORKCOPY_FORMAT.to_owned(),
            storage_scope: LOCAL_ONLY.to_owned(),
            workcopy_id: LOCAL.to_owned(),
            kind: DraftKind::Dashboard,
            name: name.clone(),
            created_at_ms: 1,
            updated_at_ms: 1,
            revision: 0,
            source: WorkcopySource {
                connection_id: CONNECTION.to_owned(),
                dashboardid,
                dashboard_name: name,
                page: None,
                adapter_id: snapshot.adapter_id().to_owned(),
                server_version: "7.4.16".to_owned(),
            },
            base: BaseReference {
                fingerprint,
                read_at_ms: 1,
                source_checked_at_ms: 1,
                page_count,
                widget_count,
            },
            operations: Vec::new(),
            created_dashboards: Vec::new(),
            local_metadata: LocalMetadata {
                items_state: ItemMetadataState::NotNeeded,
                items: BTreeMap::new(),
                value_maps: BTreeMap::new(),
                missing_items: 0,
            },
        })
        .unwrap_or_else(|error| panic!("workcopy: {error}"));
    let service = WorkcopyService::new(directory.path().to_path_buf());
    (directory, service)
}

fn save_request(revision: u64, mappings: Vec<SourceHostMapping>) -> SaveCloneMappingRequest {
    SaveCloneMappingRequest {
        expected_workcopy_revision: revision,
        connection_id: CONNECTION.to_owned(),
        server_identity_fingerprint: IDENTITY.to_owned(),
        server_version: "7.4.16".to_owned(),
        local_fingerprint: Some(IDENTITY.to_owned()),
        mappings,
        pattern_decisions: Vec::new(),
    }
}

#[test]
fn target_slots_and_metric_results_keep_the_explicit_order() {
    let slots = vec![
        TargetSlot {
            slot_id: TargetSlotId("slot-b".to_owned()),
            host: host("102"),
        },
        TargetSlot {
            slot_id: TargetSlotId("slot-a".to_owned()),
            host: host("101"),
        },
    ];
    let targets = slots
        .iter()
        .map(|slot| TargetMetricResolution {
            slot_id: slot.slot_id.clone(),
            target: Some(TargetValue::Item {
                itemid: format!("item-{}", slot.host.hostid),
            }),
            match_status: MatchStatus::Exact,
            applicability: Applicability::Editable,
            reasons: Vec::new(),
            differences: Vec::new(),
            evidence: None,
            candidates: Vec::new(),
            decision: None,
            check: CloneBindingCheck::Checked,
            check_reason: None,
        })
        .collect::<Vec<_>>();

    let mapping = SourceHostMapping {
        role_id: SourceRoleId("postgres".to_owned()),
        source_host: host("100"),
        decision: SourceHostDecision::TargetSlots(slots),
        references: vec![reference(targets)],
    };

    let SourceHostDecision::TargetSlots(slots) = &mapping.decision else {
        panic!("target slots");
    };
    assert_eq!(slots[0].slot_id.0, "slot-b");
    assert_eq!(slots[1].slot_id.0, "slot-a");
    assert_eq!(mapping.references[0].targets[0].slot_id.0, "slot-b");
    assert_eq!(mapping.references[0].targets[1].slot_id.0, "slot-a");
}

#[test]
fn match_and_applicability_are_independent_axes() {
    let exact_but_read_only = TargetMetricResolution {
        slot_id: TargetSlotId("slot-1".to_owned()),
        target: Some(TargetValue::Item {
            itemid: "40".to_owned(),
        }),
        match_status: MatchStatus::Exact,
        applicability: Applicability::ReadOnly,
        reasons: Vec::new(),
        differences: Vec::new(),
        evidence: None,
        candidates: Vec::new(),
        decision: None,
        check: CloneBindingCheck::Checked,
        check_reason: None,
    };
    let unavailable_but_editable = TargetMetricResolution::not_found_or_no_access(
        TargetSlotId("slot-2".to_owned()),
        Applicability::Editable,
    );

    assert_eq!(exact_but_read_only.match_status, MatchStatus::Exact);
    assert_eq!(exact_but_read_only.applicability, Applicability::ReadOnly);
    assert_eq!(unavailable_but_editable.match_status, MatchStatus::Denied);
    assert_eq!(
        unavailable_but_editable.applicability,
        Applicability::Editable
    );
    assert_eq!(
        unavailable_but_editable.reasons,
        vec![ReasonCode::NotFoundOrNoAccess]
    );
}

#[test]
fn explicit_keep_source_is_informational_and_has_no_target_resolution() {
    let mapping = SourceHostMapping {
        role_id: SourceRoleId("shared-load-balancer".to_owned()),
        source_host: host("100"),
        decision: SourceHostDecision::KeepSource,
        references: vec![reference(Vec::new())],
    };

    let kept = mapping.explicit_kept_on_source();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].role_id.0, "shared-load-balancer");
    assert_eq!(kept[0].source_hostid, "100");
    assert!(mapping.references[0].targets.is_empty());

    let base = clone_snapshot(json!([{
        "widgetid": "20", "type": "item", "name": "Shared", "x": "0", "y": "0",
        "width": "12", "height": "4", "view_mode": "0", "fields": [
            {"type": "4", "name": "itemid.0", "value": "30"}
        ]
    }]));
    let local = materialize_clone(&base, &[mapping], &[], &[])
        .unwrap_or_else(|error| panic!("keep source: {error}"));
    assert_eq!(local.raw_api_document(), base.raw_api_document());
}

struct AreaFixture {
    seen_filter: Mutex<Option<HostSearchFilter>>,
    hosts: Vec<HostListing>,
    groups: Vec<HostGroupListing>,
}

impl TargetAreaLookup for AreaFixture {
    fn search_host_groups(
        &self,
        _search: &str,
        _limit: usize,
    ) -> impl Future<Output = Result<HostObjects<HostGroupListing>, ZabbixError>> + Send {
        ready(Ok(HostObjects {
            objects: self.groups.clone(),
            truncated: false,
        }))
    }

    fn search_hosts_filtered(
        &self,
        filter: &HostSearchFilter,
        _limit: usize,
    ) -> impl Future<Output = Result<HostObjects<HostListing>, ZabbixError>> + Send {
        *self
            .seen_filter
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(filter.clone());
        ready(Ok(HostObjects {
            objects: self.hosts.clone(),
            truncated: false,
        }))
    }
}

#[tokio::test]
async fn target_search_uses_exact_filters_and_reports_template_coverage() {
    let fixture = AreaFixture {
        seen_filter: Mutex::new(None),
        hosts: vec![
            listing("102", &[("901", "Linux"), ("902", "PostgreSQL")]),
            listing("101", &[("901", "Linux")]),
        ],
        groups: vec![HostGroupListing {
            groupid: "81".to_owned(),
            name: "Pilot/DB".to_owned(),
        }],
    };
    let filter = HostSearchFilter {
        name: "pg-b".to_owned(),
        groupid: Some("81".to_owned()),
        tags: vec![ExactHostTag {
            tag: "role".to_owned(),
            value: "pgsql".to_owned(),
        }],
    };
    let source_templates = BTreeSet::from(["901".to_owned(), "902".to_owned()]);

    let groups = search_target_groups(&fixture, "Pilot", 20)
        .await
        .unwrap_or_else(|error| panic!("groups: {error}"));
    let found = search_target_hosts(&fixture, &filter, &source_templates, 20)
        .await
        .unwrap_or_else(|error| panic!("hosts: {error}"));

    assert_eq!(groups.objects[0].groupid, "81");
    assert_eq!(found.hosts[0].source_templates_found, 2);
    assert_eq!(found.hosts[1].source_templates_found, 1);
    assert_eq!(found.empty_reason, None);
    assert_eq!(
        fixture
            .seen_filter
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref(),
        Some(&filter)
    );
}

#[test]
fn frozen_selection_preserves_order_and_cannot_auto_add_hosts() {
    let selection = FrozenTargetSelection::new(vec![
        listing("102", &[]),
        listing("101", &[]),
        listing("103", &[]),
    ])
    .unwrap_or_else(|error| panic!("selection: {error}"));
    assert_eq!(
        selection
            .hosts()
            .iter()
            .map(|host| host.hostid.as_str())
            .collect::<Vec<_>>(),
        ["102", "101", "103"]
    );
    assert_eq!(
        FrozenTargetSelection::new(vec![listing("101", &[]), listing("101", &[])])
            .err()
            .map(|error| error.code()),
        Some("target_host_duplicate")
    );
    assert_eq!(
        FrozenTargetSelection::new(Vec::new())
            .err()
            .map(|error| error.code()),
        Some("target_hosts_empty")
    );
}

#[tokio::test]
async fn empty_search_does_not_claim_that_permissions_are_the_cause() {
    let fixture = AreaFixture {
        seen_filter: Mutex::new(None),
        hosts: Vec::new(),
        groups: Vec::new(),
    };
    let found = search_target_hosts(&fixture, &HostSearchFilter::default(), &BTreeSet::new(), 20)
        .await
        .unwrap_or_else(|error| panic!("hosts: {error}"));
    assert_eq!(found.empty_reason, Some("not_found_or_no_access"));
}

#[test]
fn mapping_revisions_survive_reopen_and_undo_redo_preserve_order() {
    let (directory, service) = storage_setup();
    let first = service
        .save_clone_mapping(LOCAL, save_request(0, vec![mapped(&[("slot-a", "101")])]))
        .unwrap_or_else(|error| panic!("first: {error}"));
    assert_eq!(first.state.mapping_revision, 1);
    assert_eq!(first.workcopy_revision, 1);

    let second_mapping = mapped(&[("slot-b", "102"), ("slot-a", "101")]);
    let second = service
        .save_clone_mapping(LOCAL, save_request(1, vec![second_mapping.clone()]))
        .unwrap_or_else(|error| panic!("second: {error}"));
    assert_eq!(second.state.mapping_revision, 2);
    assert_eq!(second.state.mappings[0], second_mapping);

    let reopened = WorkcopyService::new(directory.path().to_path_buf());
    let reopened_state = reopened
        .clone_mapping(LOCAL)
        .unwrap_or_else(|error| panic!("reopen: {error}"))
        .unwrap_or_else(|| panic!("stored mapping"));
    assert_eq!(reopened_state, second);

    let undone = reopened
        .undo_clone_mapping(LOCAL, reopened_state.workcopy_revision)
        .unwrap_or_else(|error| panic!("undo: {error}"));
    assert_eq!(undone.removed, second.state);
    assert_eq!(undone.workcopy_revision, 3);
    assert_eq!(
        undone
            .current
            .as_ref()
            .map(|record| record.state.mapping_revision),
        Some(1)
    );

    let redo = reopened
        .save_clone_mapping(
            LOCAL,
            SaveCloneMappingRequest::redo(undone.workcopy_revision, &undone.removed),
        )
        .unwrap_or_else(|error| panic!("redo: {error}"));
    assert_eq!(redo.state.mapping_revision, 4);
    assert_eq!(redo.state.mappings, second.state.mappings);
    assert_eq!(
        redo.state.connection_id, second.state.connection_id,
        "redo remains bound to the same connection"
    );
}

#[test]
fn stale_or_invalid_mapping_is_rejected_without_changing_the_workcopy() {
    let (directory, service) = storage_setup();
    let first = service
        .save_clone_mapping(LOCAL, save_request(0, vec![mapped(&[("slot-a", "101")])]))
        .unwrap_or_else(|error| panic!("first: {error}"));
    let path = Path::new(directory.path())
        .join("workcopies")
        .join(LOCAL)
        .join("workcopy.json");
    let before = std::fs::read(&path).unwrap_or_else(|error| panic!("read before: {error}"));

    let stale = service
        .save_clone_mapping(LOCAL, save_request(0, vec![mapped(&[("slot-b", "102")])]))
        .err()
        .unwrap_or_else(|| panic!("stale revision accepted"));
    assert_eq!(stale.code(), "workcopy_changed_concurrently");

    let mut invalid = mapped(&[("slot-b", "102")]);
    invalid.references[0].targets.clear();
    let rejected = service
        .save_clone_mapping(LOCAL, save_request(first.workcopy_revision, vec![invalid]))
        .err()
        .unwrap_or_else(|| panic!("invalid mapping accepted"));
    assert_eq!(rejected.code(), "clone_mapping_invalid");
    assert_eq!(
        std::fs::read(&path).unwrap_or_else(|error| panic!("read after: {error}")),
        before
    );
}

#[test]
fn clone_intent_binds_current_source_target_and_mapping_without_a_plan() {
    let (directory, service) = storage_setup();
    let saved = service
        .save_clone_mapping(
            LOCAL,
            save_request(0, vec![mapped(&[("pilot-a", "101"), ("pilot-b", "102")])]),
        )
        .unwrap_or_else(|error| panic!("mapping: {error}"));
    let selection = FrozenTargetSelection::new(vec![
        listing("101", &[("901", "PostgreSQL")]),
        listing("102", &[("901", "PostgreSQL")]),
    ])
    .unwrap_or_else(|error| panic!("selection: {error}"));
    let intent = service
        .prepare_clone_intent(PrepareCloneIntentRequest {
            source_workcopy_id: LOCAL.to_owned(),
            target_connection_id: CONNECTION.to_owned(),
            target_name: "DB Pilot copy".to_owned(),
            target_selection: selection,
            expected_mapping_revision: saved.state.mapping_revision,
        })
        .unwrap_or_else(|error| panic!("intent: {error}"));

    assert_eq!(intent.source().source_workcopy_id, LOCAL);
    assert_eq!(
        intent.source().source_workcopy_revision,
        saved.workcopy_revision
    );
    assert_eq!(intent.source().connection_id, CONNECTION);
    assert_eq!(intent.source().dashboardid, "700");
    assert_eq!(
        intent.source().base_fingerprint,
        saved.state.base_fingerprint.clone().unwrap_or_default()
    );
    assert_eq!(intent.source().local_fingerprint, IDENTITY);
    assert_eq!(intent.target().connection_id, CONNECTION);
    assert_eq!(intent.target().hosts[0].hostid, "101");
    assert_eq!(intent.target().hosts[1].hostid, "102");
    assert_eq!(intent.target_name(), "DB Pilot copy");
    assert_eq!(intent.mapping_revision(), saved.state.mapping_revision);
    assert_eq!(intent.server_identity_fingerprint(), IDENTITY);
    assert_eq!(intent.server_version(), "7.4.16");

    let path = directory
        .path()
        .join("workcopies")
        .join(LOCAL)
        .join("workcopy.json");
    let before = std::fs::read(&path).unwrap_or_else(|error| panic!("source: {error}"));
    let draft = service
        .begin_clone_workcopy(intent.clone())
        .unwrap_or_else(|error| panic!("begin clone: {error}"));
    assert_eq!(draft.intent(), &intent);
    assert_eq!(draft.base().fingerprint(), intent.source().base_fingerprint);
    assert_eq!(draft.mapping(), &saved.state);
    assert_eq!(draft.cancel(), intent);
    assert_eq!(
        std::fs::read(&path).unwrap_or_else(|error| panic!("source after cancel: {error}")),
        before
    );

    service
        .rename_draft(LOCAL, "Renamed source")
        .unwrap_or_else(|error| panic!("rename: {error}"));
    let refusal = service
        .begin_clone_workcopy(intent)
        .err()
        .unwrap_or_else(|| panic!("stale intent accepted"));
    assert_eq!(refusal.code(), "clone_intent_source_changed");
}

#[test]
fn clone_intent_rejects_cross_connection_stale_mapping_and_changed_selection() {
    const OTHER_CONNECTION: &str = "conn-ffffffffffffffffffffffffffffffff";

    let (_directory, service) = storage_setup();
    let saved = service
        .save_clone_mapping(
            LOCAL,
            save_request(0, vec![mapped(&[("pilot-a", "101"), ("pilot-b", "102")])]),
        )
        .unwrap_or_else(|error| panic!("mapping: {error}"));
    let request =
        |connection: &str, revision: u64, hostids: &[&str], name: &str| PrepareCloneIntentRequest {
            source_workcopy_id: LOCAL.to_owned(),
            target_connection_id: connection.to_owned(),
            target_name: name.to_owned(),
            target_selection: FrozenTargetSelection::new(
                hostids.iter().map(|hostid| listing(hostid, &[])).collect(),
            )
            .unwrap_or_else(|error| panic!("selection: {error}")),
            expected_mapping_revision: revision,
        };

    let cases = [
        (
            request(
                OTHER_CONNECTION,
                saved.state.mapping_revision,
                &["101", "102"],
                "Copy",
            ),
            "clone_intent_connection_changed",
        ),
        (
            request(
                CONNECTION,
                saved.state.mapping_revision + 1,
                &["101", "102"],
                "Copy",
            ),
            "clone_mapping_changed",
        ),
        (
            request(CONNECTION, saved.state.mapping_revision, &["101"], "Copy"),
            "clone_intent_target_selection_changed",
        ),
        (
            request(
                CONNECTION,
                saved.state.mapping_revision,
                &["102", "101"],
                "Copy",
            ),
            "clone_intent_target_selection_changed",
        ),
        (
            request(
                CONNECTION,
                saved.state.mapping_revision,
                &["101", "102"],
                " Copy ",
            ),
            "clone_intent_target_name_invalid",
        ),
    ];
    for (request, expected) in cases {
        let error = service
            .prepare_clone_intent(request)
            .err()
            .unwrap_or_else(|| panic!("{expected} accepted"));
        assert_eq!(error.code(), expected);
    }
}

#[test]
fn clone_workcopy_reopens_and_only_verified_read_back_links_the_source() {
    use crate::publish_journal::{
        Finish, JournalSource, JournalTarget, OperationKind, OperationRecord,
    };
    use crate::{JournalState, PublishJournal};

    let base = clone_snapshot(json!([crate::resolver::tests::widget(
        "20",
        "item",
        vec![json!({"type":"4", "name":"itemid.0", "value":"30"})],
    )]));
    let mappings = vec![mapped(&[("slot", "101")])];
    let expected = materialize_clone(&base, &mappings, &[], &[])
        .unwrap_or_else(|error| panic!("Local: {error}"));
    let (directory, service) = storage_setup_with_snapshot(base);
    let mut request = save_request(0, mappings);
    request.local_fingerprint = Some(expected.fingerprint().to_owned());
    let saved = service
        .save_clone_mapping(LOCAL, request)
        .unwrap_or_else(|error| panic!("mapping: {error}"));
    let intent = service
        .prepare_clone_intent(PrepareCloneIntentRequest {
            source_workcopy_id: LOCAL.to_owned(),
            target_connection_id: CONNECTION.to_owned(),
            target_name: "Clone result".to_owned(),
            target_selection: FrozenTargetSelection::new(vec![listing("101", &[])])
                .unwrap_or_else(|error| panic!("selection: {error}")),
            expected_mapping_revision: saved.state.mapping_revision,
        })
        .unwrap_or_else(|error| panic!("intent: {error}"));
    let draft = service
        .begin_clone_workcopy(intent.clone())
        .and_then(|draft| service.save_clone_workcopy(draft, &[]))
        .unwrap_or_else(|error| panic!("save branch: {error}"));
    let local_id = draft.local_id().to_owned();
    assert_ne!(local_id, LOCAL);
    assert!(service.draft(&local_id).is_err());

    let reopened = WorkcopyService::new(directory.path().to_path_buf());
    let copy = reopened
        .open_clone_workcopy(&local_id)
        .unwrap_or_else(|error| panic!("reopen branch: {error}"));
    assert_eq!(copy.draft().intent(), &intent);
    assert_eq!(copy.draft().mapping(), &saved.state);
    assert_eq!(copy.local().raw_api_document(), expected.raw_api_document());
    assert_eq!(
        copy.draft().base().fingerprint(),
        draft.draft().base().fingerprint()
    );

    let journal = PublishJournal::new(directory.path().to_path_buf());
    let operation = "zds-0123456789abcdef0123456789abcdef";
    let record = OperationRecord {
        kind: OperationKind::Create,
        intent: crate::clone_dashboard::CLONE_OPERATION,
        connection_id: CONNECTION.to_owned(),
        server_version: "7.4.16".to_owned(),
        endpoint_fingerprint: IDENTITY.to_owned(),
        server_identity: IDENTITY.to_owned(),
        target: JournalTarget::NewDashboard {
            name: intent.target_name().to_owned(),
        },
        source: Some(JournalSource {
            local_id: local_id.clone(),
            revision: intent.mapping_revision(),
        }),
        source_fingerprint: intent.source().local_fingerprint.clone(),
        expected_fingerprint: IDENTITY.to_owned(),
        owner_userid: Some("1".to_owned()),
        plan_created_at_ms: 1,
        confirmed_at_ms: 2,
    };
    let mut handle = journal
        .begin(operation, &record)
        .unwrap_or_else(|error| panic!("journal: {error}"));
    assert!(handle.admit_send());
    assert!(
        reopened
            .record_clone_read_back(&journal, operation)
            .is_err()
    );
    assert!(
        reopened
            .load(LOCAL)
            .unwrap_or_else(|e| panic!("{e}"))
            .created_dashboards
            .is_empty()
    );
    handle
        .finish_with(Finish {
            observed_fingerprint: Some(IDENTITY.to_owned()),
            created_dashboardid: Some("701".to_owned()),
            ..Finish::state(JournalState::Verified)
        })
        .unwrap_or_else(|error| panic!("verify: {error}"));
    for _ in 0..2 {
        reopened
            .record_clone_read_back(&journal, operation)
            .unwrap_or_else(|error| panic!("link: {error}"));
    }
    let source = WorkcopyService::new(directory.path().to_path_buf())
        .load(LOCAL)
        .unwrap_or_else(|error| panic!("source: {error}"));
    assert_eq!(source.created_dashboards.len(), 1);
    assert_eq!(source.created_dashboards[0].dashboardid, "701");
    assert_eq!(source.revision, saved.workcopy_revision + 1);
    let stale = reopened
        .open_clone_workcopy(&local_id)
        .unwrap_or_else(|error| panic!("reopen after source revision changed: {error}"));
    assert_eq!(stale.local().fingerprint(), expected.fingerprint());
    assert!(reopened.preview_clone_local(&local_id).is_err());

    let branch_path = directory
        .path()
        .join("clone-workcopies")
        .join(&local_id)
        .join("workcopy.json");
    let bytes = std::fs::read(&branch_path).unwrap_or_else(|error| panic!("branch bytes: {error}"));
    let mut broken: Value =
        serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("branch JSON: {error}"));
    broken["source"]["base_fingerprint"] = json!(IDENTITY);
    let broken = serde_json::to_vec(&broken).unwrap_or_else(|error| panic!("encode: {error}"));
    std::fs::write(&branch_path, &broken)
        .unwrap_or_else(|error| panic!("corrupt fixture: {error}"));
    let error = reopened
        .open_clone_workcopy(&local_id)
        .err()
        .unwrap_or_else(|| panic!("opened a corrupt branch"));
    assert_eq!(error.code(), "clone_workcopy_corrupt");
    assert_eq!(
        std::fs::read(&branch_path).unwrap_or_else(|error| panic!("branch bytes: {error}")),
        broken
    );
    assert_eq!(
        reopened
            .load(LOCAL)
            .unwrap_or_else(|error| panic!("source: {error}")),
        source
    );
}

fn resolved_item(slot_id: &str, itemid: &str) -> TargetMetricResolution {
    TargetMetricResolution {
        slot_id: TargetSlotId(slot_id.to_owned()),
        target: Some(TargetValue::Item {
            itemid: itemid.to_owned(),
        }),
        match_status: MatchStatus::Exact,
        applicability: Applicability::Editable,
        reasons: Vec::new(),
        differences: Vec::new(),
        evidence: None,
        candidates: Vec::new(),
        decision: None,
        check: CloneBindingCheck::Checked,
        check_reason: None,
    }
}

fn resolved_graph(slot_id: &str, graphid: &str) -> TargetMetricResolution {
    TargetMetricResolution {
        slot_id: TargetSlotId(slot_id.to_owned()),
        target: Some(TargetValue::Graph {
            graphid: graphid.to_owned(),
        }),
        match_status: MatchStatus::Exact,
        applicability: Applicability::Editable,
        reasons: Vec::new(),
        differences: Vec::new(),
        evidence: None,
        candidates: Vec::new(),
        decision: None,
        check: CloneBindingCheck::Checked,
        check_reason: None,
    }
}

fn resolved_host_pattern(slot_id: &str, pattern: &str) -> TargetMetricResolution {
    TargetMetricResolution {
        slot_id: TargetSlotId(slot_id.to_owned()),
        target: Some(TargetValue::HostPattern {
            pattern: pattern.to_owned(),
        }),
        match_status: MatchStatus::Exact,
        applicability: Applicability::Editable,
        reasons: Vec::new(),
        differences: Vec::new(),
        evidence: None,
        candidates: Vec::new(),
        decision: None,
        check: CloneBindingCheck::Checked,
        check_reason: None,
    }
}

fn source_series(slot_id: &str, field_name: &str) -> SourceSeries {
    SourceSeries {
        slot_id: TargetSlotId(slot_id.to_owned()),
        field: field(field_name),
    }
}

#[test]
fn cardinality_two_to_three_keeps_stable_slots_and_adds_from_the_chosen_template() {
    let request = CardinalityRequest {
        widget_type: "svggraph".to_owned(),
        source_slot_ids: vec![
            TargetSlotId("pg-a".to_owned()),
            TargetSlotId("pg-b".to_owned()),
        ],
        target_slots: vec![
            TargetSlot {
                slot_id: TargetSlotId("pg-b".to_owned()),
                host: host("102"),
            },
            TargetSlot {
                slot_id: TargetSlotId("pg-a".to_owned()),
                host: host("101"),
            },
            TargetSlot {
                slot_id: TargetSlotId("pg-c".to_owned()),
                host: host("103"),
            },
        ],
        datasets: vec![SvgDatasetCardinality {
            dataset_index: 0,
            binding: SeriesBindingKind::ExplicitItem,
            source_series: vec![
                source_series("pg-a", "ds.0.itemids.0"),
                source_series("pg-b", "ds.0.itemids.1"),
            ],
            template_slot_id: Some(TargetSlotId("pg-a".to_owned())),
            target_resolutions: vec![
                resolved_item("pg-b", "902"),
                resolved_item("pg-a", "901"),
                resolved_item("pg-c", "903"),
            ],
            max_series: 20,
        }],
    };

    let plan = plan_cardinality(&request);
    assert!(plan.is_ready(), "blockers: {:?}", plan.blockers);
    assert_eq!(plan.datasets.len(), 1);
    let dataset = &plan.datasets[0];
    assert!(dataset.removed.is_empty());
    assert_eq!(
        dataset
            .series
            .iter()
            .map(|series| (series.position, series.slot_id.0.as_str()))
            .collect::<Vec<_>>(),
        [(0, "pg-b"), (1, "pg-a"), (2, "pg-c")]
    );
    assert_eq!(
        dataset.series[0].origin,
        SeriesOrigin::Existing {
            source_field: field("ds.0.itemids.1")
        }
    );
    assert_eq!(
        dataset.series[1].origin,
        SeriesOrigin::Existing {
            source_field: field("ds.0.itemids.0")
        }
    );
    assert_eq!(
        dataset.series[2].origin,
        SeriesOrigin::Added {
            template_field: field("ds.0.itemids.0")
        }
    );
    assert_eq!(
        dataset.series[2].target,
        TargetValue::Item {
            itemid: "903".to_owned()
        }
    );
}

#[test]
fn cardinality_remove_and_reorder_report_the_removed_source_series() {
    let request = CardinalityRequest {
        widget_type: "svggraph".to_owned(),
        source_slot_ids: vec![
            TargetSlotId("pg-a".to_owned()),
            TargetSlotId("pg-b".to_owned()),
        ],
        target_slots: vec![TargetSlot {
            slot_id: TargetSlotId("pg-b".to_owned()),
            host: host("102"),
        }],
        datasets: vec![SvgDatasetCardinality {
            dataset_index: 3,
            binding: SeriesBindingKind::HostPattern,
            source_series: vec![
                source_series("pg-a", "ds.3.hosts.0"),
                source_series("pg-b", "ds.3.hosts.1"),
            ],
            template_slot_id: None,
            target_resolutions: vec![resolved_host_pattern("pg-b", "Pilot PostgreSQL 2")],
            max_series: 20,
        }],
    };

    let plan = plan_cardinality(&request);
    assert!(plan.is_ready(), "blockers: {:?}", plan.blockers);
    assert_eq!(plan.datasets[0].series[0].slot_id.0, "pg-b");
    assert_eq!(plan.datasets[0].removed.len(), 1);
    assert_eq!(plan.datasets[0].removed[0].slot_id.0, "pg-a");
    assert_eq!(
        plan.datasets[0].removed[0].source_field,
        field("ds.3.hosts.0")
    );
}

#[test]
fn cardinality_reports_single_host_and_every_required_svggraph_blocker() {
    let single_host = plan_cardinality(&CardinalityRequest {
        widget_type: "item".to_owned(),
        source_slot_ids: vec![TargetSlotId("pg-a".to_owned())],
        target_slots: vec![
            TargetSlot {
                slot_id: TargetSlotId("pg-a".to_owned()),
                host: host("101"),
            },
            TargetSlot {
                slot_id: TargetSlotId("pg-b".to_owned()),
                host: host("102"),
            },
        ],
        datasets: Vec::new(),
    });
    assert_eq!(
        single_host.blockers,
        vec![CardinalityBlocker {
            code: CardinalityBlockerCode::SingleHostWidget,
            dataset_index: None,
            slot_id: None,
        }]
    );

    let mut incompatible = resolved_item("pg-a", "901");
    incompatible.match_status = MatchStatus::Compatible;
    incompatible.applicability = Applicability::ReadOnly;
    incompatible.differences = vec![Difference::ValueType, Difference::Units];
    let missing = TargetMetricResolution::not_found_or_no_access(
        TargetSlotId("pg-b".to_owned()),
        Applicability::Editable,
    );
    let blocked = plan_cardinality(&CardinalityRequest {
        widget_type: "svggraph".to_owned(),
        source_slot_ids: vec![TargetSlotId("pg-a".to_owned())],
        target_slots: vec![
            TargetSlot {
                slot_id: TargetSlotId("pg-a".to_owned()),
                host: host("101"),
            },
            TargetSlot {
                slot_id: TargetSlotId("pg-b".to_owned()),
                host: host("101"),
            },
        ],
        datasets: vec![SvgDatasetCardinality {
            dataset_index: 0,
            binding: SeriesBindingKind::ExplicitItem,
            source_series: vec![source_series("pg-a", "ds.0.itemids.0")],
            template_slot_id: Some(TargetSlotId("unknown-template".to_owned())),
            target_resolutions: vec![incompatible, missing],
            max_series: 1,
        }],
    });
    let codes = blocked
        .blockers
        .iter()
        .map(|blocker| blocker.code)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        codes,
        BTreeSet::from([
            CardinalityBlockerCode::DuplicateHost,
            CardinalityBlockerCode::IncompatibleUnits,
            CardinalityBlockerCode::IncompatibleValueType,
            CardinalityBlockerCode::MetricMissing,
            CardinalityBlockerCode::SeriesLimit,
            CardinalityBlockerCode::TargetNotEditable,
            CardinalityBlockerCode::TemplateSeriesMissing,
        ])
    );

    let empty = plan_cardinality(&CardinalityRequest {
        widget_type: "svggraph".to_owned(),
        source_slot_ids: vec![TargetSlotId("pg-a".to_owned())],
        target_slots: Vec::new(),
        datasets: Vec::new(),
    });
    assert_eq!(
        empty.blockers[0].code,
        CardinalityBlockerCode::EmptyMandatoryRole
    );
}

#[test]
fn clone_reference_guard_preserves_local_links_and_rejects_invalid_graphs() {
    use crate::validate_clone_references;
    let widget = |id: &str, definition: &str, consumer: Option<&str>| {
        let mut fields = vec![json!({"type":"1", "name":"reference", "value":definition})];
        if let Some(value) = consumer {
            fields.push(json!({"type":"1", "name":"itemid._reference", "value":value}));
        }
        crate::resolver::tests::widget(id, "item", fields)
    };
    let valid = clone_snapshot(json!([
        widget("20", "AAAAA", Some("DASHBOARD._hostid")),
        widget("21", "BBBBB", Some("AAAAA._itemid"))
    ]));
    let before = valid.raw_api_document().clone();
    assert!(validate_clone_references(&valid).is_ok());
    assert_eq!(valid.raw_api_document(), &before);
    let invalid = [
        (
            "clone_reference_missing",
            widget("21", "BBBBB", Some("CCCCC._itemid")),
        ),
        ("clone_reference_duplicate", widget("21", "AAAAA", None)),
        (
            "clone_reference_cycle",
            widget("21", "BBBBB", Some("BBBBB._itemid")),
        ),
        (
            "clone_reference_source_widget_id",
            widget("21", "BBBBB", Some("20._itemid")),
        ),
    ];
    for (code, second) in invalid {
        let dashboard = clone_snapshot(json!([widget("20", "AAAAA", None), second]));
        let error = validate_clone_references(&dashboard)
            .err()
            .unwrap_or_else(|| panic!("accepted {code}"));
        assert_eq!(error.code(), code);
    }
}

#[test]
fn clone_preview_checks_saved_local_and_binds_the_kept_list() {
    let mut problems = crate::resolver::tests::widget(
        "21",
        "problems",
        vec![json!({"type":"3", "name":"hostids.0", "value":"100"})],
    );
    problems["x"] = json!("12");
    let base = clone_snapshot(json!([
        crate::resolver::tests::widget(
            "20",
            "item",
            vec![
                json!({"type":"4", "name":"itemid.0", "value":"30"}),
                json!({"type":"1", "name":"reference", "value":"AAAAA"}),
            ],
        ),
        problems
    ]));
    let mapping = vec![mapped(&[("slot", "101")])];
    let expected = materialize_clone(&base, &mapping, &[], &[])
        .unwrap_or_else(|error| panic!("expected Local: {error}"));
    for (fingerprint, should_pass) in [(expected.fingerprint(), true), (IDENTITY, false)] {
        let (_directory, service) = storage_setup_with_snapshot(base.clone());
        let mut request = save_request(0, mapping.clone());
        request.local_fingerprint = Some(fingerprint.to_owned());
        let saved = service
            .save_clone_mapping(LOCAL, request)
            .unwrap_or_else(|error| panic!("mapping: {error}"));
        let intent = service
            .prepare_clone_intent(PrepareCloneIntentRequest {
                source_workcopy_id: LOCAL.to_owned(),
                target_connection_id: CONNECTION.to_owned(),
                target_name: "Clone preview".to_owned(),
                target_selection: FrozenTargetSelection::new(vec![listing("101", &[])])
                    .unwrap_or_else(|error| panic!("selection: {error}")),
                expected_mapping_revision: saved.state.mapping_revision,
            })
            .unwrap_or_else(|error| panic!("intent: {error}"));
        let pinned = service
            .begin_clone_workcopy(intent)
            .and_then(|draft| service.save_clone_workcopy(draft, &[]));
        if should_pass {
            let draft = pinned.unwrap_or_else(|error| panic!("branch: {error}"));
            let preview = service
                .preview_clone_local(draft.local_id())
                .unwrap_or_else(|error| panic!("preview: {error}"));
            assert_eq!(preview.local().fingerprint(), expected.fingerprint());
            assert!(preview.missing_contracts().is_empty());
            assert_eq!(preview.kept_on_source().len(), 1);
            assert_eq!(preview.kept_on_source()[0].field.field_name, "hostids.0");
            assert!(preview.kept_digest().is_some());
            assert!(preview.check_confirmation(None).is_err());
            assert!(preview.check_confirmation(Some(IDENTITY)).is_err());
            assert!(preview.check_confirmation(preview.kept_digest()).is_ok());
            let intent = draft.draft().intent();
            let snapshot = |raw| {
                VersionedDashboardSnapshot::from_adapter_raw(CONNECTION.to_owned(), version(), raw)
                    .unwrap_or_else(|error| panic!("read-back fixture: {error}"))
            };
            let mut created = preview.local().raw_api_document().clone();
            created["name"] = json!(intent.target_name());
            let expected = zdashstitch_zabbix::CreateExpectation {
                connection_id: CONNECTION.to_owned(),
                server_version: version(),
                name: intent.target_name().to_owned(),
                owner_userid: "1".to_owned(),
                content_fingerprint: zdashstitch_zabbix::dashboard_content_fingerprint(&snapshot(
                    created.clone(),
                ))
                .unwrap_or_else(|| panic!("content fingerprint")),
            };
            use crate::CloneReadBackVerdict;
            use zdashstitch_zabbix::ReadBackVerdict;
            assert_eq!(
                intent.compare_read_back(&expected, &snapshot(created.clone())),
                CloneReadBackVerdict::SourceDashboard
            );
            created["dashboardid"] = json!("701");
            created["pages"][0]["dashboard_pageid"] = json!("11");
            created["pages"][0]["widgets"][0]["widgetid"] = json!("42");
            assert_eq!(
                intent.compare_read_back(&expected, &snapshot(created.clone())),
                CloneReadBackVerdict::Copy(ReadBackVerdict::Matches)
            );
            for (field, value, verdict) in [
                ("private", json!("0"), ReadBackVerdict::NotPrivate),
                ("userid", json!("2"), ReadBackVerdict::OwnerDiffers),
                (
                    "users",
                    json!([{"userid": "2", "permission": "2"}]),
                    ReadBackVerdict::Shared,
                ),
                (
                    "userGroups",
                    json!([{"usrgrpid": "2", "permission": "2"}]),
                    ReadBackVerdict::Shared,
                ),
            ] {
                let mut bad_access = created.clone();
                bad_access[field] = value;
                assert_eq!(
                    intent.compare_read_back(&expected, &snapshot(bad_access)),
                    CloneReadBackVerdict::Copy(verdict)
                );
            }
            created["pages"][0]["widgets"][0]["fields"][1]["value"] = json!("ZZZZZ");
            assert_eq!(
                intent.compare_read_back(&expected, &snapshot(created)),
                CloneReadBackVerdict::Copy(ReadBackVerdict::ContentDiffers)
            );
        } else {
            let error = pinned
                .err()
                .unwrap_or_else(|| panic!("accepted wrong Local fingerprint"));
            assert_eq!(error.code(), "clone_local_changed");
        }
    }
}

#[test]
fn clone_preview_blocks_an_unknown_widget_and_preserves_the_saved_local() {
    let widget = crate::resolver::tests::widget(
        "20",
        "vendor.synthetic",
        vec![json!({"type":"1", "name":"opaque_field", "value":"synthetic"})],
    );
    let base = clone_snapshot(json!([widget]));
    let (_directory, service) = storage_setup_with_snapshot(base.clone());
    let mut row = mapped(&[("slot", "101")]);
    row.references.clear();
    let mut request = save_request(0, vec![row]);
    request.local_fingerprint = Some(base.fingerprint().to_owned());
    let mapping = service
        .save_clone_mapping(LOCAL, request)
        .unwrap_or_else(|error| panic!("mapping: {error}"));
    let branch = service
        .prepare_clone_intent(PrepareCloneIntentRequest {
            source_workcopy_id: LOCAL.to_owned(),
            target_connection_id: CONNECTION.to_owned(),
            target_name: "Clone preview".to_owned(),
            target_selection: FrozenTargetSelection::new(vec![listing("101", &[])])
                .unwrap_or_else(|error| panic!("selection: {error}")),
            expected_mapping_revision: mapping.state.mapping_revision,
        })
        .and_then(|intent| service.begin_clone_workcopy(intent))
        .and_then(|draft| service.save_clone_workcopy(draft, &[]))
        .unwrap_or_else(|error| panic!("branch: {error}"));
    let preview = service
        .preview_clone_local(branch.local_id())
        .unwrap_or_else(|error| panic!("preview: {error}"));
    assert_eq!(preview.local().raw_api_document(), base.raw_api_document());
    assert!(
        preview
            .missing_contracts()
            .contains(&"create_copy_unknown_widgets")
    );
    assert_eq!(
        preview
            .check_confirmation(None)
            .err()
            .map(|error| error.code()),
        Some("write_contract_unverified")
    );
}

fn clone_snapshot(widgets: Value) -> VersionedDashboardSnapshot {
    VersionedDashboardSnapshot::from_adapter_raw(
        CONNECTION.to_owned(),
        version(),
        json!({
            "dashboardid": "700", "name": "Synthetic", "private": "1",
            "userid": "1", "display_period": "30", "auto_start": "1",
            "pages": [{
                "dashboard_pageid": "10", "name": "Page", "display_period": "0",
                "widgets": widgets
            }],
            "users": [], "userGroups": []
        }),
    )
    .unwrap_or_else(|error| panic!("snapshot: {error}"))
}

fn mapping_reference(
    field_name: &str,
    widget_type: &str,
    source: SourceValue,
    targets: Vec<TargetMetricResolution>,
) -> ReferenceMapping {
    ReferenceMapping {
        field: field(field_name),
        widget_type: widget_type.to_owned(),
        source,
        source_evidence: None,
        targets,
    }
}

fn mapping_for_reference(
    role: &str,
    source_hostid: &str,
    slots: Vec<TargetSlot>,
    reference: ReferenceMapping,
) -> SourceHostMapping {
    SourceHostMapping {
        role_id: SourceRoleId(role.to_owned()),
        source_host: host(source_hostid),
        decision: SourceHostDecision::TargetSlots(slots),
        references: vec![reference],
    }
}

fn slot(slot_id: &str, hostid: &str) -> TargetSlot {
    TargetSlot {
        slot_id: TargetSlotId(slot_id.to_owned()),
        host: host(hostid),
    }
}

fn field_values(snapshot: &VersionedDashboardSnapshot) -> Vec<(String, Value)> {
    snapshot.raw_api_document()["pages"][0]["widgets"][0]["fields"]
        .as_array()
        .unwrap_or_else(|| panic!("fields"))
        .iter()
        .map(|field| {
            (
                field["name"].as_str().unwrap_or_default().to_owned(),
                field["value"].clone(),
            )
        })
        .collect()
}

#[test]
fn clone_materialization_replaces_only_the_mapped_field_and_is_repeatable() {
    let base = clone_snapshot(json!([{
        "widgetid": "20", "type": "item", "name": "Value", "x": "0", "y": "0",
        "width": "12", "height": "4", "view_mode": "0", "fields": [
            {"type": "4", "name": "itemid.0", "value": "30"},
            {"type": "1", "name": "description", "value": "untouched"}
        ]
    }]));
    let target_slot = slot("pg-a", "101");
    let mappings = vec![mapping_for_reference(
        "postgres",
        "100",
        vec![target_slot],
        mapping_reference(
            "itemid.0",
            "item",
            SourceValue::Item {
                itemid: "30".to_owned(),
            },
            vec![resolved_item("pg-a", "901")],
        ),
    )];

    let first = materialize_clone(&base, &mappings, &[], &[])
        .unwrap_or_else(|error| panic!("materialize: {error}"));
    let second = materialize_clone(&base, &mappings, &[], &[])
        .unwrap_or_else(|error| panic!("materialize: {error}"));
    assert_eq!(
        base.raw_api_document()["pages"][0]["widgets"][0]["fields"][0]["value"],
        "30"
    );
    assert_eq!(
        first.raw_api_document()["pages"][0]["widgets"][0]["fields"][0]["value"],
        "901"
    );
    assert_eq!(
        first.raw_api_document()["pages"][0]["widgets"][0]["fields"][1]["value"],
        "untouched"
    );
    assert_eq!(first.raw_api_document(), second.raw_api_document());
    assert_eq!(first.fingerprint(), second.fingerprint());
}

#[test]
fn clone_materializes_sources_from_multiple_templates_on_one_page() {
    let base = clone_snapshot(json!([
        {
            "widgetid": "20", "type": "item", "name": "PostgreSQL", "x": "0", "y": "0",
            "width": "12", "height": "4", "view_mode": "0", "fields": [
                {"type": "4", "name": "itemid.0", "value": "30"}
            ]
        },
        {
            "widgetid": "21", "type": "item", "name": "Linux", "x": "12", "y": "0",
            "width": "12", "height": "4", "view_mode": "0", "fields": [
                {"type": "4", "name": "itemid.0", "value": "40"}
            ]
        }
    ]));
    let template_evidence = |itemid: &str, template_itemid: &str, template_name: &str| {
        Evidence::Item(ItemEvidence {
            itemid: itemid.to_owned(),
            key: format!("key.{itemid}"),
            name: format!("Item {itemid}"),
            value_type: 3,
            units: String::new(),
            origin: ItemOrigin::Template {
                template_itemid: template_itemid.to_owned(),
                template_name: Some(template_name.to_owned()),
            },
        })
    };
    let mut postgres = mapping_reference(
        "itemid.0",
        "item",
        SourceValue::Item {
            itemid: "30".to_owned(),
        },
        vec![resolved_item("postgres", "901")],
    );
    postgres.source_evidence = Some(template_evidence("30", "800", "PostgreSQL"));
    let mut linux = mapping_reference(
        "itemid.0",
        "item",
        SourceValue::Item {
            itemid: "40".to_owned(),
        },
        vec![resolved_item("linux", "902")],
    );
    linux.field = field_at(1, "21", "itemid.0");
    linux.source_evidence = Some(template_evidence("40", "801", "Linux"));
    let mappings = vec![
        mapping_for_reference("postgres", "100", vec![slot("postgres", "101")], postgres),
        mapping_for_reference("linux", "200", vec![slot("linux", "102")], linux),
    ];

    let local = materialize_clone(&base, &mappings, &[], &[])
        .unwrap_or_else(|error| panic!("materialize: {error}"));
    assert_eq!(
        local.raw_api_document()["pages"][0]["widgets"][0]["fields"][0]["value"],
        "901"
    );
    assert_eq!(
        local.raw_api_document()["pages"][0]["widgets"][1]["fields"][0]["value"],
        "902"
    );
    assert_eq!(
        local.raw_api_document()["pages"][0]["widgets"][1]["x"],
        "12"
    );
    assert_eq!(
        base.raw_api_document()["pages"][0]["widgets"][1]["fields"][0]["value"],
        "40"
    );
}

#[test]
fn clone_materialization_expands_host_patterns_two_to_three_without_touching_settings() {
    let base = clone_snapshot(json!([{
        "widgetid": "20", "type": "svggraph", "name": "PostgreSQL", "x": "0", "y": "0",
        "width": "36", "height": "8", "view_mode": "0", "fields": [
            {"type": "1", "name": "ds.0.hosts.0", "value": "visible 100"},
            {"type": "1", "name": "ds.0.hosts.1", "value": "Visible 200"},
            {"type": "1", "name": "ds.0.items.0", "value": "CPU *"},
            {"type": "1", "name": "ds.0.color.0", "value": "AA0000"},
            {"type": "0", "name": "ds.0.width", "value": "2"},
            {"type": "1", "name": "vendor.untouched", "value": "raw"}
        ]
    }]));
    let targets = vec![
        slot("pg-b", "102"),
        slot("pg-a", "101"),
        slot("pg-c", "103"),
    ];
    let plan = plan_cardinality(&CardinalityRequest {
        widget_type: "svggraph".to_owned(),
        source_slot_ids: vec![
            TargetSlotId("pg-a".to_owned()),
            TargetSlotId("pg-b".to_owned()),
        ],
        target_slots: targets.clone(),
        datasets: vec![SvgDatasetCardinality {
            dataset_index: 0,
            binding: SeriesBindingKind::HostPattern,
            source_series: vec![
                source_series("pg-a", "ds.0.hosts.0"),
                source_series("pg-b", "ds.0.hosts.1"),
            ],
            template_slot_id: Some(TargetSlotId("pg-a".to_owned())),
            target_resolutions: vec![
                resolved_host_pattern("pg-b", "Visible 102"),
                resolved_host_pattern("pg-a", "Visible 101"),
                resolved_host_pattern("pg-c", "Visible 103"),
            ],
            max_series: 20,
        }],
    });
    let mappings = vec![
        mapping_for_reference(
            "postgres",
            "100",
            vec![targets[1].clone(), targets[2].clone()],
            mapping_reference(
                "ds.0.hosts.0",
                "svggraph",
                SourceValue::HostPattern {
                    pattern: "visible 100".to_owned(),
                },
                vec![
                    resolved_host_pattern("pg-a", "Visible 101"),
                    resolved_host_pattern("pg-c", "Visible 103"),
                ],
            ),
        ),
        mapping_for_reference(
            "postgres",
            "200",
            vec![targets[0].clone()],
            mapping_reference(
                "ds.0.hosts.1",
                "svggraph",
                SourceValue::HostPattern {
                    pattern: "Visible 200".to_owned(),
                },
                vec![resolved_host_pattern("pg-b", "Visible 102")],
            ),
        ),
    ];
    let cardinality = CloneCardinalityTransform {
        widget: field("ds.0.hosts.0").widget,
        plan,
        added_item_colors: BTreeMap::new(),
    };

    let item_pattern = ClonePatternDecision::keep_as_is(ClonePatternRequirement {
        field: FieldAddress {
            widget: field("ds.0.items.0").widget,
            field_name: "ds.0.items.0".to_owned(),
        },
        widget_type: "svggraph".to_owned(),
        kind: ClonePatternKind::Item,
        source_pattern: "CPU *".to_owned(),
    });
    let local = materialize_clone(&base, &mappings, &[cardinality], &[item_pattern])
        .unwrap_or_else(|error| panic!("materialize: {error}"));
    let values = field_values(&local);
    assert_eq!(
        values,
        vec![
            ("ds.0.hosts.0".to_owned(), json!("Visible 102")),
            ("ds.0.hosts.1".to_owned(), json!("Visible 101")),
            ("ds.0.hosts.2".to_owned(), json!("Visible 103")),
            ("ds.0.items.0".to_owned(), json!("CPU *")),
            ("ds.0.color.0".to_owned(), json!("AA0000")),
            ("ds.0.width".to_owned(), json!("2")),
            ("vendor.untouched".to_owned(), json!("raw")),
        ]
    );
    assert_eq!(field_values(&base)[0].1, json!("visible 100"));
}

#[test]
fn clone_explicit_item_growth_requires_and_applies_a_deterministic_color() {
    let base = clone_snapshot(json!([{
        "widgetid": "20", "type": "svggraph", "name": "PostgreSQL", "x": "0", "y": "0",
        "width": "36", "height": "8", "view_mode": "0", "fields": [
            {"type": "4", "name": "ds.0.itemids.0", "value": "30"},
            {"type": "1", "name": "ds.0.color.0", "value": "AA0000"},
            {"type": "4", "name": "ds.0.itemids.1", "value": "40"},
            {"type": "1", "name": "ds.0.color.1", "value": "00BB00"},
            {"type": "0", "name": "ds.0.width", "value": "2"}
        ]
    }]));
    let targets = vec![
        slot("pg-b", "102"),
        slot("pg-a", "101"),
        slot("pg-c", "103"),
    ];
    let plan = plan_cardinality(&CardinalityRequest {
        widget_type: "svggraph".to_owned(),
        source_slot_ids: vec![
            TargetSlotId("pg-a".to_owned()),
            TargetSlotId("pg-b".to_owned()),
        ],
        target_slots: targets.clone(),
        datasets: vec![SvgDatasetCardinality {
            dataset_index: 0,
            binding: SeriesBindingKind::ExplicitItem,
            source_series: vec![
                source_series("pg-a", "ds.0.itemids.0"),
                source_series("pg-b", "ds.0.itemids.1"),
            ],
            template_slot_id: Some(TargetSlotId("pg-a".to_owned())),
            target_resolutions: vec![
                resolved_item("pg-b", "902"),
                resolved_item("pg-a", "901"),
                resolved_item("pg-c", "903"),
            ],
            max_series: 20,
        }],
    });
    let mappings = vec![
        mapping_for_reference(
            "postgres",
            "100",
            vec![targets[1].clone(), targets[2].clone()],
            mapping_reference(
                "ds.0.itemids.0",
                "svggraph",
                SourceValue::Item {
                    itemid: "30".to_owned(),
                },
                vec![resolved_item("pg-a", "901"), resolved_item("pg-c", "903")],
            ),
        ),
        mapping_for_reference(
            "postgres",
            "200",
            vec![targets[0].clone()],
            mapping_reference(
                "ds.0.itemids.1",
                "svggraph",
                SourceValue::Item {
                    itemid: "40".to_owned(),
                },
                vec![resolved_item("pg-b", "902")],
            ),
        ),
    ];
    let without_color = CloneCardinalityTransform {
        widget: field("ds.0.itemids.0").widget,
        plan: plan.clone(),
        added_item_colors: BTreeMap::new(),
    };
    let error = materialize_clone(&base, &mappings, std::slice::from_ref(&without_color), &[])
        .err()
        .unwrap_or_else(|| panic!("missing color accepted"));
    assert_eq!(error.code(), "clone_added_item_color_required");
    let choose = || {
        let mut transforms = [without_color.clone()];
        complete_clone_series_colors(&base, &mut transforms, &["aa0000", "123abc"])
            .unwrap_or_else(|error| panic!("palette: {error}"));
        transforms
    };
    assert_eq!(choose(), choose());
    assert_eq!(
        choose()[0]
            .added_item_colors
            .get(&(0, TargetSlotId("pg-c".to_owned())))
            .map(String::as_str),
        Some("123ABC")
    );

    let with_color = CloneCardinalityTransform {
        widget: field("ds.0.itemids.0").widget,
        plan,
        added_item_colors: BTreeMap::from([(
            (0, TargetSlotId("pg-c".to_owned())),
            "123abc".to_owned(),
        )]),
    };
    let local = materialize_clone(&base, &mappings, &[with_color], &[])
        .unwrap_or_else(|error| panic!("materialize: {error}"));
    assert_eq!(
        field_values(&local),
        vec![
            ("ds.0.itemids.0".to_owned(), json!("902")),
            ("ds.0.color.0".to_owned(), json!("00BB00")),
            ("ds.0.itemids.1".to_owned(), json!("901")),
            ("ds.0.color.1".to_owned(), json!("AA0000")),
            ("ds.0.itemids.2".to_owned(), json!("903")),
            ("ds.0.color.2".to_owned(), json!("123ABC")),
            ("ds.0.width".to_owned(), json!("2")),
        ]
    );
}

#[test]
fn clone_materialization_requires_a_decision_for_host_masks() {
    let base = clone_snapshot(json!([{
        "widgetid": "20", "type": "svggraph", "name": "PostgreSQL", "x": "0", "y": "0",
        "width": "36", "height": "8", "view_mode": "0", "fields": [
            {"type": "1", "name": "ds.0.hosts.0", "value": "Visible *"}
        ]
    }]));
    let mappings = vec![mapping_for_reference(
        "postgres",
        "100",
        vec![slot("pg-a", "101")],
        mapping_reference(
            "ds.0.hosts.0",
            "svggraph",
            SourceValue::HostPattern {
                pattern: "Visible *".to_owned(),
            },
            vec![resolved_host_pattern("pg-a", "Visible 101")],
        ),
    )];

    let error = materialize_clone(&base, &mappings, &[], &[])
        .err()
        .unwrap_or_else(|| panic!("host mask accepted without a decision"));
    assert_eq!(error.code(), "clone_pattern_decision_required");
    assert_eq!(
        base.raw_api_document()["pages"][0]["widgets"][0]["fields"][0]["value"],
        "Visible *"
    );
}

#[test]
fn clone_materialization_uses_the_verified_classic_graph_contract() {
    let base = clone_snapshot(json!([{
        "widgetid": "20", "type": "graph", "name": "Graph", "x": "0", "y": "0",
        "width": "12", "height": "4", "view_mode": "0", "fields": [
            {"type": "4", "name": "graphid.0", "value": "30"},
            {"type": "1", "name": "reference", "value": "untouched"}
        ]
    }]));
    let mappings = vec![mapping_for_reference(
        "postgres",
        "100",
        vec![slot("pg-a", "101")],
        mapping_reference(
            "graphid.0",
            "graph",
            SourceValue::Graph {
                graphid: "30".to_owned(),
            },
            vec![resolved_graph("pg-a", "930")],
        ),
    )];

    let local = materialize_clone(&base, &mappings, &[], &[])
        .unwrap_or_else(|error| panic!("materialize: {error}"));
    assert_eq!(
        local.raw_api_document()["pages"][0]["widgets"][0]["fields"][0]["value"],
        "930"
    );
    assert_eq!(
        local.raw_api_document()["pages"][0]["widgets"][0]["fields"][1]["value"],
        "untouched"
    );
}

struct PatternFixture {
    hosts: Vec<HostDetails>,
    items: BTreeMap<String, Vec<ItemDetails>>,
}

impl ClonePatternLookup for PatternFixture {
    fn hosts_by_ids(
        &self,
        hostids: &[String],
    ) -> impl Future<Output = Result<Vec<HostDetails>, ZabbixError>> + Send {
        ready(Ok(self
            .hosts
            .iter()
            .filter(|host| hostids.contains(&host.hostid))
            .cloned()
            .collect()))
    }

    fn host_items(
        &self,
        hostid: &str,
    ) -> impl Future<Output = Result<HostObjects<ItemDetails>, ZabbixError>> + Send {
        ready(Ok(HostObjects {
            objects: self.items.get(hostid).cloned().unwrap_or_default(),
            truncated: false,
        }))
    }
}

impl ClonePatternLookup for crate::resolver::tests::Fixture {
    fn hosts_by_ids(
        &self,
        hostids: &[String],
    ) -> impl Future<Output = Result<Vec<HostDetails>, ZabbixError>> + Send {
        ready(Ok(self
            .hosts
            .iter()
            .filter(|host| hostids.contains(&host.hostid))
            .cloned()
            .collect()))
    }

    fn host_items(
        &self,
        hostid: &str,
    ) -> impl Future<Output = Result<HostObjects<ItemDetails>, ZabbixError>> + Send {
        ready(Ok(HostObjects {
            objects: self
                .items
                .iter()
                .filter(|item| item.hostid == hostid)
                .cloned()
                .collect(),
            truncated: false,
        }))
    }
}

fn pattern_item(itemid: &str, hostid: &str, name: &str) -> ItemDetails {
    ItemDetails {
        itemid: itemid.to_owned(),
        hostid: hostid.to_owned(),
        key: format!("key.{itemid}"),
        name: name.to_owned(),
        value_type: 0,
        units: String::new(),
        flags: ItemFlags::Plain,
        templateid: None,
        discovery: None,
        owner_name: None,
    }
}

fn pattern_requirement(
    field_name: &str,
    kind: ClonePatternKind,
    source: &str,
) -> ClonePatternRequirement {
    ClonePatternRequirement {
        field: FieldAddress {
            widget: field(field_name).widget,
            field_name: field_name.to_owned(),
        },
        widget_type: "svggraph".to_owned(),
        kind,
        source_pattern: source.to_owned(),
    }
}

#[tokio::test]
async fn clone_pattern_preview_reads_frozen_targets_and_uses_frontend_matching() {
    let slots = vec![slot("pg-a", "101"), slot("pg-b", "102")];
    let fixture = PatternFixture {
        hosts: vec![host("101"), host("102")],
        items: BTreeMap::from([
            (
                "101".to_owned(),
                vec![
                    pattern_item("901", "101", "CPU utilization"),
                    pattern_item("902", "101", "Memory"),
                ],
            ),
            (
                "102".to_owned(),
                vec![pattern_item("903", "102", "cpu load")],
            ),
        ]),
    };

    let preview = preview_clone_pattern(
        &fixture,
        pattern_requirement("ds.0.items.0", ClonePatternKind::Item, "Old CPU*"),
        "CPU*".to_owned(),
        &slots,
    )
    .await
    .unwrap_or_else(|error| panic!("preview: {error}"));
    assert_eq!(
        preview.checked_slots,
        vec![slots[0].slot_id.clone(), slots[1].slot_id.clone()]
    );
    assert_eq!(preview.match_count, 2);
    assert_eq!(preview.matches[0].object_id, "901");
    assert_eq!(preview.matches[1].object_id, "903");
    assert!(!preview.truncated);
}

#[tokio::test]
async fn clone_pattern_decisions_replace_only_addressed_patterns_and_survive_reopen() {
    let base = clone_snapshot(json!([{
        "widgetid": "20", "type": "svggraph", "name": "PostgreSQL", "x": "0", "y": "0",
        "width": "36", "height": "8", "view_mode": "0", "fields": [
            {"type": "1", "name": "ds.0.hosts.0", "value": "Source-*"},
            {"type": "1", "name": "ds.0.items.0", "value": "Old CPU*"},
            {"type": "1", "name": "vendor.untouched", "value": "raw"}
        ]
    }]));
    let slots = vec![slot("pg-a", "101"), slot("pg-b", "102")];
    let fixture = PatternFixture {
        hosts: vec![host("101"), host("102")],
        items: BTreeMap::from([
            ("101".to_owned(), vec![pattern_item("901", "101", "CPU A")]),
            ("102".to_owned(), vec![pattern_item("902", "102", "CPU B")]),
        ]),
    };
    let host_preview = preview_clone_pattern(
        &fixture,
        pattern_requirement("ds.0.hosts.0", ClonePatternKind::Host, "Source-*"),
        "Visible 10*".to_owned(),
        &slots,
    )
    .await
    .unwrap_or_else(|error| panic!("host preview: {error}"));
    let item_preview = preview_clone_pattern(
        &fixture,
        pattern_requirement("ds.0.items.0", ClonePatternKind::Item, "Old CPU*"),
        "CPU*".to_owned(),
        &slots,
    )
    .await
    .unwrap_or_else(|error| panic!("item preview: {error}"));
    let decisions = vec![
        ClonePatternDecision::replace(host_preview)
            .unwrap_or_else(|error| panic!("host decision: {error}")),
        ClonePatternDecision::replace(item_preview)
            .unwrap_or_else(|error| panic!("item decision: {error}")),
    ];
    let mappings = vec![mapping_for_reference(
        "postgres",
        "100",
        slots,
        mapping_reference(
            "ds.0.hosts.0",
            "svggraph",
            SourceValue::HostPattern {
                pattern: "Source-*".to_owned(),
            },
            vec![
                TargetMetricResolution::not_found_or_no_access(
                    TargetSlotId("pg-a".to_owned()),
                    Applicability::ReadOnly,
                ),
                TargetMetricResolution::not_found_or_no_access(
                    TargetSlotId("pg-b".to_owned()),
                    Applicability::ReadOnly,
                ),
            ],
        ),
    )];

    let local = materialize_clone(&base, &mappings, &[], &decisions)
        .unwrap_or_else(|error| panic!("materialize: {error}"));
    assert_eq!(field_values(&local)[0].1, json!("Visible 10*"));
    assert_eq!(field_values(&local)[1].1, json!("CPU*"));
    assert_eq!(field_values(&local)[2].1, json!("raw"));

    let (_directory, service) = storage_setup();
    let mut request = save_request(0, mappings);
    request.pattern_decisions = decisions.clone();
    let saved = service
        .save_clone_mapping(LOCAL, request)
        .unwrap_or_else(|error| panic!("save: {error}"));
    let reopened = service
        .clone_mapping(LOCAL)
        .unwrap_or_else(|error| panic!("reopen: {error}"))
        .unwrap_or_else(|| panic!("mapping missing"));
    assert_eq!(saved.state.pattern_decisions, decisions);
    assert_eq!(reopened.state.pattern_decisions, decisions);
}

#[tokio::test]
async fn every_pattern_requires_a_decision_even_when_a_host_mask_has_many_matches() {
    let base = clone_snapshot(json!([{
        "widgetid": "20", "type": "svggraph", "name": "PostgreSQL", "x": "0", "y": "0",
        "width": "36", "height": "8", "view_mode": "0", "fields": [
            {"type": "1", "name": "ds.0.hosts.0", "value": "Source-*"},
            {"type": "1", "name": "ds.0.items.0", "value": "Old CPU*"}
        ]
    }]));
    let slots = vec![slot("pg-a", "101"), slot("pg-b", "102")];
    let fixture = PatternFixture {
        hosts: vec![host("101"), host("102")],
        items: BTreeMap::new(),
    };
    let host_preview = preview_clone_pattern(
        &fixture,
        pattern_requirement("ds.0.hosts.0", ClonePatternKind::Host, "Source-*"),
        "Visible *".to_owned(),
        &slots,
    )
    .await
    .unwrap_or_else(|error| panic!("host preview: {error}"));
    assert_eq!(
        host_preview.match_count, 2,
        "the mask remains explicit evidence"
    );
    let host_decision = ClonePatternDecision::replace(host_preview)
        .unwrap_or_else(|error| panic!("host decision: {error}"));
    let mappings = vec![mapping_for_reference(
        "postgres",
        "100",
        slots,
        mapping_reference(
            "ds.0.hosts.0",
            "svggraph",
            SourceValue::HostPattern {
                pattern: "Source-*".to_owned(),
            },
            vec![
                TargetMetricResolution::not_found_or_no_access(
                    TargetSlotId("pg-a".to_owned()),
                    Applicability::ReadOnly,
                ),
                TargetMetricResolution::not_found_or_no_access(
                    TargetSlotId("pg-b".to_owned()),
                    Applicability::ReadOnly,
                ),
            ],
        ),
    )];

    let error = materialize_clone(&base, &mappings, &[], &[host_decision])
        .err()
        .unwrap_or_else(|| panic!("missing item-pattern decision accepted"));
    assert_eq!(error.code(), "clone_pattern_decision_required");
    assert_eq!(
        base.raw_api_document()["pages"][0]["widgets"][0]["fields"][0]["value"],
        "Source-*"
    );
}

fn lld_item(itemid: &str, key: &str, prototype_key: &str) -> ItemEvidence {
    ItemEvidence {
        itemid: itemid.to_owned(),
        key: key.to_owned(),
        name: format!("Item {itemid}"),
        value_type: 3,
        units: "B".to_owned(),
        origin: ItemOrigin::Discovered {
            prototype_key: prototype_key.to_owned(),
        },
    }
}

#[test]
fn clone_lld_requires_a_manual_same_prototype_choice_and_persists_it() {
    let base = clone_snapshot(json!([{
        "widgetid": "20", "type": "item", "name": "Database size", "x": "0", "y": "0",
        "width": "12", "height": "4", "view_mode": "0", "fields": [
            {"type": "4", "name": "itemid.0", "value": "30"},
            {"type": "1", "name": "description", "value": "untouched"}
        ]
    }]));
    let source = lld_item("30", "pgsql.db.size[proddb]", "pgsql.db.size[{#DBNAME}]");
    let selected = lld_item("901", "pgsql.db.size[pilotdb]", "pgsql.db.size[{#DBNAME}]");
    let wrong_prototype = lld_item("902", "pgsql.db.size[otherdb]", "other.rule[{#DBNAME}]");
    let mut reference = mapping_reference(
        "itemid.0",
        "item",
        SourceValue::Item {
            itemid: "30".to_owned(),
        },
        vec![TargetMetricResolution {
            slot_id: TargetSlotId("pg-a".to_owned()),
            target: None,
            match_status: MatchStatus::UnresolvedLld,
            applicability: Applicability::Editable,
            reasons: vec![ReasonCode::LldInstanceMissing],
            differences: Vec::new(),
            evidence: None,
            candidates: vec![selected.clone(), wrong_prototype],
            decision: None,
            check: CloneBindingCheck::Checked,
            check_reason: None,
        }],
    );
    reference.source_evidence = Some(Evidence::Item(source));
    let slots = vec![slot("pg-a", "101")];
    let unresolved = vec![mapping_for_reference(
        "postgres",
        "100",
        slots.clone(),
        reference.clone(),
    )];

    let blocked = materialize_clone(&base, &unresolved, &[], &[])
        .err()
        .unwrap_or_else(|| panic!("unresolved LLD was applied automatically"));
    assert_eq!(blocked.code(), "clone_target_unresolved");

    let mismatch = reference
        .choose_lld_instance(&TargetSlotId("pg-a".to_owned()), "902")
        .err()
        .unwrap_or_else(|| panic!("different prototype accepted"));
    assert_eq!(mismatch.code(), "clone_lld_candidate_prototype_mismatch");

    reference
        .choose_lld_instance(&TargetSlotId("pg-a".to_owned()), "901")
        .unwrap_or_else(|error| panic!("manual choice: {error}"));
    let mappings = vec![mapping_for_reference("postgres", "100", slots, reference)];
    let local = materialize_clone(&base, &mappings, &[], &[])
        .unwrap_or_else(|error| panic!("materialize: {error}"));
    assert_eq!(field_values(&local)[0].1, json!("901"));
    assert_eq!(field_values(&local)[1].1, json!("untouched"));

    let (_directory, service) = storage_setup();
    let saved = service
        .save_clone_mapping(LOCAL, save_request(0, mappings))
        .unwrap_or_else(|error| panic!("save: {error}"));
    let reopened = service
        .clone_mapping(LOCAL)
        .unwrap_or_else(|error| panic!("reopen: {error}"))
        .unwrap_or_else(|| panic!("mapping missing"));
    assert_eq!(reopened.state, saved.state);
    let resolution = &reopened.state.mappings[0].references[0].targets[0];
    assert_eq!(
        resolution.target,
        Some(TargetValue::Item {
            itemid: "901".to_owned()
        })
    );
    assert!(matches!(
        resolution.decision,
        Some(TargetMetricDecision::LldInstance { .. })
    ));
}

#[tokio::test]
async fn clone_revalidation_marks_disappeared_lld_and_changed_pattern_in_a_new_revision() {
    let fixture = crate::resolver::tests::fixture();
    let base = VersionedDashboardSnapshot::from_adapter_raw(
        CONNECTION.to_owned(),
        version(),
        crate::resolver::tests::dashboard(),
    )
    .unwrap_or_else(|error| panic!("snapshot: {error}"));
    let model = crate::resolver::source_model(&base, &WidgetCapabilityRegistry::v7_4())
        .unwrap_or_else(|error| panic!("model: {error}"));
    let request = crate::resolver::ReplaceHostRequest {
        source_hostid: "11".to_owned(),
        target_hostid: "12".to_owned(),
        scope: crate::resolver::ResolveScope::Dashboard,
    };
    let resolved = crate::resolver::resolve_replace_host_with(&fixture, &model, &request)
        .await
        .unwrap_or_else(|error| panic!("resolve: {error}"));
    let make_reference = |field_name: &str| {
        let row = resolved
            .rows
            .iter()
            .find(|row| row.field.field_name == field_name)
            .unwrap_or_else(|| panic!("row {field_name}"));
        ReferenceMapping {
            field: row.field.clone(),
            widget_type: row.widget_type.clone(),
            source: row.source.clone(),
            source_evidence: row.source_evidence.clone(),
            targets: vec![TargetMetricResolution {
                slot_id: TargetSlotId("pilot".to_owned()),
                target: row.target.clone(),
                match_status: row.match_status,
                applicability: row.applicability,
                reasons: row.reasons.clone(),
                differences: row.differences.clone(),
                evidence: row.target_evidence.clone(),
                candidates: row.candidates.clone(),
                decision: None,
                check: CloneBindingCheck::Checked,
                check_reason: None,
            }],
        }
    };
    let direct = make_reference("itemid.0");
    let mut lld = make_reference("ds.1.itemids.1");
    lld.choose_lld_instance(&TargetSlotId("pilot".to_owned()), "203")
        .unwrap_or_else(|error| panic!("LLD decision: {error}"));
    let target_slot = TargetSlot {
        slot_id: TargetSlotId("pilot".to_owned()),
        host: resolved
            .target_host
            .clone()
            .unwrap_or_else(|| panic!("target host")),
    };
    let mappings = vec![SourceHostMapping {
        role_id: SourceRoleId("postgres".to_owned()),
        source_host: resolved.source_host,
        decision: SourceHostDecision::TargetSlots(vec![target_slot.clone()]),
        references: vec![direct, lld],
    }];
    let requirement = pattern_requirements(&model)
        .into_iter()
        .find(|requirement| requirement.field.field_name == "ds.0.items.0")
        .unwrap_or_else(|| panic!("pattern requirement"));
    let preview = preview_clone_pattern(
        &fixture,
        requirement,
        "CPU*".to_owned(),
        std::slice::from_ref(&target_slot),
    )
    .await
    .unwrap_or_else(|error| panic!("pattern preview: {error}"));
    let pattern = ClonePatternDecision::replace(preview)
        .unwrap_or_else(|error| panic!("pattern decision: {error}"));

    let base_fingerprint = base.fingerprint().to_owned();
    let (directory, service) = storage_setup_with_snapshot(base);
    let mut save = save_request(0, mappings);
    save.pattern_decisions = vec![pattern];
    let saved = service
        .save_clone_mapping(LOCAL, save)
        .unwrap_or_else(|error| panic!("save: {error}"));
    assert_eq!(
        saved.state.base_fingerprint.as_deref(),
        Some(base_fingerprint.as_str())
    );
    assert_eq!(saved.state.local_fingerprint.as_deref(), Some(IDENTITY));
    let context = CloneRevalidationContext {
        connection_id: CONNECTION.to_owned(),
        server_identity_fingerprint: IDENTITY.to_owned(),
        server_version: "7.4.16".to_owned(),
        local_fingerprint: IDENTITY.to_owned(),
    };
    let wrong_identity = CloneRevalidationContext {
        server_identity_fingerprint:
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_owned(),
        ..context.clone()
    };
    let rejected = service
        .revalidate_clone_mapping(&fixture, LOCAL, &wrong_identity)
        .await
        .err()
        .unwrap_or_else(|| panic!("another server identity accepted"));
    assert_eq!(rejected.code(), "clone_server_identity_changed");
    assert_eq!(
        service
            .clone_mapping(LOCAL)
            .unwrap_or_else(|error| panic!("mapping after refusal: {error}"))
            .unwrap_or_else(|| panic!("mapping after refusal"))
            .workcopy_revision,
        saved.workcopy_revision
    );
    let checked = service
        .revalidate_clone_mapping(&fixture, LOCAL, &context)
        .await
        .unwrap_or_else(|error| panic!("fresh revalidation: {error}"));
    assert_eq!(checked.state.mapping_revision, saved.workcopy_revision + 1);
    assert!(
        checked.state.mappings[0]
            .references
            .iter()
            .all(|reference| { reference.targets[0].check == CloneBindingCheck::Checked })
    );
    assert_eq!(
        checked.state.pattern_decisions[0].check,
        CloneBindingCheck::Checked
    );

    let mut changed = fixture;
    changed
        .items
        .retain(|item| item.hostid != "12" || item.flags != ItemFlags::Discovered);
    for item in &mut changed.items {
        if item.hostid == "12" && item.name.starts_with("CPU") {
            item.name = format!("Processor {}", item.itemid);
        }
    }
    let stale = service
        .revalidate_clone_mapping(&changed, LOCAL, &context)
        .await
        .unwrap_or_else(|error| panic!("stale revalidation: {error}"));
    assert!(stale.state.mapping_revision > checked.state.mapping_revision);
    let lld = &stale.state.mappings[0].references[1].targets[0];
    assert_eq!(lld.check, CloneBindingCheck::Invalid);
    assert_eq!(
        lld.check_reason,
        Some(CloneCheckReason::CandidateNotVisible)
    );
    assert!(lld.decision.is_some(), "stored choice remains evidence");
    assert_eq!(
        stale.state.pattern_decisions[0].check,
        CloneBindingCheck::Stale
    );
    assert_eq!(
        stale.state.pattern_decisions[0].check_reason,
        Some(CloneCheckReason::PatternChanged)
    );

    let mut hidden = changed;
    hidden.hosts.retain(|host| host.hostid != "12");
    let invalid = service
        .revalidate_clone_mapping(&hidden, LOCAL, &context)
        .await
        .unwrap_or_else(|error| panic!("hidden target revalidation: {error}"));
    assert!(invalid.state.mapping_revision > stale.state.mapping_revision);
    assert!(
        invalid.state.mappings[0]
            .references
            .iter()
            .all(|reference| {
                reference.targets[0].check == CloneBindingCheck::Invalid
                    && reference.targets[0].check_reason == Some(CloneCheckReason::TargetNotVisible)
            })
    );
    assert_eq!(
        invalid.state.pattern_decisions[0].check,
        CloneBindingCheck::Invalid
    );
    assert_eq!(
        invalid.state.pattern_decisions[0].check_reason,
        Some(CloneCheckReason::TargetNotVisible)
    );
    drop(directory);
}

#[test]
fn stale_clone_resolution_cannot_be_materialized() {
    let base = clone_snapshot(json!([{
        "widgetid": "20", "type": "item", "name": "Value", "x": "0", "y": "0",
        "width": "12", "height": "4", "view_mode": "0", "fields": [
            {"type": "4", "name": "itemid.0", "value": "30"}
        ]
    }]));
    let mut mapping = mapped(&[("slot-a", "101")]);
    mapping.references[0].targets[0].check = CloneBindingCheck::Stale;
    mapping.references[0].targets[0].check_reason = Some(CloneCheckReason::ResolutionChanged);

    let error = materialize_clone(&base, &[mapping], &[], &[])
        .err()
        .unwrap_or_else(|| panic!("stale resolution materialized"));
    assert_eq!(error.code(), "clone_target_unresolved");
}

#[test]
fn every_required_clone_blocker_prevents_materialization_without_mutating_base() {
    let base = clone_snapshot(json!([{
        "widgetid": "20", "type": "item", "name": "Value", "x": "0", "y": "0",
        "width": "12", "height": "4", "view_mode": "0", "fields": [
            {"type": "4", "name": "itemid.0", "value": "30"}
        ]
    }]));
    let original = base.raw_api_document().clone();

    let mut missing = mapped(&[("slot-a", "101")]);
    missing.references[0].targets[0] = TargetMetricResolution::not_found_or_no_access(
        TargetSlotId("slot-a".to_owned()),
        Applicability::Editable,
    );

    let mut incompatible = mapped(&[("slot-a", "101")]);
    incompatible.references[0].targets[0].differences =
        vec![Difference::ValueType, Difference::Units];

    let mut hidden = mapped(&[("slot-a", "101")]);
    hidden.references[0].targets[0].check = CloneBindingCheck::Invalid;
    hidden.references[0].targets[0].check_reason = Some(CloneCheckReason::TargetNotVisible);

    let mut unsupported = mapped(&[("slot-a", "101")]);
    unsupported.references[0].targets[0].applicability = Applicability::Unsupported;

    let duplicate_slot = mapped(&[("slot-a", "101"), ("slot-a", "102")]);
    let cases = [
        ("missing", missing, "clone_target_unresolved"),
        ("incompatible", incompatible, "clone_target_unresolved"),
        ("hidden", hidden, "clone_target_unresolved"),
        ("unsupported", unsupported, "clone_target_unresolved"),
        ("duplicate_slot", duplicate_slot, "clone_mapping_invalid"),
    ];

    for (name, mapping, expected_code) in cases {
        let error = materialize_clone(&base, &[mapping], &[], &[])
            .err()
            .unwrap_or_else(|| panic!("{name} blocker materialized"));
        assert_eq!(error.code(), expected_code, "case {name}");
        assert_eq!(base.raw_api_document(), &original, "case {name}");
    }
}
