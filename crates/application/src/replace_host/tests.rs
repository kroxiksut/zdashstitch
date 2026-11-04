// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-03A/B/C tests on the synthetic resolver fixture (no real hosts,
//! addresses or dashboards) over a real local store in a temp directory.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use zdashstitch_zabbix::{
    AccessMode, AdapterRegistry, VersionedDashboardSnapshot, ZabbixCapabilities, ZabbixVersion,
};

use super::*;
use crate::resolver::tests::{Fixture, SOURCE, TARGET, dashboard, field, fixture, widget};
use crate::resolver::{ReasonCode, ReplaceHostRequest, ResolveScope};
use crate::workcopy::formats::{
    BaseReference, DraftKind, ItemMetadataState, LOCAL_ONLY, LocalMetadata, PageSelector,
    SNAPSHOT_FORMAT, SnapshotFile, SnapshotScope, WORKCOPY_FORMAT, WorkcopyFile, WorkcopySource,
};
use crate::workcopy::operations::{parse_log, to_log};
use crate::workcopy::store::LocalStore;
use crate::workcopy::{WorkcopyError, WorkcopyService};
use crate::{CompatibilityFinding, FindingCode, FindingSeverity};

const CONNECTION: &str = "conn-0123456789abcdef0123456789abcdef";
const LOCAL: &str = "wc-00112233445566778899aabbccddeeff";
const LOCAL_TWIN: &str = "wc-0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f";
const LOCAL_PAGE: &str = "wc-ffeeddccbbaa99887766554433221100";

fn version(text: &str) -> ZabbixVersion {
    text.parse()
        .unwrap_or_else(|error| panic!("version: {error}"))
}

fn adapter_capabilities(text: &str) -> ZabbixCapabilities {
    AdapterRegistry::resolve(version(text)).unwrap_or_else(|error| panic!("adapter: {error}"))
}

fn read_only(capabilities: &ZabbixCapabilities) -> SessionContext<'_> {
    SessionContext::Connected {
        access_mode: AccessMode::ReadOnly,
        capabilities,
    }
}

fn read_write(capabilities: &ZabbixCapabilities) -> SessionContext<'_> {
    SessionContext::Connected {
        access_mode: AccessMode::ReadWrite,
        capabilities,
    }
}

fn base(raw: Value) -> VersionedDashboardSnapshot {
    VersionedDashboardSnapshot::from_adapter_raw(CONNECTION.to_owned(), version("7.4.16"), raw)
        .unwrap_or_else(|error| panic!("snapshot: {error}"))
}

struct Setup {
    dir: tempfile::TempDir,
    service: WorkcopyService,
    base: VersionedDashboardSnapshot,
}

impl Setup {
    fn workcopy_path(&self, local_id: &str) -> PathBuf {
        workcopy_path(self.dir.path(), local_id)
    }

    fn workcopy_bytes(&self, local_id: &str) -> Vec<u8> {
        std::fs::read(self.workcopy_path(local_id)).unwrap_or_else(|error| panic!("read: {error}"))
    }

    fn workcopy_json(&self, local_id: &str) -> Value {
        serde_json::from_slice(&self.workcopy_bytes(local_id))
            .unwrap_or_else(|error| panic!("json: {error}"))
    }

    fn write_workcopy_json(&self, local_id: &str, value: &Value) {
        let raw = serde_json::to_vec(value).unwrap_or_else(|error| panic!("json: {error}"));
        std::fs::write(self.workcopy_path(local_id), raw)
            .unwrap_or_else(|error| panic!("write: {error}"));
    }
}

fn workcopy_path(root: &Path, local_id: &str) -> PathBuf {
    root.join("workcopies").join(local_id).join("workcopy.json")
}

/// Stores Base and one workcopy per `(local_id, page)`.
fn setup(raw: Value, drafts: &[(&str, Option<&str>)]) -> Setup {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"));
    let store = LocalStore::at(dir.path().to_path_buf());
    let base = base(raw);
    let fingerprint = base.storage_fingerprint().to_owned();
    let dashboardid = base.dashboard_id().to_owned();
    store
        .put_snapshot(&SnapshotFile {
            format: SNAPSHOT_FORMAT.to_owned(),
            storage_scope: LOCAL_ONLY.to_owned(),
            connection_id: CONNECTION.to_owned(),
            adapter_id: base.adapter_id().to_owned(),
            server_version: "7.4.16".to_owned(),
            dashboardid: dashboardid.clone(),
            scope: SnapshotScope::Dashboard,
            first_read_at_ms: 1,
            last_read_at_ms: 2,
            first_read_fingerprint: fingerprint.clone(),
            last_read_fingerprint: fingerprint.clone(),
            fingerprint: fingerprint.clone(),
            raw_api_document: base.raw_api_document().clone(),
        })
        .unwrap_or_else(|error| panic!("snapshot: {error}"));
    for (local_id, page) in drafts {
        let page = page.map(|pageid| PageSelector {
            dashboard_pageid: pageid.to_owned(),
            page_name: "Page".to_owned(),
            page_number: 1,
        });
        store
            .create_workcopy(&WorkcopyFile {
                format: WORKCOPY_FORMAT.to_owned(),
                storage_scope: LOCAL_ONLY.to_owned(),
                workcopy_id: (*local_id).to_owned(),
                kind: if page.is_some() {
                    DraftKind::Page
                } else {
                    DraftKind::Dashboard
                },
                name: "Synthetic".to_owned(),
                created_at_ms: 10,
                updated_at_ms: 10,
                revision: 0,
                source: WorkcopySource {
                    connection_id: CONNECTION.to_owned(),
                    dashboardid: dashboardid.clone(),
                    dashboard_name: "Synthetic".to_owned(),
                    page,
                    adapter_id: base.adapter_id().to_owned(),
                    server_version: "7.4.16".to_owned(),
                },
                base: BaseReference {
                    fingerprint: fingerprint.clone(),
                    read_at_ms: 2,
                    source_checked_at_ms: 2,
                    page_count: 1,
                    widget_count: 1,
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
    }
    let service = WorkcopyService::new(dir.path().to_path_buf());
    Setup { dir, service, base }
}

fn request(scope: ResolveScope) -> ReplaceHostRequest {
    ReplaceHostRequest {
        source_hostid: SOURCE.to_owned(),
        target_hostid: TARGET.to_owned(),
        scope,
    }
}

async fn calculate(
    setup: &Setup,
    lookup: &Fixture,
    local_id: &str,
    session: SessionContext<'_>,
) -> ReplaceHostPreview {
    setup
        .service
        .calculate_replace_host(lookup, local_id, &request(ResolveScope::Dashboard), session)
        .await
        .unwrap_or_else(|error| panic!("calculate: {error}"))
}

fn ok<T>(result: Result<T, WorkcopyError>) -> T {
    result.unwrap_or_else(|error| panic!("{error} {:?}", error.params()))
}

fn code<T>(result: Result<T, WorkcopyError>) -> &'static str {
    match result {
        Ok(_) => panic!("expected an error"),
        Err(error) => error.code(),
    }
}

fn key(page_index: usize, widget_index: usize, field: &str) -> FieldKey {
    FieldKey {
        page_index,
        widget_index,
        field: field.to_owned(),
    }
}

fn row<'a>(preview: &'a ReplaceHostPreview, widgetid: &str, field: &str) -> &'a RowView {
    preview
        .rows
        .iter()
        .find(|row| row.widgetid.as_deref() == Some(widgetid) && row.key.field == field)
        .unwrap_or_else(|| panic!("row {widgetid}/{field}"))
}

