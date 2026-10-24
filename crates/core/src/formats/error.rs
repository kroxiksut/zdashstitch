// SPDX-License-Identifier: MIT OR Apache-2.0

use thiserror::Error;

/// Errors produced while decoding, encoding, or validating a v1 document.
#[derive(Debug, Error)]
pub enum FormatError {
    #[error("invalid YAML document: {0}")]
    Yaml(#[from] serde_yaml_ng::Error),
    #[error("invalid JSON document: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{code}: {message}")]
    Validation { code: &'static str, message: String },
}

impl FormatError {
    #[must_use]
    pub fn validation(code: &'static str, message: impl Into<String>) -> Self {
        Self::Validation {
            code,
            message: message.into(),
        }
    }

    /// Stable machine-readable code for semantic errors.
    #[must_use]
    pub const fn code(&self) -> Option<&'static str> {
        match self {
            Self::Validation { code, .. } => Some(code),
            Self::Yaml(_) | Self::Json(_) => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, FormatError>;
