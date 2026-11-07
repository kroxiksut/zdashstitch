// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{collections::BTreeSet, env, fs, path::PathBuf};

use zdashstitch_core::formats::{
    Diagnostic, MAX_FILE_BYTES, MetricOrigin, MetricSelector, ValueType,
};
use zdashstitch_template_import::{
    Comparison, ExportSyntax, ImportError, ImportedTemplate, MappingKind, Threshold,
    ThresholdValue, import_templates,
};

const YAML: &str = r#"
zabbix_export:
  version: '7.4'
  templates:
    - uuid: '12345678123456781234567812345678'
      template: pg-template
      name: PostgreSQL & fixtures
      macros:
        - macro: '{$INTERVAL}'
          value: 30s
        - macro: '{$LIMIT}'
          value: '42'
        - macro: '{$SECRET}'
          type: SECRET_TEXT
          value: '123456'
        - macro: '{$UNUSED}'
          value: '987654'
      valuemaps:
        - name: state
          mappings:
            - value: '1'
              newvalue: available
            - type: GREATER_OR_EQUAL
              value: '2'
              newvalue: degraded
      templates:
        - name: base-template
        - name: unavailable-parent
      items:
        - name: Transactions
          key: pgsql.transactions
          delay: '{$INTERVAL}'
          params: inert-script-sentinel
          valuemap:
            name: state
          triggers:
            - expression: 'last(/pg-template/pgsql.transactions)>{$LIMIT}'
            - expression: 'last(/pg-template/pgsql.transactions)>{$SECRET}'
            - expression: 'avg(/pg-template/pgsql.transactions,5m)>100'
      discovery_rules:
        - key: pgsql.discovery
          item_prototypes:
            - name: DB {#DBNAME}
              key: 'pgsql.db.size[{#DBNAME}]'
              value_type: FLOAT
              units: B
              delay: 15
              trigger_prototypes:
                - expression: 'last(/pg-template/pgsql.db.size[{#DBNAME}])<10'
      dashboards:
        - name: Skipped dashboard
    - template: base-template
      items:
        - name: Transactions
          key: pgsql.transactions
          value_type: 4
"#;

const JSON: &str = r#"{
    "zabbix_export": {
      "version": "7.4",
      "templates": [
        {
          "uuid": "12345678123456781234567812345678",
          "template": "pg-template",
          "name": "PostgreSQL & fixtures",
          "macros": [
            {"macro":"{$INTERVAL}","value":"30s"}, {"macro":"{$LIMIT}","value":"42"},
            {"macro":"{$SECRET}","type":1,"value":"123456"}, {"macro":"{$UNUSED}","value":"987654"}
          ],
          "valuemaps": [{"name":"state","mappings":[
            {"value":"1","newvalue":"available"},{"type":2,"value":"2","newvalue":"degraded"}
          ]}],
          "templates": [{"name":"base-template"}, {"name":"unavailable-parent"}],
          "items": [{"name":"Transactions", "key":"pgsql.transactions", "delay":"{$INTERVAL}", "params":"inert-script-sentinel",
            "valuemap":{"name":"state"},"triggers":[
              {"expression":"last(/pg-template/pgsql.transactions)>{$LIMIT}"},
              {"expression":"last(/pg-template/pgsql.transactions)>{$SECRET}"},
              {"expression":"avg(/pg-template/pgsql.transactions,5m)>100"}
            ]}],
          "discovery_rules": [{
            "key":"pgsql.discovery",
            "item_prototypes":[{"name":"DB {#DBNAME}","key":"pgsql.db.size[{#DBNAME}]","value_type":"FLOAT","units":"B",
              "delay":15,"trigger_prototypes":[{"expression":"last(/pg-template/pgsql.db.size[{#DBNAME}])<10"}]}]
          }],
          "dashboards":[{"name":"Skipped dashboard"}]
        },
        {"template":"base-template","items":[{"name":"Transactions","key":"pgsql.transactions","value_type":4}]}
      ]
    }
}"#;

