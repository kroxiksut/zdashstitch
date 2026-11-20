// SPDX-License-Identifier: MIT OR Apache-2.0

use serde_json::Value;
use zdashstitch_core::formats::{Metric, MetricsFormat, ValueType};

use crate::ImportError;

#[derive(Clone, Copy)]
pub(super) enum TemplateExportProfile {
    V7_4,
    V8_0,
}

impl TemplateExportProfile {
    pub(super) fn for_version(version: &str) -> Result<Self, ImportError> {
        match version {
            "7.4" => Ok(Self::V7_4),
            "8.0" => Ok(Self::V8_0),
            _ => Err(ImportError::new("template_export_version")),
        }
    }

    pub(super) fn value_type(self, input: Option<&Value>) -> Result<ValueType, ImportError> {
        let invalid = || ImportError::new("template_export_value_type");
        let number;
        let value = match input {
            None => return Ok(ValueType::NumericUnsigned),
            Some(Value::String(text)) => text.as_str(),
            Some(Value::Number(value)) => {
                number = value.to_string();
                number.as_str()
            }
            _ => return Err(invalid()),
        };
        match value {
            "FLOAT" | "0" => Ok(ValueType::NumericFloat),
            "CHAR" | "1" => Ok(ValueType::Character),
            "LOG" | "2" => Ok(ValueType::Log),
            "UNSIGNED" | "3" => Ok(ValueType::NumericUnsigned),
            "TEXT" | "4" => Ok(ValueType::Text),
            "BINARY" | "5" => Ok(ValueType::Binary),
            "JSON" | "6" if matches!(self, Self::V8_0) => Ok(ValueType::Json),
            _ => Err(invalid()),
        }
    }
}

pub(super) fn catalog_format(metrics: &[Metric]) -> MetricsFormat {
    if metrics
        .iter()
        .any(|metric| metric.value_type == ValueType::Json)
    {
        MetricsFormat::V2
    } else {
        MetricsFormat::V1
    }
}
