// SPDX-License-Identifier: MIT OR Apache-2.0

//! Strict document loader shared by every YAML and JSON entry point (P0-11, YAML-01).
//!
//! A document is first read into the JSON data model under the same rules as the
//! Python reference loader (`tools/validate.py`): one document, no anchors or
//! aliases, no duplicate or non-string mapping keys, no application tags or merge
//! keys, and at most [`MAX_DOCUMENT_DEPTH`] nested collections. Typed models are
//! then decoded from that value, so YAML and JSON input share one typed path.

use std::{cell::Cell, fmt};

use serde::{
    Serialize,
    de::{self, DeserializeOwned, DeserializeSeed, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Number, Value};
use serde_saphyr::{DuplicateKeyPolicy, MergeKeyPolicy, budget::BudgetBreach};
use thiserror::Error;

/// Maximum nesting of sequences and mappings, equal to the reference `MAX_DEPTH`.
pub const MAX_DOCUMENT_DEPTH: usize = 40;

/// Concrete text syntax of a document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentSyntax {
    Yaml,
    Json,
}

impl fmt::Display for DocumentSyntax {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Yaml => "YAML",
            Self::Json => "JSON",
        })
    }
}

/// Reason a document was rejected before or during typed decoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseErrorKind {
    /// Input bytes are not UTF-8.
    Utf8,
    /// A YAML anchor or alias is present.
    Alias,
    /// A mapping repeats a key.
    DuplicateKey,
    /// A mapping key is not a string.
    NonStringKey,
    /// Collections are nested deeper than [`MAX_DOCUMENT_DEPTH`].
    Depth,
    /// Syntax error, several documents, unsupported tag, merge key or another
    /// structural limit.
    Invalid,
    /// The document is well formed but does not match the typed model.
    Data,
}

/// Error returned by the strict loader. `code()` is the stable machine-readable
/// code shared with the Python reference loader.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("{message}")]
pub struct ParseError {
    syntax: DocumentSyntax,
    kind: ParseErrorKind,
    message: String,
    line: Option<u64>,
    column: Option<u64>,
}

impl ParseError {
    fn new(syntax: DocumentSyntax, kind: ParseErrorKind, message: impl Into<String>) -> Self {
        Self {
            syntax,
            kind,
            message: message.into(),
            line: None,
            column: None,
        }
    }

    #[must_use]
    pub const fn syntax(&self) -> DocumentSyntax {
        self.syntax
    }

    #[must_use]
    pub const fn kind(&self) -> ParseErrorKind {
        self.kind
    }

    /// One-based line of the offending YAML/JSON text, when known.
    #[must_use]
    pub const fn line(&self) -> Option<u64> {
        self.line
    }

    /// One-based column of the offending YAML/JSON text, when known.
    #[must_use]
    pub const fn column(&self) -> Option<u64> {
        self.column
    }

    #[must_use]
    pub const fn code(&self) -> &'static str {
        match (self.syntax, self.kind) {
            (_, ParseErrorKind::Utf8) => "utf8",
            (DocumentSyntax::Yaml, ParseErrorKind::Alias) => "yaml_alias",
            (DocumentSyntax::Yaml, ParseErrorKind::DuplicateKey) => "yaml_duplicate_key",
            (DocumentSyntax::Yaml, ParseErrorKind::NonStringKey) => "yaml_key",
            (DocumentSyntax::Yaml, ParseErrorKind::Depth) => "yaml_depth",
            (DocumentSyntax::Yaml, ParseErrorKind::Invalid) => "yaml_invalid",
            (DocumentSyntax::Yaml, ParseErrorKind::Data) => "yaml_data",
            (DocumentSyntax::Json, ParseErrorKind::Alias | ParseErrorKind::Invalid) => {
                "json_invalid"
            }
            (DocumentSyntax::Json, ParseErrorKind::DuplicateKey) => "json_duplicate_key",
            (DocumentSyntax::Json, ParseErrorKind::NonStringKey) => "json_key",
            (DocumentSyntax::Json, ParseErrorKind::Depth) => "json_depth",
            (DocumentSyntax::Json, ParseErrorKind::Data) => "json_data",
        }
    }
}

