// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::validation::{ensure, ensure_len, ensure_slug, ensure_unique};
use super::{Cardinality, FormatError, Result, Selection, Validate, ValueType};
use crate::{Version, VersionRange};

pub const PROFILE_FORMAT: &str = "zdashstitch.style-profile/v1";
pub const BLUEPRINT_FORMAT: &str = "zdashstitch.widget-blueprint/v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReusableWidgetType {
    Item,
    Svggraph,
}

impl ReusableWidgetType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Svggraph => "svggraph",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StyleProfile {
    pub format: String,
    pub profile_id: String,
    pub name: String,
    #[serde(
        default,
        deserialize_with = "optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub version: u32,
    pub adapter: String,
    #[serde(
        default,
        deserialize_with = "compatibility",
        skip_serializing_if = "Option::is_none"
    )]
    pub declared_compatibility: Option<VersionRange>,
    pub widget_types: BTreeMap<ReusableWidgetType, StyleSection>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StyleSection {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub widget_fields: BTreeMap<String, Value>,
    #[serde(
        default,
        deserialize_with = "optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub dataset_fields: Option<BTreeMap<String, i32>>,
    #[serde(
        default,
        deserialize_with = "optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub series_palette: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub color_palette: Option<i32>,
    #[serde(
        default,
        deserialize_with = "optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub thresholds: Option<Vec<StyleThreshold>>,
    #[serde(
        default,
        deserialize_with = "optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub thresholds_units: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub attributes: Option<StyleAttributes>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StyleAttributes {
    pub view_mode: u8,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StyleThreshold {
    pub threshold: String,
    pub color: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WidgetBlueprint {
    pub format: String,
    pub blueprint_id: String,
    pub name: String,
    #[serde(
        default,
        deserialize_with = "optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub version: u32,
    pub adapter: String,
    #[serde(
        default,
        deserialize_with = "compatibility",
        skip_serializing_if = "Option::is_none"
    )]
    pub declared_compatibility: Option<VersionRange>,
    pub widget_type: ReusableWidgetType,
    pub default_size: BlueprintSize,
    #[serde(
        default,
        deserialize_with = "optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub attributes: Option<BlueprintAttributes>,
    pub parameters: BlueprintParameters,
    pub fields: Vec<BlueprintField>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlueprintSize {
    pub width: u8,
    pub height: u8,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlueprintAttributes {
    #[serde(
        default,
        deserialize_with = "optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub view_mode: Option<u8>,
    #[serde(
        default,
        deserialize_with = "optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub rf_rate: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlueprintParameters {
    pub roles: Vec<BlueprintRole>,
    pub metrics: Vec<BlueprintMetric>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlueprintRole {
    pub param: String,
    pub cardinality: Cardinality,
    #[serde(
        default,
        deserialize_with = "optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub slots: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlueprintMetric {
    pub param: String,
    #[serde(
        default,
        deserialize_with = "optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub value_type: Option<ValueType>,
    #[serde(
        default,
        deserialize_with = "optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub units: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlueprintField {
    pub name: String,
    pub field_type: u8,
    pub value: BlueprintFieldValue,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BlueprintFieldValue {
    Integer {
        value: i32,
    },
    String {
        value: String,
    },
    Host {
        role: String,
        selection: Selection,
        #[serde(
            default,
            deserialize_with = "optional",
            skip_serializing_if = "Option::is_none"
        )]
        slot_id: Option<String>,
    },
    Metric {
        metric: String,
        role: String,
        selection: Selection,
        #[serde(
            default,
            deserialize_with = "optional",
            skip_serializing_if = "Option::is_none"
        )]
        slot_id: Option<String>,
    },
    Unbound {
        object: BlueprintObject,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BlueprintObject {
    Host,
    Item,
}

fn optional<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompatibilityRange {
    minimum: Version,
    #[serde(default, deserialize_with = "optional")]
    maximum_exclusive: Option<Version>,
}

fn compatibility<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<VersionRange>, D::Error> {
    let range = CompatibilityRange::deserialize(deserializer)?;
    Ok(Some(VersionRange {
        minimum: range.minimum,
        maximum_exclusive: range.maximum_exclusive,
    }))
}

#[derive(Deserialize)]
struct FieldInventory {
    item: BTreeMap<String, Vec<String>>,
    svggraph: BTreeMap<String, Vec<String>>,
    dataset_fields: Vec<String>,
    blueprint_extra: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    empty_color_resets: BTreeMap<String, Vec<String>>,
}

static FIELDS: LazyLock<FieldInventory> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../../schemas/profile-fields.json"))
        .unwrap_or_else(|error| panic!("invalid bundled profile field inventory: {error}"))
});