const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<zabbix_export>
  <version>7.4</version>
  <templates>
    <template>
      <uuid>12345678123456781234567812345678</uuid>
      <template>pg-template</template>
      <name>PostgreSQL &amp; fixtures</name>
      <macros>
        <macro><macro>{$INTERVAL}</macro><value>30s</value></macro>
        <macro><macro>{$LIMIT}</macro><value>42</value></macro>
        <macro><macro>{$SECRET}</macro><type>SECRET_TEXT</type><value>123456</value></macro>
        <macro><macro>{$UNUSED}</macro><value>987654</value></macro>
      </macros>
      <valuemaps><valuemap><name>state</name><mappings>
        <mapping><value>1</value><newvalue>available</newvalue></mapping>
        <mapping><type>GREATER_OR_EQUAL</type><value>2</value><newvalue>degraded</newvalue></mapping>
      </mappings></valuemap></valuemaps>
      <templates><template><name>base-template</name></template><template><name>unavailable-parent</name></template></templates>
      <items><item><name>Transactions</name><key>pgsql.transactions</key><delay>{$INTERVAL}</delay><params><![CDATA[inert-script-sentinel]]></params>
        <valuemap><name>state</name></valuemap>
        <triggers>
          <trigger><expression>last(/pg-template/pgsql.transactions)&gt;{$LIMIT}</expression></trigger>
          <trigger><expression>last(/pg-template/pgsql.transactions)&gt;{$SECRET}</expression></trigger>
          <trigger><expression>avg(/pg-template/pgsql.transactions,5m)&gt;100</expression></trigger>
        </triggers>
      </item></items>
      <discovery_rules><discovery_rule><key>pgsql.discovery</key><item_prototypes>
        <item_prototype><name>DB {#DBNAME}</name><key>pgsql.db.size[{#DBNAME}]</key><value_type>FLOAT</value_type><units>B</units>
          <delay>15</delay><trigger_prototypes><trigger_prototype><expression>last(/pg-template/pgsql.db.size[{#DBNAME}])&lt;10</expression></trigger_prototype></trigger_prototypes>
        </item_prototype>
      </item_prototypes></discovery_rule></discovery_rules>
      <dashboards><dashboard><name>Skipped dashboard</name></dashboard></dashboards>
    </template>
    <template><template>base-template</template><items><item><name>Transactions</name><key>pgsql.transactions</key><value_type>4</value_type></item></items></template>
  </templates>
</zabbix_export>"#;

fn parse(input: &str, syntax: ExportSyntax) -> Vec<ImportedTemplate> {
    import_templates(input.as_bytes(), syntax).unwrap_or_else(|error| panic!("import: {error}"))
}

fn reject(input: &[u8], syntax: ExportSyntax, code: &str) -> ImportError {
    let error = import_templates(input, syntax)
        .err()
        .unwrap_or_else(|| panic!("{syntax:?} should be rejected with {code}"));
    assert_eq!(error.code(), code, "{syntax:?}");
    error
}

#[test]
fn formats_agree_and_each_template_gets_its_own_catalog() {
    let yaml = parse(YAML, ExportSyntax::Yaml);
    assert_eq!(yaml, parse(JSON, ExportSyntax::Json));
    assert_eq!(yaml, parse(XML, ExportSyntax::Xml));
    assert_eq!(yaml.len(), 2);
    assert_ne!(yaml[0].catalog.catalog_id, yaml[1].catalog.catalog_id);
    let (pg, base) = (&yaml[0], &yaml[1]);
    assert_eq!(
        pg.catalog.source.export_format_version.as_deref(),
        Some("7.4")
    );
    assert!(pg.catalog.source.source_server_version.is_none());
    assert_eq!(
        pg.catalog.source.template_name.as_deref(),
        Some("PostgreSQL & fixtures")
    );
    assert_eq!(
        pg.catalog.source.template_uuid.as_deref(),
        Some("12345678123456781234567812345678")
    );
    assert_eq!(pg.omitted_template_dashboards, 1);
    assert_eq!(pg.dependencies.len(), 2);
    assert!(pg.dependencies[0].included);
    assert!(!pg.dependencies[1].included);
    assert!(
        pg.catalog
            .diagnostics
            .contains(&Diagnostic::MissingParentTemplate)
    );
    assert!(
        pg.catalog
            .diagnostics
            .contains(&Diagnostic::UnresolvedMacros)
    );
    assert!(pg.catalog.diagnostics.contains(&Diagnostic::SourcePartial));
    assert_eq!(pg.catalog.metrics.len(), 2);
    assert_eq!(base.catalog.metrics.len(), 1);
    let ordinary = pg
        .catalog
        .metrics
        .iter()
        .find(|metric| metric.origin == MetricOrigin::TemplateItem);
    let ordinary = ordinary.unwrap_or_else(|| panic!("ordinary item"));
    assert_eq!(ordinary.value_type, ValueType::NumericUnsigned);
    assert_eq!(
        ordinary.selector,
        MetricSelector::Key("pgsql.transactions".into())
    );
    assert_eq!(base.catalog.metrics[0].value_type, ValueType::Text);
    let prototype = pg
        .catalog
        .metrics
        .iter()
        .find(|metric| metric.origin == MetricOrigin::LldPrototype);
    let prototype = prototype.unwrap_or_else(|| panic!("LLD prototype"));
    assert_eq!(prototype.value_type, ValueType::NumericFloat);
    assert_eq!(
        pg.discovery_rules[0].prototype_ids,
        std::slice::from_ref(&prototype.id)
    );
    assert_eq!(
        prototype.diagnostics.as_deref(),
        Some([Diagnostic::LldRequiresInstance].as_slice())
    );
    let portable = serde_json::to_string(&pg.catalog).unwrap_or_else(|error| panic!("{error}"));
    assert!(!portable.contains("inert-script-sentinel"));
    assert!(!portable.contains("Skipped dashboard"));
}

#[test]
fn preview_inputs_are_scoped_and_keep_only_safe_data() {
    let exports = parse(YAML, ExportSyntax::Yaml);
    let hints = &exports[0].preview_hints;
    assert_eq!(hints.macro_defaults.len(), 2);
    assert_eq!(hints.macro_defaults["{$INTERVAL}"], "30s");
    assert_eq!(hints.macro_defaults["{$LIMIT}"], "42");
    assert_eq!(
        hints.value_maps["state"][1].kind,
        MappingKind::GreaterOrEqual
    );
    let ordinary = hints
        .metrics
        .values()
        .find(|hint| hint.value_map.is_some())
        .unwrap_or_else(|| panic!("mapped metric"));
    assert_eq!(ordinary.interval.as_deref(), Some("{$INTERVAL}"));
    assert_eq!(ordinary.thresholds.len(), 1);
    assert!(ordinary.thresholds.contains(&Threshold {
        comparison: Comparison::Greater,
        value: ThresholdValue::Macro("{$LIMIT}".into()),
    }));
    let prototype = hints
        .metrics
        .values()
        .find(|hint| hint.interval.as_deref() == Some("15"))
        .unwrap_or_else(|| panic!("prototype"));
    assert!(prototype.thresholds.contains(&Threshold {
        comparison: Comparison::Less,
        value: ThresholdValue::Constant("10".into()),
    }));
    assert!(exports[1].preview_hints.metrics.is_empty());
    let local = serde_json::to_string(&exports[0]).unwrap_or_else(|error| panic!("{error}"));
    assert!(!local.contains("123456\""));
    assert!(!local.contains("987654"));
    assert!(!local.contains("{$SECRET}"));
    assert!(!local.contains("inert-script-sentinel"));
    let portable =
        serde_json::to_string(&exports[0].catalog).unwrap_or_else(|error| panic!("{error}"));
    assert!(!portable.contains("preview_hints"));
    assert!(!portable.contains("macro_defaults"));
}

#[test]
fn unsafe_and_invalid_documents_are_rejected_without_echoing_content() {
    use ExportSyntax::{Json, Xml, Yaml};
    let cases = [
        (
            Yaml,
            "zabbix_export: &anchor {}\nother: *anchor\n",
            "yaml_alias",
        ),
        (
            Yaml,
            "zabbix_export:\n  version: '7.4'\n  version: '7.2'\n",
            "yaml_duplicate_key",
        ),
        (Yaml, "zabbix_export: !custom {}\n", "yaml_invalid"),
        (Yaml, "1: not-a-string-key\n", "yaml_key"),
        (
            Json,
            r#"{"zabbix_export":{"version":"7.4","version":"7.2"}}"#,
            "json_duplicate_key",
        ),
        (
            Json,
            r#"{"zabbix_export":{"version":"7.2","templates":[]}}"#,
            "template_export_version",
        ),
        (
            Json,
            r#"{"zabbix_export":{"version":"7.4","templates":[]}}"#,
            "template_export_empty",
        ),
        (
            Json,
            r#"{"zabbix_export":{"version":"7.4","templates":[{"template":"a"},{"template":"a"}]}}"#,
            "template_export_duplicate_template",
        ),
        (
            Json,
            r#"{"zabbix_export":{"version":"7.4","templates":[{"template":"x","items":[{"key":"x","name":"x","value_type":"unknown-sensitive-sentinel"}]}]}}"#,
            "template_export_value_type",
        ),
        (Xml, "<!DOCTYPE zabbix_export><zabbix_export/>", "xml_dtd"),
        (
            Xml,
            r#"<!DOCTYPE zabbix_export [<!ENTITY file SYSTEM "file:///unknown-sensitive-sentinel">]><zabbix_export>&file;</zabbix_export>"#,
            "xml_dtd",
        ),
        (
            Xml,
            r#"<!DOCTYPE zabbix_export SYSTEM "http://127.0.0.1:9/not-to-be-fetched"><zabbix_export/>"#,
            "xml_dtd",
        ),
        (
            Xml,
            "<zabbix_export>&unknown-sensitive-sentinel;</zabbix_export>",
            "xml_entity",
        ),
        (
            Xml,
            r#"<?xml-stylesheet href="http://127.0.0.1:9/style"?><zabbix_export/>"#,
            "xml_unsupported_markup",
        ),
        (
            Xml,
            r#"<zabbix_export xmlns:xi="http://www.w3.org/2001/XInclude"><xi:include href="file:///unknown-sensitive-sentinel"/></zabbix_export>"#,
            "xml_unsupported_markup",
        ),
        (
            Xml,
            "<zabbix_export><version>7.4</version><version>7.2</version></zabbix_export>",
            "xml_duplicate_element",
        ),
        (
            Xml,
            "<zabbix_export><version></zabbix_export>",
            "xml_invalid",
        ),
    ];
    for (syntax, document, expected) in cases {
        let error = reject(document.as_bytes(), syntax, expected);
        assert!(!format!("{error:?} {error}").contains("unknown-sensitive-sentinel"));
    }
}

#[test]
fn size_depth_and_node_limits_cover_ignored_sections() {
    use ExportSyntax::{Json, Xml, Yaml};
    let oversized = vec![b' '; MAX_FILE_BYTES + 1];
    for syntax in [Yaml, Json, Xml] {
        reject(&oversized, syntax, "template_export_size");
        reject(&[0xff], syntax, "utf8");
    }
    let deep_data = format!("{}0{}", "[".repeat(45), "]".repeat(45));
    for (syntax, input, code) in [
        (Json, format!(r#"{{"ignored":{deep_data}}}"#), "json_depth"),
        (Yaml, format!("ignored: {deep_data}\n"), "yaml_depth"),
        (
            Xml,
            format!(
                "<zabbix_export>{}{}</zabbix_export>",
                "<node>".repeat(45),
                "</node>".repeat(45)
            ),
            "xml_depth",
        ),
    ] {
        reject(input.as_bytes(), syntax, code);
    }
    let wide = format!(r#"{{"ignored":[{}0]}}"#, "0,".repeat(100_000));
    reject(wide.as_bytes(), Json, "template_export_objects");
    let wide = format!("ignored: [{}0]", "0,".repeat(100_000));
    reject(wide.as_bytes(), Yaml, "template_export_objects");
    let wide = format!(
        "<zabbix_export>{}</zabbix_export>",
        "<node/>".repeat(100_000)
    );
    reject(wide.as_bytes(), Xml, "template_export_objects");
}

/// Actual P1-IN-05 exports are local-only by project policy.
#[test]
#[ignore = "set ZDASHSTITCH_TEMPLATE_REFERENCE_DIR to the P1-IN-05 combined YAML/JSON/XML exports"]
fn lab_reference_exports_agree() {
    let root = env::var_os("ZDASHSTITCH_TEMPLATE_REFERENCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("reference directory required"));
    let load = |ext: &str, syntax| {
        let bytes = fs::read(root.join(format!("combined.{ext}")))
            .unwrap_or_else(|_| panic!("missing {ext} reference"));
        let mut templates = import_templates(&bytes, syntax)
            .unwrap_or_else(|error| panic!("{ext} reference: {error:?}"));
        templates.sort_by(|left, right| left.template_key.cmp(&right.template_key));
        templates
    };
    let yaml = load("yaml", ExportSyntax::Yaml);
    for (ext, syntax) in [("json", ExportSyntax::Json), ("xml", ExportSyntax::Xml)] {
        let other = load(ext, syntax);
        assert!(
            yaml == other,
            "{ext} reference projects differently from YAML"
        );
    }
    assert_eq!(yaml.len(), 3);
    assert_eq!(
        yaml.iter()
            .map(|item| &item.catalog.catalog_id)
            .collect::<BTreeSet<_>>()
            .len(),
        3
    );
    let totals = yaml
        .iter()
        .map(|item| item.catalog.metrics.len())
        .collect::<BTreeSet<_>>();
    assert_eq!(totals, BTreeSet::from([68, 79, 110]));
    assert!(
        yaml.iter()
            .all(|item| item.catalog.source.template_uuid.is_some())
    );
}
