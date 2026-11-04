// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{collections::BTreeMap, fs, path::PathBuf};

use zdashstitch_core::formats::{
    DeploymentState, FormatError, LegacyRootSelection, LocalBindings, MetricCatalog,
    PROJECT_V1_MAX_PAGES, PROJECT_V2_MAX_PAGES, Page, PortableProject, ProjectKind,
    ProjectProvenance, ProjectRoot, SourceKind, TypedProject, ValueType, Widget,
    ZabbixWidgetFieldValue, from_json_str, from_yaml_str, migrate_legacy_project, to_json_string,
    to_yaml_string, validate_project, validate_typed_project,
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    fs::read_to_string(root().join(relative))
        .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
}

fn project_and_catalogs() -> (PortableProject, BTreeMap<String, MetricCatalog>) {
    let project: PortableProject = from_yaml_str(&read("examples/db-pilot.yaml"))
        .unwrap_or_else(|error| panic!("project fixture must parse: {error}"));
    let catalogs = project
        .catalogs
        .iter()
        .map(|reference| {
            let catalog: MetricCatalog =
                from_yaml_str(&read(&format!("examples/{}", reference.file)))
                    .unwrap_or_else(|error| panic!("catalog {} must parse: {error}", reference.id));
            (reference.id.clone(), catalog)
        })
        .collect();
    (project, catalogs)
}

fn bindings() -> LocalBindings {
    from_json_str(&read("examples/private-synthetic/bindings.example.json"))
        .unwrap_or_else(|error| panic!("bindings fixture must parse: {error}"))
}

fn deployment() -> DeploymentState {
    from_json_str(&read("examples/private-synthetic/deployment.example.json"))
        .unwrap_or_else(|error| panic!("deployment fixture must parse: {error}"))
}

fn assert_code(error: FormatError, expected: &str) {
    assert_eq!(error.code(), Some(expected), "unexpected error: {error}");
}

fn failure<T>(result: Result<T, FormatError>) -> FormatError {
    match result {
        Ok(_) => panic!("expected validation failure"),
        Err(error) => error,
    }
}

#[test]
fn positive_examples_validate_and_round_trip_without_loss() {
    let (project, catalogs) = project_and_catalogs();
    validate_project(&project, &catalogs)
        .unwrap_or_else(|error| panic!("project relationships must validate: {error}"));

    let bindings = bindings();
    bindings
        .validate_against(Some(&project), Some(&catalogs))
        .unwrap_or_else(|error| panic!("bindings relationships must validate: {error}"));
    let deployment = deployment();
    deployment
        .validate_against(Some(&project), Some(&bindings))
        .unwrap_or_else(|error| panic!("deployment relationships must validate: {error}"));

    let project_yaml =
        to_yaml_string(&project).unwrap_or_else(|error| panic!("project must serialize: {error}"));
    assert_eq!(
        project,
        from_yaml_str(&project_yaml)
            .unwrap_or_else(|error| panic!("serialized project must parse: {error}"))
    );
    for catalog in catalogs.values() {
        let yaml = to_yaml_string(catalog)
            .unwrap_or_else(|error| panic!("catalog must serialize: {error}"));
        assert_eq!(
            catalog,
            &from_yaml_str(&yaml)
                .unwrap_or_else(|error| panic!("serialized catalog must parse: {error}"))
        );
    }
    for (json, bindings_round_trip) in [
        (
            to_json_string(&bindings)
                .unwrap_or_else(|error| panic!("bindings must serialize: {error}")),
            true,
        ),
        (
            to_json_string(&deployment)
                .unwrap_or_else(|error| panic!("deployment must serialize: {error}")),
            false,
        ),
    ] {
        if bindings_round_trip {
            let decoded: LocalBindings = from_json_str(&json)
                .unwrap_or_else(|error| panic!("serialized bindings must parse: {error}"));
            assert_eq!(bindings, decoded);
        } else {
            let decoded: DeploymentState = from_json_str(&json)
                .unwrap_or_else(|error| panic!("serialized deployment must parse: {error}"));
            assert_eq!(deployment, decoded);
        }
    }
}

