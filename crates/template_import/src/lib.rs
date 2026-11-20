// SPDX-License-Identifier: MIT OR Apache-2.0

//! Offline template-export parsing boundary.
//!
//! Zabbix 7.4 and 8.0 exports are untrusted input. Parsing is in-memory and never
//! invokes Zabbix, reads an external resource, or evaluates export content.

mod hints;
mod profile;
mod xml;

pub use hints::{
    Comparison, MappingKind, MetricPreviewHint, PreviewHints, Threshold, ThresholdValue,
    ValueMapping,
};

use std::collections::BTreeSet;

use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use zdashstitch_core::ProductStage;
use zdashstitch_core::formats::strict::{ParseError, json_value_from_str, yaml_value_from_str};
use zdashstitch_core::formats::{
    CatalogSource, Diagnostic, ExportProfile, MAX_FILE_BYTES, Metric, MetricCatalog, MetricOrigin,
    MetricSelector, SourceKind, Validate,
};

use profile::{TemplateExportProfile, catalog_format};

const MAX_EXPORT_NODES: usize = 100_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportSyntax {
    Yaml,
    Json,
    Xml,
}

/// Local import metadata travels beside the portable catalog.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ImportedTemplate {
    pub catalog: MetricCatalog,
    pub template_key: String,
    pub dependencies: Vec<TemplateDependency>,
    pub discovery_rules: Vec<DiscoveryRule>,
    pub omitted_template_dashboards: usize,
    pub preview_hints: PreviewHints,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TemplateDependency {
    pub template_key: String,
    /// The dependency exists in this file. Its metrics stay in its own catalog.
    pub included: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct DiscoveryRule {
    pub key: String,
    pub uuid: Option<String>,
    pub prototype_ids: Vec<String>,
}

/// Codes and coordinates only. Parser messages and source values are omitted.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("{code}")]
pub struct ImportError {
    code: &'static str,
    line: Option<u64>,
    column: Option<u64>,
}

impl ImportError {
    const fn new(code: &'static str) -> Self {
        Self {
            code,
            line: None,
            column: None,
        }
    }

    #[must_use]
    pub const fn code(self) -> &'static str {
        self.code
    }

    #[must_use]
    pub const fn line(self) -> Option<u64> {
        self.line
    }

    #[must_use]
    pub const fn column(self) -> Option<u64> {
        self.column
    }
}

impl From<ParseError> for ImportError {
    fn from(error: ParseError) -> Self {
        Self {
            code: error.code(),
            line: error.line(),
            column: error.column(),
        }
    }
}

/// Parses a complete export before returning any catalog. Included templates
/// are returned in file order, with their own items and discovery prototypes.
pub fn import_templates(
    input: &[u8],
    syntax: ExportSyntax,
) -> Result<Vec<ImportedTemplate>, ImportError> {
    if input.len() > MAX_FILE_BYTES {
        return Err(ImportError::new("template_export_size"));
    }
    let text = std::str::from_utf8(input).map_err(|_| ImportError::new("utf8"))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let export = match syntax {
        ExportSyntax::Yaml => yaml_value_from_str(text)?,
        ExportSyntax::Json => json_value_from_str(text)?,
        ExportSyntax::Xml => xml::read(text)?,
    };
    check_nodes(&export)?;
    let root = export
        .get("zabbix_export")
        .filter(|root| root.is_object())
        .ok_or_else(shape)?;
    let version = required_text(root, "version")?;
    let profile = TemplateExportProfile::for_version(version)?;
    let templates = list(root, "templates")?;
    if templates.is_empty() {
        return Err(ImportError::new("template_export_empty"));
    }
    let mut keys = BTreeSet::new();
    for template in templates {
        if !keys.insert(required_text(template, "template")?) {
            return Err(ImportError::new("template_export_duplicate_template"));
        }
    }
    let mut catalog_ids = BTreeSet::new();
    let root_thresholds = hints::root_thresholds(root)?;
    templates
        .iter()
        .map(|template| {
            let imported = import_template(
                template,
                &keys,
                has_omitted(root, ROOT_OMITTED)
                    || has_unknown(root, ROOT_FIELDS)
                    || has_unknown(&export, &["zabbix_export"]),
                &root_thresholds,
                profile,
                version,
            )?;
            if !catalog_ids.insert(imported.catalog.catalog_id.clone()) {
                return Err(ImportError::new("template_export_duplicate_template"));
            }
            Ok(imported)
        })
        .collect()
}

fn check_nodes(root: &Value) -> Result<(), ImportError> {
    let mut pending = vec![root];
    let mut total = 0;
    while let Some(value) = pending.pop() {
        total += 1;
        if total + pending.len() > MAX_EXPORT_NODES {
            return Err(ImportError::new("template_export_objects"));
        }
        match value {
            Value::Array(children) => pending.extend(children),
            Value::Object(children) => pending.extend(children.values()),
            _ => {}
        }
    }
    Ok(())
}

