// SPDX-License-Identifier: MIT OR Apache-2.0

//! Connection profile records (P2-01H). A profile never contains a secret:
//! only an opaque reference to the system credential store, and a reference
//! to locally stored CA material.

use serde::{Deserialize, Serialize};
use zdashstitch_zabbix::{AuthMethod, ZabbixEndpoint};

use super::ConnectionError;

pub(crate) const MAX_NAME_CHARS: usize = 128;
pub(crate) const MAX_USERNAME_CHARS: usize = 255;

/// How the secret of an API-token profile is kept.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretStorage {
    /// Only in process memory for the current session.
    None,
    /// In the operating-system credential store, by the user's choice.
    OsStore,
}

impl SecretStorage {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::OsStore => "os_store",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProfileAuth {
    ApiToken {
        secret_storage: SecretStorage,
    },
    /// The password is never stored; only the user name is.
    Password {
        username: String,
    },
}

impl ProfileAuth {
    #[must_use]
    pub const fn method(&self) -> AuthMethod {
        match self {
            Self::ApiToken { .. } => AuthMethod::ApiToken,
            Self::Password { .. } => AuthMethod::Password,
        }
    }

    #[must_use]
    pub const fn secret_storage(&self) -> SecretStorage {
        match self {
            Self::ApiToken { secret_storage } => *secret_storage,
            Self::Password { .. } => SecretStorage::None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TrustSetting {
    /// Operating-system certificate store (default).
    System,
    /// System store plus a CA bundle stored by the application.
    CustomCa { ca_ref: String },
}

impl TrustSetting {
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::CustomCa { .. } => "custom_ca",
        }
    }
}

/// Mode right after connecting. P1-02E: always read-only; writes are
/// enabled separately and only until disconnect or restart.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModeOnConnect {
    #[default]
    ReadOnly,
}

/// Identity confirmed for this profile on an earlier connection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StoredIdentity {
    pub endpoint_fingerprint: String,
    pub server_fingerprint: String,
    pub server_version: String,
    pub verified_at: u64,
}

/// Outcome of the last connection check. `result_code` is `None` on success.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LastCheck {
    pub at: u64,
    pub server_version: Option<String>,
    pub result_code: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionProfile {
    pub connection_id: String,
    pub name: String,
    /// Normalized base URL of the Zabbix front end.
    pub base_url: String,
    pub auth: ProfileAuth,
    pub mode_on_connect: ModeOnConnect,
    pub trust: TrustSetting,
    /// Opaque name of the credential store entry. It is not a secret and
    /// does not reveal the server or account.
    pub credential_ref: String,
    /// Incremented whenever the endpoint, account or trust material changes:
    /// bindings, snapshots and publication plans of an older generation must
    /// be checked again.
    pub context_generation: u64,
    pub identity: Option<StoredIdentity>,
    pub last_check: Option<LastCheck>,
}

impl ConnectionProfile {
    pub fn endpoint(&self) -> Result<ZabbixEndpoint, ConnectionError> {
        ZabbixEndpoint::parse(&self.base_url).map_err(ConnectionError::from_zabbix)
    }

    pub(crate) fn validate(&self) -> Result<(), ConnectionError> {
        if !is_prefixed_hex(&self.connection_id, "conn-")
            || !is_prefixed_hex(&self.credential_ref, "cred-")
        {
            return Err(ConnectionError::new("profile_store_invalid"));
        }
        validate_name(&self.name)?;
        let endpoint = self.endpoint()?;
        if endpoint.base_url().as_str() != self.base_url {
            return Err(ConnectionError::new("profile_store_invalid"));
        }
        if let ProfileAuth::Password { username } = &self.auth {
            validate_username(username)?;
        }
        if let TrustSetting::CustomCa { ca_ref } = &self.trust
            && !is_ca_reference(ca_ref)
        {
            return Err(ConnectionError::new("profile_store_invalid"));
        }
        Ok(())
    }
}

pub(crate) fn validate_name(name: &str) -> Result<(), ConnectionError> {
    let length = name.chars().count();
    if name.trim().is_empty() || length > MAX_NAME_CHARS || name.chars().any(char::is_control) {
        return Err(ConnectionError::new("profile_name_invalid")
            .with("max_chars", MAX_NAME_CHARS.to_string()));
    }
    Ok(())
}

pub(crate) fn validate_username(username: &str) -> Result<(), ConnectionError> {
    let length = username.chars().count();
    if username.is_empty() || length > MAX_USERNAME_CHARS || username.chars().any(char::is_control)
    {
        return Err(ConnectionError::new("username_invalid")
            .with("max_chars", MAX_USERNAME_CHARS.to_string()));
    }
    Ok(())
}

pub(crate) fn is_ca_reference(value: &str) -> bool {
    value.strip_prefix("sha256-").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}

fn is_prefixed_hex(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|hex| {
        hex.len() == 32
            && hex
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}

/// Requested authentication of a profile in `profile.create`/`update`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthInput {
    ApiToken { store_token: bool },
    Password { username: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrustInput {
    System,
    CustomCa { ca_ref: String },
}

/// Profile fields entered by the user. Contains no secret.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileInput {
    pub name: String,
    /// Address as typed, e.g. `https://zabbix.example.invalid:8443/zabbix`.
    pub address: String,
    pub auth: AuthInput,
    pub trust: TrustInput,
    /// Explicit confirmation that a token may be stored for a plain `http`
    /// endpoint, where it travels in clear text (P1-02B).
    pub confirm_plain_http_token_storage: bool,
}

/// Non-secret view of a profile for the interface (P1-02H).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileView {
    pub local_id: String,
    pub name: String,
    pub endpoint_display: String,
    pub api_url_display: String,
    pub plain_http: bool,
    pub auth_method: AuthMethod,
    pub secret_storage: SecretStorage,
    pub username: Option<String>,
    pub trust: &'static str,
    pub ca_fingerprints: Vec<String>,
    pub last_check: Option<LastCheck>,
    pub state: ConnectionState,
    pub mode: Option<zdashstitch_zabbix::AccessMode>,
    pub context_generation: u64,
}

/// Connection state axis of P1-01C as shown on a profile card.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    NotConnected,
    Connected,
    LastCheckFailed,
    NeedsSignIn,
}

impl ConnectionState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotConnected => "not_connected",
            Self::Connected => "connected",
            Self::LastCheckFailed => "last_check_failed",
            Self::NeedsSignIn => "needs_sign_in",
        }
    }
}
