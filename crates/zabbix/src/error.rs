// SPDX-License-Identifier: MIT OR Apache-2.0

use thiserror::Error;

pub type Result<T> = std::result::Result<T, ZabbixError>;

#[derive(Debug, Error)]
pub enum ZabbixError {
    #[error("invalid Zabbix API endpoint: {0}")]
    InvalidEndpoint(String),
    #[error("could not construct the HTTP client")]
    Client(#[source] reqwest::Error),
    #[error("Zabbix API request failed")]
    Transport(#[source] reqwest::Error),
    #[error("Zabbix API response exceeded {limit} bytes")]
    ResponseTooLarge { limit: usize },
    #[error("Zabbix API returned HTTP status {0}")]
    HttpStatus(reqwest::StatusCode),
    #[error("Zabbix API response is not valid JSON-RPC")]
    InvalidResponse(#[source] serde_json::Error),
    #[error("Zabbix API response id does not match the request")]
    ResponseIdMismatch,
    #[error("Zabbix API {method} failed with code {code}: {message}")]
    Api {
        method: String,
        code: i64,
        message: String,
        data: Option<String>,
    },
    #[error("unsupported Zabbix version: {0}")]
    UnsupportedVersion(String),
    #[error("invalid Zabbix version: {0}")]
    InvalidVersion(String),
    #[error("authentication failed: {0}")]
    Authentication(String),
    #[error("invalid dashboard snapshot: {0}")]
    SnapshotShape(String),
    #[error("dashboard write contract rejected the operation: {0}")]
    WriteContract(String),
}

impl ZabbixError {
    pub fn is_permission_denied(&self) -> bool {
        match self {
            Self::Api { message, data, .. } => {
                let text = format!("{message} {}", data.as_deref().unwrap_or_default())
                    .to_ascii_lowercase();
                text.contains("permission")
                    || text.contains("not authorized")
                    || text.contains("not authorised")
            }
            _ => false,
        }
    }
}
