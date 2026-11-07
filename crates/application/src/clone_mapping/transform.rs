// SPDX-License-Identifier: MIT OR Apache-2.0

//! Deterministic typed Clone transformations (P3-02F).
//!
//! The transformer is pure: it changes a cloned dashboard snapshot, never
//! the source snapshot or a server. Mapping rows replace existing fields;
//! ready cardinality plans rebuild only the addressed `svggraph` series.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde_json::{Map, Value};
use zdashstitch_zabbix::{AdapterRegistry, VersionedDashboardSnapshot, WriteContract};

use super::{
    CardinalityPlan, ClonePatternAction, ClonePatternDecision, PlannedDataset, ReferenceMapping,
    SeriesBindingKind, SeriesOrigin, SourceHostDecision, SourceHostMapping, TargetMetricResolution,
    TargetSlotId,
};
use crate::resolver::{
    Applicability, Difference, FieldAddress, SourceValue, TargetValue, WidgetAddress, source_model,
};

/// Per-widget colors supplied by the later deterministic color policy
/// (P3-03B). A color is required only for an added explicit-item series.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloneCardinalityTransform {
    pub widget: WidgetAddress,
    pub plan: CardinalityPlan,
    pub added_item_colors: BTreeMap<(u32, TargetSlotId), String>,
}

/// Stable data-only transformation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CloneTransformError {
    code: &'static str,
    params: BTreeMap<&'static str, String>,
}

impl CloneTransformError {
    fn new(code: &'static str) -> Self {
        Self {
            code,
            params: BTreeMap::new(),
        }
    }

    fn with(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.params.insert(key, value.into());
        self
    }

    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    #[must_use]
    pub fn params(&self) -> &BTreeMap<&'static str, String> {
        &self.params
    }
}

impl fmt::Display for CloneTransformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code)
    }
}

impl std::error::Error for CloneTransformError {}

fn fail(code: &'static str, field: &FieldAddress) -> CloneTransformError {
    CloneTransformError::new(code)
        .with("page_index", field.widget.page_index.to_string())
        .with("widget_index", field.widget.widget_index.to_string())
        .with(
            "field",
            field.field_name.chars().take(96).collect::<String>(),
        )
}

fn source_raw(source: &SourceValue) -> &str {
    match source {
        SourceValue::Item { itemid } | SourceValue::ItemPrototype { itemid } => itemid,
        SourceValue::Graph { graphid } | SourceValue::GraphPrototype { graphid } => graphid,
        SourceValue::Host { hostid } => hostid,
        SourceValue::HostPattern { pattern } => pattern,
    }
}

fn target_raw(target: &TargetValue) -> &str {
    match target {
        TargetValue::Item { itemid } => itemid,
        TargetValue::Graph { graphid } => graphid,
        TargetValue::Host { hostid } => hostid,
        TargetValue::HostPattern { pattern } => pattern,
    }
}

fn compatible_values(source: &SourceValue, target: &TargetValue) -> bool {
    matches!(
        (source, target),
        (SourceValue::Item { .. }, TargetValue::Item { .. })
            | (SourceValue::ItemPrototype { .. }, TargetValue::Item { .. })
            | (SourceValue::Graph { .. }, TargetValue::Graph { .. })
            | (
                SourceValue::GraphPrototype { .. },
                TargetValue::Graph { .. }
            )
            | (SourceValue::Host { .. }, TargetValue::Host { .. })
            | (
                SourceValue::HostPattern { .. },
                TargetValue::HostPattern { .. }
            )
    )
}

fn usable_resolution(resolution: &TargetMetricResolution) -> bool {
    resolution.check == super::CloneBindingCheck::Checked
        && resolution.is_resolved_for_clone()
        && resolution.applicability == Applicability::Editable
        && !resolution.differences.contains(&Difference::ValueType)
        && !resolution.differences.contains(&Difference::Units)
}

fn same_visible_name(left: &str, right: &str) -> bool {
    left.to_lowercase() == right.to_lowercase()
}

