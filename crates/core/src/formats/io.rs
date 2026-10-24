// SPDX-License-Identifier: MIT OR Apache-2.0

use serde::{Serialize, de::DeserializeOwned};

use super::{Result, Validate};

/// Decode and validate a typed v1 document from UTF-8 YAML held in memory.
pub fn from_yaml_str<T>(input: &str) -> Result<T>
where
    T: DeserializeOwned + Validate,
{
    let value: T = serde_yaml_ng::from_str(input)?;
    value.validate()?;
    Ok(value)
}

/// Decode and validate a typed v1 document from UTF-8 JSON held in memory.
pub fn from_json_str<T>(input: &str) -> Result<T>
where
    T: DeserializeOwned + Validate,
{
    let value: T = serde_json::from_str(input)?;
    value.validate()?;
    Ok(value)
}

/// Validate and serialize a typed v1 document as YAML.
pub fn to_yaml_string<T>(value: &T) -> Result<String>
where
    T: Serialize + Validate,
{
    value.validate()?;
    Ok(serde_yaml_ng::to_string(value)?)
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