/// Read UTF-8 YAML bytes into the JSON data model under the strict rules.
pub fn yaml_value_from_slice(raw: &[u8]) -> Result<Value, ParseError> {
    let text = std::str::from_utf8(raw).map_err(|_| {
        ParseError::new(
            DocumentSyntax::Yaml,
            ParseErrorKind::Utf8,
            "document is not valid UTF-8",
        )
    })?;
    yaml_value_from_str(text)
}

/// Read YAML text into the JSON data model under the strict rules.
pub fn yaml_value_from_str(text: &str) -> Result<Value, ParseError> {
    let violation = Cell::new(None);
    let seed = StrictSeed {
        depth: 0,
        violation: &violation,
    };
    serde_saphyr::with_deserializer_from_str_with_options(text, yaml_options(), |deserializer| {
        seed.deserialize(deserializer)
    })
    .map_err(|error| yaml_error(&error, violation.get()))
}

/// Decode a typed document from UTF-8 YAML bytes under the strict rules.
pub fn yaml_from_slice<T: DeserializeOwned>(raw: &[u8]) -> Result<T, ParseError> {
    decode(DocumentSyntax::Yaml, yaml_value_from_slice(raw)?)
}

/// Decode a typed document from YAML text under the strict rules.
pub fn yaml_from_str<T: DeserializeOwned>(text: &str) -> Result<T, ParseError> {
    decode(DocumentSyntax::Yaml, yaml_value_from_str(text)?)
}

/// Read JSON text into the JSON data model, rejecting duplicate keys and deep
/// nesting that `serde_json` would otherwise accept or resolve silently.
pub fn json_value_from_str(text: &str) -> Result<Value, ParseError> {
    let violation = Cell::new(None);
    let seed = StrictSeed {
        depth: 0,
        violation: &violation,
    };
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let value = seed
        .deserialize(&mut deserializer)
        .and_then(|value| deserializer.end().map(|()| value))
        .map_err(|error| {
            let mut parsed = ParseError::new(
                DocumentSyntax::Json,
                violation.get().unwrap_or(ParseErrorKind::Invalid),
                error.to_string(),
            );
            if error.line() > 0 {
                parsed.line = u64::try_from(error.line()).ok();
                parsed.column = u64::try_from(error.column()).ok();
            }
            parsed
        })?;
    Ok(value)
}

/// Decode a typed document from JSON text under the strict rules.
pub fn json_from_str<T: DeserializeOwned>(text: &str) -> Result<T, ParseError> {
    decode(DocumentSyntax::Json, json_value_from_str(text)?)
}

/// Decode a typed model from a value produced by the strict loader.
pub fn decode<T: DeserializeOwned>(syntax: DocumentSyntax, value: Value) -> Result<T, ParseError> {
    serde_json::from_value(value)
        .map_err(|error| ParseError::new(syntax, ParseErrorKind::Data, error.to_string()))
}

/// Serialize a value as block YAML with fixed, deterministic options.
pub fn yaml_to_string<T: Serialize>(value: &T) -> Result<String, ParseError> {
    let options = serde_saphyr::ser_options! {
        indent_step: 2,
        compact_list_indent: true,
        empty_as_braces: true,
        prefer_block_scalars: false,
        tagged_enums: false,
        quote_all: false,
        yaml_12: false,
    };
    serde_saphyr::to_string_with_options(value, options).map_err(|error| {
        ParseError::new(
            DocumentSyntax::Yaml,
            ParseErrorKind::Invalid,
            error.to_string(),
        )
    })
}

fn yaml_options() -> serde_saphyr::Options {
    serde_saphyr::options! {
        budget: serde_saphyr::budget! {
            max_aliases: 0,
            max_anchors: 0,
            max_depth: MAX_DOCUMENT_DEPTH,
            max_documents: 1,
        },
        duplicate_keys: DuplicateKeyPolicy::Error,
        merge_keys: MergeKeyPolicy::Error,
        strict_booleans: true,
        legacy_octal_numbers: false,
        reject_unsupported_tags: true,
        with_snippet: false,
    }
}

fn yaml_error(error: &serde_saphyr::Error, violation: Option<ParseErrorKind>) -> ParseError {
    let inner = error.without_snippet();
    let kind = violation.unwrap_or_else(|| classify_yaml(inner));
    let mut parsed = ParseError::new(DocumentSyntax::Yaml, kind, inner.to_string());
    if let Some(location) = inner.location()
        && location.line() > 0
    {
        parsed.line = Some(location.line());
        parsed.column = Some(location.column());
    }
    parsed
}

