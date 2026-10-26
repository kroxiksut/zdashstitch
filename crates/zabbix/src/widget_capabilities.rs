// SPDX-License-Identifier: MIT OR Apache-2.0

use zdashstitch_core::{Version, VersionRange};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum WidgetFieldType {
    Integer = 0,
    String = 1,
    HostGroup = 2,
    Host = 3,
    Item = 4,
    ItemPrototype = 5,
    Graph = 6,
    GraphPrototype = 7,
    Map = 8,
    Service = 9,
    Sla = 10,
    User = 11,
    Action = 12,
    MediaType = 13,
}

impl WidgetFieldType {
    const fn bit(self) -> u16 {
        1 << self as u8
    }
}

impl TryFrom<u16> for WidgetFieldType {
    type Error = ();

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Integer),
            1 => Ok(Self::String),
            2 => Ok(Self::HostGroup),
            3 => Ok(Self::Host),
            4 => Ok(Self::Item),
            5 => Ok(Self::ItemPrototype),
            6 => Ok(Self::Graph),
            7 => Ok(Self::GraphPrototype),
            8 => Ok(Self::Map),
            9 => Ok(Self::Service),
            10 => Ok(Self::Sla),
            11 => Ok(Self::User),
            12 => Ok(Self::Action),
            13 => Ok(Self::MediaType),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KnownWidgetField {
    pub pattern: &'static str,
    type_mask: u16,
}

impl KnownWidgetField {
    const fn one(pattern: &'static str, field_type: WidgetFieldType) -> Self {
        Self {
            pattern,
            type_mask: field_type.bit(),
        }
    }

    const fn either(
        pattern: &'static str,
        first: WidgetFieldType,
        second: WidgetFieldType,
    ) -> Self {
        Self {
            pattern,
            type_mask: first.bit() | second.bit(),
        }
    }

