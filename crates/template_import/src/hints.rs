// SPDX-License-Identifier: MIT OR Apache-2.0

//! Local-only data for synthetic preview (P1-04I). No expression evaluation.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zdashstitch_core::formats::{MetricCatalog, MetricSelector};

use super::{ImportError, list};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PreviewHints {
    pub metrics: BTreeMap<String, MetricPreviewHint>,
    pub macro_defaults: BTreeMap<String, String>,
    pub value_maps: BTreeMap<String, Vec<ValueMapping>>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MetricPreviewHint {
    pub interval: Option<String>,
    pub value_map: Option<String>,
    pub thresholds: BTreeSet<Threshold>,
}

/// Value maps are shared by name within a single template version.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ValueMapping {
    pub kind: MappingKind,
    pub value: String,
    pub label: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MappingKind {
    Equal,
    GreaterOrEqual,
    LessOrEqual,
    InRange,
    RegularExpression,
    Default,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Threshold {
    pub comparison: Comparison,
    pub value: ThresholdValue,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Comparison {
    Greater,
    Less,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThresholdValue {
    Constant(String),
    Macro(String),
}

pub(super) type ThresholdIndex<'a> = BTreeMap<&'a str, BTreeSet<Threshold>>;

// Accept data-only scalars consistently across YAML, JSON and XML.
fn scalar(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn number(value: &str) -> bool {
    value.len() <= 48 && value.parse::<f64>().is_ok_and(f64::is_finite)
}

fn macro_name(value: &str) -> bool {
    value
        .strip_prefix("{$")
        .and_then(|value| value.strip_suffix('}'))
        .is_some_and(|name| {
            !name.is_empty()
                && name.len() <= 128
                && name.bytes().all(|byte| {
                    byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"._".contains(&byte)
                })
        })
}

fn interval(value: &str) -> bool {
    let digits = value.trim_end_matches(['s', 'm', 'h', 'd', 'w']);
    value.len() - digits.len() <= 1 && digits.parse::<u32>().is_ok_and(|seconds| seconds > 0)
}

fn is_plain_macro(entry: &Value) -> bool {
    entry
        .get("type")
        .is_none_or(|kind| matches!(scalar(Some(kind)).as_deref(), Some("TEXT" | "0")))
}

fn simple_threshold(expression: &str) -> Option<(&str, Threshold)> {
    let (item, comparison) = expression.trim().strip_prefix("last(/")?.rsplit_once(')')?;
    let comparison = comparison.trim_start();
    let (comparison, operand) = if let Some(operand) = comparison.strip_prefix('>') {
        (Comparison::Greater, operand)
    } else {
        (Comparison::Less, comparison.strip_prefix('<')?)
    };
    let operand = operand.trim();
    let value = if number(operand) {
        ThresholdValue::Constant(operand.to_owned())
    } else if macro_name(operand) {
        ThresholdValue::Macro(operand.to_owned())
    } else {
        return None;
    };
    Some((item, Threshold { comparison, value }))
}

fn add_thresholds<'a>(index: &mut ThresholdIndex<'a>, entries: &'a [Value]) {
    for entry in entries {
        if let Some((item, threshold)) = entry
            .get("expression")
            .and_then(Value::as_str)
            .and_then(simple_threshold)
        {
            index.entry(item).or_default().insert(threshold);
        }
    }
}

pub(super) fn root_thresholds(root: &Value) -> Result<ThresholdIndex<'_>, ImportError> {
    let mut index = BTreeMap::new();
    add_thresholds(&mut index, list(root, "triggers")?);
    Ok(index)
}

fn mapping(entry: &Value) -> Option<ValueMapping> {
    let kind = match entry
        .get("type")
        .map(|value| scalar(Some(value)))
        .unwrap_or_else(|| Some("EQUAL".to_owned()))?
        .as_str()
    {
        "EQUAL" | "0" => MappingKind::Equal,
        "GREATER_OR_EQUAL" | "2" => MappingKind::GreaterOrEqual,
        "LESS_OR_EQUAL" | "3" => MappingKind::LessOrEqual,
        "IN_RANGE" | "4" => MappingKind::InRange,
        "REGEXP" | "5" => MappingKind::RegularExpression,
        "DEFAULT" | "6" => MappingKind::Default,
        _ => return None,
    };
    let value = scalar(entry.get("value")).unwrap_or_default();
    let label = scalar(entry.get("newvalue"))?;
    (value.len() <= 512 && label.len() <= 512).then_some(ValueMapping { kind, value, label })
}

pub(super) fn extract(
    raw: &Value,
    template: &str,
    catalog: &MetricCatalog,
    root: &ThresholdIndex<'_>,
) -> Result<PreviewHints, ImportError> {
    let mut local = BTreeMap::new();
    add_thresholds(&mut local, list(raw, "triggers")?);
    let mut items = BTreeMap::new();
    for item in list(raw, "items")? {
        add_thresholds(&mut local, list(item, "triggers")?);
        items.insert(
            super::identifier(
                "metric",
                super::optional_text(item, "uuid")?,
                &["", super::required_text(item, "key")?],
            ),
            item,
        );
    }
    for rule in list(raw, "discovery_rules")? {
        add_thresholds(&mut local, list(rule, "trigger_prototypes")?);
        for item in list(rule, "item_prototypes")? {
            add_thresholds(&mut local, list(item, "trigger_prototypes")?);
            items.insert(
                super::identifier(
                    "metric",
                    super::optional_text(item, "uuid")?,
                    &[
                        super::required_text(rule, "key")?,
                        super::required_text(item, "key")?,
                    ],
                ),
                item,
            );
        }
    }
    let mut hints = PreviewHints::default();
    let macros = list(raw, "macros")?;
    let non_plain: BTreeSet<_> = macros
        .iter()
        .filter(|entry| !is_plain_macro(entry))
        .filter_map(|entry| entry.get("macro").and_then(Value::as_str))
        .collect();
    let mut referenced_macros = BTreeSet::new();
    let mut referenced_maps = BTreeSet::new();
    let mut keys = BTreeSet::new();
    let mut ambiguous = BTreeSet::new();
    for metric in &catalog.metrics {
        if let MetricSelector::Key(key) = &metric.selector
            && !keys.insert(key)
        {
            ambiguous.insert(key);
        }
    }
    for metric in &catalog.metrics {
        let MetricSelector::Key(key) = &metric.selector else {
            continue;
        };
        let Some(item) = items.get(&metric.id) else {
            continue;
        };
        let interval = scalar(item.get("delay")).filter(|value| {
            interval(value) || (macro_name(value) && !non_plain.contains(value.as_str()))
        });
        let value_map = item
            .get("valuemap")
            .and_then(|value| value.get("name"))
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty() && name.len() <= 512)
            .map(str::to_owned);
        let mut thresholds = BTreeSet::new();
        if metric.value_type.is_numeric() && !ambiguous.contains(key) {
            let path = format!("{template}/{key}");
            for threshold in [root.get(path.as_str()), local.get(path.as_str())]
                .into_iter()
                .flatten()
                .flatten()
            {
                if let ThresholdValue::Macro(name) = &threshold.value {
                    if non_plain.contains(name.as_str()) {
                        continue;
                    }
                    referenced_macros.insert(name.clone());
                }
                thresholds.insert(threshold.clone());
            }
        }
        if let Some(name) = interval.as_ref().filter(|name| macro_name(name)) {
            referenced_macros.insert(name.clone());
        }
        if let Some(name) = &value_map {
            referenced_maps.insert(name.clone());
        }
        if interval.is_some() || value_map.is_some() || !thresholds.is_empty() {
            hints.metrics.insert(
                metric.id.clone(),
                MetricPreviewHint {
                    interval,
                    value_map,
                    thresholds,
                },
            );
        }
    }
    for entry in macros {
        let Some(name) = entry
            .get("macro")
            .and_then(Value::as_str)
            .filter(|name| referenced_macros.contains(*name))
        else {
            continue;
        };
        if let Some(value) =
            scalar(entry.get("value")).filter(|value| number(value) || interval(value))
        {
            hints.macro_defaults.insert(name.to_owned(), value);
        }
    }
    for entry in list(raw, "valuemaps")? {
        let Some(name) = entry
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| referenced_maps.contains(*name))
        else {
            continue;
        };
        let mappings: Vec<_> = list(entry, "mappings")?
            .iter()
            .filter_map(mapping)
            .collect();
        if !mappings.is_empty() {
            hints.value_maps.insert(name.to_owned(), mappings);
        }
    }
    Ok(hints)
}