#[test]
fn legacy_version_is_read_only_and_written_as_explicit_origin() {
    let catalog: MetricCatalog = from_yaml_str(&read(
        "fixtures/formats/v1/positive/legacy-template-catalog.yaml",
    ))
    .unwrap_or_else(|error| panic!("legacy fixture must parse: {error}"));
    assert_eq!(catalog.source.kind, SourceKind::TemplateExport);
    assert_eq!(catalog.source.export_format_version.as_deref(), Some("6.0"));

    let output = to_yaml_string(&catalog)
        .unwrap_or_else(|error| panic!("normalized catalog must serialize: {error}"));
    // The version stays a quoted string, never the float 6.0.
    assert!(output.contains("export_format_version: \"6.0\""));
    assert!(!output.contains("zabbix_version"));
    assert_eq!(
        catalog,
        from_yaml_str(&output)
            .unwrap_or_else(|error| panic!("normalized catalog must parse: {error}"))
    );
}

#[test]
fn shared_negative_fixtures_are_rejected() {
    let duplicate = failure(from_yaml_str::<MetricCatalog>(&read(
        "fixtures/formats/v1/negative/duplicate-metric.yaml",
    )));
    assert_code(duplicate, "duplicate_metric");

    assert!(
        from_json_str::<LocalBindings>(&read("fixtures/formats/v1/negative/numeric-hostid.json"))
            .is_err()
    );
    assert!(
        from_json_str::<DeploymentState>(&read(
            "fixtures/formats/v1/negative/unverified-deployment.json"
        ))
        .is_err()
    );
}

#[test]
fn uniqueness_relationship_slot_and_type_rules_match_reference() {
    let (project, catalogs) = project_and_catalogs();

    let mut duplicate_dashboard = project.clone();
    duplicate_dashboard
        .dashboards
        .push(duplicate_dashboard.dashboards[0].clone());
    assert_code(
        failure(validate_project(&duplicate_dashboard, &catalogs)),
        "duplicate_dashboard",
    );

    let mut unknown_metric = project.clone();
    if let Widget::Graph { series, .. } = &mut unknown_metric.dashboards[0].pages[0].widgets[0] {
        series[0].metric.metric = "missing_metric".to_owned();
    }
    assert_code(
        failure(validate_project(&unknown_metric, &catalogs)),
        "unknown_metric",
    );

    let mut binary_graph_catalogs = catalogs.clone();
    let query = binary_graph_catalogs
        .get_mut("ch")
        .and_then(|catalog| {
            catalog
                .metrics
                .iter_mut()
                .find(|metric| metric.id == "queries")
        })
        .unwrap_or_else(|| panic!("queries metric missing from fixture"));
    query.value_type = ValueType::Binary;
    assert_code(
        failure(validate_project(&project, &binary_graph_catalogs)),
        "nonnumeric_graph",
    );

    let mut binary_value_catalogs = catalogs.clone();
    let availability = binary_value_catalogs
        .get_mut("keeper")
        .and_then(|catalog| {
            catalog
                .metrics
                .iter_mut()
                .find(|metric| metric.id == "availability")
        })
        .unwrap_or_else(|| panic!("availability metric missing from fixture"));
    availability.value_type = ValueType::Binary;
    assert_code(
        failure(validate_project(&project, &binary_value_catalogs)),
        "binary_item_value",
    );

    let mut duplicate_slots = bindings();
    let pg_slot = duplicate_slots.roles["pg_nodes"][0].slot_id.clone();
    duplicate_slots
        .roles
        .get_mut("ch_nodes")
        .unwrap_or_else(|| panic!("ch role missing"))[0]
        .slot_id = pg_slot;
    assert_code(
        failure(duplicate_slots.validate_against(Some(&project), Some(&catalogs))),
        "duplicate_slot",
    );

    let mut role_local_project = project.clone();
    role_local_project
        .roles
        .get_mut("ch_nodes")
        .unwrap_or_else(|| panic!("ch role missing"))
        .slots = Some(vec!["pg_a".to_owned()]);
    assert_code(
        failure(bindings().validate_against(Some(&role_local_project), Some(&catalogs))),
        "missing_role_slot",
    );
}

