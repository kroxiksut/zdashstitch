// SPDX-License-Identifier: MIT OR Apache-2.0

use zdashstitch_core::formats::{
    Diagnostic, MetricOrigin, MetricSelector, MetricsFormat, ValueType,
};
use zdashstitch_template_import::{ExportSyntax, ImportedTemplate, import_templates};

const YAML: &str = include_str!("../../../fixtures/zabbix/8.0/template-export-synthetic.yaml");
const JSON: &str = include_str!("../../../fixtures/zabbix/8.0/template-export-synthetic.json");
const XML: &str = include_str!("../../../fixtures/zabbix/8.0/template-export-synthetic.xml");
const FIXTURES: [(ExportSyntax, &str); 3] = [
    (ExportSyntax::Yaml, YAML),
    (ExportSyntax::Json, JSON),
    (ExportSyntax::Xml, XML),
];

fn parse(input: &str, syntax: ExportSyntax) -> Vec<ImportedTemplate> {
    import_templates(input.as_bytes(), syntax).unwrap_or_else(|error| panic!("{syntax:?}: {error}"))
}

fn reject(input: &str, syntax: ExportSyntax, code: &str) {
    let error = import_templates(input.as_bytes(), syntax)
        .err()
        .unwrap_or_else(|| panic!("{syntax:?} should be rejected with {code}"));
    assert_eq!(error.code(), code, "{syntax:?}");
    assert!(!format!("{error:?} {error}").contains("private-placeholder"));
}

#[test]
fn documented_formats_agree_on_catalog_boundaries_types_and_provenance() {
    let expected = parse(YAML, ExportSyntax::Yaml);
    for (syntax, fixture) in FIXTURES {
        assert_eq!(parse(fixture, syntax), expected);
    }
    let [database, base] = expected.as_slice() else {
        panic!("one catalog per template");
    };
    for template in [database, base] {
        let source = &template.catalog.source;
        assert_eq!(source.export_format_version.as_deref(), Some("8.0"));
        assert!(source.source_server_version.is_none());
        assert!(source.template_uuid.is_some());
        assert!(
            template
                .catalog
                .diagnostics
                .contains(&Diagnostic::SourcePartial)
        );
    }
    assert_ne!(database.catalog.catalog_id, base.catalog.catalog_id);
    assert_eq!(database.catalog.format, MetricsFormat::V2);
    assert_eq!(base.catalog.format, MetricsFormat::V1);
    assert_eq!(database.catalog.metrics.len(), 3);
    assert_eq!(base.catalog.metrics.len(), 1);
    assert_eq!(base.catalog.metrics[0].value_type, ValueType::Text);
    assert!(database.catalog.metrics.iter().any(|metric| {
        metric.value_type == ValueType::Json
            && metric.origin == MetricOrigin::TemplateItem
            && metric.selector == base.catalog.metrics[0].selector
    }));
    assert!(database.catalog.metrics.iter().any(|metric| {
        metric.value_type == ValueType::NumericUnsigned
            && metric.selector == MetricSelector::Key("synthetic.pool.size".into())
    }));
    assert_eq!(database.dependencies.len(), 2);
    assert!(database.dependencies[0].included);
    assert!(!database.dependencies[1].included);
    assert!(
        database
            .catalog
            .diagnostics
            .contains(&Diagnostic::MissingParentTemplate)
    );

    let rule = &database.discovery_rules[0];
    assert_eq!(rule.key, "synthetic.pool.discovery");
    assert_eq!(
        rule.uuid.as_deref(),
        Some("f7c9f1d51b574c27b59f2a65c9ce4afb")
    );
    let prototype = database
        .catalog
        .metrics
        .iter()
        .find(|metric| metric.origin == MetricOrigin::LldPrototype)
        .unwrap_or_else(|| panic!("JSON LLD prototype"));
    assert_eq!(rule.prototype_ids, std::slice::from_ref(&prototype.id));
    assert_eq!(prototype.value_type, ValueType::Json);
    assert_eq!(
        prototype.selector,
        MetricSelector::Key("synthetic.pool.state[{#POOL}]".into())
    );
    assert_eq!(
        prototype.diagnostics.as_deref(),
        Some([Diagnostic::LldRequiresInstance].as_slice())
    );
}

#[test]
fn json_value_types_are_profile_scoped_and_unknown_types_reject_the_file() {
    let expected = parse(YAML, ExportSyntax::Yaml);
    let numeric_spellings = [
        (
            ExportSyntax::Yaml,
            YAML.replace("value_type: JSON", "value_type: 6"),
        ),
        (
            ExportSyntax::Json,
            JSON.replace(r#""value_type": "JSON""#, r#""value_type": 6"#),
        ),
        (
            ExportSyntax::Xml,
            XML.replace(">JSON</value_type>", ">6</value_type>"),
        ),
        (
            ExportSyntax::Yaml,
            YAML.replace("value_type: JSON", "value_type: '6'"),
        ),
        (
            ExportSyntax::Json,
            JSON.replace(r#""value_type": "JSON""#, r#""value_type": "6""#),
        ),
    ];
    for (syntax, fixture) in numeric_spellings {
        assert_eq!(parse(&fixture, syntax), expected);
        reject(
            &fixture.replace("8.0", "7.4"),
            syntax,
            "template_export_value_type",
        );
    }
    for (syntax, fixture) in FIXTURES {
        reject(
            &fixture.replace("8.0", "7.4"),
            syntax,
            "template_export_value_type",
        );
        reject(
            &fixture.replace("8.0", "8.0.0"),
            syntax,
            "template_export_version",
        );
        reject(
            &fixture.replace("8.0", "8.0rc1"),
            syntax,
            "template_export_version",
        );
        let unknown_in_later_template = fixture.replace("TEXT", "private-placeholder");
        reject(
            &unknown_in_later_template,
            syntax,
            "template_export_value_type",
        );
    }
    let invalid_prototype = JSON.replacen(
        r#""value_type": "JSON"
              }"#,
        r#""value_type": null
              }"#,
        1,
    );
    reject(
        &invalid_prototype,
        ExportSyntax::Json,
        "template_export_value_type",
    );
}

#[test]
fn untrusted_input_rules_apply_to_supported_8_0_exports() {
    let alias = YAML
        .replacen(
            "name: Templates/Synthetic",
            "name: &group Templates/Synthetic",
            1,
        )
        .replacen("- name: Templates/Synthetic", "- name: *group", 1);
    let duplicate = YAML.replace("version: '8.0'", "version: '8.0'\n  version: '7.4'");
    reject(&duplicate, ExportSyntax::Yaml, "yaml_duplicate_key");
    reject(&alias, ExportSyntax::Yaml, "yaml_alias");
    let duplicate = JSON.replace(
        r#""version": "8.0","#,
        r#""version": "8.0", "version": "7.4","#,
    );
    reject(&duplicate, ExportSyntax::Json, "json_duplicate_key");
    let duplicate = XML.replace(
        "<version>8.0</version>",
        "<version>8.0</version><version>7.4</version>",
    );
    reject(&duplicate, ExportSyntax::Xml, "xml_duplicate_element");
    let dtd = XML.replace(
        "<zabbix_export>",
        r#"<!DOCTYPE zabbix_export SYSTEM "file:///private-placeholder"><zabbix_export>"#,
    );
    reject(&dtd, ExportSyntax::Xml, "xml_dtd");
}