fn profile_fields(widget_type: ReusableWidgetType) -> &'static BTreeMap<String, Vec<String>> {
    match widget_type {
        ReusableWidgetType::Item => &FIELDS.item,
        ReusableWidgetType::Svggraph => &FIELDS.svggraph,
    }
}

fn field_kind(map: &BTreeMap<String, Vec<String>>, name: &str) -> Option<&'static str> {
    [
        "integer",
        "string",
        "color",
        "host",
        "pattern_host",
        "metric",
        "pattern_metric",
    ]
    .into_iter()
    .find(|kind| {
        map.get(*kind)
            .is_some_and(|names| names.iter().any(|pattern| matches_name(pattern, name)))
    })
}

fn matches_name(pattern: &str, name: &str) -> bool {
    let mut parts = name.split('.');
    pattern.split('.').all(|part| {
        parts.next().is_some_and(|value| {
            if part == "#" {
                is_index(value)
            } else {
                part == value
            }
        })
    }) && parts.next().is_none()
}

fn is_index(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value.len() == 1 || !value.starts_with('0'))
}

fn color(value: &str, adapter: &str, name: &str) -> Result<()> {
    let uppercase = value.len() == 6
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b));
    let reset = value.is_empty()
        && FIELDS
            .empty_color_resets
            .get(adapter)
            .is_some_and(|fields| fields.iter().any(|pattern| matches_name(pattern, name)));
    ensure(
        uppercase || reset,
        "profile_colour_format",
        "invalid color or unsupported reset",
    )
}

fn metadata(
    name: &str,
    id: &str,
    version: u32,
    adapter: &str,
    text: (Option<&str>, &[String]),
    compatibility: Option<&VersionRange>,
    blueprint: bool,
) -> Result<()> {
    let (description, tags) = text;
    let valid_id = id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            14 => b == b'4',
            19 => matches!(b, b'8' | b'9' | b'a' | b'b'),
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        });
    let (id_code, name_code, max_name) = if blueprint {
        ("blueprint_id_invalid", "blueprint_name_required", 255)
    } else {
        ("profile_id_invalid", "profile_name_required", 120)
    };
    ensure(valid_id, id_code, "invalid v4 UUID")?;
    ensure_len(name, 1, max_name, name_code, "name")?;
    ensure(
        !name.trim().is_empty() && !name.chars().any(char::is_control),
        name_code,
        "invalid name",
    )?;
    ensure(
        version > 0,
        "profile_version_invalid",
        "version must be positive",
    )?;
    ensure(
        adapter.split_once('.').is_some_and(|(major, minor)| {
            [major, minor]
                .into_iter()
                .all(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
        }),
        "profile_adapter_invalid",
        "adapter must have major and minor version",
    )?;
    if let Some(description) = description {
        ensure_len(
            description,
            0,
            2000,
            "profile_description_length",
            "description",
        )?;
    }
    ensure(tags.len() <= 20, "profile_tags_limit", "too many tags")?;
    for tag in tags {
        ensure_len(tag, 1, 40, "profile_tag_length", "tag")?;
    }
    ensure_unique(
        tags.iter().map(|tag| tag.to_lowercase()),
        "profile_tag_duplicate",
        "tag",
    )?;
    ensure(
        compatibility.is_none_or(VersionRange::is_non_empty),
        "profile_compatibility_range",
        "empty compatibility range",
    )
}