fn row_key(preview: &ReplaceHostPreview, widgetid: &str, field: &str) -> FieldKey {
    row(preview, widgetid, field).key.clone()
}

fn finding<'a>(
    preview: &'a ReplaceHostPreview,
    code: FindingCode,
    widgetid: &str,
    field: &str,
) -> &'a CompatibilityFinding {
    preview
        .report
        .with_code(code)
        .find(|finding| {
            finding.subject.as_ref().is_some_and(|subject| {
                subject.widgetid.as_deref() == Some(widgetid)
                    && subject.field.as_deref() == Some(field)
            })
        })
        .unwrap_or_else(|| panic!("finding {code:?} {widgetid}/{field}"))
}

fn blocker_codes(preview: &ReplaceHostPreview) -> BTreeSet<FindingCode> {
    preview
        .report
        .blockers()
        .map(|finding| finding.code)
        .collect()
}

fn contracts(preview: &ReplaceHostPreview) -> BTreeSet<String> {
    preview
        .report
        .with_code(FindingCode::WriteContractUnverified)
        .filter_map(|finding| finding.param("contract").map(str::to_owned))
        .collect()
}

fn widget_value<'a>(raw: &'a Value, page: usize, widget: usize, name: &str) -> &'a Value {
    raw["pages"][page]["widgets"][widget]["fields"]
        .as_array()
        .and_then(|fields| fields.iter().find(|field| field["name"] == name))
        .map(|field| &field["value"])
        .unwrap_or_else(|| panic!("field {page}/{widget}/{name}"))
}

/// The resolver dashboard plus a third page: an item widget and an
/// `svggraph` with one series on the source host and one untouched series.
fn multi_page_dashboard() -> Value {
    let mut raw = dashboard();
    if let Some(pages) = raw["pages"].as_array_mut() {
        pages.push(
            json!({"dashboard_pageid": "1002", "name": "Third", "widgets": [
                widget("3000", "item", vec![field("4", "itemid.0", "101")]),
                widget("3001", "svggraph", vec![
                    field("1", "ds.0.hosts.0", "db-old-01"),
                    field("1", "ds.0.items.0", "PostgreSQL ping"),
                    field("1", "ds.1.hosts.0", "shared-srv"),
                    field("1", "ds.1.items.0", "CPU utilization"),
                ]),
            ]}),
        );
    }
    raw
}

/// Only proven item changes plus one O-003 reference kept on the source.
fn item_only_dashboard() -> Value {
    json!({
        "dashboardid": "901",
        "name": "Synthetic item dashboard",
        "pages": [{"dashboard_pageid": "1100", "name": "Main", "widgets": [
            widget("2200", "item", vec![field("4", "itemid.0", "101")]),
            widget("2201", "problems", vec![field("3", "hostids.0", SOURCE)]),
        ]}]
    })
}

// ---------- P2-03A: intent, calculation, operation log ----------

#[tokio::test]
async fn a_read_only_session_calculates_and_stores_without_a_plan() {
    let setup = setup(dashboard(), &[(LOCAL, None)]);
    let lookup = fixture();
    let capabilities = adapter_capabilities("7.4.16");
    let preview = calculate(&setup, &lookup, LOCAL, read_only(&capabilities)).await;

    // Stored as the first workcopy operation.
    let stored = setup.workcopy_json(LOCAL);
    assert_eq!(stored["operations"][0]["op"], "replace_host");
    assert_eq!(stored["operations"][0]["op_version"], 1);
    assert_eq!(stored["operations"][0]["seq"], 1);
    assert_eq!(stored["operations"][0]["source_hostid"], SOURCE);
    assert_eq!(stored["operations"][0]["target_hostid"], TARGET);
    assert_eq!(
        stored["operations"][0]["scope"],
        json!({"kind": "dashboard"})
    );
    assert_eq!(stored["revision"], 1);
    let history = ok(setup.service.operation_history(LOCAL));
    assert_eq!(history.len(), 1);
    assert_eq!((history[0].seq, history[0].op), (1, "replace_host"));

    // The local preview exists; publication is blocked by the mode only on
    // the connection side, and no plan type is returned at all.
    assert!(!preview.changes.is_empty());
    assert!(!preview.diff.pages.is_empty());
    assert_ne!(preview.local_fingerprint, preview.base_fingerprint);
    assert!(!preview.report.allows_publication());
    let mode = preview
        .report
        .with_code(FindingCode::WriteModeRequired)
        .next()
        .unwrap_or_else(|| panic!("write mode finding"));
    assert_eq!(mode.severity, FindingSeverity::BlocksPublication);

    // Offline the same stored operation is previewed without the server.
    let offline = ok(setup
        .service
        .replace_host_preview(LOCAL, SessionContext::Offline));
    assert_eq!(offline.rows, preview.rows);
    assert_eq!(offline.local_fingerprint, preview.local_fingerprint);
    assert!(blocker_codes(&offline).contains(&FindingCode::ConnectionRequired));

    // Only allowlisted reads were made.
    assert!(lookup.calls().iter().all(|call| {
        call.starts_with("host.get")
            || call.starts_with("item.get")
            || call.starts_with("graph.get")
            || call.starts_with("itemprototype.get")
            || call.starts_with("graphprototype.get")
    }));
}

#[tokio::test]
async fn a_same_input_gives_the_same_local_bytes() {
    let setup = setup(dashboard(), &[(LOCAL, None), (LOCAL_TWIN, None)]);
    let lookup = fixture();
    let first = calculate(&setup, &lookup, LOCAL, SessionContext::Offline).await;
    let second = calculate(&setup, &lookup, LOCAL_TWIN, SessionContext::Offline).await;
    assert_eq!(first.local_fingerprint, second.local_fingerprint);
    assert_eq!(first.changes, second.changes);
    assert_eq!(first.diff, second.diff);

    let local = ok(setup.service.local_snapshot(LOCAL));
    let twin = ok(setup.service.local_snapshot(LOCAL_TWIN));
    let bytes = |snapshot: &VersionedDashboardSnapshot| {
        serde_json::to_vec(snapshot.raw_api_document())
            .unwrap_or_else(|error| panic!("json: {error}"))
    };
    assert_eq!(bytes(&local), bytes(&twin));
    assert_eq!(
        bytes(&local),
        bytes(&ok(setup.service.local_snapshot(LOCAL)))
    );
    assert_eq!(local.fingerprint(), first.local_fingerprint);

    // Replaying the stored log on Base is pure and gives the same bytes.
    let operations = setup.workcopy_json(LOCAL)["operations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let replayed = ok(super::service::materialize_for_tests(
        &setup.base,
        &operations,
    ));
    assert_eq!(bytes(&replayed), bytes(&local));
}