#[test]
fn deployment_relationships_and_string_ids_are_enforced() {
    let (project, catalogs) = project_and_catalogs();
    let bindings = bindings();
    bindings
        .validate_against(Some(&project), Some(&catalogs))
        .unwrap_or_else(|error| panic!("bindings fixture must validate: {error}"));

    let mut duplicate_widget = deployment();
    duplicate_widget.dashboards[0].pages[0].widgets[1].widgetid = "3001".to_owned();
    assert_code(
        failure(duplicate_widget.validate_against(Some(&project), Some(&bindings))),
        "duplicate_deployed_widget",
    );

    let mut unknown_slot = deployment();
    unknown_slot.dashboards[0].pages[0].widgets[1].slot_id = Some("missing_slot".to_owned());
    assert_code(
        failure(unknown_slot.validate_against(Some(&project), Some(&bindings))),
        "unknown_deployment_slot",
    );
}

#[test]
fn typed_dashboard_and_page_round_trip_with_kind_and_root() {
    for (relative, expected_kind) in [
        ("examples/dashboard-v2.yaml", ProjectKind::Dashboard),
        ("examples/page-v2.yaml", ProjectKind::Page),
    ] {
        let project: TypedProject = from_yaml_str(&read(relative))
            .unwrap_or_else(|error| panic!("typed project must parse: {error}"));
        assert_eq!(project.kind, expected_kind);
        let catalog: MetricCatalog = from_yaml_str(&read("examples/metrics/linux.yaml"))
            .unwrap_or_else(|error| panic!("linux catalog must parse: {error}"));
        let catalogs = BTreeMap::from([("linux".to_owned(), catalog)]);
        validate_typed_project(&project, &catalogs)
            .unwrap_or_else(|error| panic!("typed project must validate: {error}"));
        let yaml = to_yaml_string(&project)
            .unwrap_or_else(|error| panic!("typed project must serialize: {error}"));
        let reopened: TypedProject = from_yaml_str(&yaml)
            .unwrap_or_else(|error| panic!("typed project must reopen: {error}"));
        assert_eq!(project, reopened);
    }
}

#[test]
fn generic_and_unknown_zabbix_widgets_round_trip_without_server_ids() {
    for relative in [
        "examples/clock-page-v2.yaml",
        "examples/global-view-v2.yaml",
        "fixtures/formats/v2/positive/unknown-widget.yaml",
    ] {
        let project: TypedProject = from_yaml_str(&read(relative))
            .unwrap_or_else(|error| panic!("generic project must parse: {error}"));
        validate_typed_project(&project, &BTreeMap::new())
            .unwrap_or_else(|error| panic!("generic project must validate: {error}"));
        let yaml = to_yaml_string(&project)
            .unwrap_or_else(|error| panic!("generic project must serialize: {error}"));
        assert!(!yaml.contains("widgetid"));
        assert_object_fields_hold_no_server_ids(&project.root);
        let reopened: TypedProject = from_yaml_str(&yaml)
            .unwrap_or_else(|error| panic!("generic project must reopen: {error}"));
        assert_eq!(project, reopened);
    }
}

/// Fields of Zabbix object types 2..=13 must carry portable references or an unresolved
/// marker, never a raw integer or string that could be a server object ID.
fn assert_object_fields_hold_no_server_ids(project_root: &ProjectRoot) {
    let pages = match project_root {
        ProjectRoot::Dashboard(dashboard) => dashboard.pages.iter().collect::<Vec<_>>(),
        ProjectRoot::Page(page) => vec![page],
    };
    for widget in pages.iter().flat_map(|page| page.widgets.iter()) {
        let Widget::Zabbix { fields, .. } = widget else {
            continue;
        };
        for field in fields {
            if (2..=13).contains(&field.field_type) {
                assert!(
                    matches!(
                        field.value,
                        ZabbixWidgetFieldValue::Host { .. }
                            | ZabbixWidgetFieldValue::Metric { .. }
                            | ZabbixWidgetFieldValue::Unresolved { .. }
                    ),
                    "field {} of type {} holds a raw value",
                    field.name,
                    field.field_type
                );
            }
        }
    }
}