impl Validate for StyleProfile {
    fn validate(&self) -> Result<()> {
        ensure(
            self.format == PROFILE_FORMAT,
            "profile_format",
            "unsupported profile format",
        )?;
        metadata(
            &self.name,
            &self.profile_id,
            self.version,
            &self.adapter,
            (self.description.as_deref(), &self.tags),
            self.declared_compatibility.as_ref(),
            false,
        )?;
        ensure(
            !self.widget_types.is_empty(),
            "profile_widget_types_empty",
            "profile needs a widget type",
        )?;
        for (&widget_type, section) in &self.widget_types {
            section.validate(widget_type, &self.adapter)?;
        }
        Ok(())
    }
}

impl StyleSection {
    fn validate(&self, widget_type: ReusableWidgetType, adapter: &str) -> Result<()> {
        ensure(
            self.attributes.as_ref().is_none_or(|a| a.view_mode <= 1),
            "profile_field_type_mismatch",
            "invalid view mode",
        )?;
        match widget_type {
            ReusableWidgetType::Svggraph => self.validate_graph(adapter)?,
            ReusableWidgetType::Item => self.validate_item(adapter)?,
        }
        for (name, value) in &self.widget_fields {
            if name == "show" && widget_type == ReusableWidgetType::Item {
                continue;
            }
            match field_kind(profile_fields(widget_type), name) {
                Some("integer") => ensure(
                    value.as_i64().is_some_and(|n| i32::try_from(n).is_ok()),
                    "profile_field_type_mismatch",
                    "expected an integer",
                )?,
                Some("string") => {
                    string_value(value, if name == "units" { 255 } else { 2048 })?;
                }
                Some("color") => color(
                    string_value(value, 6)?,
                    adapter,
                    &format!("{}.{}", widget_type.as_str(), name),
                )?,
                _ => {
                    return Err(FormatError::validation(
                        "profile_unknown_field",
                        "field is outside the style inventory",
                    ));
                }
            }
        }
        Ok(())
    }

    fn validate_graph(&self, adapter: &str) -> Result<()> {
        ensure(
            self.thresholds.is_none() && self.thresholds_units.is_none(),
            "profile_unknown_field",
            "thresholds are limited to item profiles",
        )?;
        ensure(
            !self.widget_fields.is_empty()
                || self.dataset_fields.as_ref().is_some_and(|m| !m.is_empty())
                || self.series_palette.is_some()
                || self.color_palette.is_some()
                || self.attributes.is_some(),
            "profile_section_empty",
            "empty graph style",
        )?;
        ensure(
            self.series_palette.is_none() || self.color_palette.is_none(),
            "profile_palette_exclusive",
            "palette types are mutually exclusive",
        )?;
        if let Some(palette) = &self.series_palette {
            ensure(
                (1..=64).contains(&palette.len()),
                "profile_palette_too_long",
                "invalid series palette length",
            )?;
            for value in palette {
                color(value, adapter, "svggraph.series_palette")?;
            }
        }
        ensure(
            self.color_palette.is_none_or(|value| value >= 0),
            "profile_field_type_mismatch",
            "negative palette number",
        )?;
        if let Some(fields) = &self.dataset_fields {
            ensure(
                fields
                    .keys()
                    .all(|name| FIELDS.dataset_fields.contains(name)),
                "profile_unknown_field",
                "field is outside the dataset style inventory",
            )?;
        }
        Ok(())
    }

    fn validate_item(&self, adapter: &str) -> Result<()> {
        ensure(
            self.dataset_fields.is_none()
                && self.series_palette.is_none()
                && self.color_palette.is_none(),
            "profile_unknown_field",
            "graph-only fields in item style",
        )?;
        let show = self
            .widget_fields
            .get("show")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                FormatError::validation("profile_show_required", "missing item show options")
            })?;
        ensure(
            !show.is_empty() && show.iter().all(|v| matches!(v.as_i64(), Some(1..=5))),
            "profile_show_required",
            "invalid item show options",
        )?;
        ensure_unique(
            show.iter().map(Value::as_i64),
            "profile_show_required",
            "item show option",
        )?;
        if let Some(units) = &self.thresholds_units {
            ensure(
                self.thresholds.as_ref().is_some_and(|v| !v.is_empty()),
                "profile_thresholds_units_orphan",
                "threshold units require thresholds",
            )?;
            ensure_len(units, 0, 32, "profile_threshold_pair", "threshold units")?;
        }
        if let Some(thresholds) = &self.thresholds {
            ensure(
                thresholds.len() <= 64,
                "profile_threshold_pair",
                "too many thresholds",
            )?;
            ensure_unique(
                thresholds.iter().map(|v| &v.threshold),
                "profile_threshold_pair",
                "threshold",
            )?;
            for threshold in thresholds {
                ensure_len(
                    &threshold.threshold,
                    1,
                    32,
                    "profile_threshold_pair",
                    "threshold",
                )?;
                ensure(
                    threshold.threshold.parse::<f64>().is_ok_and(f64::is_finite),
                    "profile_threshold_pair",
                    "non-numeric threshold",
                )?;
                color(&threshold.color, adapter, "item.thresholds.color")?;
            }
        }
        Ok(())
    }
}