#[tokio::test]
async fn a_untouched_fields_stay_byte_identical() {
    let setup = setup(dashboard(), &[(LOCAL, None)]);
    let preview = calculate(&setup, &fixture(), LOCAL, SessionContext::Offline).await;
    let local = ok(setup.service.local_snapshot(LOCAL));
    let changed: BTreeSet<(String, String)> = preview
        .changes
        .iter()
        .map(|change| {
            (
                change.widgetid.clone().unwrap_or_default(),
                change.key.field.clone(),
            )
        })
        .collect();
    let expected: BTreeSet<(String, String)> = [
        ("2000", "itemid.0"),
        ("2001", "ds.0.hosts.0"),
        ("2001", "ds.1.itemids.0"),
        ("2004", "itemid.0"),
        ("2005", "itemid.0"),
        // Classic graph, same name and template (P2-04H on 7.4.16).
        ("2102", "graphid.0"),
    ]
    .into_iter()
    .map(|(widget, field)| (widget.to_owned(), field.to_owned()))
    .collect();
    assert_eq!(changed, expected);

    let raw = local.raw_api_document();
    assert_eq!(widget_value(raw, 0, 0, "itemid.0"), "201");
    assert_eq!(widget_value(raw, 0, 1, "ds.0.hosts.0"), "db-new-01");
    assert_eq!(widget_value(raw, 0, 1, "ds.1.itemids.0"), "202");
    // Masks, item patterns, unresolved and kept references are untouched.
    assert_eq!(widget_value(raw, 0, 1, "ds.0.items.0"), "CPU*");
    assert_eq!(widget_value(raw, 0, 1, "ds.2.hosts.0"), "db-old-*");
    assert_eq!(widget_value(raw, 0, 1, "ds.3.hosts.0"), "*");
    assert_eq!(widget_value(raw, 0, 1, "ds.1.itemids.1"), "103");
    assert_eq!(widget_value(raw, 0, 2, "itemid.0"), "104");
    assert_eq!(widget_value(raw, 1, 0, "hostids.0"), SOURCE);

    // Reverting exactly the changed values gives Base back byte for byte.
    let mut reverted = raw.clone();
    for change in &preview.changes {
        let fields =
            reverted["pages"][change.key.page_index]["widgets"][change.key.widget_index]["fields"]
                .as_array_mut()
                .unwrap_or_else(|| panic!("fields"));
        for field in fields.iter_mut() {
            if field["name"] == change.key.field.as_str() {
                field["value"] = Value::String(change.source.raw().to_owned());
            }
        }
    }
    let to_bytes =
        |value: &Value| serde_json::to_vec(value).unwrap_or_else(|error| panic!("json: {error}"));
    assert_eq!(to_bytes(&reverted), to_bytes(setup.base.raw_api_document()));
}

#[tokio::test]
async fn a_op_log_round_trips_and_unknown_ops_reject_without_touching_the_file() {
    let setup = setup(dashboard(), &[(LOCAL, None)]);
    let preview = calculate(&setup, &fixture(), LOCAL, SessionContext::Offline).await;
    let lld = row_key(&preview, "2001", "ds.1.itemids.1");
    let preview = ok(setup.service.decide_replace_host(
        LOCAL,
        &lld,
        &RowDecision::ChooseLldInstance {
            itemid: "203".to_owned(),
        },
        SessionContext::Offline,
    ));
    ok(setup.service.confirm_kept_on_source(
        LOCAL,
        &preview.kept_on_source.digest,
        SessionContext::Offline,
    ));

    // Round trip: parse → serialize gives the stored entries back.
    let stored = setup.workcopy_json(LOCAL);
    let entries = stored["operations"].as_array().cloned().unwrap_or_default();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[1]["op"], "replace_host_decision");
    assert_eq!(entries[1]["replace_seq"], 1);
    assert_eq!(
        entries[1]["decision"],
        json!({"kind": "choose_lld_instance", "itemid": "203"})
    );
    assert_eq!(entries[2]["op"], "replace_host_confirm_kept");
    let fingerprint = setup.base.storage_fingerprint().to_owned();
    let parsed = ok(parse_log(&entries, &fingerprint));
    assert_eq!(ok(to_log(&parsed)), entries);

    // Unknown operation name or version: rejected, file untouched.
    for unknown in [
        json!({"seq": 9, "op": "move_widget", "op_version": 1}),
        json!({"seq": 9, "op": "replace_host", "op_version": 2}),
    ] {
        let mut changed = stored.clone();
        if let Some(operations) = changed["operations"].as_array_mut() {
            operations.push(unknown.clone());
        }
        setup.write_workcopy_json(LOCAL, &changed);
        let before = setup.workcopy_bytes(LOCAL);
        let error = setup
            .service
            .decide_replace_host(
                LOCAL,
                &lld,
                &RowDecision::KeepOnSource,
                SessionContext::Offline,
            )
            .err()
            .unwrap_or_else(|| panic!("unknown op must reject"));
        assert_eq!(error.code(), "workcopy_unsupported_operation");
        assert_eq!(
            error.param("op"),
            unknown["op"]
                .as_str()
                .map(|op| op.replace('_', ""))
                .as_deref()
        );
        assert_eq!(
            code(
                setup
                    .service
                    .replace_host_preview(LOCAL, SessionContext::Offline)
            ),
            "workcopy_unsupported_operation"
        );
        assert_eq!(
            code(setup.service.local_snapshot(LOCAL)),
            "workcopy_unsupported_operation"
        );
        assert_eq!(setup.workcopy_bytes(LOCAL), before);
        assert!(setup.service.list_drafts().is_ok_and(|list| {
            list.damaged
                .iter()
                .any(|damaged| damaged.code == "workcopy_unsupported_operation")
        }));
    }

    // Known names that break the log rules are corrupt.
    let corrupt_logs = [
        // Decision before its replace_host / dangling replace_seq.
        json!([entries[1].clone()]),
        // Non-increasing seq.
        json!([entries[0].clone(), entries[0].clone()]),
        // Calculation of another Base.
        {
            let mut entry = entries[0].clone();
            entry["calculation"]["base_fingerprint"] = Value::String("0".repeat(64));
            json!([entry])
        },
        // Unknown field inside a known entry.
        {
            let mut entry = entries[0].clone();
            entry["extra"] = json!(true);
            json!([entry])
        },
    ];
    for operations in corrupt_logs {
        let mut changed = stored.clone();
        changed["operations"] = operations;
        setup.write_workcopy_json(LOCAL, &changed);
        let before = setup.workcopy_bytes(LOCAL);
        assert_eq!(
            code(
                setup
                    .service
                    .replace_host_preview(LOCAL, SessionContext::Offline)
            ),
            "workcopy_corrupt"
        );
        assert_eq!(setup.workcopy_bytes(LOCAL), before);
    }
    setup.write_workcopy_json(LOCAL, &stored);
    assert!(
        setup
            .service
            .replace_host_preview(LOCAL, SessionContext::Offline)
            .is_ok()
    );
}

