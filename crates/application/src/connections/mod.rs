// SPDX-License-Identifier: MIT OR Apache-2.0

//! Connections (P2-01): profiles, sign-in, trust material, credential store,
//! access-mode gateways and the step-by-step check. The API returns stable
//! codes and safe parameters only, never prose, and never a secret.

mod check;
mod credentials;
mod profile;
mod repository;
mod service;

use std::fmt;

use zdashstitch_zabbix::{FailureParam, ZabbixError};

pub use check::{ConnectionCheckReport, IdentityMatch, StepReport, StepStatus};
pub use credentials::{
    CREDENTIAL_SERVICE, CredentialState, CredentialStore, MemoryCredentialStore,
    SystemCredentialStore,
};
pub use profile::{
    AuthInput, ConnectionProfile, ConnectionState, LastCheck, ModeOnConnect, ProfileAuth,
    ProfileInput, ProfileView, SecretStorage, StoredIdentity, TrustInput, TrustSetting,
};
pub use repository::{ImportedTrust, ProfileRepository, TrustStore};
/// Secret wrapper of [`ConnectRequest::secret`], re-exported so the HTTP
/// boundary can wrap a secret right after parsing without its own dependency.
pub use secrecy::SecretString;
pub use service::{
    ClosedSession, ConnectOutcome, ConnectRequest, ConnectionService, DeleteOutcome,
    DisconnectOutcome, ProfileUpdate,
};

impl ConnectionService {
    /// Service over the platform configuration directory, the directory of
    /// the settings file (P2-01H): `connections.yaml` and `trusted-ca/`.
    pub fn platform_default(
        credentials: Box<dyn CredentialStore>,
    ) -> Result<Self, ConnectionError> {
        let (configuration, _) = crate::settings::platform_paths(|name| std::env::var_os(name))
            .map_err(|_| ConnectionError::new("profile_store_unavailable"))?;
        Ok(Self::new(configuration, credentials))
    }
}

/// Failure of a connection use case outside the network check itself: a
/// code plus safe parameters. `Display` shows only the code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionError {
    code: &'static str,
    params: Vec<FailureParam>,
}

impl ConnectionError {
    pub(crate) fn new(code: &'static str) -> Self {
        Self {
            code,
            params: Vec::new(),
        }
    }

    pub(crate) fn with(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.params.push(FailureParam {
            name,
            value: value.into(),
        });
        self
    }

    pub(crate) fn from_zabbix(error: ZabbixError) -> Self {
        let failure = error.failure();
        Self {
            code: failure.code.as_str(),
            params: failure.params,
        }
    }

    pub(crate) fn from_credential(state: CredentialState) -> Self {
        Self::new(state.code())
    }

    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    #[must_use]
    pub fn params(&self) -> &[FailureParam] {
        &self.params
    }

    #[must_use]
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|param| param.name == name)
            .map(|param| param.value.as_str())
    }
}

impl fmt::Display for ConnectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "connection operation failed: {}", self.code)
    }
}

impl std::error::Error for ConnectionError {}