#[test]
fn legacy_migration_requires_an_explicit_root_and_keeps_source_separate() {
    let (legacy, catalogs) = project_and_catalogs();
    let page = migrate_legacy_project(
        &legacy,
        &LegacyRootSelection::Page {
            dashboard_id: "db-pilot".to_owned(),
            page_id: "clickhouse".to_owned(),
        },
    )
    .unwrap_or_else(|error| panic!("legacy page must migrate: {error}"));
    assert_eq!(page.kind, ProjectKind::Page);
    assert!(matches!(&page.root, ProjectRoot::Page(root) if root.id == "clickhouse"));
    assert!(matches!(
        &page.provenance,
        ProjectProvenance::MigratedV1 {
            source_dashboard_id,
            source_page_id: Some(source_page_id),
            ..
        } if source_dashboard_id == "db-pilot" && source_page_id == "clickhouse"
    ));
    validate_typed_project(&page, &catalogs)
        .unwrap_or_else(|error| panic!("migrated page must validate: {error}"));

    let missing = migrate_legacy_project(
        &legacy,
        &LegacyRootSelection::Dashboard {
            dashboard_id: "missing-dashboard".to_owned(),
        },
    )
    .err()
    .unwrap_or_else(|| panic!("missing root must fail"));
    assert_code(missing, "legacy_root_not_found");
}

fn empty_page(number: usize) -> Page {
    Page {
        id: format!("extra-page-{number:02}"),
        name: format!("Page {number}"),
        widgets: Vec::new(),
    }
}

#[test]
fn page_limit_is_fifty_for_v2_and_stays_thirty_two_for_v1() {
    let fifty: TypedProject = from_yaml_str(&read("fixtures/formats/v2/positive/fifty-pages.yaml"))
        .unwrap_or_else(|error| panic!("fifty-page project must parse: {error}"));
    validate_typed_project(&fifty, &BTreeMap::new())
        .unwrap_or_else(|error| panic!("fifty pages must validate: {error}"));
    let ProjectRoot::Dashboard(dashboard) = &fifty.root else {
        panic!("fifty-page fixture must have a dashboard root");
    };
    assert_eq!(dashboard.pages.len(), PROJECT_V2_MAX_PAGES);

    let mut too_many = fifty.clone();
    if let ProjectRoot::Dashboard(dashboard) = &mut too_many.root {
        dashboard.pages.push(empty_page(PROJECT_V2_MAX_PAGES + 1));
    }
    assert_code(
        failure(validate_typed_project(&too_many, &BTreeMap::new())),
        "page_count",
    );

    let (mut legacy, catalogs) = project_and_catalogs();
    let mut number = 0;
    while legacy.dashboards[0].pages.len() < PROJECT_V1_MAX_PAGES {
        number += 1;
        legacy.dashboards[0].pages.push(empty_page(number));
    }
    validate_project(&legacy, &catalogs)
        .unwrap_or_else(|error| panic!("v1 project with 32 pages must validate: {error}"));
    legacy.dashboards[0].pages.push(empty_page(number + 1));
    assert_code(failure(validate_project(&legacy, &catalogs)), "page_count");
}

#[test]
fn unknown_typed_kind_and_target_intent_are_not_ignored() {
    for relative in [
        "fixtures/formats/v2/negative/unknown-kind.yaml",
        "fixtures/formats/v2/negative/target-intent.yaml",
    ] {
        assert!(
            from_yaml_str::<TypedProject>(&read(relative)).is_err(),
            "{relative} must fail"
        );
    }
}

fn duplicate_role_project() -> String {
    let text = read("examples/db-pilot.yaml");
    assert!(text.contains("roles:\n  pg_nodes:\n"));
    text.replacen(
        "roles:\n  pg_nodes:\n",
        "roles:\n  pg_nodes:\n    cardinality: one\n  pg_nodes:\n",
        1,
    )
}