#[tokio::test]
async fn a_decisions_are_operations_and_undo_restores_the_previous_state() {
    let mut lookup = fixture();
    for item in &mut lookup.items {
        if item.itemid == "204" {
            item.value_type = 1;
        }
    }
    let setup = setup(dashboard(), &[(LOCAL, None)]);
    let preview = calculate(&setup, &lookup, LOCAL, SessionContext::Offline).await;
    let lld = row_key(&preview, "2001", "ds.1.itemids.1");
    assert_eq!(
        row(&preview, "2001", "ds.1.itemids.1").state,
        ChangeState::Unresolved
    );
    assert_eq!(
        row(&preview, "2001", "ds.1.itemids.1").allowed_decisions,
        vec![DecisionKind::ChooseLldInstance, DecisionKind::KeepOnSource]
    );
    let decide = |key: &FieldKey, decision: RowDecision| {
        setup
            .service
            .decide_replace_host(LOCAL, key, &decision, SessionContext::Offline)
    };

    // Rejected decisions write nothing.
    let before = setup.workcopy_bytes(LOCAL);
    assert_eq!(
        code(decide(
            &lld,
            RowDecision::ChooseCandidate {
                itemid: "203".to_owned()
            }
        )),
        "decision_not_applicable"
    );
    assert_eq!(
        code(decide(
            &lld,
            RowDecision::ChooseLldInstance {
                itemid: "209".to_owned()
            }
        )),
        "candidate_not_visible"
    );
    assert_eq!(
        code(decide(
            &lld,
            RowDecision::ChooseLldInstance {
                itemid: "204".to_owned()
            }
        )),
        "candidate_value_type_incompatible"
    );
    let mask = row_key(&preview, "2001", "ds.2.hosts.0");
    assert_eq!(
        code(decide(&mask, RowDecision::KeepOnSource)),
        "decision_not_applicable"
    );
    let hidden = row_key(&preview, "2007", "itemid.0");
    assert_eq!(
        code(decide(
            &hidden,
            RowDecision::ChooseCandidate {
                itemid: "201".to_owned()
            }
        )),
        "decision_not_applicable"
    );
    assert_eq!(
        code(decide(&key(0, 99, "itemid.0"), RowDecision::KeepOnSource)),
        "binding_not_found"
    );
    assert_eq!(setup.workcopy_bytes(LOCAL), before);

    // LLD instance choice: the row changes to the chosen instance.
    let chosen = ok(decide(
        &lld,
        RowDecision::ChooseLldInstance {
            itemid: "203".to_owned(),
        },
    ));
    let lld_row = row(&chosen, "2001", "ds.1.itemids.1");
    assert_eq!(lld_row.state, ChangeState::Changed);
    assert_eq!(lld_row.decision_state, DecisionState::Applied);
    assert_eq!(
        lld_row.target,
        Some(FieldValue::Item {
            itemid: "203".to_owned()
        })
    );
    assert!(
        lld_row.differences.is_empty(),
        "same prototype, type, units"
    );
    let local = ok(setup.service.local_snapshot(LOCAL));
    assert_eq!(
        widget_value(local.raw_api_document(), 0, 1, "ds.1.itemids.1"),
        "203"
    );

    // Keep on source: a missing row and an unverified reference.
    let missing = row_key(&preview, "2002", "itemid.0");
    ok(decide(&missing, RowDecision::KeepOnSource));
    let kept = ok(decide(&hidden, RowDecision::KeepOnSource));
    for (widgetid, field) in [("2002", "itemid.0"), ("2007", "itemid.0")] {
        let kept_row = row(&kept, widgetid, field);
        assert_eq!(kept_row.state, ChangeState::KeptOnSource);
        assert!(kept.kept_on_source.entries.iter().any(|entry| {
            entry.widgetid.as_deref() == Some(widgetid) && entry.field == field && entry.by_decision
        }));
    }
    assert!(
        !kept
            .report
            .with_code(FindingCode::SourceUnverified)
            .any(|finding| finding
                .subject
                .as_ref()
                .and_then(|subject| subject.widgetid.as_deref())
                == Some("2007"))
    );

    // A later decision wins; undo restores the earlier one.
    let latest = ok(decide(&lld, RowDecision::KeepOnSource));
    assert_eq!(
        row(&latest, "2001", "ds.1.itemids.1").state,
        ChangeState::KeptOnSource
    );
    let latest_seq = row(&latest, "2001", "ds.1.itemids.1")
        .decision_seq
        .unwrap_or_else(|| panic!("decision seq"));
    assert_eq!(
        ok(setup.service.remove_operation(LOCAL, latest_seq)),
        vec![latest_seq]
    );
    let restored = ok(setup
        .service
        .replace_host_preview(LOCAL, SessionContext::Offline));
    assert_eq!(
        row(&restored, "2001", "ds.1.itemids.1").state,
        ChangeState::Changed
    );
    let first_seq = row(&restored, "2001", "ds.1.itemids.1")
        .decision_seq
        .unwrap_or_else(|| panic!("decision seq"));
    ok(setup.service.remove_operation(LOCAL, first_seq));
    let undone = ok(setup
        .service
        .replace_host_preview(LOCAL, SessionContext::Offline));
    assert_eq!(
        row(&undone, "2001", "ds.1.itemids.1").state,
        ChangeState::Unresolved
    );
    assert_eq!(row(&undone, "2001", "ds.1.itemids.1").decision, None);

    // Removing replace_host removes its decisions too; Local is Base again.
    assert_eq!(
        code(setup.service.remove_operation(LOCAL, 999)),
        "operation_not_found"
    );
    let removed = ok(setup.service.remove_operation(LOCAL, 1));
    assert_eq!(removed.len(), 3);
    assert!(ok(setup.service.operation_history(LOCAL)).is_empty());
    assert_eq!(
        ok(setup.service.local_snapshot(LOCAL)).fingerprint(),
        setup.base.fingerprint()
    );
    assert_eq!(
        code(
            setup
                .service
                .replace_host_preview(LOCAL, SessionContext::Offline)
        ),
        "replace_host_not_calculated"
    );
}