const ROOT_OMITTED: &[&str] = &["graphs", "triggers", "hosts", "maps", "media_types"];
const TEMPLATE_OMITTED: &[&str] = &[
    "dashboards",
    "macros",
    "valuemaps",
    "httptests",
    "graphs",
    "triggers",
];
const RULE_OMITTED: &[&str] = &[
    "filter",
    "preprocessing",
    "overrides",
    "lld_macro_paths",
    "host_prototypes",
    "graph_prototypes",
    "trigger_prototypes",
    "params",
];
const METRIC_OMITTED: &[&str] = &[
    "params",
    "preprocessing",
    "url",
    "username",
    "password",
    "headers",
    "triggers",
    "master_item",
    "valuemap",
];

// Fields outside this importer’s understood subset must remain visible as
// source incompleteness. The source document itself is never persisted.
fn has_unknown(object: &Value, known: &[&str]) -> bool {
    object
        .as_object()
        .is_some_and(|entries| entries.keys().any(|key| !known.contains(&key.as_str())))
}

const ROOT_FIELDS: &[&str] = &[
    "version",
    "templates",
    "graphs",
    "triggers",
    "hosts",
    "maps",
    "media_types",
];
const TEMPLATE_FIELDS: &[&str] = &[
    "uuid",
    "template",
    "name",
    "templates",
    "items",
    "discovery_rules",
    "dashboards",
    "macros",
    "valuemaps",
    "httptests",
    "graphs",
    "triggers",
];
const RULE_FIELDS: &[&str] = &[
    "uuid",
    "key",
    "item_prototypes",
    "filter",
    "preprocessing",
    "overrides",
    "lld_macro_paths",
    "host_prototypes",
    "graph_prototypes",
    "trigger_prototypes",
    "params",
];
const METRIC_FIELDS: &[&str] = &[
    "uuid",
    "name",
    "key",
    "units",
    "value_type",
    "delay",
    "params",
    "preprocessing",
    "url",
    "username",
    "password",
    "headers",
    "triggers",
    "trigger_prototypes",
    "master_item",
    "valuemap",
];

fn has_omitted(object: &Value, fields: &[&str]) -> bool {
    fields.iter().any(|field| match object.get(field) {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(entries)) => !entries.is_empty(),
        Some(Value::String(text)) => !text.is_empty(),
        Some(_) => true,
    })
}

const fn shape() -> ImportError {
    ImportError::new("template_export_shape")
}

fn list<'a>(object: &'a Value, key: &str) -> Result<&'a [Value], ImportError> {
    match object.as_object().ok_or_else(shape)?.get(key) {
        Some(Value::Array(entries)) => Ok(entries),
        None => Ok(&[]),
        _ => Err(shape()),
    }
}

fn optional_text<'a>(object: &'a Value, key: &str) -> Result<Option<&'a str>, ImportError> {
    object
        .as_object()
        .ok_or_else(shape)?
        .get(key)
        .map(|value| value.as_str().ok_or_else(shape))
        .transpose()
}

fn required_text<'a>(object: &'a Value, key: &str) -> Result<&'a str, ImportError> {
    optional_text(object, key)?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(shape)
}

fn identifier(prefix: &str, uuid: Option<&str>, parts: &[&str]) -> String {
    if let Some(uuid) = uuid {
        return format!("{prefix}-{uuid}");
    }
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    let hash = format!("{:x}", digest.finalize());
    format!("{prefix}-{}", &hash[..32])
}

fn import_metric(
    raw: &Value,
    rule: Option<&str>,
    profile: TemplateExportProfile,
) -> Result<Metric, ImportError> {
    let name = required_text(raw, "name")?;
    let key = required_text(raw, "key")?;
    let uuid = optional_text(raw, "uuid")?;
    let units = optional_text(raw, "units")?.unwrap_or_default();
    let mut diagnostics = Vec::new();
    if [
        name,
        key,
        units,
        raw.get("delay").and_then(Value::as_str).unwrap_or_default(),
    ]
    .iter()
    .any(|value| value.contains("{$"))
    {
        diagnostics.push(Diagnostic::UnresolvedMacros);
    }
    let origin = if rule.is_some() {
        diagnostics.push(Diagnostic::LldRequiresInstance);
        MetricOrigin::LldPrototype
    } else {
        MetricOrigin::TemplateItem
    };
    if has_omitted(raw, METRIC_OMITTED) || has_unknown(raw, METRIC_FIELDS) {
        diagnostics.push(Diagnostic::SourcePartial);
    }
    Ok(Metric {
        id: identifier("metric", uuid, &[rule.unwrap_or_default(), key]),
        name: name.to_owned(),
        selector: MetricSelector::Key(key.to_owned()),
        value_type: profile.value_type(raw.get("value_type"))?,
        units: units.to_owned(),
        origin,
        uuid: uuid.map(str::to_owned),
        diagnostics: (!diagnostics.is_empty()).then_some(diagnostics),
    })
}