fn validate_reference(
    mapping: &SourceHostMapping,
    reference: &ReferenceMapping,
    classic_graph_verified: bool,
) -> Result<(), CloneTransformError> {
    if (reference.widget_type == "graph"
        || matches!(
            reference.source,
            SourceValue::Graph { .. } | SourceValue::GraphPrototype { .. }
        ))
        && !classic_graph_verified
    {
        return Err(fail("clone_classic_graph_unsupported", &reference.field));
    }
    let SourceValue::HostPattern { pattern } = &reference.source else {
        return Ok(());
    };
    if pattern.contains('*')
        || mapping.source_host.name.is_empty()
        || !same_visible_name(pattern, &mapping.source_host.name)
    {
        return Err(fail("clone_pattern_decision_required", &reference.field));
    }
    let SourceHostDecision::TargetSlots(slots) = &mapping.decision else {
        return Ok(());
    };
    for (resolution, slot) in reference.targets.iter().zip(slots) {
        if resolution.target.as_ref().is_some_and(|target| {
            !matches!(
                target,
                TargetValue::HostPattern { pattern }
                    if !pattern.contains('*') && pattern == &slot.host.name
            )
        }) {
            return Err(fail("clone_target_host_pattern_mismatch", &reference.field));
        }
    }
    Ok(())
}

fn widget_mut<'a>(
    raw: &'a mut Value,
    address: &WidgetAddress,
) -> Result<&'a mut Value, CloneTransformError> {
    let error = || {
        CloneTransformError::new("clone_widget_not_found")
            .with("page_index", address.page_index.to_string())
            .with("widget_index", address.widget_index.to_string())
    };
    let widget = raw
        .get_mut("pages")
        .and_then(Value::as_array_mut)
        .and_then(|pages| pages.get_mut(address.page_index))
        .and_then(|page| page.get_mut("widgets"))
        .and_then(Value::as_array_mut)
        .and_then(|widgets| widgets.get_mut(address.widget_index))
        .ok_or_else(error)?;
    if let Some(widgetid) = &address.widgetid
        && widget.get("widgetid").and_then(Value::as_str) != Some(widgetid.as_str())
    {
        return Err(error());
    }
    Ok(widget)
}

fn replace_json_value(value: &mut Value, source: &str, target: &str) -> bool {
    match value {
        Value::String(text) if text == source => {
            *text = target.to_owned();
            true
        }
        Value::Number(number) if number.to_string() == source => {
            let Ok(target) = target.parse() else {
                return false;
            };
            *number = target;
            true
        }
        _ => false,
    }
}

fn replace_field(
    raw: &mut Value,
    address: &FieldAddress,
    source: &SourceValue,
    target: &TargetValue,
) -> Result<(), CloneTransformError> {
    if !compatible_values(source, target) {
        return Err(fail("clone_target_kind_mismatch", address));
    }
    let widget = widget_mut(raw, &address.widget)?;
    let fields = widget
        .get_mut("fields")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| fail("clone_fields_missing", address))?;
    let mut matches = fields
        .iter_mut()
        .filter(|field| field.get("name").and_then(Value::as_str) == Some(&address.field_name))
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(fail("clone_field_not_unique", address));
    }
    let value = matches[0]
        .get_mut("value")
        .ok_or_else(|| fail("clone_field_value_missing", address))?;
    if !replace_json_value(value, source_raw(source), target_raw(target)) {
        return Err(fail("clone_source_changed", address));
    }
    Ok(())
}

fn replace_pattern_field(
    raw: &mut Value,
    address: &FieldAddress,
    source: &str,
    target: &str,
) -> Result<(), CloneTransformError> {
    let widget = widget_mut(raw, &address.widget)?;
    let fields = widget
        .get_mut("fields")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| fail("clone_fields_missing", address))?;
    let mut matches = fields
        .iter_mut()
        .filter(|field| field.get("name").and_then(Value::as_str) == Some(&address.field_name))
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(fail("clone_field_not_unique", address));
    }
    let value = matches[0]
        .get_mut("value")
        .ok_or_else(|| fail("clone_field_value_missing", address))?;
    if !replace_json_value(value, source, target) {
        return Err(fail("clone_source_changed", address));
    }
    Ok(())
}

