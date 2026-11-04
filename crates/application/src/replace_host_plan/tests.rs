// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-04B plan verification on the synthetic resolver fixture over a real
//! local store. Capabilities and authority are test values: plans built
//! here can never be sent (detached authority). The send path is covered
//! by `tests/replace_host_plan.rs` on a mock transport.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use zdashstitch_zabbix::{
    AccessMode, AdapterRegistry, CapabilityState, DashboardWriteContracts, PublicationContext,
    VersionedDashboardSnapshot, ZabbixCapabilities,
};

use super::*;
use crate::publisher::CompatibilityReport;
use crate::replace_host::ReplaceScope;
use crate::resolver::tests::{SOURCE, TARGET, field, fixture};
use crate::resolver::{ReplaceHostRequest, ResolveScope};
use crate::workcopy::formats::{
    BaseReference, DraftKind, ItemMetadataState, LOCAL_ONLY, LocalMetadata, SNAPSHOT_FORMAT,
    SnapshotFile, SnapshotScope, WORKCOPY_FORMAT, WorkcopyFile, WorkcopySource,
};
use crate::workcopy::store::LocalStore;
use crate::{CONTRACT_RESEND_WIDGET_TYPE, FindingCode, LocalSession};

const CONNECTION: &str = "conn-0123456789abcdef0123456789abcdef";
const LOCAL: &str = "wc-00112233445566778899aabbccddeeff";

fn capabilities() -> ZabbixCapabilities {
    AdapterRegistry::resolve(
        "7.4.16"
            .parse()
            .unwrap_or_else(|error| panic!("version: {error}")),
    )
    .unwrap_or_else(|error| panic!("adapter: {error}"))
}

/// The 7.4.16 capabilities with the listed LAB-02 contracts withdrawn
/// (`Unsupported`): only for tests of the gates.
fn without_contracts(withdrawn: &[&str]) -> ZabbixCapabilities {
    let mut capabilities = capabilities();
    let contracts: &mut DashboardWriteContracts = &mut capabilities.dashboard_write_contracts;
    for name in withdrawn {
        let state = match *name {
            "update_several_pages" => &mut contracts.update_several_pages,
            "svggraph_dataset_itemids" => &mut contracts.svggraph_dataset_itemids,
            "svggraph_dataset_hosts" => &mut contracts.svggraph_dataset_hosts,
            other => panic!("unknown contract {other}"),
        };
        *state = CapabilityState::Unsupported;
    }
    capabilities
}

fn widget(widgetid: &str, widget_type: &str, x: u32, fields: Vec<Value>) -> Value {
    json!({
        "widgetid": widgetid, "type": widget_type, "name": format!("w{widgetid}"),
        "x": x.to_string(), "y": "0", "width": "12", "height": "4", "view_mode": "0",
        "fields": fields
    })
}

/// Page 0: two item widgets on the source host and one untouched item of
/// another host; page 1: one item on the source host; page 2: untouched.
fn dashboard() -> Value {
    json!({
        "dashboardid": "950", "name": "Synthetic plan dashboard", "userid": "3",
        "private": "1", "display_period": "30", "auto_start": "1",
        "pages": [
            {"dashboard_pageid": "1500", "name": "Main", "display_period": "0", "widgets": [
                widget("2500", "item", 0, vec![
                    field("4", "itemid.0", "101"),
                    field("1", "reference", "AAAAA"),
                    field("0", "show.0", "2"),
                ]),
                widget("2501", "item", 12, vec![field("4", "itemid.0", "102")]),
                widget("2502", "item", 24, vec![field("4", "itemid.0", "130")]),
            ]},
            {"dashboard_pageid": "1501", "name": "Second", "display_period": "60", "widgets": [
                widget("2510", "item", 0, vec![field("4", "itemid.0", "108")]),
            ]},
            {"dashboard_pageid": "1502", "name": "Untouched", "display_period": "0", "widgets": [
                widget("2520", "clock", 0, vec![field("0", "clock_type", "1")]),
            ]}
        ],
        "users": [], "userGroups": []
    })
}

struct Setup {
    _dir: tempfile::TempDir,
    service: WorkcopyService,
    base: VersionedDashboardSnapshot,
}

