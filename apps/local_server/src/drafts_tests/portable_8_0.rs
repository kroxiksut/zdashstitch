// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use zdashstitch_application::offline_pages::OfflinePageStore;
use zdashstitch_core::formats::{CatalogReference, ProjectRoot, build_typed_package_bytes};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn portable_source_addresses_and_host_confirmation_use_saved_revision() {
    let app = App::start(Script {
        list: json!({}),
        ..default_script()
    })
    .await;
    let _connection = app.connect().await;
    let bundle = serde_json::from_value(json!({
        "project":{"format":"zdashstitch.project/v2","project_id":"sample","kind":"dashboard",
            "provenance":{"kind":"local"},"declared_compatibility":{"minimum":"7.4.0"},
            "catalogs":[{"id":"metrics","file":"metrics/metrics.yaml"}],
            "roles":{"hosts":{"cardinality":"many","slots":["first","second"]}},
            "root":{"id":"sample","kind":"global","name":"Sample","pages":[{"id":"page","name":"Page","widgets":[
                {"type":"graph","id":"graph","title":"Two rows","expansion":"series_per_host","series":[
                    {"metric":{"catalog":"metrics","metric":"rate"},"hosts":{"role":"hosts","selection":"all"}}
                ],"layout":{"mode":"fixed","x":0,"y":0,"w":18,"h":4}},
                {"type":"zabbix","id":"item","name":"Item","widget_type":"item","registry_state":"known",
                    "layout":{"mode":"fixed","x":18,"y":0,"w":18,"h":4},"fields":[
                        {"name":"override_hostid.0","field_type":3,"registry_state":"known",
                        "value":{"kind":"host","role":"hosts","selection":"slot","slot_id":"first"}}]}
            ]}]}},
        "catalogs":{"metrics":{"format":"zdashstitch.metrics/v1","catalog_id":"metrics",
            "export_profile":"portable_with_public_keys","source":{"kind":"manual"},"diagnostics":[],"metrics":[{
                "id":"rate","name":"Rate","selector":{"kind":"key","value":"synthetic.rate"},
                "value_type":"numeric_float","units":"","origin":"host_item"}]}}
    })).unwrap_or_else(|error| panic!("dashboard fixture: {error}"));
    let draft = zdashstitch_application::offline_dashboards::OfflineDashboardStore::new(
        app.storage.path().into(),
    )
    .save_imported(bundle)
    .unwrap_or_else(|error| panic!("saved fixture: {error}"));
    let url = format!("/api/drafts/{}", draft.local_id);
    let before = app.methods();
    let (status, listed) = app
        .call(
            "GET",
            &format!("{url}/portable-sources?revision={}", draft.revision),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed.as_array().map(Vec::len), Some(3));
    assert_eq!(listed[0]["slot"], "first");
    assert_eq!(listed[1]["slot"], "second");
    assert_eq!(listed[2]["field_name"], "override_hostid.0");
    assert!(listed[2]["metric_name"].is_null());
    assert_eq!(app.methods(), before);
    let (status, _) = app
        .call(
            "GET",
            &format!("{url}/portable-sources?revision={}", draft.revision + 1),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let row = |widget, field| json!({"revision":draft.revision,"page_index":0,"widget_index":widget,"field_index":field,"hostid":"40001"});
    for (widget, field, candidate) in [(0, 0, Some("30001")), (1, 0, None)] {
        let request = row(widget, field);
        let (status, preview) = app
            .call(
                "POST",
                &format!("{url}/portable-lld-review"),
                Some(request.clone()),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{preview}");
        if candidate.is_none() {
            assert_eq!(preview["host"]["hostid"], "40001");
            let (status, _) = app
                .call(
                    "POST",
                    &format!("{url}/portable-lld-confirm"),
                    Some(json!({
                        "review":request,"itemid":"30001","fingerprint":preview["fingerprint"],
                    })),
                )
                .await;
            assert_eq!(status, StatusCode::CONFLICT);
        } else {
            assert_eq!(preview["candidates"][0]["itemid"].as_str(), candidate);
        }
        let (status, saved) = app
            .call(
                "POST",
                &format!("{url}/portable-lld-confirm"),
                Some(json!({
                    "review":request,"itemid":candidate,"fingerprint":preview["fingerprint"],
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{saved}");
    }
    let reopen = json!({"revision":draft.revision,"page_index":0,"widget_index":1,"field_index":0});
    let (status, reopened) = app
        .call(
            "POST",
            &format!("{url}/portable-lld-reopen"),
            Some(reopen.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(reopened["itemid"].is_null());
    app.script.lock().unwrap_or_else(|_| panic!("script")).list["pattern_host_changed"] =
        json!(true);
    let (status, _) = app
        .call("POST", &format!("{url}/portable-lld-reopen"), Some(reopen))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        app.methods()
            .iter()
            .all(|name| name != "dashboard.create" && name != "dashboard.update")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn portable_8_0_composes_versioned_snapshot_local_and_file_pages() {
    let page = json!({
        "dashboard_pageid":"70331", "name":"Snapshot page", "sortorder":"0",
        "widgets":[
            {"widgetid":"71511","type":"item","name":"Provider","x":"0","y":"0","width":"12","height":"4",
                "fields":[{"name":"reference","type":1,"value":"ABCDE"}]},
            {"widgetid":"71512","type":"svggraph","name":"Graph","x":"12","y":"0","width":"12","height":"4",
                "fields":[
                    {"name":"ds.0.override_hostid._reference","type":1,"value":"ABCDE._hostid"},
                    {"name":"ds.0.invert_values","type":0,"value":"1"}
                ]}
        ]
    });
    let mut unknown = page.clone();
    unknown["dashboard_pageid"] = json!("70332");
    unknown["sortorder"] = json!("1");
    unknown["widgets"][1]["fields"][1]["name"] = json!("vendor.unknown");
    let source = json!({
        "dashboardid":"70110","name":"Synthetic dashboard","userid":"3","private":"1",
        "display_period":"30","auto_start":"1","users":[],"userGroups":[],
        "pages":[page, unknown]
    });
    let app = App::start(Script {
        snapshots: VecDeque::from([json!([source])]),
        list: json!({"clone_8_0":true}),
        methods: Vec::new(),
    })
    .await;
    let connection = app.connect().await;
    let (status, imported) = app
        .call(
            "POST",
            &format!("/api/server/{connection}/dashboards/70110/import"),
            Some(json!({"scope":"dashboard"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{imported}");
    let id = imported["local_id"].as_str().unwrap_or_default();
    let draft_path = format!("/api/drafts/{id}");
    let (_, before) = app.call("GET", &draft_path, None).await;
    let before_methods = app.methods();
    let library_path = format!("/api/library/pages/from-workcopy/{id}/pages");
    let (status, blocked) = app
        .call("POST", &format!("{library_path}/70332"), Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{blocked}");
    assert_eq!(blocked["code"], "portable_conversion_field_unsupported");
    let (status, saved) = app
        .call("POST", &format!("{library_path}/70331"), Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{saved}");
    let library_id = saved["library_id"].as_str().unwrap_or_default();

    let library =
        zdashstitch_application::page_library::PageLibrary::new(app.storage.path().to_owned());
    let library_page = library
        .get(library_id)
        .unwrap_or_else(|err| panic!("library: {err}"))
        .unwrap_or_else(|| panic!("library page missing"));
    let mut file_bundle = library_page.bundle.clone();
    let metrics = inspect_template_file(
        "file.json",
        br#"{"zabbix_export":{"version":"8.0","templates":[{"template":"File metrics","items":[{"name":"Document","key":"test.document","value_type":"JSON"}]}]}}"#,
    )
    .unwrap_or_else(|| panic!("extension"))
    .unwrap_or_else(|err| panic!("catalog: {err}"));
    let catalog = metrics[0].catalog.clone();
    let catalog_id = catalog.catalog_id.clone();
    file_bundle.project.catalogs.push(CatalogReference {
        id: catalog_id.clone(),
        file: format!("metrics/{catalog_id}.yaml"),
    });
    file_bundle.catalogs.insert(catalog_id.clone(), catalog);
    let ProjectRoot::Page(file_page) = &mut file_bundle.project.root else {
        panic!("page root")
    };
    file_page.name = "File page".into();
    let bytes =
        build_typed_package_bytes(&file_bundle).unwrap_or_else(|err| panic!("package: {err}"));
    let parsed =
        read_typed_package_bytes(&bytes).unwrap_or_else(|err| panic!("package import: {err}"));
    let page_store = OfflinePageStore::new(app.storage.path().to_owned());
    let file = page_store
        .save_imported(parsed)
        .unwrap_or_else(|err| panic!("file page: {err}"));
    let mut older = file_bundle.clone();
    older.project.declared_compatibility.minimum = zdashstitch_core::Version::new(7, 4, 0);
    let ProjectRoot::Page(older_page) = &mut older.project.root else {
        panic!("page root")
    };
    older_page.widgets.clear();
    let older = page_store
        .save_imported(older)
        .unwrap_or_else(|err| panic!("older page: {err}"));
    let mut opaque = file_bundle;
    let ProjectRoot::Page(opaque_page) = &mut opaque.project.root else {
        panic!("page root")
    };
    let zdashstitch_core::formats::Widget::Zabbix { fields, .. } = &mut opaque_page.widgets[1]
    else {
        panic!("generic widget")
    };
    fields[1].name = "vendor.not_in_registry".into();
    let opaque = page_store
        .save_imported(opaque)
        .unwrap_or_else(|err| panic!("opaque page: {err}"));
    let local_metrics = inspect_template_file(
        "local.json",
        br#"{"zabbix_export":{"version":"7.4","templates":[{"template":"Local metrics","items":[{"name":"Number","key":"test.number","value_type":"FLOAT"}]}]}}"#,
    )
    .unwrap_or_else(|| panic!("extension"))
    .unwrap_or_else(|err| panic!("catalog: {err}"));
    let local_catalog = local_metrics[0].catalog.catalog_id.clone();
    let local_metric = local_metrics[0].catalog.metrics[0].id.clone();
    CatalogLibrary::new(app.storage.path().to_owned())
        .save(local_metrics)
        .unwrap_or_else(|err| panic!("catalog save: {err}"));
    let (status, local) = app
        .call(
            "POST",
            "/api/library/offline-pages",
            Some(json!({
                "name":"Local page",
                "evaluation_profile":"8.0",
                "catalogs":[{"catalog_id":local_catalog,"version":1}],
                "roles":{"source":{"cardinality":"one","slots":["node-a"]}},
                "widgets":[{"kind":"item_value","title":"Item","metric":{"catalog_id":local_catalog,"metric_id":local_metric},
                    "hosts":{"role":"source","selection":"one"},"expansion":"single"}]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{local}");
    let local_id = local["local_id"].as_str().unwrap_or_default();
    let dashboard_path = "/api/library/offline-dashboards";
    for incompatible in [&older.local_id, &opaque.local_id] {
        let (status, rejected) = app
            .call(
                "POST",
                dashboard_path,
                Some(json!({"name":"Rejected","page_ids":[library_id,incompatible]})),
            )
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{rejected}");
        assert_eq!(rejected["code"], "offline_dashboard_sources_conflict");
    }
    let (status, assembled) = app
        .call(
            "POST",
            dashboard_path,
            Some(json!({"name":"Assembled","page_ids":[library_id, &file.local_id, local_id]})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{assembled}");
    let dashboard_id = assembled["local_id"].as_str().unwrap_or_default();
    let (_, reopened) = app
        .call("GET", &format!("{dashboard_path}/{dashboard_id}"), None)
        .await;
    assert_eq!(reopened, assembled);
    let store = zdashstitch_application::offline_dashboards::OfflineDashboardStore::new(
        app.storage.path().to_owned(),
    );
    let exported = store
        .export_package(dashboard_id)
        .unwrap_or_else(|err| panic!("export: {err}"))
        .unwrap_or_else(|| panic!("dashboard missing"));
    let exported =
        read_typed_package_bytes(&exported).unwrap_or_else(|err| panic!("reopen: {err}"));
    let ProjectRoot::Dashboard(root) = &exported.project.root else {
        panic!("dashboard root")
    };
    assert_eq!(
        root.pages
            .iter()
            .map(|page| page.name.as_str())
            .collect::<Vec<_>>(),
        ["Snapshot page", "File page", "Local page"]
    );
    assert_eq!(
        exported.catalogs[&catalog_id],
        file.bundle.catalogs[&catalog_id]
    );
    assert!(exported.catalogs.contains_key(&local_catalog));
    let doc = serde_json::to_value(&exported).unwrap_or_default();
    for (index, provider) in ["ABCDE", "R0000"].into_iter().enumerate() {
        let widgets = &doc["project"]["root"]["pages"][index]["widgets"];
        assert_eq!(widgets[0]["fields"][0]["value"]["value"], provider);
        assert_eq!(
            widgets[1]["fields"][0]["value"]["value"],
            format!("{provider}._hostid")
        );
        assert_eq!(widgets[1]["fields"][1]["value"]["value"], 1);
    }
    assert!(!doc.to_string().contains("71511"));
    assert_eq!(app.call("GET", &draft_path, None).await.1, before);
    assert_eq!(app.methods(), before_methods);
    assert_eq!(
        library
            .get(library_id)
            .unwrap_or_default()
            .map(|stored| stored.bundle),
        Some(library_page.bundle)
    );
}