#[tokio::test]
async fn a_recalculation_keeps_decisions_and_another_intent_is_rejected() {
    let setup = setup(dashboard(), &[(LOCAL, None)]);
    let lookup = fixture();
    let preview = calculate(&setup, &lookup, LOCAL, SessionContext::Offline).await;
    let lld = row_key(&preview, "2001", "ds.1.itemids.1");
    ok(setup.service.decide_replace_host(
        LOCAL,
        &lld,
        &RowDecision::ChooseLldInstance {
            itemid: "203".to_owned(),
        },
        SessionContext::Offline,
    ));
    let again = calculate(&setup, &lookup, LOCAL, SessionContext::Offline).await;
    assert_eq!(again.operation.seq, 1);
    assert_eq!(
        row(&again, "2001", "ds.1.itemids.1").state,
        ChangeState::Changed
    );
    assert_eq!(ok(setup.service.operation_history(LOCAL)).len(), 2);

    let other = ReplaceHostRequest {
        target_hostid: "13".to_owned(),
        ..request(ResolveScope::Dashboard)
    };
    let error = setup
        .service
        .calculate_replace_host(&lookup, LOCAL, &other, SessionContext::Offline)
        .await
        .err()
        .unwrap_or_else(|| panic!("pending intent"));
    assert_eq!(error.code(), "replace_host_pending");
    assert_eq!(error.param("seq"), Some("1"));

    let calls = lookup.calls().len();
    for (source, target, expected) in [
        ("x", TARGET, "resolver_hostid_invalid"),
        (SOURCE, SOURCE, "replace_host_same_host"),
    ] {
        let invalid = ReplaceHostRequest {
            source_hostid: source.to_owned(),
            target_hostid: target.to_owned(),
            scope: ResolveScope::Dashboard,
        };
        assert_eq!(
            code(
                setup
                    .service
                    .calculate_replace_host(&lookup, LOCAL, &invalid, SessionContext::Offline)
                    .await
            ),
            expected
        );
    }
    assert_eq!(lookup.calls().len(), calls, "invalid intents read nothing");
}

// ---------- P2-03B: structural diff ----------

#[tokio::test]
async fn b_multi_page_diff_by_page_widget_and_series() {
    let setup = setup(multi_page_dashboard(), &[(LOCAL, None)]);
    let capabilities = adapter_capabilities("7.4.16");
    let preview = calculate(&setup, &fixture(), LOCAL, read_write(&capabilities)).await;
    let diff = &preview.diff;
    assert_eq!(diff.pages.len(), 3);
    let states: Vec<ChangeState> = diff.pages.iter().map(|page| page.state).collect();
    assert_eq!(
        states,
        vec![
            ChangeState::Unresolved,
            ChangeState::Unresolved,
            ChangeState::Changed
        ]
    );
    assert_eq!(diff.pages[2].name, "Third");
    assert_eq!(diff.pages[2].widget_counts.changed, 2);

    let widget = |page: usize, widgetid: &str| {
        diff.pages[page]
            .widgets
            .iter()
            .find(|widget| widget.widgetid.as_deref() == Some(widgetid))
            .unwrap_or_else(|| panic!("widget {widgetid}"))
    };
    // Unrelated widgets are untouched and carry no rows.
    for widgetid in ["2006", "2008"] {
        assert_eq!(widget(0, widgetid).state, ChangeState::Untouched);
        assert!(widget(0, widgetid).rows.is_empty());
    }
    assert_eq!(widget(1, "2107").state, ChangeState::Untouched);
    assert_eq!(widget(0, "2000").state, ChangeState::Changed);
    assert_eq!(widget(0, "2002").state, ChangeState::Unresolved);
    assert_eq!(widget(1, "2100").state, ChangeState::KeptOnSource);

    // svggraph series: changed, unresolved (LLD), kept (mask, `*`), untouched.
    let graph = widget(0, "2001");
    assert_eq!(graph.state, ChangeState::Unresolved);
    let series: Vec<(u32, ChangeState)> = graph
        .series
        .iter()
        .map(|series| (series.dataset_index, series.state))
        .collect();
    assert_eq!(
        series,
        vec![
            (0, ChangeState::Changed),
            (1, ChangeState::Unresolved),
            (2, ChangeState::KeptOnSource),
            (3, ChangeState::KeptOnSource),
            (4, ChangeState::Untouched),
        ]
    );
    assert_eq!(graph.series[1].fields.changed, 1);
    assert_eq!(graph.series[1].fields.unresolved, 1);
    let third = widget(2, "3001");
    let third_series: Vec<ChangeState> = third.series.iter().map(|series| series.state).collect();
    assert_eq!(
        third_series,
        vec![ChangeState::Changed, ChangeState::Untouched]
    );
    assert_eq!(
        row(&preview, "3001", "ds.0.hosts.0").match_status,
        Some(crate::resolver::MatchStatus::Exact)
    );

    let summary = diff.summary;
    assert_eq!(summary.pages.total, 3);
    // Classic graphs are editable on 7.4.16: 2102 changes, 2103 (no
    // same-name graph on the target) is unresolved.
    assert_eq!(summary.fields.changed, 8);
    assert_eq!(summary.fields.unresolved, 3 + 2 + 1);
    assert_eq!(summary.fields.kept_on_source, 8);
    assert_eq!(summary.series.total, 7);
    assert_eq!(summary.series.untouched, 2);

    // Local changes both pages; one update over several pages is proven on
    // 7.4.16 (LAB-02), so it is not reported as an unverified contract.
    let local = ok(setup.service.local_snapshot(LOCAL));
    assert_eq!(
        widget_value(local.raw_api_document(), 2, 0, "itemid.0"),
        "201"
    );
    assert_eq!(
        widget_value(local.raw_api_document(), 2, 1, "ds.0.hosts.0"),
        "db-new-01"
    );
    assert_eq!(
        widget_value(local.raw_api_document(), 2, 1, "ds.1.hosts.0"),
        "shared-srv"
    );
    assert!(!contracts(&preview).contains(CONTRACT_SEVERAL_PAGES));
}

