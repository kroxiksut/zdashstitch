// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{collections::BTreeMap, fs, path::PathBuf};

use zdashstitch_core::formats::{
    DeploymentState, FormatError, LocalBindings, MetricCatalog, PortableProject, SourceKind,
    ValueType, Widget, from_json_str, from_yaml_str, to_json_string, to_yaml_string,
    validate_project,
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
    assert!(output.contains("export_format_version: '6.0'"));
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
