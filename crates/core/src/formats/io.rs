// SPDX-License-Identifier: MIT OR Apache-2.0

use serde::{Serialize, de::DeserializeOwned};

use super::{Result, Validate, strict};

/// Decode and validate a typed document from UTF-8 YAML held in memory.
///
/// Uses the strict loader: anchors/aliases, duplicate or non-string keys,
/// application tags, merge keys and excessive nesting are rejected, as in the
/// file and package readers.
pub fn from_yaml_str<T>(input: &str) -> Result<T>
where
    T: DeserializeOwned + Validate,
{
    let value: T = strict::yaml_from_str(input)?;
    value.validate()?;
    Ok(value)
}

/// Decode and validate a typed document from UTF-8 JSON held in memory.
///
/// Duplicate keys and excessive nesting are rejected instead of the
/// `serde_json` default of keeping the last value.
pub fn from_json_str<T>(input: &str) -> Result<T>
where
    T: DeserializeOwned + Validate,
{
    let value: T = strict::json_from_str(input)?;
    value.validate()?;
    Ok(value)
}

/// Validate and serialize a typed document as YAML.
pub fn to_yaml_string<T>(value: &T) -> Result<String>
where
    T: Serialize + Validate,
{
    value.validate()?;
    Ok(strict::yaml_to_string(value)?)
}

/// Validate and serialize a typed v1 document as pretty JSON with a final LF.
pub fn to_json_string<T>(value: &T) -> Result<String>
where
    T: Serialize + Validate,
{
    value.validate()?;
    let mut output = serde_json::to_string_pretty(value)?;
    output.push('\n');
    Ok(output)
}