fn series_field_name(dataset: u32, binding: SeriesBindingKind, position: usize) -> String {
    let kind = match binding {
        SeriesBindingKind::HostPattern => "hosts",
        SeriesBindingKind::ExplicitItem => "itemids",
    };
    format!("ds.{dataset}.{kind}.{position}")
}

fn color_field_name(dataset: u32, position: usize) -> String {
    format!("ds.{dataset}.color.{position}")
}

fn parse_series_field(name: &str, dataset: u32, binding: SeriesBindingKind) -> Option<usize> {
    let parts = name.split('.').collect::<Vec<_>>();
    let kind = match binding {
        SeriesBindingKind::HostPattern => "hosts",
        SeriesBindingKind::ExplicitItem => "itemids",
    };
    match parts.as_slice() {
        ["ds", set, actual, position]
            if *actual == kind && set.parse::<u32>().ok() == Some(dataset) =>
        {
            position.parse().ok()
        }
        _ => None,
    }
}

fn field_object<'a>(
    fields: &'a [Value],
    address: &FieldAddress,
) -> Result<&'a Map<String, Value>, CloneTransformError> {
    let matches = fields
        .iter()
        .filter(|field| field.get("name").and_then(Value::as_str) == Some(&address.field_name))
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(fail("clone_field_not_unique", address));
    }
    matches[0]
        .as_object()
        .ok_or_else(|| fail("clone_field_invalid", address))
}

fn rewritten_field(
    source: &Map<String, Value>,
    name: String,
    expected: &str,
    target: &str,
    address: &FieldAddress,
) -> Result<Value, CloneTransformError> {
    let mut field = source.clone();
    field.insert("name".to_owned(), Value::String(name));
    let value = field
        .get_mut("value")
        .ok_or_else(|| fail("clone_field_value_missing", address))?;
    if !replace_json_value(value, expected, target) {
        return Err(fail("clone_source_changed", address));
    }
    Ok(Value::Object(field))
}