#[tokio::test]
async fn b_page_draft_limits_scope_diff_and_local() {
    let setup = setup(dashboard(), &[(LOCAL_PAGE, Some("1000"))]);
    let lookup = fixture();
    let preview = calculate(&setup, &lookup, LOCAL_PAGE, SessionContext::Offline).await;
    assert_eq!(preview.kind, DraftKind::Page);
    assert_eq!(
        preview.operation.scope,
        ReplaceScope::Pages {
            dashboard_pageids: vec!["1000".to_owned()]
        }
    );
    assert_eq!(preview.diff.pages.len(), 1);
    assert_eq!(
        preview.diff.pages[0].dashboard_pageid.as_deref(),
        Some("1000")
    );
    assert!(preview.rows.iter().all(|row| row.key.page_index == 0));
    let outside = preview
        .report
        .with_code(FindingCode::ReferencesOutsideScope)
        .next()
        .unwrap_or_else(|| panic!("outside scope"));
    assert_eq!(outside.param("count"), Some("6"));
    assert_eq!(outside.severity, FindingSeverity::Informational);

    let local = ok(setup.service.local_snapshot(LOCAL_PAGE));
    assert_eq!(
        local.raw_api_document()["pages"][1],
        setup.base.raw_api_document()["pages"][1]
    );
    assert_eq!(
        widget_value(local.raw_api_document(), 0, 0, "itemid.0"),
        "201"
    );

    ok(setup.service.remove_operation(LOCAL_PAGE, 1));
    for scope in [
        ResolveScope::Pages(BTreeSet::from(["1001".to_owned()])),
        ResolveScope::Widgets(BTreeSet::from(["2100".to_owned()])),
    ] {
        assert_eq!(
            code(
                setup
                    .service
                    .calculate_replace_host(
                        &lookup,
                        LOCAL_PAGE,
                        &request(scope),
                        SessionContext::Offline
                    )
                    .await
            ),
            "replace_host_scope_outside_draft"
        );
    }
    assert_eq!(
        code(
            setup
                .service
                .calculate_replace_host(
                    &lookup,
                    LOCAL_PAGE,
                    &request(ResolveScope::Pages(BTreeSet::from(["4040".to_owned()]))),
                    SessionContext::Offline
                )
                .await
        ),
        "replace_host_scope_invalid"
    );
}

#[tokio::test]
async fn b_widget_scope_reports_link_crossings() {
    let setup = setup(dashboard(), &[(LOCAL, None)]);
    let preview = setup
        .service
        .calculate_replace_host(
            &fixture(),
            LOCAL,
            &request(ResolveScope::Widgets(BTreeSet::from(["2107".to_owned()]))),
            SessionContext::Offline,
        )
        .await
        .unwrap_or_else(|error| panic!("calculate: {error}"));
    let crossing = finding(
        &preview,
        FindingCode::ScopeLinkCrossing,
        "2107",
        "override_hostid._reference",
    );
    assert_eq!(crossing.param("side"), Some("provider_outside"));
    assert_eq!(crossing.severity, FindingSeverity::Informational);
    assert!(blocker_codes(&preview).contains(&FindingCode::NoChanges));
}

// ---------- P2-03C: blockers ----------

#[tokio::test]
async fn c_every_resolver_negative_surfaces_as_the_right_finding() {
    let setup = setup(dashboard(), &[(LOCAL, None)]);
    let capabilities = adapter_capabilities("7.4.16");
    let preview = calculate(&setup, &fixture(), LOCAL, read_write(&capabilities)).await;
    let blocking = |code, widgetid, field, reason| {
        let found = finding(&preview, code, widgetid, field);
        assert_eq!(
            found.severity,
            FindingSeverity::BlocksPublication,
            "{widgetid}/{field}"
        );
        assert_eq!(found.reason, Some(reason), "{widgetid}/{field}");
    };
    blocking(
        FindingCode::BindingUnresolved,
        "2002",
        "itemid.0",
        ReasonCode::KeyNotFound,
    );
    blocking(
        FindingCode::BindingUnresolved,
        "2001",
        "ds.1.itemids.1",
        ReasonCode::LldInstanceMissing,
    );
    blocking(
        FindingCode::BindingUnresolved,
        "2003",
        "itemid.0",
        ReasonCode::ValueTypeIncompatible,
    );
    blocking(
        FindingCode::SourceUnverified,
        "2007",
        "itemid.0",
        ReasonCode::NotFoundOrNoAccess,
    );
    blocking(
        FindingCode::BindingUnresolved,
        "2103",
        "graphid.0",
        ReasonCode::GraphNameNotFound,
    );
    blocking(
        FindingCode::SourceUnverified,
        "2106",
        "graphid.0",
        ReasonCode::PrototypeOwnerUnknown,
    );
    assert_eq!(
        finding(
            &preview,
            FindingCode::BindingUnresolved,
            "2001",
            "ds.1.itemids.1"
        )
        .param("match"),
        Some("unresolved_lld")
    );

    // O-003: kept references are informational, never blockers by themselves.
    for (widgetid, field, reason) in [
        ("2001", "ds.2.hosts.0", ReasonCode::HostMask),
        ("2001", "ds.3.hosts.0", ReasonCode::HostWildcardAll),
        (
            "2001",
            "problemhosts.0",
            ReasonCode::PatternFieldNotSupported,
        ),
        ("2100", "hostids.0", ReasonCode::HostFieldNotSupported),
        ("2101", "hostids.0", ReasonCode::HostFieldNotSupported),
        ("2104", "itemid.0", ReasonCode::WidgetNotEditable),
        ("2105", "itemid.0", ReasonCode::UnknownWidget),
    ] {
        let kept = finding(&preview, FindingCode::KeptOnSource, widgetid, field);
        assert_eq!(kept.severity, FindingSeverity::Informational);
        assert_eq!(kept.reason, Some(reason), "{widgetid}/{field}");
    }
    assert_eq!(preview.kept_on_source.entries.len(), 8);
    assert!(!preview.kept_on_source.confirmed);
    let unconfirmed = preview
        .report
        .with_code(FindingCode::KeptOnSourceUnconfirmed)
        .next()
        .unwrap_or_else(|| panic!("unconfirmed list"));
    assert!(unconfirmed.blocks_publication());
    assert_eq!(unconfirmed.param("count"), Some("8"));
    assert_eq!(
        unconfirmed.param("digest"),
        Some(preview.kept_on_source.digest.as_str())
    );

    // `compatible` rows change with a warning.
    for (widgetid, field) in [
        ("2004", "itemid.0"),
        ("2005", "itemid.0"),
        ("2001", "ds.0.hosts.0"),
    ] {
        let warning = finding(&preview, FindingCode::MatchDifferences, widgetid, field);
        assert_eq!(warning.severity, FindingSeverity::Informational);
    }
    assert_eq!(
        finding(&preview, FindingCode::MatchDifferences, "2004", "itemid.0").param("differences"),
        Some("units")
    );

    // svggraph writes are proven on 7.4.16 (P2-04A, LAB-02): no contract
    // finding in read_write.
    assert!(contracts(&preview).is_empty(), "{:?}", contracts(&preview));
    // The classic graph change is checked against its own contract (P2-04H).
    assert!(preview.report.checks.iter().any(|check| {
        check.name == "classic_graph_graphid"
            && check.state == zdashstitch_zabbix::CapabilityState::Supported
            && check.blocking
    }));
    assert!(!blocker_codes(&preview).contains(&FindingCode::WriteModeRequired));
    assert!(!preview.report.allows_publication());
}