fn string_value(value: &Value, max: usize) -> Result<&str> {
    let value = value.as_str().ok_or_else(|| {
        FormatError::validation("profile_field_type_mismatch", "expected a string")
    })?;
    ensure_len(value, 0, max, "profile_field_type_mismatch", "field value")?;
    Ok(value)
}

impl Validate for WidgetBlueprint {
    fn validate(&self) -> Result<()> {
        ensure(
            self.format == BLUEPRINT_FORMAT,
            "blueprint_format",
            "unsupported blueprint format",
        )?;
        metadata(
            &self.name,
            &self.blueprint_id,
            self.version,
            &self.adapter,
            (self.description.as_deref(), &self.tags),
            self.declared_compatibility.as_ref(),
            true,
        )?;
        ensure(
            (1..=72).contains(&self.default_size.width)
                && (1..=64).contains(&self.default_size.height),
            "blueprint_default_size_invalid",
            "size outside the grid",
        )?;
        ensure(
            self.attributes
                .as_ref()
                .and_then(|a| a.view_mode)
                .is_none_or(|v| v <= 1),
            "blueprint_field_type_mismatch",
            "invalid view mode",
        )?;
        let mut roles = BTreeMap::new();
        let mut metrics = BTreeSet::new();
        ensure(
            self.parameters.roles.len() <= 64 && self.parameters.metrics.len() <= 1024,
            "blueprint_parameter_limit",
            "too many parameters",
        )?;
        for role in &self.parameters.roles {
            ensure_slug(&role.param, "role parameter")?;
            ensure(
                roles.insert(role.param.as_str(), role).is_none(),
                "blueprint_parameter_duplicate",
                "duplicate role parameter",
            )?;
            if let Some(slots) = &role.slots {
                ensure(
                    role.cardinality == Cardinality::Many && (1..=256).contains(&slots.len()),
                    "blueprint_role_slots",
                    "invalid slots for role",
                )?;
                ensure_unique(slots, "blueprint_role_slots", "slot")?;
                for slot in slots {
                    ensure_slug(slot, "parameter slot")?;
                }
            }
            if let Some(description) = &role.description {
                ensure_len(
                    description,
                    0,
                    500,
                    "blueprint_parameter_length",
                    "role description",
                )?;
            }
        }
        for metric in &self.parameters.metrics {
            ensure_slug(&metric.param, "metric parameter")?;
            ensure(
                !roles.contains_key(metric.param.as_str()) && metrics.insert(metric.param.as_str()),
                "blueprint_parameter_duplicate",
                "duplicate metric parameter",
            )?;
            ensure(
                !matches!(metric.value_type, Some(ValueType::Binary | ValueType::Json)),
                "blueprint_metric_value_type",
                "unsupported metric value type",
            )?;
            if let Some(units) = &metric.units {
                ensure_len(units, 0, 255, "blueprint_parameter_length", "metric units")?;
            }
            if let Some(description) = &metric.description {
                ensure_len(
                    description,
                    0,
                    500,
                    "blueprint_parameter_length",
                    "metric description",
                )?;
            }
        }
        self.validate_fields(&roles, &metrics)
    }
}

