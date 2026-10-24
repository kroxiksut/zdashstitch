// SPDX-License-Identifier: MIT OR Apache-2.0

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use super::{FormatError, Result, Validate};
use crate::formats::validation::{
    ensure, ensure_len, ensure_slug, ensure_unique, validate_hex, validate_version,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MetricsFormat {
    #[serde(rename = "zdashstitch.metrics/v1")]
    V1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MetricCatalog {
    pub format: MetricsFormat,
    pub catalog_id: String,
    pub export_profile: ExportProfile,
    pub source: CatalogSource,
    pub diagnostics: Vec<Diagnostic>,
    pub metrics: Vec<Metric>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportProfile {
    PortableWithPublicKeys,
    RedactedAliases,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Diagnostic {
    MissingParentTemplate,
    UnresolvedMacros,
    LldRequiresInstance,
    SourcePartial,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogSource {
    pub kind: SourceKind,
    pub export_format_version: Option<String>,
    pub source_server_version: Option<String>,
    pub template_name: Option<String>,
    pub template_uuid: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    TemplateExport,
    ZabbixApi,
    Manual,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Metric {
    pub id: String,
    pub name: String,
    pub selector: MetricSelector,
    pub value_type: ValueType,
    pub units: String,
    pub origin: MetricOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Vec<Diagnostic>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MetricSelector {
    Key(String),
    Alias(String),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    NumericFloat,
    NumericUnsigned,
    Character,
    Text,
    Log,
    Binary,
}

impl ValueType {
    #[must_use]
    pub const fn is_numeric(self) -> bool {
        matches!(self, Self::NumericFloat | Self::NumericUnsigned)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricOrigin {
    TemplateItem,
    HostItem,
    LldPrototype,
    DiscoveredItem,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCatalogSource {
    kind: SourceKind,
    #[serde(default)]
    export_format_version: Option<String>,
    #[serde(default)]
    source_server_version: Option<String>,
    #[serde(default)]
    zabbix_version: Option<String>,
    #[serde(default)]
    template_name: Option<String>,
    #[serde(default)]
    template_uuid: Option<String>,
}

#[derive(Serialize)]
struct CurrentCatalogSource<'a> {
    kind: SourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    export_format_version: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_server_version: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    template_name: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    template_uuid: &'a Option<String>,
}

impl<'de> Deserialize<'de> for CatalogSource {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawCatalogSource::deserialize(deserializer)?;
        if raw.zabbix_version.is_some()
            && (raw.export_format_version.is_some() || raw.source_server_version.is_some())
        {
            return Err(de::Error::custom(
                "legacy zabbix_version cannot be mixed with explicit version fields",
            ));
        }

        let (export_format_version, source_server_version) = match raw.kind {
            SourceKind::TemplateExport => (
                raw.export_format_version.or(raw.zabbix_version),
                raw.source_server_version,
            ),
            SourceKind::ZabbixApi => {
                if raw.export_format_version.is_some() {
                    return Err(de::Error::custom(
                        "zabbix_api source cannot contain export_format_version",
                    ));
                }
                (None, raw.source_server_version.or(raw.zabbix_version))
            }
            SourceKind::Manual => {
                if raw.export_format_version.is_some() || raw.source_server_version.is_some() {
                    return Err(de::Error::custom(
                        "manual source cannot contain explicit version fields",
                    ));
                }
                (None, None)
            }
        };

        let value = Self {
            kind: raw.kind,
            export_format_version,
            source_server_version,
            template_name: raw.template_name,
            template_uuid: raw.template_uuid,
        };
        value.validate().map_err(de::Error::custom)?;
        Ok(value)
    }
}

impl Serialize for CatalogSource {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        CurrentCatalogSource {
            kind: self.kind,
            export_format_version: &self.export_format_version,
            source_server_version: &self.source_server_version,
            template_name: &self.template_name,
            template_uuid: &self.template_uuid,
        }
        .serialize(serializer)
    }
}

impl Validate for MetricCatalog {
    fn validate(&self) -> Result<()> {
        ensure_slug(&self.catalog_id, "catalog id")?;
        self.source.validate()?;
        ensure_unique(
            self.diagnostics.iter().copied(),
            "duplicate_diagnostic",
            "catalog diagnostic",
        )?;
        ensure(
            self.metrics.len() <= 20_000,
            "metric_count",
            "a catalog may contain at most 20000 metrics",
        )?;
        ensure_unique(
            self.metrics.iter().map(|metric| metric.id.as_str()),
            "duplicate_metric",
            "metric id",
        )?;
        for metric in &self.metrics {
            metric.validate()?;
            match (&self.export_profile, &metric.selector) {
                (ExportProfile::PortableWithPublicKeys, MetricSelector::Key(_))
                | (ExportProfile::RedactedAliases, MetricSelector::Alias(_)) => {}
                (ExportProfile::PortableWithPublicKeys, MetricSelector::Alias(_)) => {
                    return Err(FormatError::validation(
                        "selector_profile",
                        "portable_with_public_keys requires key selectors",
                    ));
                }
                (ExportProfile::RedactedAliases, MetricSelector::Key(_)) => {
                    return Err(FormatError::validation(
                        "selector_profile",
                        "redacted_aliases requires alias selectors",
                    ));
                }
            }
        }
        Ok(())
    }
}

impl Validate for CatalogSource {
    fn validate(&self) -> Result<()> {
        match self.kind {
            SourceKind::TemplateExport => ensure(
                self.export_format_version.is_some(),
                "source_version",
                "template_export source requires export_format_version",
            )?,
            SourceKind::ZabbixApi => {
                ensure(
                    self.export_format_version.is_none(),
                    "source_version",
                    "zabbix_api source cannot contain export_format_version",
                )?;
                ensure(
                    self.source_server_version.is_some(),
                    "source_version",
                    "zabbix_api source requires source_server_version",
                )?;
            }
            SourceKind::Manual => ensure(
                self.export_format_version.is_none() && self.source_server_version.is_none(),
                "source_version",
                "manual source cannot contain version fields",
            )?,
        }
        if let Some(version) = &self.export_format_version {
            validate_version(version, "export_format_version")?;
        }
        if let Some(version) = &self.source_server_version {
            validate_version(version, "source_server_version")?;
        }
        if let Some(name) = &self.template_name {
            ensure_len(name, 0, 200, "template_name", "template name")?;
        }
        if let Some(uuid) = &self.template_uuid {
            validate_hex(uuid, 32, false, "template_uuid")?;
        }
        Ok(())
    }
}

impl Validate for Metric {
    fn validate(&self) -> Result<()> {
        ensure_slug(&self.id, "metric id")?;
        ensure_len(&self.name, 1, 200, "metric_name", "metric name")?;
        self.selector.validate()?;
        ensure_len(&self.units, 0, 32, "metric_units", "metric units")?;
        if let Some(uuid) = &self.uuid {
            validate_hex(uuid, 32, false, "metric_uuid")?;
        }
        if let Some(diagnostics) = &self.diagnostics {
            ensure_unique(
                diagnostics.iter().copied(),
                "duplicate_diagnostic",
                "metric diagnostic",
            )?;
        }
        Ok(())
    }
}

impl Validate for MetricSelector {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Key(value) => ensure_len(value, 1, 512, "metric_key", "metric key"),
            Self::Alias(value) => ensure_slug(value, "metric alias"),
        }
    }
}
