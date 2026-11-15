// SPDX-License-Identifier: MIT OR Apache-2.0

//! Store-level tests: immutability, verification on read, version gates,
//! crash simulation and link handling. Server reads are covered by
//! `tests/workcopy_import.rs` against a mock transport.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};
use zdashstitch_core::formats::{
    ProjectKind, ProjectRoot, build_typed_package_bytes, read_typed_package_bytes,
};
use zdashstitch_zabbix::{VersionedDashboardSnapshot, ZabbixVersion};

use super::formats::{
    BaseReference, DraftKind, ItemMetadataState, LOCAL_ONLY, LocalMetadata, PageSelector,
    SNAPSHOT_FORMAT, SnapshotFile, SnapshotScope, WORKCOPY_FORMAT, WorkcopyFile, WorkcopySource,
};
use super::store::{CrashPoint, LocalStore, fail};
use super::{WorkcopyService, is_workcopy_id};

const CONNECTION: &str = "conn-0123456789abcdef0123456789abcdef";
const WORKCOPY: &str = "wc-00112233445566778899aabbccddeeff";

fn raw_dashboard(name: &str) -> Value {
    json!({
        "dashboardid": "42",
        "name": name,
        "userid": "3",
        "private": "1",
        "display_period": "30",
        "auto_start": "1",
        "pages": [{
            "dashboard_pageid": "420",
            "name": "Main",
            "display_period": "0",
            "widgets": [{
                "widgetid": "4200", "type": "vendor.custom", "name": "", "x": "0", "y": "0",
                "width": "12", "height": "4", "view_mode": "0", "x_vendor": {"kept": true},
                "fields": [{"type": "1", "name": "vendor.knob", "value": "7"}]
            }]
        }]
    })
}

fn version() -> ZabbixVersion {
    "7.4.16"
        .parse()
        .unwrap_or_else(|error| panic!("version: {error}"))
}

fn snapshot_file(raw: Value) -> SnapshotFile {
    let snapshot =
        VersionedDashboardSnapshot::from_adapter_raw(CONNECTION.to_owned(), version(), raw)
            .unwrap_or_else(|error| panic!("snapshot: {error}"));
    let fingerprint = snapshot.storage_fingerprint().to_owned();
    SnapshotFile {
        format: SNAPSHOT_FORMAT.to_owned(),
        storage_scope: LOCAL_ONLY.to_owned(),
        connection_id: CONNECTION.to_owned(),
        adapter_id: snapshot.adapter_id().to_owned(),
        server_version: "7.4.16".to_owned(),
        dashboardid: "42".to_owned(),
        scope: SnapshotScope::Dashboard,
        first_read_at_ms: 1,
        last_read_at_ms: 2,
        first_read_fingerprint: fingerprint.clone(),
        last_read_fingerprint: fingerprint.clone(),
        fingerprint,
        raw_api_document: snapshot.raw_api_document().clone(),
    }
}