struct BlueprintReferences<'a> {
    roles: &'a BTreeMap<&'a str, &'a BlueprintRole>,
    metrics: &'a BTreeSet<&'a str>,
    used: BTreeSet<&'a str>,
}

impl<'a> BlueprintReferences<'a> {
    fn role(
        &mut self,
        name: &'a str,
        selection: Selection,
        slot: Option<&str>,
        direct: bool,
    ) -> Result<()> {
        let param = reference_parameter(name)?;
        let role = self.roles.get(param).ok_or_else(|| {
            FormatError::validation(
                "blueprint_parameter_undeclared",
                "undeclared role parameter",
            )
        })?;
        let valid = match selection {
            Selection::One => role.cardinality == Cardinality::One && slot.is_none(),
            Selection::All => !direct && slot.is_none(),
            Selection::Slot => role.slots.as_ref().is_some_and(|slots| {
                slot.is_some_and(|slot| slots.iter().any(|name| name == slot))
            }),
        };
        ensure(
            valid,
            "blueprint_role_selection",
            "role and selection do not address an available node",
        )?;
        self.used.insert(param);
        Ok(())
    }

    fn metric(&mut self, name: &'a str) -> Result<()> {
        let param = reference_parameter(name)?;
        ensure(
            self.metrics.contains(param),
            "blueprint_parameter_undeclared",
            "undeclared metric parameter",
        )?;
        self.used.insert(param);
        Ok(())
    }
}

fn reference_parameter(name: &str) -> Result<&str> {
    name.strip_prefix('$').ok_or_else(|| {
        FormatError::validation(
            "blueprint_parameter_name_invalid",
            "parameter references start with a dollar sign",
        )
    })
}

impl WidgetBlueprint {
    fn validate_fields(
        &self,
        roles: &BTreeMap<&str, &BlueprintRole>,
        metrics: &BTreeSet<&str>,
    ) -> Result<()> {
        ensure(
            (1..=1024).contains(&self.fields.len()),
            "blueprint_incomplete",
            "invalid blueprint field count",
        )?;
        let mut names = BTreeSet::new();
        let mut references = BlueprintReferences {
            roles,
            metrics,
            used: BTreeSet::new(),
        };
        for field in &self.fields {
            ensure(
                names.insert(field.name.as_str()),
                "blueprint_duplicate_field",
                "duplicate blueprint field",
            )?;
            field.validate(self.widget_type, &self.adapter, &mut references)?;
        }
        ensure(
            references.used.len() == roles.len() + metrics.len(),
            "blueprint_parameter_unused",
            "unused parameter",
        )?;
        ensure(
            match self.widget_type {
                ReusableWidgetType::Item => names.contains("itemid.0"),
                ReusableWidgetType::Svggraph => names.iter().any(|name| {
                    matches_name("ds.#.itemids.0", name)
                        || name.strip_suffix(".items.0").is_some_and(|prefix| {
                            matches_name("ds.#", prefix)
                                && names.contains(format!("{prefix}.hosts.0").as_str())
                        })
                }),
            },
            "blueprint_incomplete",
            "widget has no source",
        )?;
        if self.widget_type == ReusableWidgetType::Svggraph {
            self.validate_patterns()?;
        }
        Ok(())
    }

    fn validate_patterns(&self) -> Result<()> {
        let mut hosts = BTreeMap::<&str, BTreeSet<(&str, u8, Option<&str>)>>::new();
        for field in &self.fields {
            if let (
                Some((dataset, _)),
                BlueprintFieldValue::Host {
                    role,
                    selection,
                    slot_id,
                },
            ) = (field.name.split_once(".hosts."), &field.value)
            {
                hosts.entry(dataset).or_default().insert((
                    role,
                    selection_key(*selection),
                    slot_id.as_deref(),
                ));
            }
        }
        for field in &self.fields {
            if let (
                Some((dataset, _)),
                BlueprintFieldValue::Metric {
                    role,
                    selection,
                    slot_id,
                    ..
                },
            ) = (field.name.split_once(".items."), &field.value)
            {
                ensure(
                    hosts.get(dataset).is_some_and(|rows| {
                        rows.contains(&(role, selection_key(*selection), slot_id.as_deref()))
                    }),
                    "blueprint_role_selection",
                    "pattern metric has no matching explicit host selection",
                )?;
            }
        }
        Ok(())
    }
}