    #[must_use]
    pub fn accepts(&self, name: &str, field_type: WidgetFieldType) -> bool {
        self.type_mask & field_type.bit() != 0 && field_pattern_matches(self.pattern, name)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WidgetCapabilityFlags {
    pub read: bool,
    pub preview: bool,
    pub edit: bool,
    pub create: bool,
    pub geometry: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WidgetCapability {
    pub widget_type: &'static str,
    pub versions: VersionRange,
    pub fields: &'static [KnownWidgetField],
    pub flags: WidgetCapabilityFlags,
}

impl WidgetCapability {
    #[must_use]
    pub fn knows_field(&self, name: &str, field_type: WidgetFieldType) -> bool {
        self.fields
            .iter()
            .any(|field| field.accepts(name, field_type))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WidgetCapabilityRegistry {
    adapter: &'static str,
    capabilities: Vec<WidgetCapability>,
}

impl WidgetCapabilityRegistry {
    #[must_use]
    pub fn v7_4() -> Self {
        let versions = VersionRange {
            minimum: Version::new(7, 4, 0),
            maximum_exclusive: None,
        };
        let mut capabilities = Vec::with_capacity(32);
        for (widget_type, fields, flags) in DETAILED_WIDGETS {
            capabilities.push(WidgetCapability {
                widget_type,
                versions: versions.clone(),
                fields,
                flags: *flags,
            });
        }
        for widget_type in RECOGNIZED_WIDGETS {
            capabilities.push(WidgetCapability {
                widget_type,
                versions: versions.clone(),
                fields: &[],
                flags: READ_ONLY,
            });
        }
        Self {
            adapter: "7.4",
            capabilities,
        }
    }

    #[must_use]
    pub const fn adapter(&self) -> &'static str {
        self.adapter
    }

    #[must_use]
    pub fn capabilities(&self) -> &[WidgetCapability] {
        &self.capabilities
    }

    #[must_use]
    pub fn find(&self, widget_type: &str) -> Option<&WidgetCapability> {
        self.capabilities
            .iter()
            .find(|capability| capability.widget_type == widget_type)
    }
}

const I: WidgetFieldType = WidgetFieldType::Integer;
const S: WidgetFieldType = WidgetFieldType::String;
const GROUP: WidgetFieldType = WidgetFieldType::HostGroup;
const HOST: WidgetFieldType = WidgetFieldType::Host;
const ITEM: WidgetFieldType = WidgetFieldType::Item;
const GRAPH: WidgetFieldType = WidgetFieldType::Graph;

const READ_PREVIEW: WidgetCapabilityFlags = WidgetCapabilityFlags {
    read: true,
    preview: true,
    edit: false,
    create: false,
    geometry: true,
};
const READ_PREVIEW_CREATE: WidgetCapabilityFlags = WidgetCapabilityFlags {
    create: true,
    ..READ_PREVIEW
};
const READ_PREVIEW_EDIT_CREATE: WidgetCapabilityFlags = WidgetCapabilityFlags {
    edit: true,
    create: true,
    ..READ_PREVIEW
};
const READ_ONLY: WidgetCapabilityFlags = WidgetCapabilityFlags {
    read: true,
    preview: false,
    edit: false,
    create: false,
    geometry: true,
};

const COMMON_REFERENCE: KnownWidgetField = KnownWidgetField::one("reference", S);
const OVERRIDE_HOST: KnownWidgetField = KnownWidgetField::one("override_hostid.#", HOST);
const OVERRIDE_REFERENCE: KnownWidgetField = KnownWidgetField::one("override_hostid._reference", S);

const PROBLEMS_FIELDS: &[KnownWidgetField] = &[
    COMMON_REFERENCE,
    KnownWidgetField::one("show", I),
    KnownWidgetField::one("groupids.#", GROUP),
    KnownWidgetField::one("exclude_groupids.#", GROUP),
    KnownWidgetField::one("hostids.#", HOST),
    KnownWidgetField::one("problem", S),
    KnownWidgetField::one("severities.#", I),
    KnownWidgetField::one("evaltype", I),
    KnownWidgetField::one("tags.#.tag", S),
    KnownWidgetField::one("tags.#.operator", I),
    KnownWidgetField::one("tags.#.value", S),
    KnownWidgetField::one("show_tags", I),
    KnownWidgetField::one("tag_name_format", I),
    KnownWidgetField::one("tag_priority", S),
    KnownWidgetField::one("show_opdata", I),
    KnownWidgetField::one("show_symptoms", I),
    KnownWidgetField::one("show_suppressed", I),
    KnownWidgetField::one("acknowledgement_status", I),
    KnownWidgetField::one("acknowledged_by_me", I),
    KnownWidgetField::one("sort_triggers", I),
    KnownWidgetField::one("show_timeline", I),
    KnownWidgetField::one("highlight_row", I),
    KnownWidgetField::one("show_lines", I),
    OVERRIDE_HOST,
    OVERRIDE_REFERENCE,
];

const CLOCK_FIELDS: &[KnownWidgetField] = &[
    COMMON_REFERENCE,
    KnownWidgetField::one("time_type", I),
    KnownWidgetField::one("itemid.#", ITEM),
    KnownWidgetField::either("clock_type", I, S),
    KnownWidgetField::either("show.#", I, S),
    KnownWidgetField::either("time_format", I, S),
    KnownWidgetField::either("time_sec", I, S),
    KnownWidgetField::either("tzone_timezone", I, S),
    KnownWidgetField::either("tzone_format", I, S),
    KnownWidgetField::either("bg_color", I, S),
    KnownWidgetField::either("date_*", I, S),
    KnownWidgetField::either("time_*", I, S),
    KnownWidgetField::either("tzone_*", I, S),
    OVERRIDE_HOST,
    OVERRIDE_REFERENCE,
];

const HOSTAVAIL_FIELDS: &[KnownWidgetField] = &[
    COMMON_REFERENCE,
    KnownWidgetField::one("groupids.#", GROUP),
    KnownWidgetField::one("interface_type.#", I),
    KnownWidgetField::one("layout", I),
    KnownWidgetField::one("maintenance", I),
    KnownWidgetField::one("only_totals", I),
    OVERRIDE_HOST,
    OVERRIDE_REFERENCE,
];

const ITEM_FIELDS: &[KnownWidgetField] = &[
    COMMON_REFERENCE,
    KnownWidgetField::one("itemid.#", ITEM),
    KnownWidgetField::one("show.#", I),
    KnownWidgetField::one("time_period.from", S),
    KnownWidgetField::one("time_period.to", S),
    KnownWidgetField::one("time_period._reference", S),
    KnownWidgetField::one("aggregate_function", I),
    KnownWidgetField::one("history", I),
    KnownWidgetField::one("sparkline.*", I),
    KnownWidgetField::one("sparkline.color", S),
    KnownWidgetField::one("sparkline.time_period.from", S),
    KnownWidgetField::one("sparkline.time_period.to", S),
    KnownWidgetField::one("thresholds.#.color", S),
    KnownWidgetField::one("thresholds.#.threshold", S),
    KnownWidgetField::either("description", I, S),
    KnownWidgetField::either("desc_*", I, S),
    KnownWidgetField::either("decimal_*", I, S),
    KnownWidgetField::either("value_*", I, S),
    KnownWidgetField::either("units*", I, S),
    KnownWidgetField::either("time_*", I, S),
    KnownWidgetField::either("up_color", I, S),
    KnownWidgetField::either("down_color", I, S),
    KnownWidgetField::either("updown_color", I, S),
    KnownWidgetField::either("bg_color", I, S),
    OVERRIDE_HOST,
    OVERRIDE_REFERENCE,
];

const PROBLEMS_BY_SEVERITY_FIELDS: &[KnownWidgetField] = &[
    COMMON_REFERENCE,
    KnownWidgetField::one("groupids.#", GROUP),
    KnownWidgetField::one("exclude_groupids.#", GROUP),
    KnownWidgetField::one("hostids.#", HOST),
    KnownWidgetField::one("problem", S),
    KnownWidgetField::one("severities.#", I),
    KnownWidgetField::one("evaltype", I),
    KnownWidgetField::one("tags.#.tag", S),
    KnownWidgetField::one("tags.#.operator", I),
    KnownWidgetField::one("tags.#.value", S),
    KnownWidgetField::one("show_type", I),
    KnownWidgetField::one("layout", I),
    KnownWidgetField::one("show_opdata", I),
    KnownWidgetField::one("show_suppressed", I),
    KnownWidgetField::one("hide_empty_groups", I),
    KnownWidgetField::one("ext_ack", I),
    KnownWidgetField::one("show_timeline", I),
    OVERRIDE_HOST,
    OVERRIDE_REFERENCE,
];

const SYSTEMINFO_FIELDS: &[KnownWidgetField] = &[
    COMMON_REFERENCE,
    KnownWidgetField::one("info_type", I),
    KnownWidgetField::one("show_software_update_check_details", I),
];

const TOPHOSTS_FIELDS: &[KnownWidgetField] = &[
    COMMON_REFERENCE,
    KnownWidgetField::one("groupids.#", GROUP),
    KnownWidgetField::one("hostids.#", HOST),
    KnownWidgetField::one("evaltype", I),
    KnownWidgetField::one("tags.#.tag", S),
    KnownWidgetField::one("tags.#.operator", I),
    KnownWidgetField::one("tags.#.value", S),
    KnownWidgetField::one("maintenance", I),
    KnownWidgetField::one("columns.#.name", S),
    KnownWidgetField::one("columns.#.item", S),
    KnownWidgetField::one("columns.#.text", S),
    KnownWidgetField::one("columns.#.base_color", S),
    KnownWidgetField::one("columns.#.min", S),
    KnownWidgetField::one("columns.#.max", S),
    KnownWidgetField::one("columns.#.data", I),
    KnownWidgetField::one("columns.#.display_value_as", I),
    KnownWidgetField::one("columns.#.display", I),
    KnownWidgetField::one("columns.#.decimal_places", I),
    KnownWidgetField::one("columns.#.show_thumbnail", I),
    KnownWidgetField::one("columns.#.aggregate_function", I),
    KnownWidgetField::one("columns.#.history", I),
    KnownWidgetField::one("columnsthresholds.#.color.#", S),
    KnownWidgetField::one("columnsthresholds.#.threshold.#", S),
    KnownWidgetField::one("column", I),
    KnownWidgetField::one("order", I),
    KnownWidgetField::one("show_lines", I),
    OVERRIDE_HOST,
    OVERRIDE_REFERENCE,
];

const SVGGRAPH_FIELDS: &[KnownWidgetField] = &[
    COMMON_REFERENCE,
    KnownWidgetField::one("ds.#.dataset_type", I),
    KnownWidgetField::one("ds.#.hosts.#", S),
    KnownWidgetField::one("ds.#.items.#", S),
    KnownWidgetField::one("ds.#.itemids.#", ITEM),
    KnownWidgetField::one("ds.#.itemids.#._reference", S),
    KnownWidgetField::one("ds.#.color.#", S),
    KnownWidgetField::one("ds.#.color_palette", I),
    KnownWidgetField::one("ds.#.override_hostid._reference", S),
    KnownWidgetField::one("ds.#.type", I),
    KnownWidgetField::one("ds.#.stacked", I),
    KnownWidgetField::one("ds.#.width", I),
    KnownWidgetField::one("ds.#.pointsize", I),
    KnownWidgetField::one("ds.#.transparency", I),
    KnownWidgetField::one("ds.#.fill", I),
    KnownWidgetField::one("ds.#.axisy", I),
    KnownWidgetField::one("ds.#.missingdatafunc", I),
    KnownWidgetField::one("ds.#.aggregate_function", I),
    KnownWidgetField::one("ds.#.aggregate_grouping", I),
    KnownWidgetField::one("ds.#.approximation", I),
    KnownWidgetField::one("ds.#.timeshift", S),
    KnownWidgetField::one("ds.#.aggregate_interval", S),
    KnownWidgetField::one("ds.#.data_set_label", S),
    KnownWidgetField::one("or.#.hosts.#", S),
    KnownWidgetField::one("or.#.items.#", S),
    KnownWidgetField::one("or.#.*", I),
    KnownWidgetField::one("source", I),
    KnownWidgetField::one("time_period.from", S),
    KnownWidgetField::one("time_period.to", S),
    KnownWidgetField::one("time_period._reference", S),
    KnownWidgetField::one("problemhosts.#", S),
    KnownWidgetField::one("severities.#", I),
    KnownWidgetField::one("problem_name", S),
    KnownWidgetField::one("evaltype", I),
    KnownWidgetField::one("tags.#.tag", S),
    KnownWidgetField::one("tags.#.operator", I),
    KnownWidgetField::one("tags.#.value", S),
    KnownWidgetField::either("show_problems", I, S),
    KnownWidgetField::either("graph_item_problems", I, S),
    KnownWidgetField::either("simple_triggers", I, S),
    KnownWidgetField::either("working_time", I, S),
    KnownWidgetField::either("percentile_*", I, S),
    KnownWidgetField::either("lefty*", I, S),
    KnownWidgetField::either("righty*", I, S),
    KnownWidgetField::either("axisx", I, S),
    KnownWidgetField::either("legend*", I, S),
    OVERRIDE_HOST,
    OVERRIDE_REFERENCE,
];

const GRAPH_FIELDS: &[KnownWidgetField] = &[
    COMMON_REFERENCE,
    KnownWidgetField::one("source_type", I),
    KnownWidgetField::one("graphid.#", GRAPH),
    KnownWidgetField::one("itemid.#", ITEM),
    KnownWidgetField::one("time_period.from", S),
    KnownWidgetField::one("time_period.to", S),
    KnownWidgetField::one("show_legend", I),
    OVERRIDE_HOST,
    OVERRIDE_REFERENCE,
];

const DETAILED_WIDGETS: &[(&str, &[KnownWidgetField], WidgetCapabilityFlags)] = &[
    ("clock", CLOCK_FIELDS, READ_PREVIEW_CREATE),
    ("hostavail", HOSTAVAIL_FIELDS, READ_PREVIEW),
    ("item", ITEM_FIELDS, READ_PREVIEW_EDIT_CREATE),
    ("problems", PROBLEMS_FIELDS, READ_PREVIEW_CREATE),
    ("problemsbysv", PROBLEMS_BY_SEVERITY_FIELDS, READ_PREVIEW),
    ("svggraph", SVGGRAPH_FIELDS, READ_PREVIEW_EDIT_CREATE),
    ("systeminfo", SYSTEMINFO_FIELDS, READ_PREVIEW),
    ("tophosts", TOPHOSTS_FIELDS, READ_PREVIEW),
    ("graph", GRAPH_FIELDS, READ_PREVIEW),
];

const RECOGNIZED_WIDGETS: &[&str] = &[
    "actionlog",
    "discovery",
    "favgraphs",
    "favmaps",
    "gauge",
    "geomap",
    "graphprototype",
    "honeycomb",
    "hostcard",
    "hostnavigator",
    "itemcard",
    "itemhistory",
    "itemnavigator",
    "map",
    "navtree",
    "piechart",
    "problemhosts",
    "slareport",
    "topitems",
    "toptriggers",
    "trigover",
    "url",
    "web",
];

fn field_pattern_matches(pattern: &str, name: &str) -> bool {
    let pattern_segments = pattern.split('.').collect::<Vec<_>>();
    let name_segments = name.split('.').collect::<Vec<_>>();
    pattern_segments.len() == name_segments.len()
        && pattern_segments
            .iter()
            .zip(name_segments)
            .all(|(pattern, segment)| match *pattern {
                "#" => !segment.is_empty() && segment.bytes().all(|byte| byte.is_ascii_digit()),
                value if value.contains('*') => glob_segment_matches(value, segment),
                value => value == segment,
            })
}

fn glob_segment_matches(pattern: &str, value: &str) -> bool {
    let Some((prefix, suffix)) = pattern.split_once('*') else {
        return pattern == value;
    };
    value.len() >= prefix.len() + suffix.len()
        && value.starts_with(prefix)
        && value.ends_with(suffix)
}

#[cfg(test)]
mod tests {
    use crate::AdapterRegistry;

    use super::*;

    fn capability<'a>(
        registry: &'a WidgetCapabilityRegistry,
        widget_type: &str,
    ) -> &'a WidgetCapability {
        registry
            .find(widget_type)
            .unwrap_or_else(|| panic!("missing test capability: {widget_type}"))
    }

    #[test]
    fn registry_has_the_32_explicitly_listed_7_4_widget_types() {
        let version = "7.4.16"
            .parse()
            .unwrap_or_else(|error| panic!("valid test version: {error}"));
        let registry = AdapterRegistry::widget_capabilities(&version)
            .unwrap_or_else(|error| panic!("registered adapter: {error}"));
        assert_eq!(registry.adapter(), "7.4");
        assert_eq!(registry.capabilities().len(), 32);
        assert!(registry.find("systeminfo").is_some());
        assert!(registry.find("vendor.private").is_none());
    }

    #[test]
    fn known_field_patterns_check_names_and_types() {
        let registry = WidgetCapabilityRegistry::v7_4();
        let graph = capability(&registry, "svggraph");
        assert!(graph.knows_field("ds.0.itemids.2", WidgetFieldType::Item));
        assert!(!graph.knows_field("ds.0.itemids.2", WidgetFieldType::String));
        assert!(!graph.knows_field("ds.0.private", WidgetFieldType::String));
    }

    #[test]
    fn write_flags_only_follow_existing_laboratory_contracts() {
        let registry = WidgetCapabilityRegistry::v7_4();
        assert!(capability(&registry, "item").flags.edit);
        assert!(capability(&registry, "svggraph").flags.edit);
        assert!(!capability(&registry, "clock").flags.edit);
        assert!(capability(&registry, "clock").flags.create);
        assert!(capability(&registry, "problems").flags.create);
        assert!(
            registry
                .capabilities()
                .iter()
                .all(|capability| capability.flags.geometry)
        );
        assert!(!capability(&registry, "graph").flags.create);
        assert!(!capability(&registry, "map").flags.edit);
    }
}