fn workcopy_file(snapshot: &SnapshotFile, id: &str) -> WorkcopyFile {
    WorkcopyFile {
        format: WORKCOPY_FORMAT.to_owned(),
        storage_scope: LOCAL_ONLY.to_owned(),
        workcopy_id: id.to_owned(),
        kind: DraftKind::Dashboard,
        name: "Synthetic".to_owned(),
        created_at_ms: 10,
        updated_at_ms: 10,
        revision: 0,
        source: WorkcopySource {
            connection_id: CONNECTION.to_owned(),
            dashboardid: "42".to_owned(),
            dashboard_name: "Synthetic".to_owned(),
            page: None,
            adapter_id: snapshot.adapter_id.clone(),
            server_version: snapshot.server_version.clone(),
        },
        base: BaseReference {
            fingerprint: snapshot.fingerprint.clone(),
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
    }
}

fn snapshot_path(root: &Path, file: &SnapshotFile) -> std::path::PathBuf {
    root.join("snapshots")
        .join(CONNECTION)
        .join("42")
        .join(format!("{}.json", file.fingerprint))
}

fn workcopy_path(root: &Path, id: &str) -> std::path::PathBuf {
    root.join("workcopies").join(id).join("workcopy.json")
}

fn temp_root() -> tempfile::TempDir {
    tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"))
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn write(path: &Path, raw: &[u8]) {
    std::fs::write(path, raw).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

#[test]
fn portable_inventory_reads_local_workcopy_without_mutating_base() {
    let directory = temp_root();
    let store = LocalStore::at(directory.path().to_path_buf());
    let file = snapshot_file(raw_dashboard("Private source"));
    store
        .put_snapshot(&file)
        .unwrap_or_else(|error| panic!("snapshot: {error}"));
    store
        .create_workcopy(&workcopy_file(&file, WORKCOPY))
        .unwrap_or_else(|error| panic!("workcopy: {error}"));
    let saved_snapshot = read(&snapshot_path(directory.path(), &file));
    let saved_workcopy = read(&workcopy_path(directory.path(), WORKCOPY));
    let inventory = WorkcopyService::new(directory.path().to_path_buf())
        .portable_conversion_inventory(WORKCOPY)
        .unwrap_or_else(|error| panic!("inventory: {error}"));
    assert_eq!(inventory.kind, DraftKind::Dashboard);
    assert_eq!(inventory.pages, 1);
    assert_eq!(inventory.widgets, 1);
    assert_eq!(inventory.unclassified_widgets, 1);
    assert_eq!(inventory.item_fields, 0);
    assert_eq!(
        read(&snapshot_path(directory.path(), &file)),
        saved_snapshot
    );
    assert_eq!(
        read(&workcopy_path(directory.path(), WORKCOPY)),
        saved_workcopy
    );

    let page_id = "wc-00112233445566778899aabbccddeefe";
    let mut page_workcopy = workcopy_file(&file, page_id);
    page_workcopy.kind = DraftKind::Page;
    page_workcopy.source.page = Some(PageSelector {
        dashboard_pageid: "420".to_owned(),
        page_name: "Main".to_owned(),
        page_number: 1,
    });
    store
        .create_workcopy(&page_workcopy)
        .unwrap_or_else(|error| panic!("page workcopy: {error}"));
    let page = WorkcopyService::new(directory.path().to_path_buf())
        .portable_conversion_inventory(page_id)
        .unwrap_or_else(|error| panic!("page inventory: {error}"));
    assert_eq!(page.kind, DraftKind::Page);
    assert_eq!(page.pages, 1);
    assert_eq!(page.widgets, 1);
    assert_eq!(
        read(&snapshot_path(directory.path(), &file)),
        saved_snapshot
    );
}

#[test]
fn stored_workcopies_convert_without_changing_source_files() {
    let directory = temp_root();
    let store = LocalStore::at(directory.path().to_path_buf());
    let file = snapshot_file(json!({
        "dashboardid": "42", "name": "Server dashboard", "pages": [
            {"dashboard_pageid": "420", "name": "Server page", "sortorder": "0",
             "widgets": [{"widgetid": "4200", "type": "clock", "name": "Clock",
                "x": "0", "y": "0", "width": "12", "height": "4",
                "fields": [{"name": "reference", "type": 1, "value": "ABCDE"}]},
               {"widgetid": "4201", "type": "item", "name": "Item",
                "x": "12", "y": "0", "width": "12", "height": "4",
                "fields": [{"name": "time_period._reference", "type": 1, "value": "ABCDE._timeperiod"}]}]}
        ]
    }));
    store
        .put_snapshot(&file)
        .unwrap_or_else(|error| panic!("snapshot: {error}"));
    let mut dashboard = workcopy_file(&file, WORKCOPY);
    dashboard.revision = 1;
    dashboard.operations = vec![json!({
        "seq": 1, "op": "clone_mapping_set", "op_version": 1,
        "recorded_at_ms": 11,
        "state": {
            "mapping_revision": 1, "connection_id": CONNECTION,
            "server_identity_fingerprint": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "server_version": "7.4.16", "base_fingerprint": file.fingerprint,
            "mappings": [{"role_id": "source-1",
                "source_host": {"hostid": "10", "host": "source", "name": "Source"},
                "decision": "keep_source", "references": []}]
        }
    })];
    store
        .create_workcopy(&dashboard)
        .unwrap_or_else(|error| panic!("dashboard: {error}"));
    let page_id = "wc-00112233445566778899aabbccddeefe";
    let mut page = workcopy_file(&file, page_id);
    page.kind = DraftKind::Page;
    page.name = "Local page".to_owned();
    page.source.page = Some(PageSelector {
        dashboard_pageid: "420".to_owned(),
        page_name: "Server page".to_owned(),
        page_number: 1,
    });
    store
        .create_workcopy(&page)
        .unwrap_or_else(|error| panic!("page: {error}"));
    let saved_base = read(&snapshot_path(directory.path(), &file));
    let saved_dashboard = read(&workcopy_path(directory.path(), WORKCOPY));
    let saved_page = read(&workcopy_path(directory.path(), page_id));
    let service = WorkcopyService::new(directory.path().to_path_buf());
    let preview = service
        .preview_portable_conversion(WORKCOPY)
        .unwrap_or_else(|error| panic!("preview: {error}"));
    assert!(preview.ready);
    assert_eq!(preview.blocker, None);
    for (id, kind, name) in [
        (WORKCOPY, ProjectKind::Dashboard, "Synthetic"),
        (page_id, ProjectKind::Page, "Local page"),
    ] {
        let bundle = if kind == ProjectKind::Dashboard {
            service.convert_dependency_free_dashboard(id)
        } else {
            service.convert_dependency_free_page(id)
        }
        .unwrap_or_else(|error| panic!("convert: {error}"));
        assert_eq!(bundle.project.kind, kind);
        let project_json = serde_json::to_string(&bundle.project)
            .unwrap_or_else(|error| panic!("serialize: {error}"));
        assert!(project_json.contains("ABCDE._timeperiod"));
        let root_name = match &bundle.project.root {
            ProjectRoot::Dashboard(root) => &root.name,
            ProjectRoot::Page(root) => &root.name,
        };
        assert_eq!(root_name, name);
        let package =
            build_typed_package_bytes(&bundle).unwrap_or_else(|error| panic!("package: {error}"));
        assert_eq!(
            read_typed_package_bytes(&package).unwrap_or_else(|error| panic!("reopen: {error}")),
            bundle
        );
        let saved = service
            .make_portable(id)
            .unwrap_or_else(|error| panic!("save: {error}"));
        let again = service
            .make_portable(id)
            .unwrap_or_else(|error| panic!("save again: {error}"));
        assert_ne!(saved.local_id, again.local_id);
        let reopened = if kind == ProjectKind::Dashboard {
            assert_eq!(saved.kind, DraftKind::Dashboard);
            crate::offline_dashboards::OfflineDashboardStore::new(directory.path().to_path_buf())
                .get(&saved.local_id)
                .unwrap_or_else(|error| panic!("open: {error}"))
                .unwrap_or_else(|| panic!("missing dashboard"))
                .bundle
        } else {
            assert_eq!(saved.kind, DraftKind::Page);
            crate::offline_pages::OfflinePageStore::new(directory.path().to_path_buf())
                .get(&saved.local_id)
                .unwrap_or_else(|error| panic!("open: {error}"))
                .unwrap_or_else(|| panic!("missing page"))
                .bundle
        };
        assert_eq!(reopened.project.kind, kind);
        assert_eq!(reopened.project.project_id, saved.local_id);
        assert_eq!(
            match &reopened.project.root {
                ProjectRoot::Dashboard(root) => &root.id,
                ProjectRoot::Page(root) => &root.id,
            },
            &saved.local_id
        );
        assert_eq!(
            match &reopened.project.root {
                ProjectRoot::Dashboard(root) => &root.name,
                ProjectRoot::Page(root) => &root.name,
            },
            name
        );
    }
    assert_eq!(read(&snapshot_path(directory.path(), &file)), saved_base);
    assert_eq!(
        read(&workcopy_path(directory.path(), WORKCOPY)),
        saved_dashboard
    );
    assert_eq!(read(&workcopy_path(directory.path(), page_id)), saved_page);
}

#[test]
fn stored_workcopy_conversion_refuses_unportable_data_without_writes() {
    let directory = temp_root();
    let store = LocalStore::at(directory.path().to_path_buf());
    let file = snapshot_file(raw_dashboard("Private source"));
    store
        .put_snapshot(&file)
        .unwrap_or_else(|error| panic!("snapshot: {error}"));
    store
        .create_workcopy(&workcopy_file(&file, WORKCOPY))
        .unwrap_or_else(|error| panic!("workcopy: {error}"));
    let before_base = read(&snapshot_path(directory.path(), &file));
    let before_workcopy = read(&workcopy_path(directory.path(), WORKCOPY));
    let service = WorkcopyService::new(directory.path().to_path_buf());
    let preview = service
        .preview_portable_conversion(WORKCOPY)
        .unwrap_or_else(|error| panic!("preview: {error}"));
    assert!(!preview.ready);
    assert_eq!(
        preview.blocker,
        Some("portable_conversion_unsupported_data")
    );
    assert_eq!(preview.inventory.unclassified_widgets, 1);
    assert_eq!(preview.inventory.unknown_fields, 0);
    assert!(
        preview
            .blockers
            .contains(&"portable_conversion_unsupported_data")
    );
    let serialized =
        serde_json::to_string(&preview).unwrap_or_else(|error| panic!("serialize: {error}"));
    for private in ["Private source", "4200", "x_vendor"] {
        assert!(!serialized.contains(private));
    }
    let error = service
        .convert_dependency_free_dashboard(WORKCOPY)
        .err()
        .map(|error| error.code());
    assert_eq!(error, Some("portable_conversion_unsupported_data"));
    assert_eq!(
        service
            .make_portable(WORKCOPY)
            .err()
            .map(|error| error.code()),
        Some("portable_conversion_unsupported_data")
    );
    assert!(!directory.path().join("drafts").exists());
    assert_eq!(read(&snapshot_path(directory.path(), &file)), before_base);
    assert_eq!(
        read(&workcopy_path(directory.path(), WORKCOPY)),
        before_workcopy
    );
}

struct CrashGuard;

impl CrashGuard {
    fn at(point: CrashPoint) -> Self {
        fail::AT.with(|cell| cell.set(Some(point)));
        Self
    }
}

impl Drop for CrashGuard {
    fn drop(&mut self) {
        fail::AT.with(|cell| cell.set(None));
    }
}

#[test]
fn snapshot_is_write_once_and_verified_on_read() {
    let directory = temp_root();
    let store = LocalStore::at(directory.path().to_path_buf());
    let file = snapshot_file(raw_dashboard("Synthetic"));
    assert!(
        !store
            .put_snapshot(&file)
            .unwrap_or_else(|error| panic!("{error}"))
    );
    assert!(
        store
            .put_snapshot(&file)
            .unwrap_or_else(|error| panic!("{error}"))
    );
    let (loaded, snapshot) = store
        .load_snapshot(CONNECTION, "42", &file.fingerprint)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(loaded, file);
    assert_eq!(snapshot.storage_fingerprint(), file.fingerprint);
    // Unknown widget fields and properties survive the disk round trip.
    assert_eq!(
        snapshot.raw_api_document()["pages"][0]["widgets"][0]["x_vendor"],
        json!({"kept": true})
    );
}

#[test]
fn corrupt_snapshot_is_rejected_and_never_overwritten() {
    let directory = temp_root();
    let store = LocalStore::at(directory.path().to_path_buf());
    let file = snapshot_file(raw_dashboard("Synthetic"));
    store
        .put_snapshot(&file)
        .unwrap_or_else(|error| panic!("{error}"));
    let path = snapshot_path(directory.path(), &file);
    let tampered = String::from_utf8(read(&path))
        .unwrap_or_else(|error| panic!("{error}"))
        .replace("\"Main\"", "\"Edited outside\"");
    write(&path, tampered.as_bytes());

    let error = store
        .load_snapshot(CONNECTION, "42", &file.fingerprint)
        .err()
        .map(|error| error.code());
    assert_eq!(error, Some("snapshot_corrupt"));
    let error = store.put_snapshot(&file).err().map(|error| error.code());
    assert_eq!(error, Some("snapshot_corrupt"));
    assert_eq!(read(&path), tampered.as_bytes(), "Base must stay untouched");

    write(&path, b"{not json");
    let error = store
        .load_snapshot(CONNECTION, "42", &file.fingerprint)
        .err()
        .map(|error| error.code());
    assert_eq!(error, Some("snapshot_corrupt"));
}

#[test]
fn unknown_internal_versions_are_rejected_without_changes() {
    let directory = temp_root();
    let store = LocalStore::at(directory.path().to_path_buf());
    let file = snapshot_file(raw_dashboard("Synthetic"));
    store
        .put_snapshot(&file)
        .unwrap_or_else(|error| panic!("{error}"));
    store
        .create_workcopy(&workcopy_file(&file, WORKCOPY))
        .unwrap_or_else(|error| panic!("{error}"));

    let snapshot = snapshot_path(directory.path(), &file);
    let future = String::from_utf8(read(&snapshot))
        .unwrap_or_else(|error| panic!("{error}"))
        .replace(SNAPSHOT_FORMAT, "zdashstitch.raw-snapshot/v9");
    write(&snapshot, future.as_bytes());
    let error = store
        .load_snapshot(CONNECTION, "42", &file.fingerprint)
        .err();
    assert_eq!(
        error.as_ref().map(|error| error.code()),
        Some("snapshot_unsupported_version")
    );
    assert_eq!(
        error.as_ref().and_then(|error| error.param("format")),
        Some("zdashstitch.raw-snapshot/v9")
    );
    assert_eq!(read(&snapshot), future.as_bytes());

    let workcopy = workcopy_path(directory.path(), WORKCOPY);
    let future = String::from_utf8(read(&workcopy))
        .unwrap_or_else(|error| panic!("{error}"))
        .replace(WORKCOPY_FORMAT, "zdashstitch.workcopy/v1");
    write(&workcopy, future.as_bytes());
    let error = store
        .load_workcopy(WORKCOPY)
        .err()
        .map(|error| error.code());
    assert_eq!(error, Some("workcopy_unsupported_version"));
    assert_eq!(read(&workcopy), future.as_bytes());

    write(&workcopy, br#"{"format":"something/else"}"#);
    let error = store
        .load_workcopy(WORKCOPY)
        .err()
        .map(|error| error.code());
    assert_eq!(error, Some("workcopy_corrupt"));
}

#[test]
fn operations_unknown_to_this_build_reject_the_workcopy_untouched() {
    let directory = temp_root();
    let store = LocalStore::at(directory.path().to_path_buf());
    let file = snapshot_file(raw_dashboard("Synthetic"));
    store
        .put_snapshot(&file)
        .unwrap_or_else(|error| panic!("{error}"));
    let mut workcopy = workcopy_file(&file, WORKCOPY);
    store
        .create_workcopy(&workcopy)
        .unwrap_or_else(|error| panic!("{error}"));
    workcopy.operations = vec![json!({"seq": 1, "op": "replace_host", "op_version": 2})];
    workcopy.revision = 1;
    store
        .replace_workcopy(&workcopy)
        .unwrap_or_else(|error| panic!("{error}"));
    let before = read(&workcopy_path(directory.path(), WORKCOPY));
    let error = store.load_workcopy(WORKCOPY).err();
    assert_eq!(
        error.as_ref().map(|error| error.code()),
        Some("workcopy_unsupported_operation")
    );
    assert_eq!(
        error.as_ref().and_then(|error| error.param("op")),
        Some("replacehost")
    );
    assert_eq!(read(&workcopy_path(directory.path(), WORKCOPY)), before);
    // Base is unaffected and still verifies.
    assert!(
        store
            .load_snapshot(CONNECTION, "42", &file.fingerprint)
            .is_ok()
    );
}

#[test]
fn crash_before_rename_leaves_previous_state_and_no_temporaries() {
    let directory = temp_root();
    let store = LocalStore::at(directory.path().to_path_buf());
    let file = snapshot_file(raw_dashboard("Synthetic"));
    {
        let _crash = CrashGuard::at(CrashPoint::FileRename);
        let error = store.put_snapshot(&file).err().map(|error| error.code());
        assert_eq!(error, Some("storage_write_failed"));
    }
    let snapshot_dir = directory
        .path()
        .join("snapshots")
        .join(CONNECTION)
        .join("42");
    let leftovers: Vec<_> = std::fs::read_dir(&snapshot_dir)
        .unwrap_or_else(|error| panic!("{error}"))
        .flatten()
        .collect();
    assert!(
        leftovers.is_empty(),
        "no committed or temporary file remains"
    );

    store
        .put_snapshot(&file)
        .unwrap_or_else(|error| panic!("{error}"));
    let workcopy = workcopy_file(&file, WORKCOPY);
    store
        .create_workcopy(&workcopy)
        .unwrap_or_else(|error| panic!("{error}"));
    let committed = read(&workcopy_path(directory.path(), WORKCOPY));
    {
        let _crash = CrashGuard::at(CrashPoint::FileRename);
        let mut renamed = workcopy.clone();
        renamed.name = "Renamed".to_owned();
        renamed.revision = 1;
        assert!(store.replace_workcopy(&renamed).is_err());
    }
    assert_eq!(read(&workcopy_path(directory.path(), WORKCOPY)), committed);
    assert_eq!(
        store
            .load_workcopy(WORKCOPY)
            .unwrap_or_else(|error| panic!("{error}"))
            .name,
        "Synthetic"
    );
}

#[test]
fn crash_before_workcopy_commit_creates_no_draft() {
    let directory = temp_root();
    let store = LocalStore::at(directory.path().to_path_buf());
    let file = snapshot_file(raw_dashboard("Synthetic"));
    store
        .put_snapshot(&file)
        .unwrap_or_else(|error| panic!("{error}"));
    {
        let _crash = CrashGuard::at(CrashPoint::WorkcopyCommit);
        assert!(
            store
                .create_workcopy(&workcopy_file(&file, WORKCOPY))
                .is_err()
        );
    }
    let entries: Vec<_> = std::fs::read_dir(directory.path().join("workcopies"))
        .unwrap_or_else(|error| panic!("{error}"))
        .flatten()
        .collect();
    assert!(entries.is_empty(), "staging directory is removed");
    let service = WorkcopyService::new(directory.path().to_path_buf());
    let list = service
        .list_drafts()
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(list.drafts.is_empty() && list.damaged.is_empty());
}

#[test]
fn leftovers_of_a_crashed_process_are_ignored_and_cleaned() {
    let directory = temp_root();
    let store = LocalStore::at(directory.path().to_path_buf());
    let file = snapshot_file(raw_dashboard("Synthetic"));
    store
        .put_snapshot(&file)
        .unwrap_or_else(|error| panic!("{error}"));
    let workcopies = directory.path().join("workcopies");
    std::fs::create_dir_all(workcopies.join(".zds-staging-0-wc-x"))
        .unwrap_or_else(|error| panic!("{error}"));
    write(
        &workcopies.join(".zds-staging-0-wc-x").join("workcopy.json"),
        b"{}",
    );
    let service = WorkcopyService::new(directory.path().to_path_buf());
    let list = service
        .list_drafts()
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(list.drafts.is_empty() && list.damaged.is_empty());

    store
        .create_workcopy(&workcopy_file(&file, WORKCOPY))
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(!workcopies.join(".zds-staging-0-wc-x").exists());
    let stale = workcopies.join(WORKCOPY).join(".zds-0-7.tmp");
    write(&stale, b"partial");
    let mut renamed = workcopy_file(&file, WORKCOPY);
    renamed.revision = 1;
    store
        .replace_workcopy(&renamed)
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(!stale.exists());
}

#[test]
fn damaged_workcopies_are_listed_with_a_code() {
    let directory = temp_root();
    let store = LocalStore::at(directory.path().to_path_buf());
    let file = snapshot_file(raw_dashboard("Synthetic"));
    store
        .put_snapshot(&file)
        .unwrap_or_else(|error| panic!("{error}"));
    store
        .create_workcopy(&workcopy_file(&file, WORKCOPY))
        .unwrap_or_else(|error| panic!("{error}"));
    let broken = "wc-ffffffffffffffffffffffffffffffff";
    store
        .create_workcopy(&workcopy_file(&file, broken))
        .unwrap_or_else(|error| panic!("{error}"));
    write(&workcopy_path(directory.path(), broken), b"\x00garbage");

    let service = WorkcopyService::new(directory.path().to_path_buf());
    let list = service
        .list_drafts()
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(list.drafts.len(), 1);
    assert_eq!(list.damaged.len(), 1);
    assert_eq!(list.damaged[0].local_id, broken);
    assert_eq!(list.damaged[0].code, "workcopy_corrupt");
    assert!(is_workcopy_id(&list.drafts[0].local_id));

    let detail = service
        .draft(WORKCOPY)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(detail.pages[0].widgets[0].widget_type, "vendor.custom");
    assert!(!detail.pages[0].widgets[0].editable);
    assert_eq!(detail.pages[0].display_period, None);
    assert_eq!(
        detail
            .properties
            .map(|properties| (properties.display_period, properties.auto_start)),
        Some((30, true))
    );
    assert_eq!(
        service
            .draft("wc-../../etc")
            .err()
            .map(|error| error.code()),
        Some("workcopy_id_invalid")
    );
}

#[test]
fn a_storage_root_reached_through_a_link_is_accepted() {
    // Owner decision of 2026-10-26 (revised S-1): the root path itself may go
    // through a symbolic link; only links below the root are refused.
    let outside = temp_root();
    let holder = temp_root();
    let link = holder.path().join("linked-root");
    #[cfg(unix)]
    let created = std::os::unix::fs::symlink(outside.path(), &link).is_ok();
    #[cfg(windows)]
    let created = std::os::windows::fs::symlink_dir(outside.path(), &link).is_ok();
    if !created {
        // Creating links needs a privilege on some Windows hosts.
        return;
    }
    let store = LocalStore::at(link.clone());
    let file = snapshot_file(raw_dashboard("Synthetic"));
    store
        .put_snapshot(&file)
        .unwrap_or_else(|error| panic!("{error}"));
    store
        .create_workcopy(&workcopy_file(&file, WORKCOPY))
        .unwrap_or_else(|error| panic!("{error}"));
    let service = WorkcopyService::new(link);
    assert_eq!(
        service
            .list_drafts()
            .unwrap_or_else(|error| panic!("{error}"))
            .drafts
            .len(),
        1
    );
    assert!(
        std::fs::read_dir(outside.path())
            .unwrap_or_else(|error| panic!("{error}"))
            .count()
            > 0
    );
}

#[test]
fn linked_directories_below_the_root_are_not_followed() {
    let directory = temp_root();
    let outside = temp_root();
    let link = directory.path().join("workcopies");
    #[cfg(unix)]
    let created = std::os::unix::fs::symlink(outside.path(), &link).is_ok();
    #[cfg(windows)]
    let created = std::os::windows::fs::symlink_dir(outside.path(), &link).is_ok();
    if !created {
        // Creating links needs a privilege on some Windows hosts.
        return;
    }
    let store = LocalStore::at(directory.path().to_path_buf());
    let file = snapshot_file(raw_dashboard("Synthetic"));
    let error = store
        .create_workcopy(&workcopy_file(&file, WORKCOPY))
        .err()
        .map(|error| error.code());
    assert_eq!(error, Some("storage_linked_path"));
    let service = WorkcopyService::new(directory.path().to_path_buf());
    assert_eq!(
        service.list_drafts().err().map(|error| error.code()),
        Some("storage_linked_path")
    );
    assert_eq!(
        std::fs::read_dir(outside.path())
            .unwrap_or_else(|error| panic!("{error}"))
            .count(),
        0
    );
}

#[cfg(unix)]
#[test]
fn files_and_directories_are_private_on_unix() {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = temp_root();
    let root = directory.path().join("store");
    let store = LocalStore::at(root.clone());
    let file = snapshot_file(raw_dashboard("Synthetic"));
    store
        .put_snapshot(&file)
        .unwrap_or_else(|error| panic!("{error}"));
    store
        .create_workcopy(&workcopy_file(&file, WORKCOPY))
        .unwrap_or_else(|error| panic!("{error}"));
    let mode = |path: &Path| {
        std::fs::metadata(path)
            .unwrap_or_else(|error| panic!("{error}"))
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode(&root), 0o700);
    assert_eq!(mode(&root.join("snapshots")), 0o700);
    assert_eq!(mode(&root.join("workcopies").join(WORKCOPY)), 0o700);
    assert_eq!(mode(&snapshot_path(&root, &file)), 0o600);
    assert_eq!(mode(&workcopy_path(&root, WORKCOPY)), 0o600);
}

/// OPEN_DECISIONS 28.10.2026 (evening), item 10: «изменение поддерживается»
/// is decided per field, so a simple classic graph (`source_type=1`, an
/// item that is only resent unchanged) is not marked editable while a
/// classic graph by `graphid` is.
#[test]
fn editable_mark_is_per_field_for_classic_graphs() {
    let registry = zdashstitch_zabbix::WidgetCapabilityRegistry::v7_4();
    let graph = |fields: Value| json!({"type": "graph", "fields": fields});
    let by_graph = graph(json!([
        {"type": "0", "name": "source_type", "value": "0"},
        {"type": "6", "name": "graphid.0", "value": "700"}
    ]));
    let simple = graph(json!([
        {"type": "0", "name": "source_type", "value": "1"},
        {"type": "4", "name": "itemid.0", "value": "701"}
    ]));
    assert!(super::view::widget_editable(&by_graph, &registry));
    assert!(!super::view::widget_editable(&simple, &registry));
    let clock = json!({"type": "clock", "fields": []});
    assert!(!super::view::widget_editable(&clock, &registry));
}

#[test]
fn snapshot_library_lists_verified_metadata_and_reports_corruption_without_raw_values() {
    let root = temp_root();
    let store = LocalStore::at(root.path().to_owned());
    let file = snapshot_file(raw_dashboard("private-dashboard-title"));
    store
        .put_snapshot(&file)
        .unwrap_or_else(|error| panic!("{error}"));
    store
        .create_workcopy(&workcopy_file(&file, WORKCOPY))
        .unwrap_or_else(|error| panic!("{error}"));
    let view = crate::snapshot_library::SnapshotLibrary::open(root.path().to_owned())
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(view.snapshots.len(), 1);
    assert_eq!(view.snapshots[0].used_by, vec![WORKCOPY]);
    assert!(!view.references_incomplete);
    let serialized = serde_json::to_string(&view).unwrap_or_else(|error| panic!("{error}"));
    assert!(!serialized.contains("private-dashboard-title"));
    assert!(!serialized.contains("dashboardid"));
    assert!(!serialized.contains("raw_api_document"));
    assert!(!serialized.contains("\"42\""));
    write(&workcopy_path(root.path(), WORKCOPY), b"{broken");
    let uncertain = crate::snapshot_library::SnapshotLibrary::open(root.path().to_owned())
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(uncertain.references_incomplete);
    assert_eq!(uncertain.snapshots.len(), 1);
    write(&snapshot_path(root.path(), &file), b"{broken");
    let damaged = crate::snapshot_library::SnapshotLibrary::open(root.path().to_owned())
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(damaged.snapshots.is_empty());
    assert_eq!(damaged.damaged.len(), 1);
    assert_eq!(damaged.damaged[0].code, "snapshot_corrupt");
}

#[test]
fn snapshot_library_compares_only_saved_bases_without_exposing_content() {
    let root = temp_root();
    let store = LocalStore::at(root.path().to_owned());
    let first = snapshot_file(raw_dashboard("secret-one"));
    let second = snapshot_file(raw_dashboard("secret-two"));
    store
        .put_snapshot(&first)
        .unwrap_or_else(|error| panic!("{error}"));
    store
        .put_snapshot(&second)
        .unwrap_or_else(|error| panic!("{error}"));
    let listing = crate::snapshot_library::SnapshotLibrary::open(root.path().to_owned())
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(listing.snapshots.len(), 2);
    let a = &listing.snapshots[0].library_id;
    let b = &listing.snapshots[1].library_id;
    let different = crate::snapshot_library::SnapshotLibrary::compare(root.path().to_owned(), a, b)
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(!different.identical);
    assert_eq!(
        (
            different.left_pages,
            different.right_pages,
            different.left_widgets,
            different.right_widgets
        ),
        (1, 1, 1, 1)
    );
    let raw = serde_json::to_string(&different).unwrap_or_else(|error| panic!("{error}"));
    assert!(!raw.contains("secret"));
    assert!(
        crate::snapshot_library::SnapshotLibrary::compare(root.path().to_owned(), a, a)
            .unwrap_or_else(|error| panic!("{error}"))
            .identical
    );
    assert_eq!(
        crate::snapshot_library::SnapshotLibrary::compare(
            root.path().to_owned(),
            a,
            "snap-invalid"
        )
        .err()
        .map(|error| error.code()),
        Some("snapshot_library_not_found")
    );
    assert_eq!(
        crate::snapshot_library::SnapshotLibrary::delete(root.path().to_owned(), a, false)
            .err()
            .map(|error| error.code()),
        Some("snapshot_delete_not_confirmed")
    );
    crate::snapshot_library::SnapshotLibrary::delete(root.path().to_owned(), a, true)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        crate::snapshot_library::SnapshotLibrary::open(root.path().to_owned())
            .unwrap_or_else(|error| panic!("{error}"))
            .snapshots
            .len(),
        1
    );
}

#[test]
fn snapshot_deletion_rechecks_workcopies_and_fails_closed_on_damage() {
    let root = temp_root();
    let path = root.path().to_owned();
    let store = LocalStore::at(path.clone());
    let file = snapshot_file(raw_dashboard("Synthetic"));
    store
        .put_snapshot(&file)
        .unwrap_or_else(|error| panic!("{error}"));
    let id = crate::snapshot_library::SnapshotLibrary::open(path.clone())
        .unwrap_or_else(|error| panic!("{error}"))
        .snapshots[0]
        .library_id
        .clone();
    assert!(
        crate::snapshot_library::SnapshotLibrary::deletion_preview(path.clone(), &id)
            .unwrap_or_else(|error| panic!("{error}"))
            .deletable
    );
    store
        .create_workcopy(&workcopy_file(&file, WORKCOPY))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        crate::snapshot_library::SnapshotLibrary::delete(path.clone(), &id, true)
            .err()
            .map(|error| error.code()),
        Some("snapshot_delete_blocked")
    );
    write(&workcopy_path(root.path(), WORKCOPY), b"{broken");
    let preview = crate::snapshot_library::SnapshotLibrary::deletion_preview(path.clone(), &id)
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(preview.references_unknown);
    assert!(!preview.deletable);
    assert!(snapshot_path(root.path(), &file).exists());
}

#[test]
fn snapshot_deletion_blocks_unfinished_finished_and_damaged_journals() {
    use crate::publish_journal::{JournalTarget, OperationKind, OperationRecord, PublishJournal};
    let root = temp_root();
    let path = root.path().to_owned();
    let store = LocalStore::at(path.clone());
    let file = snapshot_file(raw_dashboard("Synthetic"));
    store
        .put_snapshot(&file)
        .unwrap_or_else(|error| panic!("{error}"));
    let id = crate::snapshot_library::SnapshotLibrary::open(path.clone())
        .unwrap_or_else(|error| panic!("{error}"))
        .snapshots[0]
        .library_id
        .clone();
    let journal = PublishJournal::new(path.clone());
    let record = OperationRecord {
        kind: OperationKind::Update,
        intent: "replace_host",
        connection_id: CONNECTION.to_owned(),
        server_version: "7.4.16".to_owned(),
        endpoint_fingerprint: "a".repeat(64),
        server_identity: "b".repeat(64),
        target: JournalTarget::ExistingDashboard {
            dashboardid: "42".to_owned(),
            name: None,
        },
        source: None,
        source_fingerprint: "c".repeat(64),
        expected_fingerprint: "d".repeat(64),
        owner_userid: None,
        plan_created_at_ms: 1,
        confirmed_at_ms: 2,
    };
    let mut handle = journal
        .begin("zds-snapshot-delete-test", &record)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        crate::snapshot_library::SnapshotLibrary::deletion_preview(path.clone(), &id)
            .unwrap_or_else(|error| panic!("{error}"))
            .journal_count,
        1
    );
    assert!(
        !crate::snapshot_library::SnapshotLibrary::deletion_preview(path.clone(), &id)
            .unwrap_or_else(|error| panic!("{error}"))
            .deletable
    );
    assert!(handle.admit_send());
    handle
        .finish(crate::publish_journal::JournalState::Verified, None)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        crate::snapshot_library::SnapshotLibrary::delete(path.clone(), &id, true)
            .err()
            .map(|error| error.code()),
        Some("snapshot_delete_blocked")
    );
    std::fs::write(
        path.join("journal/zds-snapshot-delete-test.jsonl"),
        b"{broken\n",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        crate::snapshot_library::SnapshotLibrary::deletion_preview(path.clone(), &id)
            .unwrap_or_else(|error| panic!("{error}"))
            .references_unknown
    );
    assert!(snapshot_path(root.path(), &file).exists());
}