#[tokio::test]
async fn c_hidden_target_is_denied_and_blocks_only_publication() {
    let mut lookup = fixture();
    lookup.hosts.retain(|host| host.hostid != TARGET);
    let setup = setup(dashboard(), &[(LOCAL, None)]);
    let preview = calculate(&setup, &lookup, LOCAL, SessionContext::Offline).await;
    assert!(blocker_codes(&preview).contains(&FindingCode::TargetHostNotVisible));
    let denied = finding(&preview, FindingCode::BindingUnresolved, "2000", "itemid.0");
    assert_eq!(denied.reason, Some(ReasonCode::NotFoundOrNoAccess));
    assert_eq!(denied.param("match"), Some("denied"));
    // The local preview is still complete; Local equals Base.
    assert!(!preview.diff.pages.is_empty());
    assert!(preview.changes.is_empty());
    assert_eq!(preview.local_fingerprint, preview.base_fingerprint);
    assert!(blocker_codes(&preview).contains(&FindingCode::NoChanges));
}

#[tokio::test]
async fn c_kept_list_confirmation_binds_the_exact_list() {
    let setup = setup(dashboard(), &[(LOCAL, None)]);
    let preview = calculate(&setup, &fixture(), LOCAL, SessionContext::Offline).await;
    let before = setup.workcopy_bytes(LOCAL);
    assert_eq!(
        code(
            setup
                .service
                .confirm_kept_on_source(LOCAL, &"0".repeat(64), SessionContext::Offline)
        ),
        "kept_on_source_list_changed"
    );
    assert_eq!(setup.workcopy_bytes(LOCAL), before);
    let confirmed = ok(setup.service.confirm_kept_on_source(
        LOCAL,
        &preview.kept_on_source.digest,
        SessionContext::Offline,
    ));
    assert!(confirmed.kept_on_source.confirmed);
    assert!(!blocker_codes(&confirmed).contains(&FindingCode::KeptOnSourceUnconfirmed));
    // Confirming the same list again records nothing new.
    let again = ok(setup.service.confirm_kept_on_source(
        LOCAL,
        &preview.kept_on_source.digest,
        SessionContext::Offline,
    ));
    assert_eq!(again.revision, confirmed.revision);

    // A new kept row changes the list: the confirmation becomes stale.
    let missing = row_key(&preview, "2002", "itemid.0");
    let changed = ok(setup.service.decide_replace_host(
        LOCAL,
        &missing,
        &RowDecision::KeepOnSource,
        SessionContext::Offline,
    ));
    assert!(!changed.kept_on_source.confirmed);
    assert_ne!(changed.kept_on_source.digest, preview.kept_on_source.digest);
    assert!(blocker_codes(&changed).contains(&FindingCode::KeptOnSourceConfirmationStale));
    let reconfirmed = ok(setup.service.confirm_kept_on_source(
        LOCAL,
        &changed.kept_on_source.digest,
        SessionContext::Offline,
    ));
    assert!(reconfirmed.kept_on_source.confirmed);
}

#[tokio::test]
async fn c_missing_write_mode_blocks_only_publication() {
    let setup = setup(item_only_dashboard(), &[(LOCAL, None)]);
    let capabilities = adapter_capabilities("7.4.16");
    let preview = calculate(&setup, &fixture(), LOCAL, read_only(&capabilities)).await;
    assert_eq!(
        blocker_codes(&preview),
        BTreeSet::from([
            FindingCode::WriteModeRequired,
            FindingCode::KeptOnSourceUnconfirmed
        ])
    );
    let confirmed = ok(setup.service.confirm_kept_on_source(
        LOCAL,
        &preview.kept_on_source.digest,
        read_only(&capabilities),
    ));
    assert_eq!(
        blocker_codes(&confirmed),
        BTreeSet::from([FindingCode::WriteModeRequired])
    );
    assert!(!confirmed.report.allows_publication());
    // Same stored state, read_write session: only proven item changes and a
    // confirmed kept list remain, so P2-04 may build a plan.
    let writable = ok(setup
        .service
        .replace_host_preview(LOCAL, read_write(&capabilities)));
    assert!(
        writable.report.allows_publication(),
        "{:?}",
        writable.report
    );
    assert_eq!(writable.changes.len(), 1);
    assert_eq!(writable.kept_on_source.entries.len(), 1);
    // Offline: the connection is missing, the preview is the same.
    let offline = ok(setup
        .service
        .replace_host_preview(LOCAL, SessionContext::Offline));
    assert_eq!(offline.diff, writable.diff);
    assert!(blocker_codes(&offline).contains(&FindingCode::ConnectionRequired));
}

#[tokio::test]
async fn c_revalidation_marks_stale_and_invalid_rows_without_writing() {
    let setup = setup(item_only_dashboard(), &[(LOCAL, None)]);
    let capabilities = adapter_capabilities("7.4.16");
    calculate(&setup, &fixture(), LOCAL, read_write(&capabilities)).await;
    let before = setup.workcopy_bytes(LOCAL);

    // Unchanged facts: everything checked.
    let same = ok(setup
        .service
        .revalidate_replace_host(&fixture(), LOCAL, read_write(&capabilities))
        .await);
    assert!(same.checked_at_ms.is_some());
    assert!(same.rows.iter().all(|row| row.check == RowCheck::Checked));

    // The target item disappears: stale, the stored target is kept and no
    // other item is chosen.
    let mut changed = fixture();
    changed.items.retain(|item| item.itemid != "201");
    let stale = ok(setup
        .service
        .revalidate_replace_host(&changed, LOCAL, read_write(&capabilities))
        .await);
    let item_row = row(&stale, "2200", "itemid.0");
    assert_eq!(item_row.check, RowCheck::Stale);
    assert_eq!(item_row.check_reason, Some(CheckReason::ResolutionChanged));
    assert_eq!(
        item_row.target,
        Some(FieldValue::Item {
            itemid: "201".to_owned()
        })
    );
    assert_eq!(
        finding(&stale, FindingCode::BindingStale, "2200", "itemid.0").param("check_reason"),
        Some("resolution_changed")
    );

    // The target host is hidden: invalid.
    let mut hidden = fixture();
    hidden.hosts.retain(|host| host.hostid != TARGET);
    let invalid = ok(setup
        .service
        .revalidate_replace_host(&hidden, LOCAL, read_write(&capabilities))
        .await);
    assert_eq!(row(&invalid, "2200", "itemid.0").check, RowCheck::Invalid);
    assert!(blocker_codes(&invalid).contains(&FindingCode::BindingInvalid));

    // Another server version: every row stale, version mismatch.
    let newer = adapter_capabilities("7.4.17");
    let other_version = ok(setup
        .service
        .replace_host_preview(LOCAL, read_write(&newer)));
    assert!(other_version.rows.iter().all(|row| {
        row.check == RowCheck::Stale && row.check_reason == Some(CheckReason::ServerVersionChanged)
    }));
    assert!(blocker_codes(&other_version).contains(&FindingCode::ServerVersionMismatch));

    assert_eq!(
        setup.workcopy_bytes(LOCAL),
        before,
        "revalidation writes nothing"
    );
}

