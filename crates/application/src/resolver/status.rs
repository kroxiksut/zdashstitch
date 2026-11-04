// SPDX-License-Identifier: MIT OR Apache-2.0

//! Status vocabulary of the P1-IN-03 model: match and applicability are
//! independent axes, reasons and differences are stable machine codes.
//! The serde names equal `as_str` (snake_case); workcopy operations store
//! them (local-only format, P2-03A).

use serde::{Deserialize, Serialize};

/// How well a source was matched on the target (P1-IN-03).
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchStatus {
    /// Same `key_` (or same template graph name), same value type and units.
    Exact,
    /// Same `key_`, but units, value type or origin differ; see differences.
    Compatible,
    /// A mask or name pattern stands for several candidates.
    Ambiguous,
    /// The target has no object with that key or name; not an LLD instance.
    Missing,
    /// The source is an LLD instance and the target has no such instance.
    UnresolvedLld,
    /// The object is not visible: missing or hidden by permissions, which
    /// Zabbix does not tell apart (P1-IN-08). Shown as «not found or no
    /// access», never as a proven permission denial.
    Denied,
}

impl MatchStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Compatible => "compatible",
            Self::Ambiguous => "ambiguous",
            Self::Missing => "missing",
            Self::UnresolvedLld => "unresolved_lld",
            Self::Denied => "denied",
        }
    }

    /// `exact` and `compatible` name one concrete target.
    #[must_use]
    pub const fn is_resolved(self) -> bool {
        matches!(self, Self::Exact | Self::Compatible)
    }
}

/// Whether ZDashstitch may change the referencing field (P1-IN-03).
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Applicability {
    /// Widget type and field are writable for this operation (registry edit
    /// flag and O-003).
    Editable,
    /// The reference is understood, but this operation does not change it.
    ReadOnly,
    /// Unknown widget or field, or a write contract that is not proven yet.
    Unsupported,
}

impl Applicability {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Editable => "editable",
            Self::ReadOnly => "read_only",
            Self::Unsupported => "unsupported",
        }
    }
}

/// What differs between a source and its `compatible` target.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Difference {
    ValueType,
    Units,
    /// Template origin (`templateid`) or LLD prototype differs.
    Origin,
    /// Item patterns of a data set match another number of items.
    PatternCoverage,
}

impl Difference {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ValueType => "value_type",
            Self::Units => "units",
            Self::Origin => "origin",
            Self::PatternCoverage => "pattern_coverage",
        }
    }
}

/// Stable reason codes for the UI and the publication checks. They carry no
/// prose; the UI maps them to message IDs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    /// The object is missing or hidden by permissions (P1-IN-08).
    NotFoundOrNoAccess,
    /// No item with the same `key_` on the target host.
    KeyNotFound,
    /// LLD instance absent on the target; instances of the same prototype
    /// are offered as candidates, never chosen automatically.
    LldInstanceMissing,
    /// More than one target object matches; never chosen automatically.
    SeveralTargetCandidates,
    /// The value type changes class (numeric, text, binary) or the widget
    /// needs a numeric item. Blocks the change until decided.
    ValueTypeIncompatible,
    /// The value type differs within the same class.
    ValueTypeDiffers,
    UnitsDiffer,
    /// Template origin differs. For a classic graph it blocks the change
    /// (O-003: only the same-name graph of the same template replaces it);
    /// for an item it is informational.
    OriginDiffers,
    /// Item patterns of the data set match another number of items.
    PatternCoverageDiffers,
    /// Item patterns match items on the source host but none on the target.
    ItemPatternsWithoutTargetItems,
    /// The target visible name contains `*`; as a pattern it would be a mask.
    TargetNameIsMask,
    /// Host mask with `*` that matches the source host: kept as is (O-003).
    HostMask,
    /// A single `*`: every host. Kept as is (O-003).
    HostWildcardAll,
    /// Host field (`ZBX_WIDGET_FIELD_TYPE_HOST`): no operation replaces it.
    HostFieldNotSupported,
    /// Host pattern outside data sets (`problemhosts`, overrides): not
    /// changed by ReplaceHost.
    PatternFieldNotSupported,
    /// Classic graph replacement needs laboratory contract P2-04H
    /// (`classic_graph_graphid`), which the adapter does not support for
    /// this server version.
    ClassicGraphContractPending,
    /// No graph with the same name on the target host.
    GraphNameNotFound,
    /// The widget type has no write capability in the registry.
    WidgetNotEditable,
    /// The field is understood but O-003 does not change it.
    FieldNotReplaceable,
    /// Widget type unknown to the adapter registry.
    UnknownWidget,
    /// Field name or type unknown to the adapter registry.
    UnknownField,
    /// The prototype is missing or hidden by permissions
    /// (`itemprototype.get` / `graphprototype.get` return nothing), so its
    /// owner cannot be checked.
    PrototypeOwnerUnknown,
}

impl ReasonCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotFoundOrNoAccess => "not_found_or_no_access",
            Self::KeyNotFound => "key_not_found",
            Self::LldInstanceMissing => "lld_instance_missing",
            Self::SeveralTargetCandidates => "several_target_candidates",
            Self::ValueTypeIncompatible => "value_type_incompatible",
            Self::ValueTypeDiffers => "value_type_differs",
            Self::UnitsDiffer => "units_differ",
            Self::OriginDiffers => "origin_differs",
            Self::PatternCoverageDiffers => "pattern_coverage_differs",
            Self::ItemPatternsWithoutTargetItems => "item_patterns_without_target_items",
            Self::TargetNameIsMask => "target_name_is_mask",
            Self::HostMask => "host_mask",
            Self::HostWildcardAll => "host_wildcard_all",
            Self::HostFieldNotSupported => "host_field_not_supported",
            Self::PatternFieldNotSupported => "pattern_field_not_supported",
            Self::ClassicGraphContractPending => "classic_graph_contract_pending",
            Self::GraphNameNotFound => "graph_name_not_found",
            Self::WidgetNotEditable => "widget_not_editable",
            Self::FieldNotReplaceable => "field_not_replaceable",
            Self::UnknownWidget => "unknown_widget",
            Self::UnknownField => "unknown_field",
            Self::PrototypeOwnerUnknown => "prototype_owner_unknown",
        }
    }

    /// Reasons that keep a resolved row from being applied until the user
    /// decides (P1-05D/P1-08C).
    #[must_use]
    pub const fn blocks_change(self) -> bool {
        matches!(self, Self::ValueTypeIncompatible | Self::TargetNameIsMask)
    }

    /// [`Self::blocks_change`] for a row of `widget_type`: a classic graph
    /// whose same-name target comes from another template, or from none, is
    /// not replaced automatically (O-003, P2-02F).
    #[must_use]
    pub fn blocks_change_of(self, widget_type: &str) -> bool {
        self.blocks_change() || (widget_type == "graph" && self == Self::OriginDiffers)
    }
}