#[test]
fn concurrent_workcopy_creation_and_snapshot_deletion_never_leave_dangling_base() {
    let root = temp_root();
    let path = root.path().to_owned();
    let store = LocalStore::at(path.clone());
    let file = snapshot_file(raw_dashboard("Synthetic"));
    store
        .put_snapshot(&file)
        .unwrap_or_else(|error| panic!("{error}"));
    let id = crate::snapshot_library::SnapshotLibrary::open(path.clone())
        .unwrap_or_else(|error| panic!("{error}"))
        .snapshots[0]
        .library_id
        .clone();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let path_for_create = path.clone();
        let file_for_create = file.clone();
        let barrier_ref = &barrier;
        scope.spawn(move || {
            barrier_ref.wait();
            let _ = LocalStore::at(path_for_create)
                .create_workcopy(&workcopy_file(&file_for_create, WORKCOPY));
        });
        barrier.wait();
        let _ = crate::snapshot_library::SnapshotLibrary::delete(path.clone(), &id, true);
    });
    if store
        .workcopy_ids()
        .unwrap_or_else(|error| panic!("{error}"))
        .contains(&WORKCOPY.to_owned())
    {
        assert!(
            store
                .load_snapshot(CONNECTION, "42", &file.fingerprint)
                .is_ok()
        );
    }
}