fn is_color(value: &str) -> bool {
    value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

const CLONE_SERIES_PALETTE: &[&str] = &[
    "3156A3", "8B5CF6", "1F6B43", "BD5B00", "B32962", "007B91", "734821", "617300", "4D52C3",
    "A13624", "0F8270", "7A4082", "1E6497", "9A6400", "43695A", "8F3954", "555A23", "AB4817",
];

/// Fills missing colors for new explicit-item series, in dataset and final
/// series order. A supplied profile palette takes precedence over the
/// fallback; existing and explicitly supplied colors are kept.
pub fn complete_clone_series_colors(
    base: &VersionedDashboardSnapshot,
    transforms: &mut [CloneCardinalityTransform],
    palette: &[&str],
) -> Result<(), CloneTransformError> {
    if palette.iter().any(|color| !is_color(color)) {
        return Err(CloneTransformError::new("clone_series_palette_invalid"));
    }
    let candidates = palette
        .iter()
        .chain(CLONE_SERIES_PALETTE)
        .map(|color| color.to_ascii_uppercase())
        .collect::<Vec<_>>();
    let mut used_by_widget = BTreeMap::<WidgetAddress, BTreeSet<String>>::new();
    for transform in &*transforms {
        let used = used_by_widget.entry(transform.widget.clone()).or_default();
        let widget = &base.raw_api_document()["pages"][transform.widget.page_index]["widgets"]
            [transform.widget.widget_index];
        let fields = widget["fields"]
            .as_array()
            .ok_or_else(|| CloneTransformError::new("clone_fields_missing"))?;
        for field in fields {
            let is_series_color = field["name"].as_str().is_some_and(|name| {
                let parts = name.split('.').collect::<Vec<_>>();
                matches!(parts.as_slice(), ["ds", _, "color"] | ["ds", _, "color", _])
            });
            if is_series_color && let Some(value) = field["value"].as_str() {
                used.insert(value.to_ascii_uppercase());
            }
        }
        used.extend(
            transform
                .added_item_colors
                .values()
                .map(|value| value.to_ascii_uppercase()),
        );
    }
    let mut pending = Vec::new();
    for (index, transform) in transforms.iter().enumerate() {
        for dataset in &transform.plan.datasets {
            if dataset.binding != SeriesBindingKind::ExplicitItem {
                continue;
            }
            for series in &dataset.series {
                let key = (dataset.dataset_index, series.slot_id.clone());
                if matches!(series.origin, SeriesOrigin::Added { .. })
                    && !transform.added_item_colors.contains_key(&key)
                {
                    pending.push((
                        transform.widget.clone(),
                        key.0,
                        series.position,
                        key.1,
                        index,
                    ));
                }
            }
        }
    }
    pending.sort_unstable();
    let mut generated = Vec::with_capacity(pending.len());
    for (widget, dataset, _, slot, index) in pending {
        let used = used_by_widget.entry(widget).or_default();
        let color = candidates
            .iter()
            .find(|color| !used.contains(*color))
            .ok_or_else(|| CloneTransformError::new("clone_series_palette_exhausted"))?;
        used.insert(color.clone());
        generated.push((index, (dataset, slot), color.clone()));
    }
    for (index, key, color) in generated {
        transforms[index].added_item_colors.insert(key, color);
    }
    Ok(())
}

fn source_address(origin: &SeriesOrigin) -> &FieldAddress {
    match origin {
        SeriesOrigin::Existing { source_field } => source_field,
        SeriesOrigin::Added { template_field } => template_field,
    }
}

fn validate_dataset_widget(
    transform: &CloneCardinalityTransform,
    dataset: &PlannedDataset,
) -> Result<(), CloneTransformError> {
    for address in dataset
        .series
        .iter()
        .map(|series| source_address(&series.origin))
        .chain(dataset.removed.iter().map(|series| &series.source_field))
    {
        if address.widget != transform.widget
            || parse_series_field(&address.field_name, dataset.dataset_index, dataset.binding)
                .is_none()
        {
            return Err(fail("clone_cardinality_address_mismatch", address));
        }
    }
    Ok(())
}

fn apply_dataset(
    raw: &mut Value,
    transform: &CloneCardinalityTransform,
    dataset: &PlannedDataset,
    mapped_targets: &BTreeMap<(FieldAddress, TargetSlotId), TargetValue>,
) -> Result<(), CloneTransformError> {
    validate_dataset_widget(transform, dataset)?;
    let widget = widget_mut(raw, &transform.widget)?;
    if widget.get("type").and_then(Value::as_str) != Some("svggraph") {
        return Err(CloneTransformError::new(
            "clone_cardinality_widget_unsupported",
        ));
    }
    let fields = widget
        .get_mut("fields")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| CloneTransformError::new("clone_fields_missing"))?;

    let mut rebuilt = Vec::new();
    let mut remove_names = BTreeSet::new();
    let mut insertion = fields.len();
    for address in dataset
        .series
        .iter()
        .map(|series| source_address(&series.origin))
        .chain(dataset.removed.iter().map(|series| &series.source_field))
    {
        remove_names.insert(address.field_name.clone());
        if dataset.binding == SeriesBindingKind::ExplicitItem
            && let Some(position) =
                parse_series_field(&address.field_name, dataset.dataset_index, dataset.binding)
        {
            let color = color_field_name(dataset.dataset_index, position);
            remove_names.insert(color);
        }
    }
    for (index, field) in fields.iter().enumerate() {
        if field
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| remove_names.contains(name))
        {
            insertion = insertion.min(index);
        }
    }

    for series in &dataset.series {
        let address = source_address(&series.origin);
        if mapped_targets.get(&(address.clone(), series.slot_id.clone())) != Some(&series.target) {
            return Err(fail("clone_cardinality_mapping_mismatch", address));
        }
        let source = field_object(fields, address)?;
        let target = match (&dataset.binding, &series.target) {
            (SeriesBindingKind::HostPattern, TargetValue::HostPattern { pattern }) => pattern,
            (SeriesBindingKind::ExplicitItem, TargetValue::Item { itemid }) => itemid,
            _ => return Err(fail("clone_target_kind_mismatch", address)),
        };
        let expected = source
            .get("value")
            .and_then(|value| match value {
                Value::String(value) => Some(value.clone()),
                Value::Number(value) => Some(value.to_string()),
                _ => None,
            })
            .ok_or_else(|| fail("clone_field_value_missing", address))?;
        rebuilt.push(rewritten_field(
            source,
            series_field_name(dataset.dataset_index, dataset.binding, series.position),
            &expected,
            target,
            address,
        )?);

        if dataset.binding == SeriesBindingKind::ExplicitItem {
            let old_position =
                parse_series_field(&address.field_name, dataset.dataset_index, dataset.binding)
                    .ok_or_else(|| fail("clone_cardinality_address_mismatch", address))?;
            let old_color_name = color_field_name(dataset.dataset_index, old_position);
            let old_color = fields
                .iter()
                .find(|field| field.get("name").and_then(Value::as_str) == Some(&old_color_name));
            let new_color = match &series.origin {
                SeriesOrigin::Existing { .. } => old_color
                    .and_then(|field| field.get("value"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                SeriesOrigin::Added { .. } => {
                    if old_color.and_then(Value::as_object).is_none() {
                        return Err(fail("clone_template_color_missing", address));
                    }
                    let color = transform
                        .added_item_colors
                        .get(&(dataset.dataset_index, series.slot_id.clone()))
                        .ok_or_else(|| {
                            CloneTransformError::new("clone_added_item_color_required")
                                .with("dataset_index", dataset.dataset_index.to_string())
                                .with("slot_id", series.slot_id.0.clone())
                        })?;
                    if !is_color(color) {
                        return Err(CloneTransformError::new("clone_added_item_color_invalid")
                            .with("dataset_index", dataset.dataset_index.to_string())
                            .with("slot_id", series.slot_id.0.clone()));
                    }
                    Some(color.to_ascii_uppercase())
                }
            };
            if let Some(color) = new_color {
                let mut color_field = old_color
                    .and_then(Value::as_object)
                    .cloned()
                    .ok_or_else(|| fail("clone_template_color_missing", address))?;
                color_field.insert(
                    "name".to_owned(),
                    Value::String(color_field_name(dataset.dataset_index, series.position)),
                );
                color_field.insert("value".to_owned(), Value::String(color));
                rebuilt.push(Value::Object(color_field));
            }
        }
    }

    let mut kept = Vec::with_capacity(fields.len() + rebuilt.len());
    let mut inserted = false;
    for (index, field) in std::mem::take(fields).into_iter().enumerate() {
        if index == insertion {
            kept.append(&mut rebuilt);
            inserted = true;
        }
        if !field
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| remove_names.contains(name))
        {
            kept.push(field);
        }
    }
    if !inserted {
        kept.append(&mut rebuilt);
    }
    *fields = kept;
    Ok(())
}