fn classify_yaml(error: &serde_saphyr::Error) -> ParseErrorKind {
    match error {
        serde_saphyr::Error::DuplicateMappingKey { .. } => ParseErrorKind::DuplicateKey,
        serde_saphyr::Error::UnknownAnchor { .. } | serde_saphyr::Error::AliasError { .. } => {
            ParseErrorKind::Alias
        }
        serde_saphyr::Error::Budget { breach, .. } => match breach {
            BudgetBreach::Aliases { .. }
            | BudgetBreach::Anchors { .. }
            | BudgetBreach::AliasAnchorRatio { .. }
            | BudgetBreach::RecordedAnchorEvents { .. }
            | BudgetBreach::RecordedAnchorBytes { .. } => ParseErrorKind::Alias,
            BudgetBreach::Depth { .. } => ParseErrorKind::Depth,
            _ => ParseErrorKind::Invalid,
        },
        _ => ParseErrorKind::Invalid,
    }
}

/// Builds a `serde_json::Value` and records which strict rule failed.
#[derive(Clone, Copy)]
struct StrictSeed<'a> {
    depth: usize,
    violation: &'a Cell<Option<ParseErrorKind>>,
}

impl StrictSeed<'_> {
    fn fail<E: de::Error>(self, kind: ParseErrorKind, message: impl fmt::Display) -> E {
        self.violation.set(Some(kind));
        E::custom(message)
    }

    fn nested<E: de::Error>(self) -> Result<Self, E> {
        let depth = self.depth + 1;
        if depth > MAX_DOCUMENT_DEPTH {
            return Err(self.fail(ParseErrorKind::Depth, "document nesting is too deep"));
        }
        Ok(Self { depth, ..self })
    }
}

impl<'de> DeserializeSeed<'de> for StrictSeed<'_> {
    type Value = Value;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for StrictSeed<'_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON-compatible document value")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| self.fail(ParseErrorKind::Invalid, "non-finite number"))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_bytes<E: de::Error>(self, _value: &[u8]) -> Result<Value, E> {
        Err(self.fail(ParseErrorKind::Invalid, "binary values are not supported"))
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        let item = self.nested()?;
        let mut items = Vec::new();
        while let Some(value) = sequence.next_element_seed(item)? {
            items.push(value);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut mapping: A) -> Result<Value, A::Error> {
        let entry = self.nested()?;
        let mut entries = Map::new();
        while let Some(key) = mapping.next_key_seed(KeySeed(self.violation))? {
            if entries.contains_key(&key) {
                return Err(self.fail(ParseErrorKind::DuplicateKey, "duplicate mapping key"));
            }
            let value = mapping.next_value_seed(entry)?;
            entries.insert(key, value);
        }
        Ok(Value::Object(entries))
    }
}

/// Accepts only string mapping keys, as the reference loader does.
struct KeySeed<'a>(&'a Cell<Option<ParseErrorKind>>);

impl KeySeed<'_> {
    fn reject<E: de::Error>(self) -> E {
        self.0.set(Some(ParseErrorKind::NonStringKey));
        E::custom("mapping keys must be strings")
    }
}