#[test]
fn snapshot_compare_rejects_another_dashboard_and_clone_branches_block_delete() {
    let root = temp_root();
    let path = root.path().to_owned();
    let store = LocalStore::at(path.clone());
    let original = snapshot_file(raw_dashboard("Original"));
    store
        .put_snapshot(&original)
        .unwrap_or_else(|error| panic!("{error}"));
    let mut other_raw = raw_dashboard("Other");
    other_raw["dashboardid"] = json!("43");
    let mut other = snapshot_file(other_raw);
    other.dashboardid = "43".to_owned();
    store
        .put_snapshot(&other)
        .unwrap_or_else(|error| panic!("{error}"));
    let listing = crate::snapshot_library::SnapshotLibrary::open(path.clone())
        .unwrap_or_else(|error| panic!("{error}"));
    let left = &listing.snapshots[0].library_id;
    let right = &listing.snapshots[1].library_id;
    assert_eq!(
        crate::snapshot_library::SnapshotLibrary::compare(path.clone(), left, right)
            .err()
            .map(|error| error.code()),
        Some("snapshot_scope_mismatch")
    );
    std::fs::create_dir(path.join("clone-workcopies")).unwrap_or_else(|error| panic!("{error}"));
    std::fs::write(path.join("clone-workcopies/probe"), b"damaged")
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        crate::snapshot_library::SnapshotLibrary::deletion_preview(path.clone(), left)
            .unwrap_or_else(|error| panic!("{error}"))
            .references_unknown
    );
}