#[tokio::test]
async fn c_revalidation_invalidates_a_chosen_candidate_that_disappeared() {
    let setup = setup(dashboard(), &[(LOCAL, None)]);
    let preview = calculate(&setup, &fixture(), LOCAL, SessionContext::Offline).await;
    let lld = row_key(&preview, "2001", "ds.1.itemids.1");
    ok(setup.service.decide_replace_host(
        LOCAL,
        &lld,
        &RowDecision::ChooseLldInstance {
            itemid: "203".to_owned(),
        },
        SessionContext::Offline,
    ));
    let mut changed = fixture();
    changed.items.retain(|item| item.itemid != "203");
    let checked = ok(setup
        .service
        .revalidate_replace_host(&changed, LOCAL, SessionContext::Offline)
        .await);
    let lld_row = row(&checked, "2001", "ds.1.itemids.1");
    assert_eq!(lld_row.check, RowCheck::Invalid);
    assert_eq!(lld_row.check_reason, Some(CheckReason::CandidateNotVisible));
    // The stored choice is shown, never replaced by another instance.
    assert_eq!(
        lld_row.target,
        Some(FieldValue::Item {
            itemid: "203".to_owned()
        })
    );
    assert!(blocker_codes(&checked).contains(&FindingCode::BindingInvalid));
}

// ---------- P2-03E: wizard inputs (09-01, 09-02) ----------

#[tokio::test]
async fn e_inventory_lists_references_with_outcomes_and_the_page_scope() {
    let setup = setup(dashboard(), &[(LOCAL, None), (LOCAL_PAGE, Some("1000"))]);
    let lookup = fixture();
    let whole = ok(setup.service.replace_host_inventory(&lookup, LOCAL).await);
    assert_eq!(whole.kind, DraftKind::Dashboard);
    assert_eq!(
        whole.pages.len(),
        setup.base.raw_api_document()["pages"]
            .as_array()
            .map_or(0, Vec::len)
    );
    let source = whole
        .hosts
        .iter()
        .find(|host| host.hostid == SOURCE)
        .unwrap_or_else(|| panic!("source host: {whole:?}"));
    assert!(
        source
            .references
            .iter()
            .any(|reference| reference.outcome == ExpectedOutcome::Changes)
    );
    // A reason exactly for references that stay on the old host.
    assert!(source.references.iter().all(|reference| {
        (reference.outcome == ExpectedOutcome::Changes) == reference.reason.is_none()
    }));
    // The calculation agrees on what can change.
    let preview = calculate(&setup, &lookup, LOCAL, SessionContext::Offline).await;
    let editable = preview
        .rows
        .iter()
        .filter(|row| row.applicability == Some(crate::resolver::Applicability::Editable))
        .count();
    let changes = source
        .references
        .iter()
        .filter(|reference| reference.outcome == ExpectedOutcome::Changes)
        .count();
    assert_eq!(changes, editable);

    let page = ok(setup
        .service
        .replace_host_inventory(&lookup, LOCAL_PAGE)
        .await);
    assert_eq!(page.kind, DraftKind::Page);
    assert_eq!(page.pages.len(), 1);
    assert_eq!(page.pages[0].dashboard_pageid.as_deref(), Some("1000"));
    assert!(page.hosts.iter().all(|host| {
        host.references
            .iter()
            .all(|reference| reference.page_index == 0)
    }));
    assert!(
        page.unattributed
            .iter()
            .all(|reference| reference.page_index == 0)
    );
    assert_eq!(
        code(setup.service.replace_host_inventory(&lookup, "../x").await),
        "workcopy_id_invalid"
    );
}

struct Hosts;

fn listing(hostid: &str, name: &str, templates: &[&str]) -> zdashstitch_zabbix::HostListing {
    zdashstitch_zabbix::HostListing {
        hostid: hostid.to_owned(),
        host: name.to_owned(),
        name: name.to_owned(),
        groups: vec!["Synthetic".to_owned()],
        templates: templates
            .iter()
            .map(|templateid| zdashstitch_zabbix::TemplateRef {
                templateid: (*templateid).to_owned(),
                name: format!("Template {templateid}"),
            })
            .collect(),
    }
}

impl HostSearchLookup for Hosts {
    async fn search_hosts(
        &self,
        search: &str,
        limit: usize,
    ) -> Result<
        zdashstitch_zabbix::HostObjects<zdashstitch_zabbix::HostListing>,
        zdashstitch_zabbix::ZabbixError,
    > {
        assert_eq!(limit, TARGET_SEARCH_LIMIT + 1);
        let all = [
            listing(SOURCE, "db-old-01", &["901", "902"]),
            listing(TARGET, "db-new-01", &["901", "902"]),
            listing("7777", "db-new-02", &["901"]),
        ];
        Ok(zdashstitch_zabbix::HostObjects {
            objects: all
                .into_iter()
                .filter(|host| host.name.contains(search))
                .collect(),
            truncated: false,
        })
    }

    async fn host_listings(
        &self,
        hostids: &[String],
    ) -> Result<Vec<zdashstitch_zabbix::HostListing>, zdashstitch_zabbix::ZabbixError> {
        Ok(hostids
            .iter()
            .filter(|id| id.as_str() == SOURCE)
            .map(|_| listing(SOURCE, "db-old-01", &["901", "902"]))
            .collect())
    }
}

#[tokio::test]
async fn e_target_search_excludes_the_replaced_host_and_counts_templates() {
    let setup = setup(dashboard(), &[(LOCAL, None)]);
    let found = ok(setup
        .service
        .search_replace_host_targets(&Hosts, LOCAL, SOURCE, " db-new ")
        .await);
    assert_eq!(
        found
            .hosts
            .iter()
            .map(|host| (host.hostid.as_str(), host.source_templates_found))
            .collect::<Vec<_>>(),
        [(TARGET, 2), ("7777", 1)]
    );
    assert_eq!(found.source_templates.map(|names| names.len()), Some(2));
    assert_eq!(found.empty_reason, None);
    let all = ok(setup
        .service
        .search_replace_host_targets(&Hosts, LOCAL, SOURCE, "")
        .await);
    assert!(all.hosts.iter().all(|host| host.hostid != SOURCE));
    let none = ok(setup
        .service
        .search_replace_host_targets(&Hosts, LOCAL, SOURCE, "zzz")
        .await);
    assert_eq!(none.empty_reason, Some("not_visible_or_denied"));
    let long = "x".repeat(MAX_TARGET_SEARCH_CHARS + 1);
    let refused = setup
        .service
        .search_replace_host_targets(&Hosts, LOCAL, SOURCE, &long)
        .await;
    assert_eq!(code(refused), "search_invalid");
    assert_eq!(
        code(
            setup
                .service
                .search_replace_host_targets(&Hosts, LOCAL, "x1", "")
                .await
        ),
        "resolver_hostid_invalid"
    );
}