fn setup(raw: Value) -> Setup {
    let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"));
    let store = LocalStore::at(dir.path().to_path_buf());
    let base = VersionedDashboardSnapshot::from_adapter_raw(
        CONNECTION.to_owned(),
        capabilities().version,
        raw,
    )
    .unwrap_or_else(|error| panic!("snapshot: {error}"));
    let fingerprint = base.storage_fingerprint().to_owned();
    store
        .put_snapshot(&SnapshotFile {
            format: SNAPSHOT_FORMAT.to_owned(),
            storage_scope: LOCAL_ONLY.to_owned(),
            connection_id: CONNECTION.to_owned(),
            adapter_id: base.adapter_id().to_owned(),
            server_version: "7.4.16".to_owned(),
            dashboardid: base.dashboard_id().to_owned(),
            scope: SnapshotScope::Dashboard,
            first_read_at_ms: 1,
            last_read_at_ms: 2,
            first_read_fingerprint: fingerprint.clone(),
            last_read_fingerprint: fingerprint.clone(),
            fingerprint: fingerprint.clone(),
            raw_api_document: base.raw_api_document().clone(),
        })
        .unwrap_or_else(|error| panic!("snapshot: {error}"));
    store
        .create_workcopy(&WorkcopyFile {
            format: WORKCOPY_FORMAT.to_owned(),
            storage_scope: LOCAL_ONLY.to_owned(),
            workcopy_id: LOCAL.to_owned(),
            kind: DraftKind::Dashboard,
            name: "Synthetic".to_owned(),
            created_at_ms: 10,
            updated_at_ms: 10,
            revision: 0,
            source: WorkcopySource {
                connection_id: CONNECTION.to_owned(),
                dashboardid: base.dashboard_id().to_owned(),
                dashboard_name: "Synthetic".to_owned(),
                page: None,
                adapter_id: base.adapter_id().to_owned(),
                server_version: "7.4.16".to_owned(),
            },
            base: BaseReference {
                fingerprint,
                read_at_ms: 2,
                source_checked_at_ms: 2,
                page_count: 3,
                widget_count: 5,
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
    Setup {
        service: WorkcopyService::new(dir.path().to_path_buf()),
        _dir: dir,
        base,
    }
}

fn read_write(capabilities: &ZabbixCapabilities) -> SessionContext<'_> {
    SessionContext::Connected {
        access_mode: AccessMode::ReadWrite,
        capabilities,
    }
}

fn pages(ids: &[&str]) -> ResolveScope {
    ResolveScope::Pages(ids.iter().map(|id| (*id).to_owned()).collect())
}

async fn calculate(
    setup: &Setup,
    scope: ResolveScope,
    capabilities: &ZabbixCapabilities,
) -> ReplaceHostPreview {
    setup
        .service
        .calculate_replace_host(
            &fixture(),
            LOCAL,
            &ReplaceHostRequest {
                source_hostid: SOURCE.to_owned(),
                target_hostid: TARGET.to_owned(),
                scope,
            },
            read_write(capabilities),
        )
        .await
        .unwrap_or_else(|error| panic!("calculate: {error} {:?}", error.params()))
}

fn request(preview: &ReplaceHostPreview, session: &LocalSession) -> ReplaceHostPlanRequest {
    ReplaceHostPlanRequest {
        local_id: preview.local_id.clone(),
        connection_id: CONNECTION.to_owned(),
        revision: preview.revision,
        base_fingerprint: preview.base_fingerprint.clone(),
        local_fingerprint: preview.local_fingerprint.clone(),
        kept_digest: (!preview.kept_on_source.entries.is_empty())
            .then(|| preview.kept_on_source.digest.clone()),
        session: session.current(),
    }
}

fn local(setup: &Setup) -> VersionedDashboardSnapshot {
    setup
        .service
        .local_snapshot(LOCAL)
        .unwrap_or_else(|error| panic!("local: {error}"))
}

fn verify(
    setup: &Setup,
    request: &ReplaceHostPlanRequest,
    preview: &ReplaceHostPreview,
    capabilities: &ZabbixCapabilities,
) -> Result<VerifiedPlan, PlanError> {
    verify_replace_host_plan(&PlanInputs {
        request,
        preview,
        local: &local(setup),
        remote: Some(&setup.base),
        editable: true,
        context: &PublicationContext::detached_for_testing(capabilities.clone()),
    })
}

fn incompatible(result: Result<VerifiedPlan, PlanError>) -> CompatibilityReport {
    match result {
        Err(PlanError::Incompatible(report)) => report,
        Err(error) => panic!("expected incompatible, got {error}"),
        Ok(_) => panic!("expected incompatible, got a plan"),
    }
}

fn invalidated(result: Result<VerifiedPlan, PlanError>) -> PlanInvalidReason {
    match result {
        Err(PlanError::Invalidated(reason)) => reason,
        Err(error) => panic!("expected invalidated, got {error}"),
        Ok(_) => panic!("expected invalidated, got a plan"),
    }
}

fn unverified_contracts(report: &CompatibilityReport) -> BTreeSet<String> {
    report
        .with_code(FindingCode::WriteContractUnverified)
        .filter_map(|finding| finding.param("contract").map(str::to_owned))
        .collect()
}

#[tokio::test]
async fn item_widgets_on_one_page_make_one_plan_equal_to_local() {
    let setup = setup(dashboard());
    let capabilities = capabilities();
    let preview = calculate(&setup, pages(&["1500"]), &capabilities).await;
    assert_eq!(preview.changes.len(), 2, "{:?}", preview.report);
    let session = LocalSession::new();
    let plan = verify(
        &setup,
        &request(&preview, &session),
        &preview,
        &capabilities,
    )
    .unwrap_or_else(|error| panic!("plan: {error}"));
    let shown = plan.preview();
    assert_eq!(shown.operation, "replace_host");
    assert_eq!(shown.replacement_count, 2);
    assert_eq!(shown.changed_pages, vec![0]);
    assert_eq!(shown.write_requests, 1);
    assert_eq!(shown.replacement_kinds, vec![("item_widget_itemid", 2)]);
    assert!(!shown.contracts.contains(&"update_several_pages"));
    assert_eq!(shown.source_fingerprint, preview.base_fingerprint);
    assert_eq!(shown.expected_fingerprint, preview.local_fingerprint);
    assert_eq!(shown.local_id.as_deref(), Some(LOCAL));
    assert_eq!(shown.revision, Some(preview.revision));
    assert_eq!(plan.session(), session.current());
}

#[tokio::test]
async fn several_pages_are_refused_without_their_contract() {
    let setup = setup(dashboard());
    let withdrawn = without_contracts(&["update_several_pages"]);
    let preview = calculate(&setup, ResolveScope::Dashboard, &withdrawn).await;
    let pages_changed: BTreeSet<usize> = preview
        .changes
        .iter()
        .map(|change| change.key.page_index)
        .collect();
    assert_eq!(pages_changed, BTreeSet::from([0, 1]));
    let session = LocalSession::new();
    let report = incompatible(verify(
        &setup,
        &request(&preview, &session),
        &preview,
        &withdrawn,
    ));
    assert_eq!(
        unverified_contracts(&report),
        BTreeSet::from(["update_several_pages".to_owned()])
    );

    // On 7.4.16 the contract is proven (LAB-02): the same changes form one
    // plan for both pages.
    let proven = capabilities();
    let preview = setup
        .service
        .replace_host_preview(LOCAL, read_write(&proven))
        .unwrap_or_else(|error| panic!("preview: {error}"));
    let plan = verify(&setup, &request(&preview, &session), &preview, &proven)
        .unwrap_or_else(|error| panic!("plan: {error}"));
    let shown = plan.preview();
    assert_eq!(shown.changed_pages, vec![0, 1]);
    assert_eq!(shown.replacement_count, 3);
    assert!(shown.contracts.contains(&"update_several_pages"));
}

#[tokio::test]
async fn svggraph_changes_are_refused_without_their_contracts() {
    let mut raw = dashboard();
    raw["pages"][0]["widgets"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("widgets"))
        .push(widget(
            "2503",
            "svggraph",
            36,
            vec![
                field("1", "ds.0.hosts.0", "db-old-01"),
                field("1", "ds.0.items.0", "PostgreSQL ping"),
                field("0", "ds.1.dataset_type", "0"),
                field("4", "ds.1.itemids.0", "102"),
                field("1", "ds.1.color.0", "477BDE"),
            ],
        ));
    let setup = setup(raw);
    let withdrawn = without_contracts(&["svggraph_dataset_itemids", "svggraph_dataset_hosts"]);
    let preview = calculate(&setup, pages(&["1500"]), &withdrawn).await;
    let session = LocalSession::new();
    let report = incompatible(verify(
        &setup,
        &request(&preview, &session),
        &preview,
        &withdrawn,
    ));
    assert_eq!(
        unverified_contracts(&report),
        BTreeSet::from([
            "svggraph_dataset_hosts".to_owned(),
            "svggraph_dataset_itemids".to_owned()
        ])
    );

    // On 7.4.16 both contracts are proven (LAB-02).
    let proven = capabilities();
    let preview = setup
        .service
        .replace_host_preview(LOCAL, read_write(&proven))
        .unwrap_or_else(|error| panic!("preview: {error}"));
    let plan = verify(&setup, &request(&preview, &session), &preview, &proven)
        .unwrap_or_else(|error| panic!("plan: {error}"));
    let kinds = plan.preview().replacement_kinds;
    assert!(kinds.contains(&("svggraph_dataset_hosts", 1)), "{kinds:?}");
    assert!(
        kinds.contains(&("svggraph_dataset_itemids", 1)),
        "{kinds:?}"
    );
}

#[tokio::test]
async fn a_changed_page_with_an_unproven_widget_type_is_refused() {
    let mut raw = dashboard();
    raw["pages"][0]["widgets"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("widgets"))
        .push(widget(
            "2504",
            "clock",
            36,
            vec![field("0", "clock_type", "1")],
        ));
    let setup = setup(raw);
    let capabilities = capabilities();
    let preview = calculate(&setup, pages(&["1500"]), &capabilities).await;
    assert!(preview.report.allows_publication(), "{:?}", preview.report);
    let session = LocalSession::new();
    let report = incompatible(verify(
        &setup,
        &request(&preview, &session),
        &preview,
        &capabilities,
    ));
    let finding = report
        .with_code(FindingCode::WriteContractUnverified)
        .find(|finding| finding.param("contract") == Some(CONTRACT_RESEND_WIDGET_TYPE))
        .unwrap_or_else(|| panic!("resend finding: {report:?}"));
    assert_eq!(finding.param("widget_type"), Some("clock"));
    assert_eq!(
        finding.subject.as_ref().map(|subject| subject.widget_index),
        Some(3)
    );
}

#[tokio::test]
async fn stale_draft_inputs_invalidate_the_plan() {
    let setup = setup(dashboard());
    let capabilities = capabilities();
    let preview = calculate(&setup, pages(&["1500"]), &capabilities).await;
    let session = LocalSession::new();
    let shown = request(&preview, &session);

    let mut other = shown.clone();
    other.revision += 1;
    assert_eq!(
        invalidated(verify(&setup, &other, &preview, &capabilities)),
        PlanInvalidReason::DraftChanged
    );
    let mut other = shown.clone();
    other.local_fingerprint = "0".repeat(64);
    assert_eq!(
        invalidated(verify(&setup, &other, &preview, &capabilities)),
        PlanInvalidReason::DraftChanged
    );
    let mut other = shown.clone();
    other.base_fingerprint = "1".repeat(64);
    assert_eq!(
        invalidated(verify(&setup, &other, &preview, &capabilities)),
        PlanInvalidReason::DraftChanged
    );
    let mut other = shown.clone();
    other.kept_digest = Some("2".repeat(64));
    assert_eq!(
        invalidated(verify(&setup, &other, &preview, &capabilities)),
        PlanInvalidReason::BindingsChanged
    );
    let mut other = shown.clone();
    other.connection_id = "conn-ffffffffffffffffffffffffffffffff".to_owned();
    assert_eq!(
        invalidated(verify(&setup, &other, &preview, &capabilities)),
        PlanInvalidReason::ConnectionChanged
    );

    // A recalculation commits a new revision: the request shown before it
    // no longer matches the fresh preview.
    let fresh = calculate(&setup, pages(&["1500"]), &capabilities).await;
    assert_ne!(fresh.revision, preview.revision);
    assert_eq!(
        invalidated(verify(&setup, &shown, &fresh, &capabilities)),
        PlanInvalidReason::DraftChanged
    );
    assert!(verify(&setup, &request(&fresh, &session), &fresh, &capabilities).is_ok());
}

#[tokio::test]
async fn remote_changed_since_base_gives_no_plan() {
    let setup = setup(dashboard());
    let capabilities = capabilities();
    let preview = calculate(&setup, pages(&["1500"]), &capabilities).await;
    let session = LocalSession::new();
    let shown = request(&preview, &session);
    let mut changed = setup.base.raw_api_document().clone();
    changed["pages"][2]["name"] = json!("Concurrent edit");
    let changed = VersionedDashboardSnapshot::from_adapter_raw(
        CONNECTION.to_owned(),
        capabilities.version.clone(),
        changed,
    )
    .unwrap_or_else(|error| panic!("snapshot: {error}"));
    let context = PublicationContext::detached_for_testing(capabilities.clone());
    for remote in [Some(&changed), None] {
        let result = verify_replace_host_plan(&PlanInputs {
            request: &shown,
            preview: &preview,
            local: &local(&setup),
            remote,
            editable: true,
            context: &context,
        });
        assert_eq!(invalidated(result), PlanInvalidReason::SourceChanged);
    }
}

#[tokio::test]
async fn read_only_mode_and_missing_permission_give_no_plan() {
    let setup = setup(dashboard());
    let capabilities = capabilities();
    calculate(&setup, pages(&["1500"]), &capabilities).await;
    let session = LocalSession::new();
    let read_only = setup
        .service
        .replace_host_preview(
            LOCAL,
            SessionContext::Connected {
                access_mode: AccessMode::ReadOnly,
                capabilities: &capabilities,
            },
        )
        .unwrap_or_else(|error| panic!("preview: {error}"));
    let report = incompatible(verify(
        &setup,
        &request(&read_only, &session),
        &read_only,
        &capabilities,
    ));
    assert!(
        report
            .with_code(FindingCode::WriteModeRequired)
            .next()
            .is_some()
    );

    let preview = setup
        .service
        .replace_host_preview(LOCAL, read_write(&capabilities))
        .unwrap_or_else(|error| panic!("preview: {error}"));
    let report = incompatible(verify_replace_host_plan(&PlanInputs {
        request: &request(&preview, &session),
        preview: &preview,
        local: &local(&setup),
        remote: Some(&setup.base),
        editable: false,
        context: &PublicationContext::detached_for_testing(capabilities.clone()),
    }));
    assert!(
        report
            .with_code(FindingCode::DashboardNotEditable)
            .next()
            .is_some()
    );
}

#[tokio::test]
async fn the_kept_list_must_be_confirmed_and_stays_out_of_the_update() {
    let mut raw = dashboard();
    raw["pages"][2]["widgets"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("widgets"))
        .push(widget(
            "2521",
            "problems",
            12,
            vec![field("3", "hostids.0", SOURCE)],
        ));
    let setup = setup(raw);
    let capabilities = capabilities();
    let preview = calculate(&setup, pages(&["1500", "1502"]), &capabilities).await;
    assert_eq!(preview.kept_on_source.entries.len(), 1);
    assert_eq!(
        preview.operation.scope,
        ReplaceScope::from_resolve(&pages(&["1500", "1502"]))
    );
    let session = LocalSession::new();
    let report = incompatible(verify(
        &setup,
        &request(&preview, &session),
        &preview,
        &capabilities,
    ));
    assert!(
        report
            .with_code(FindingCode::KeptOnSourceUnconfirmed)
            .next()
            .is_some()
    );

    let confirmed = setup
        .service
        .confirm_kept_on_source(
            LOCAL,
            &preview.kept_on_source.digest,
            read_write(&capabilities),
        )
        .unwrap_or_else(|error| panic!("confirm: {error}"));
    let shown = request(&confirmed, &session);
    assert_eq!(
        shown.kept_digest.as_deref(),
        Some(preview.kept_on_source.digest.as_str())
    );
    let plan = verify(&setup, &shown, &confirmed, &capabilities)
        .unwrap_or_else(|error| panic!("plan: {error}"));
    let shown_plan = plan.preview();
    // The kept widget's page is not sent with widgets.
    assert_eq!(shown_plan.changed_pages, vec![0]);
    assert_eq!(shown_plan.kept_digest, shown.kept_digest);

    // Hiding the kept list from the request is a bindings change.
    let mut hidden = shown;
    hidden.kept_digest = None;
    assert_eq!(
        invalidated(verify(&setup, &hidden, &confirmed, &capabilities)),
        PlanInvalidReason::BindingsChanged
    );
}