fn import_template(
    raw: &Value,
    included: &BTreeSet<&str>,
    root_partial: bool,
    root_thresholds: &hints::ThresholdIndex<'_>,
    profile: TemplateExportProfile,
    version: &str,
) -> Result<ImportedTemplate, ImportError> {
    let template_key = required_text(raw, "template")?;
    let uuid = optional_text(raw, "uuid")?;
    let name = optional_text(raw, "name")?
        .filter(|name| !name.is_empty())
        .unwrap_or(template_key);
    let dependency_keys = list(raw, "templates")?
        .iter()
        .map(|entry| required_text(entry, "name"))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let dependencies: Vec<_> = dependency_keys
        .into_iter()
        .map(|key| TemplateDependency {
            template_key: key.to_owned(),
            included: included.contains(key),
        })
        .collect();
    let mut metrics = list(raw, "items")?
        .iter()
        .map(|item| import_metric(item, None, profile))
        .collect::<Result<Vec<_>, _>>()?;
    let mut discovery_rules = Vec::new();
    let mut rule_keys = BTreeSet::new();
    let mut partial = root_partial
        || has_omitted(raw, TEMPLATE_OMITTED)
        || has_unknown(raw, TEMPLATE_FIELDS)
        || list(raw, "templates")?
            .iter()
            .any(|entry| has_unknown(entry, &["name"]));
    for rule in list(raw, "discovery_rules")? {
        let key = required_text(rule, "key")?;
        if !rule_keys.insert(key) {
            return Err(ImportError::new("template_export_duplicate_discovery_rule"));
        }
        let rule_uuid = optional_text(rule, "uuid")?;
        if rule_uuid.is_some_and(|uuid| {
            uuid.len() != 32 || !uuid.bytes().all(|byte| byte.is_ascii_hexdigit())
        }) {
            return Err(ImportError::new("template_export_discovery_uuid"));
        }
        let mut prototype_ids = Vec::new();
        for prototype in list(rule, "item_prototypes")? {
            let metric = import_metric(prototype, Some(key), profile)?;
            prototype_ids.push(metric.id.clone());
            metrics.push(metric);
        }
        prototype_ids.sort();
        discovery_rules.push(DiscoveryRule {
            key: key.to_owned(),
            uuid: rule_uuid.map(str::to_owned),
            prototype_ids,
        });
        partial |= has_omitted(rule, RULE_OMITTED) || has_unknown(rule, RULE_FIELDS);
    }
    metrics.sort_by(|left, right| left.id.cmp(&right.id));
    discovery_rules.sort_by(|left, right| left.key.cmp(&right.key));
    let mut diagnostics = Vec::new();
    if dependencies.iter().any(|dependency| !dependency.included) {
        diagnostics.push(Diagnostic::MissingParentTemplate);
    }
    for diagnostic in [
        Diagnostic::UnresolvedMacros,
        Diagnostic::LldRequiresInstance,
    ] {
        if metrics.iter().any(|metric| {
            metric
                .diagnostics
                .as_ref()
                .is_some_and(|codes| codes.contains(&diagnostic))
        }) {
            diagnostics.push(diagnostic);
        }
    }
    if partial
        || metrics.iter().any(|metric| {
            metric
                .diagnostics
                .as_ref()
                .is_some_and(|codes| codes.contains(&Diagnostic::SourcePartial))
        })
    {
        diagnostics.push(Diagnostic::SourcePartial);
    }
    let catalog = MetricCatalog {
        format: catalog_format(&metrics),
        catalog_id: identifier("template", uuid, &[template_key]),
        export_profile: ExportProfile::PortableWithPublicKeys,
        source: CatalogSource {
            kind: SourceKind::TemplateExport,
            export_format_version: Some(version.to_owned()),
            source_server_version: None,
            template_name: Some(name.to_owned()),
            template_uuid: uuid.map(str::to_owned),
        },
        diagnostics,
        metrics,
    };
    catalog
        .validate()
        .map_err(|error| ImportError::new(error.code().unwrap_or("template_export_catalog")))?;
    let preview_hints = hints::extract(raw, template_key, &catalog, root_thresholds)?;
    Ok(ImportedTemplate {
        catalog,
        template_key: template_key.to_owned(),
        dependencies,
        discovery_rules,
        omitted_template_dashboards: list(raw, "dashboards")?.len(),
        preview_hints,
    })
}

/// Current readiness of the importer boundary.
#[must_use]
pub const fn stage() -> ProductStage {
    ProductStage::Scaffold
}