fn cardinality_fields(
    cardinalities: &[CloneCardinalityTransform],
) -> Result<BTreeSet<FieldAddress>, CloneTransformError> {
    let mut fields = BTreeSet::new();
    let mut datasets = BTreeSet::new();
    for transform in cardinalities {
        if !transform.plan.is_ready() {
            return Err(CloneTransformError::new("clone_cardinality_blocked"));
        }
        for dataset in &transform.plan.datasets {
            if !datasets.insert((transform.widget.clone(), dataset.dataset_index)) {
                return Err(CloneTransformError::new(
                    "clone_cardinality_duplicate_dataset",
                ));
            }
            for address in dataset
                .series
                .iter()
                .map(|series| source_address(&series.origin))
                .chain(dataset.removed.iter().map(|series| &series.source_field))
            {
                fields.insert(address.clone());
            }
        }
    }
    Ok(fields)
}

/// Materialize Clone Local from Base, mapping rows and ready cardinality
/// plans. The source snapshot is immutable. Repeating the same input produces
/// the same raw JSON and fingerprint.
pub fn materialize_clone(
    base: &VersionedDashboardSnapshot,
    mappings: &[SourceHostMapping],
    cardinalities: &[CloneCardinalityTransform],
    pattern_decisions: &[ClonePatternDecision],
) -> Result<VersionedDashboardSnapshot, CloneTransformError> {
    let classic_graph_verified =
        AdapterRegistry::resolve(base.server_version().clone()).is_ok_and(|capabilities| {
            capabilities
                .dashboard_write_contracts
                .is_verified(WriteContract::ClassicGraphGraphid)
        });
    let registry = AdapterRegistry::widget_capabilities(base.server_version())
        .map_err(|_| CloneTransformError::new("clone_pattern_inventory_failed"))?;
    let model = source_model(base, &registry)
        .map_err(|_| CloneTransformError::new("clone_pattern_inventory_failed"))?;
    let requirements = super::pattern_requirements(&model);
    let pattern_decisions = super::patterns::decisions_by_field(&requirements, pattern_decisions)
        .map_err(|error| CloneTransformError::new(error.code()))?;
    let cardinality_fields = cardinality_fields(cardinalities)?;
    let mut replacements = Vec::new();
    let mut addressed = BTreeSet::new();
    let mut mapped_cardinality_fields = BTreeSet::new();
    let mut mapped_cardinality_targets = BTreeMap::new();
    for mapping in mappings {
        if !mapping.is_well_formed() {
            return Err(CloneTransformError::new("clone_mapping_invalid"));
        }
        if mapping.decision == SourceHostDecision::KeepSource {
            continue;
        }
        for reference in &mapping.references {
            if let Some(decision) = pattern_decisions.get(&reference.field) {
                if !matches!(
                    &reference.source,
                    SourceValue::HostPattern { pattern }
                        if pattern == &decision.requirement.source_pattern
                ) {
                    return Err(fail("clone_pattern_mapping_mismatch", &reference.field));
                }
                continue;
            }
            validate_reference(mapping, reference, classic_graph_verified)?;
            if cardinality_fields.contains(&reference.field) {
                if reference.widget_type != "svggraph"
                    || reference.targets.is_empty()
                    || reference
                        .targets
                        .iter()
                        .any(|target| !usable_resolution(target))
                {
                    return Err(fail("clone_target_unresolved", &reference.field));
                }
                if !mapped_cardinality_fields.insert(reference.field.clone()) {
                    return Err(fail("clone_field_mapped_twice", &reference.field));
                }
                for resolution in &reference.targets {
                    let target = resolution
                        .target
                        .as_ref()
                        .ok_or_else(|| fail("clone_target_missing", &reference.field))?;
                    mapped_cardinality_targets.insert(
                        (reference.field.clone(), resolution.slot_id.clone()),
                        target.clone(),
                    );
                }
                continue;
            }
            if reference.targets.len() > 1 {
                return Err(fail("clone_cardinality_plan_required", &reference.field));
            }
            let resolution = reference
                .targets
                .first()
                .ok_or_else(|| fail("clone_target_missing", &reference.field))?;
            if !usable_resolution(resolution) {
                return Err(fail("clone_target_unresolved", &reference.field));
            }
            let target = resolution
                .target
                .as_ref()
                .ok_or_else(|| fail("clone_target_missing", &reference.field))?;
            if !addressed.insert(reference.field.clone()) {
                return Err(fail("clone_field_mapped_twice", &reference.field));
            }
            replacements.push((
                reference.field.clone(),
                reference.source.clone(),
                target.clone(),
            ));
        }
    }
    if mapped_cardinality_fields != cardinality_fields {
        return Err(CloneTransformError::new(
            "clone_cardinality_mapping_incomplete",
        ));
    }

    let mut raw = base.raw_api_document().clone();
    for decision in pattern_decisions.values() {
        if let ClonePatternAction::Replace { preview } = &decision.action {
            replace_pattern_field(
                &mut raw,
                &decision.requirement.field,
                &decision.requirement.source_pattern,
                &preview.target_pattern,
            )?;
        }
    }
    for (field, source, target) in replacements {
        replace_field(&mut raw, &field, &source, &target)?;
    }
    for transform in cardinalities {
        for dataset in &transform.plan.datasets {
            apply_dataset(&mut raw, transform, dataset, &mapped_cardinality_targets)?;
        }
    }
    VersionedDashboardSnapshot::from_adapter_raw(
        base.connection_id().to_owned(),
        base.server_version().clone(),
        raw,
    )
    .map_err(|_| CloneTransformError::new("clone_materialization_failed"))
}