impl<'de> DeserializeSeed<'de> for KeySeed<'_> {
    type Value = String;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<String, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for KeySeed<'_> {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a string mapping key")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<String, E> {
        Ok(value.to_owned())
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<String, E> {
        Ok(value)
    }

    fn visit_bool<E: de::Error>(self, _value: bool) -> Result<String, E> {
        Err(self.reject())
    }

    fn visit_i64<E: de::Error>(self, _value: i64) -> Result<String, E> {
        Err(self.reject())
    }

    fn visit_u64<E: de::Error>(self, _value: u64) -> Result<String, E> {
        Err(self.reject())
    }

    fn visit_f64<E: de::Error>(self, _value: f64) -> Result<String, E> {
        Err(self.reject())
    }

    fn visit_bytes<E: de::Error>(self, _value: &[u8]) -> Result<String, E> {
        Err(self.reject())
    }

    fn visit_unit<E: de::Error>(self) -> Result<String, E> {
        Err(self.reject())
    }

    fn visit_none<E: de::Error>(self) -> Result<String, E> {
        Err(self.reject())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, _sequence: A) -> Result<String, A::Error> {
        Err(self.reject())
    }

    fn visit_map<A: MapAccess<'de>>(self, _mapping: A) -> Result<String, A::Error> {
        Err(self.reject())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nested(depth: usize) -> String {
        let mut text = String::from("1");
        for _ in 0..depth {
            text = format!("{{a: {text}}}");
        }
        text
    }

    fn yaml_code(text: &str) -> &'static str {
        yaml_value_from_str(text)
            .err()
            .unwrap_or_else(|| panic!("{text:?} must be rejected"))
            .code()
    }

    fn json_code(text: &str) -> &'static str {
        json_value_from_str(text)
            .err()
            .unwrap_or_else(|| panic!("{text:?} must be rejected"))
            .code()
    }

    #[test]
    fn yaml_rules_match_the_reference_loader() {
        assert_eq!(yaml_code("a: &x [1]\nb: *x\n"), "yaml_alias");
        assert_eq!(yaml_code("a: &x 1\n"), "yaml_alias");
        assert_eq!(yaml_code("a: *x\n"), "yaml_alias");
        assert_eq!(yaml_code("format: a\nformat: b\n"), "yaml_duplicate_key");
        assert_eq!(yaml_code("r:\n  x: 1\n  x: 2\n"), "yaml_duplicate_key");
        assert_eq!(yaml_code("1: a\n"), "yaml_key");
        assert_eq!(yaml_code("~: a\n"), "yaml_key");
        assert_eq!(yaml_code("[a]: b\n"), "yaml_key");
        assert_eq!(yaml_code(&nested(MAX_DOCUMENT_DEPTH + 1)), "yaml_depth");
        assert_eq!(yaml_code("a: !custom b\n"), "yaml_invalid");
        assert_eq!(yaml_code("a: 1\n<<: {b: 2}\n"), "yaml_invalid");
        assert_eq!(yaml_code("a: 1\n---\nb: 2\n"), "yaml_invalid");
        assert_eq!(yaml_code("a: .nan\n"), "yaml_invalid");
        assert_eq!(yaml_code("format: broken: yaml"), "yaml_invalid");
        assert_eq!(
            yaml_value_from_slice(b"a: \xff\n")
                .err()
                .map(|error| error.code()),
            Some("utf8")
        );
    }

    #[test]
    fn yaml_accepts_plain_documents_at_the_depth_limit() {
        let value = yaml_value_from_str(&nested(MAX_DOCUMENT_DEPTH))
            .unwrap_or_else(|error| panic!("depth limit must pass: {error}"));
        assert!(value.is_object());
        let value = yaml_value_from_str("a: Tom &Jerry\nb: x*y\nc: !!str 123\nd: yes\n")
            .unwrap_or_else(|error| panic!("plain scalars must pass: {error}"));
        assert_eq!(
            value,
            serde_json::json!({"a": "Tom &Jerry", "b": "x*y", "c": "123", "d": "yes"})
        );
    }

    #[test]
    fn yaml_errors_keep_a_location() {
        let error = yaml_value_from_str("a: 1\na: 2\n")
            .err()
            .unwrap_or_else(|| panic!("duplicate must fail"));
        assert_eq!((error.line(), error.column()), (Some(2), Some(1)));
    }

    #[test]
    fn json_rejects_duplicate_keys_and_deep_nesting() {
        assert_eq!(json_code(r#"{"a":1,"a":2}"#), "json_duplicate_key");
        assert_eq!(json_code(r#"{"a":{"b":1,"b":1}}"#), "json_duplicate_key");
        let deep = format!(
            "{}1{}",
            "[".repeat(MAX_DOCUMENT_DEPTH + 1),
            "]".repeat(MAX_DOCUMENT_DEPTH + 1)
        );
        assert_eq!(json_code(&deep), "json_depth");
        assert_eq!(json_code(r#"{"a":1} {"b":2}"#), "json_invalid");
        let limit = format!(
            "{}1{}",
            "[".repeat(MAX_DOCUMENT_DEPTH),
            "]".repeat(MAX_DOCUMENT_DEPTH)
        );
        assert!(json_value_from_str(&limit).is_ok());
    }
}