fn aliased_project() -> String {
    let text = read("examples/db-pilot.yaml");
    assert!(text.contains("roles:\n  pg_nodes:\n    cardinality: many\n"));
    text.replacen(
        "roles:\n  pg_nodes:\n    cardinality: many\n",
        "roles:\n  pg_nodes: &role\n    cardinality: many\n  extra_nodes: *role\n",
        1,
    )
}

#[test]
fn in_memory_yaml_uses_the_strict_loader() {
    assert_code(
        failure(from_yaml_str::<PortableProject>(&duplicate_role_project())),
        "yaml_duplicate_key",
    );
    assert_code(
        failure(from_yaml_str::<PortableProject>(&aliased_project())),
        "yaml_alias",
    );
    assert_code(
        failure(from_yaml_str::<TypedProject>(&read(
            "fixtures/formats/v2/negative/yaml-non-string-key.yaml",
        ))),
        "yaml_key",
    );
    assert_code(
        failure(from_yaml_str::<TypedProject>(&read(
            "fixtures/formats/v2/negative/yaml-depth.yaml",
        ))),
        "yaml_depth",
    );
}

#[test]
fn in_memory_json_rejects_duplicate_keys() {
    let text = read("examples/private-synthetic/bindings.example.json");
    let duplicate = text.replacen(
        "\"project_id\": \"db-pilot\",",
        "\"project_id\": \"other\",\n  \"project_id\": \"db-pilot\",",
        1,
    );
    assert_ne!(duplicate, text);
    assert_code(
        failure(from_json_str::<LocalBindings>(&duplicate)),
        "json_duplicate_key",
    );
}

/// serde-rs/serde#2294: unit variants of an internally tagged enum ignore
/// `deny_unknown_fields`. `ProjectProvenance` must still reject extra fields.
#[test]
fn tagged_unit_provenance_rejects_extra_fields() {
    let page = read("examples/page-v2.yaml");
    let project: TypedProject =
        from_yaml_str(&page).unwrap_or_else(|error| panic!("page example must parse: {error}"));
    for (provenance, kind) in [
        (ProjectProvenance::Local, "local"),
        (ProjectProvenance::ServerSnapshot, "server_snapshot"),
    ] {
        let mut typed = project.clone();
        typed.provenance = provenance.clone();
        let yaml =
            to_yaml_string(&typed).unwrap_or_else(|error| panic!("{kind} must serialize: {error}"));
        let marker = format!("provenance:\n  kind: {kind}\n");
        assert!(yaml.contains(&marker), "{kind} serialized form changed");
        let reopened: TypedProject =
            from_yaml_str(&yaml).unwrap_or_else(|error| panic!("{kind} must parse: {error}"));
        assert_eq!(reopened.provenance, provenance);
        let extra = yaml.replacen(&marker, &format!("{marker}  extra: true\n"), 1);
        assert_code(failure(from_yaml_str::<TypedProject>(&extra)), "yaml_data");

        let json = to_json_string(&typed)
            .unwrap_or_else(|error| panic!("{kind} must serialize as JSON: {error}"));
        let json_marker = format!("\"kind\": \"{kind}\"");
        assert!(json.contains(&json_marker), "{kind} JSON form changed");
        let reopened: TypedProject =
            from_json_str(&json).unwrap_or_else(|error| panic!("{kind} JSON must parse: {error}"));
        assert_eq!(reopened.provenance, provenance);
        let extra = json.replacen(&json_marker, &format!("{json_marker}, \"extra\": true"), 1);
        assert_code(failure(from_json_str::<TypedProject>(&extra)), "json_data");

        let direct = serde_json::from_str::<ProjectProvenance>(&format!(
            "{{\"kind\":\"{kind}\",\"extra\":true}}"
        ));
        assert!(
            direct.is_err(),
            "{kind} must reject extra fields in serde_json"
        );
    }
}
