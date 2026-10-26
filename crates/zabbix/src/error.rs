// SPDX-License-Identifier: MIT OR Apache-2.0

use thiserror::Error;

use crate::taxonomy::{Failure, FailureCode, chain_of, classify_api, classify_transport};
use crate::{EndpointRejection, TrustMaterialError};

pub type Result<T> = std::result::Result<T, ZabbixError>;

/// Adapter error. `Display` and `Debug` never contain secrets: tokens and
/// passwords are not part of any variant, and transport errors only carry the
/// API URL, which cannot contain credentials, a query or a fragment.
#[derive(Debug, Error)]
pub enum ZabbixError {
    #[error("invalid Zabbix API endpoint: {}", .0.as_str())]
    InvalidEndpoint(EndpointRejection),
    #[error("trusted CA material is invalid: {}", .0.as_str())]
    TrustMaterial(TrustMaterialError),
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
    #[error("Zabbix API response is not a JSON-RPC result or error")]
    InvalidResponseShape,
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
    /// Sign-in was rejected; the code is final and already attributed to the
    /// authentication step (P1-IN-01).
    #[error("Zabbix sign-in failed: {}", .0.code.as_str())]
    SignIn(Failure),
    #[error("authentication failed: {0}")]
    Authentication(String),
    #[error("invalid dashboard snapshot: {0}")]
    SnapshotShape(String),
    #[error("dashboard write contract rejected the operation: {0}")]
    WriteContract(String),
}

impl ZabbixError {
    /// Role permission errors only. `Not authorized.` and `Session
    /// terminated…` are authentication failures, not missing permissions.
    pub fn is_permission_denied(&self) -> bool {
        match self {
            Self::Api { message, data, .. } => {
                let text = format!("{message} {}", data.as_deref().unwrap_or_default())
                    .to_ascii_lowercase();
                text.contains("no permissions to call")
                    || text.contains("no permissions to referred object")
            }
            _ => false,
        }
    }

    /// Whether the credential or session is no longer accepted.
    pub fn is_authorization_failure(&self) -> bool {
        matches!(
            self.failure().code,
            FailureCode::AuthFailed
                | FailureCode::TokenNotAuthorized
                | FailureCode::TokenExpired
                | FailureCode::MfaSessionTerminated
                | FailureCode::SessionExpired
                | FailureCode::SystemAccessDenied
        )
    }

    /// Check step this error belongs to when it occurs while reaching the
    /// server: every transport-level failure, including a peer that does not
    /// speak HTTP, is an endpoint/TLS failure (P1-IN-01).
    #[must_use]
    pub fn check_step(&self) -> crate::CheckStep {
        match self {
            Self::InvalidEndpoint(_)
            | Self::TrustMaterial(_)
            | Self::Client(_)
            | Self::Transport(_) => crate::CheckStep::Endpoint,
            _ => self.failure().code.default_step(),
        }
    }

    /// Stable code and safe parameters for the application API.
    #[must_use]
    pub fn failure(&self) -> Failure {
        match self {
            Self::InvalidEndpoint(rejection) => {
                Failure::new(FailureCode::InvalidAddress).with("reason", rejection.as_str())
            }
            Self::TrustMaterial(error) => {
                Failure::new(FailureCode::TrustMaterialInvalid).with("reason", error.as_str())
            }
            Self::Client(_) => Failure::new(FailureCode::ConnectionFailed).with("stage", "client"),
            Self::Transport(error) => classify_transport(&chain_of(error), error.is_timeout()),
            Self::ResponseTooLarge { .. } => {
                Failure::new(FailureCode::UnexpectedResponse).with("stage", "http")
            }
            Self::HttpStatus(status) => {
                let code = if *status == reqwest::StatusCode::NOT_FOUND {
                    FailureCode::WrongBasePath
                } else if status.is_redirection() {
                    FailureCode::Redirect
                } else {
                    FailureCode::NotJsonrpc
                };
                Failure::new(code)
                    .with("http_status", status.as_u16().to_string())
                    .with("stage", "http")
            }
            Self::InvalidResponse(_) | Self::InvalidResponseShape | Self::ResponseIdMismatch => {
                Failure::new(FailureCode::NotJsonrpc).with("stage", "jsonrpc")
            }
            Self::Api {
                method,
                code,
                message,
                data,
            } => classify_api(method, *code, message, data.as_deref()),
            Self::UnsupportedVersion(version) => {
                Failure::new(FailureCode::UnsupportedVersion).with("version", version.clone())
            }
            Self::InvalidVersion(_) => {
                Failure::new(FailureCode::NotJsonrpc).with("stage", "version")
            }
            Self::SignIn(failure) => failure.clone(),
            Self::Authentication(_) | Self::SnapshotShape(_) | Self::WriteContract(_) => {
                Failure::new(FailureCode::UnexpectedResponse)
            }
        }
    }
}