fn selection_key(selection: Selection) -> u8 {
    match selection {
        Selection::One => 0,
        Selection::All => 1,
        Selection::Slot => 2,
    }
}

impl BlueprintField {
    fn validate<'a>(
        &'a self,
        widget_type: ReusableWidgetType,
        adapter: &str,
        refs: &mut BlueprintReferences<'a>,
    ) -> Result<()> {
        let name = self.name.as_str();
        ensure_len(name, 1, 128, "blueprint_unknown_field", "field name")?;
        ensure(
            name != "reference" && !name.ends_with("._reference"),
            "blueprint_reference",
            "widget references are prohibited",
        )?;
        ensure(
            !name.starts_with("or."),
            "blueprint_override",
            "dataset overrides are prohibited",
        )?;
        let inventory = FIELDS
            .blueprint_extra
            .get(widget_type.as_str())
            .ok_or_else(|| {
                FormatError::validation("blueprint_unknown_field", "missing widget inventory")
            })?;
        let kind = field_kind(profile_fields(widget_type), name)
            .or_else(|| field_kind(inventory, name))
            .or_else(|| {
                if widget_type == ReusableWidgetType::Svggraph
                    && FIELDS
                        .dataset_fields
                        .iter()
                        .any(|field| matches_name(&format!("ds.#.{field}"), name))
                {
                    Some("integer")
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                FormatError::validation("blueprint_unknown_field", "unknown blueprint field")
            })?;
        match (kind, self.field_type, &self.value) {
            ("integer", 0, BlueprintFieldValue::Integer { .. }) => Ok(()),
            ("color", 1, BlueprintFieldValue::String { value }) => color(
                value,
                adapter,
                &format!("{}.{}", widget_type.as_str(), name),
            ),
            ("string", 1, BlueprintFieldValue::String { value }) => ensure_len(
                value,
                0,
                2048,
                "blueprint_field_type_mismatch",
                "field value",
            ),
            (
                "host",
                3,
                BlueprintFieldValue::Host {
                    role,
                    selection,
                    slot_id,
                },
            ) => refs.role(role, *selection, slot_id.as_deref(), true),
            (
                "pattern_host",
                1,
                BlueprintFieldValue::Host {
                    role,
                    selection,
                    slot_id,
                },
            ) => refs.role(role, *selection, slot_id.as_deref(), false),
            (
                "metric",
                4,
                BlueprintFieldValue::Metric {
                    metric,
                    role,
                    selection,
                    slot_id,
                },
            )
            | (
                "pattern_metric",
                1,
                BlueprintFieldValue::Metric {
                    metric,
                    role,
                    selection,
                    slot_id,
                },
            ) => {
                refs.role(role, *selection, slot_id.as_deref(), kind == "metric")?;
                refs.metric(metric)
            }
            (
                "host",
                3,
                BlueprintFieldValue::Unbound {
                    object: BlueprintObject::Host,
                },
            )
            | (
                "pattern_host",
                1,
                BlueprintFieldValue::Unbound {
                    object: BlueprintObject::Host,
                },
            )
            | (
                "metric",
                4,
                BlueprintFieldValue::Unbound {
                    object: BlueprintObject::Item,
                },
            )
            | (
                "pattern_metric",
                1,
                BlueprintFieldValue::Unbound {
                    object: BlueprintObject::Item,
                },
            ) => Ok(()),
            (
                "host" | "pattern_host",
                _,
                BlueprintFieldValue::Integer { .. } | BlueprintFieldValue::String { .. },
            ) => Err(FormatError::validation(
                "blueprint_host_pattern",
                "literal host sources are prohibited",
            )),
            (
                "metric" | "pattern_metric",
                _,
                BlueprintFieldValue::Integer { .. } | BlueprintFieldValue::String { .. },
            ) => Err(FormatError::validation(
                "blueprint_item_pattern",
                "literal item sources are prohibited",
            )),
            _ => Err(FormatError::validation(
                "blueprint_field_type_mismatch",
                "field type does not match field name and value kind",
            )),
        }
    }
}
