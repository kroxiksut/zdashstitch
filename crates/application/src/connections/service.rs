// SPDX-License-Identifier: MIT OR Apache-2.0

//! Connection use cases (P1-02H commands): profile CRUD, check, connect,
//! disconnect, access mode and token storage. At most one connection is
//! active; it always starts read-only (P1-02E).

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use secrecy::ExposeSecret;
use secrecy::SecretString;
use zdashstitch_zabbix::{
    AccessMode, Authentication, CheckStep, FailureParam, ReadOnlyGateway, ReadWriteGateway,
    TrustAnchors, ZabbixEndpoint,
};

use super::ConnectionError;
use super::check::{ConnectionCheckReport, IdentityMatch, run_check};
use super::credentials::{CredentialState, CredentialStore};
use super::profile::{
    AuthInput, ConnectionProfile, ConnectionState, LastCheck, ModeOnConnect, ProfileAuth,
    ProfileInput, ProfileView, SecretStorage, StoredIdentity, TrustInput, TrustSetting,
    validate_name, validate_username,
};
use super::repository::{ImportedTrust, MAX_PROFILES, ProfileRepository, TrustStore};

/// Secret and choices sent once with `connection.connect`. `Debug` hides
/// the secret.
#[derive(Default)]
pub struct ConnectRequest {
    /// Password, or API token when none is stored.
    pub secret: Option<SecretString>,
    /// Explicit confirmation that a different server at this address is
    /// the intended one (P1-02G).
    pub accept_new_identity: bool,
}

impl fmt::Debug for ConnectRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectRequest")
            .field("secret", &self.secret.as_ref().map(|_| "[REDACTED]"))
            .field("accept_new_identity", &self.accept_new_identity)
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectOutcome {
    pub report: ConnectionCheckReport,
    pub connected: bool,
    /// The profile context generation was raised (new server identity).
    pub context_changed: bool,
    /// Code of a failed token save after a successful connection.
    pub credential_save: Option<&'static str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileUpdate {
    pub view: ProfileView,
    /// Event `connection.context_changed`: bindings, snapshots and plans of
    /// this connection must be checked again.
    pub context_changed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeleteOutcome {
    /// `None` when the local token entry was removed or absent. The token
    /// is never revoked on the server.
    pub local_credential: Option<&'static str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisconnectOutcome {
    pub connection_id: Option<String>,
    /// Failure code of `user.logout`, if any; the session is dropped anyway.
    pub logout: Option<&'static str>,
}

enum ActiveGateway {
    ReadOnly(ReadOnlyGateway),
    ReadWrite(ReadWriteGateway),
    /// Transient placeholder while the mode is being switched.
    Switching,
}

struct ActiveConnection {
    connection_id: String,
    context_generation: u64,
    gateway: ActiveGateway,
}

pub struct ConnectionService {
    profiles: ProfileRepository,
    trust: TrustStore,
    credentials: Box<dyn CredentialStore>,
    active: Option<ActiveConnection>,
    sign_in_failures: HashMap<String, u32>,
}

impl fmt::Debug for ConnectionService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectionService")
            .field(
                "active",
                &self.active.as_ref().map(|active| &active.connection_id),
            )
            .finish_non_exhaustive()
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn random_id(prefix: &str) -> Result<String, ConnectionError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| ConnectionError::new("random_source_unavailable"))?;
    let mut id = String::with_capacity(prefix.len() + 32);
    id.push_str(prefix);
    for byte in bytes {
        id.push_str(&format!("{byte:02x}"));
    }
    Ok(id)
}

const SIGN_IN_CODES: [&str; 5] = [
    "token_not_authorized",
    "token_expired",
    "session_expired",
    "mfa_session_terminated",
    "auth_failed",
];

impl ConnectionService {
    /// Service over the application configuration directory.
    #[must_use]
    pub fn new(configuration_directory: PathBuf, credentials: Box<dyn CredentialStore>) -> Self {
        Self {
            profiles: ProfileRepository::at(configuration_directory.clone()),
            trust: TrustStore::at(configuration_directory),
            credentials,
            active: None,
            sign_in_failures: HashMap::new(),
        }
    }

    pub fn list_profiles(&self) -> Result<Vec<ProfileView>, ConnectionError> {
        self.profiles
            .load()?
            .iter()
            .map(|profile| self.view(profile))
            .collect()
    }

    pub fn profile(&self, connection_id: &str) -> Result<ProfileView, ConnectionError> {
        let profiles = self.profiles.load()?;
        self.view(find(&profiles, connection_id)?)
    }

    /// Checks an address while it is typed: normalized base URL and the API
    /// URL computed from it (P1-02B).
    pub fn preview_address(address: &str) -> Result<(String, String, bool), ConnectionError> {
        let endpoint = ZabbixEndpoint::parse(address).map_err(ConnectionError::from_zabbix)?;
        Ok((
            endpoint.base_url().to_string(),
            endpoint.api_url().to_string(),
            endpoint.is_plain_http(),
        ))
    }

    pub fn import_trusted_ca(&self, pem: &[u8]) -> Result<ImportedTrust, ConnectionError> {
        self.trust.import(pem)
    }

    pub fn create_profile(&mut self, input: &ProfileInput) -> Result<ProfileView, ConnectionError> {
        let mut profiles = self.profiles.load()?;
        if profiles.len() >= MAX_PROFILES {
            return Err(
                ConnectionError::new("profile_limit_reached").with("max", MAX_PROFILES.to_string())
            );
        }
        let (base_url, auth, trust) = self.validated_input(input)?;
        let profile = ConnectionProfile {
            connection_id: random_id("conn-")?,
            name: input.name.trim().to_owned(),
            base_url,
            auth,
            mode_on_connect: ModeOnConnect::ReadOnly,
            trust,
            credential_ref: random_id("cred-")?,
            context_generation: 0,
            identity: None,
            last_check: None,
        };
        profiles.push(profile.clone());
        self.profiles.save(&profiles)?;
        self.view(&profile)
    }

    /// Renaming keeps the identity. Changing the address, account, auth
    /// method or trust material is a context change: the stored identity is
    /// cleared, the generation is raised, an active session of this profile
    /// is closed and a stored token for the old server is removed.
    pub async fn update_profile(
        &mut self,
        connection_id: &str,
        input: &ProfileInput,
    ) -> Result<ProfileUpdate, ConnectionError> {
        let mut profiles = self.profiles.load()?;
        let (base_url, auth, trust) = self.validated_input(input)?;
        let current = find(&profiles, connection_id)?.clone();
        let context_changed = current.base_url != base_url
            || current.auth.method() != auth.method()
            || current.auth != auth && matches!(auth, ProfileAuth::Password { .. })
            || current.trust != trust;
        let token_must_go = context_changed
            || (current.auth.secret_storage() == SecretStorage::OsStore
                && auth.secret_storage() == SecretStorage::None);
        if token_must_go && current.auth.secret_storage() == SecretStorage::OsStore {
            self.credentials
                .delete(&current.credential_ref)
                .map_err(ConnectionError::from_credential)?;
        }
        if context_changed && self.is_active(connection_id) {
            let _ = self.disconnect().await;
        }
        let profile = profiles
            .iter_mut()
            .find(|profile| profile.connection_id == connection_id)
            .ok_or_else(|| ConnectionError::new("profile_not_found"))?;
        profile.name = input.name.trim().to_owned();
        profile.base_url = base_url;
        profile.auth = auth;
        profile.trust = trust;
        if context_changed {
            profile.context_generation = profile.context_generation.saturating_add(1);
            profile.identity = None;
            profile.last_check = None;
        }
        let updated = profile.clone();
        self.profiles.save(&profiles)?;
        Ok(ProfileUpdate {
            view: self.view(&updated)?,
            context_changed,
        })
    }

    /// Copies address, auth method, user name and trust reference. Neither
    /// a stored token nor the identity is copied.
    pub fn duplicate_profile(
        &mut self,
        connection_id: &str,
        name: &str,
    ) -> Result<ProfileView, ConnectionError> {
        validate_name(name)?;
        let mut profiles = self.profiles.load()?;
        if profiles.len() >= MAX_PROFILES {
            return Err(
                ConnectionError::new("profile_limit_reached").with("max", MAX_PROFILES.to_string())
            );
        }
        let source = find(&profiles, connection_id)?;
        let copy = ConnectionProfile {
            connection_id: random_id("conn-")?,
            name: name.trim().to_owned(),
            base_url: source.base_url.clone(),
            auth: source.auth.clone(),
            mode_on_connect: ModeOnConnect::ReadOnly,
            trust: source.trust.clone(),
            credential_ref: random_id("cred-")?,
            context_generation: 0,
            identity: None,
            last_check: None,
        };
        profiles.push(copy.clone());
        self.profiles.save(&profiles)?;
        self.view(&copy)
    }

    /// Disconnects first when needed. The local token entry is removed; the
    /// token itself stays valid on the server until revoked there.
    pub async fn delete_profile(
        &mut self,
        connection_id: &str,
    ) -> Result<DeleteOutcome, ConnectionError> {
        let mut profiles = self.profiles.load()?;
        let profile = find(&profiles, connection_id)?.clone();
        if self.is_active(connection_id) {
            let _ = self.disconnect().await;
        }
        let local_credential = self
            .credentials
            .delete(&profile.credential_ref)
            .err()
            .map(CredentialState::code);
        profiles.retain(|candidate| candidate.connection_id != connection_id);
        self.profiles.save(&profiles)?;
        self.sign_in_failures.remove(connection_id);
        Ok(DeleteOutcome { local_credential })
    }

    /// `credential.forget`: removes the stored token from this computer.
    pub fn forget_token(&mut self, connection_id: &str) -> Result<(), ConnectionError> {
        let profiles = self.profiles.load()?;
        let profile = find(&profiles, connection_id)?;
        self.credentials
            .delete(&profile.credential_ref)
            .map_err(ConnectionError::from_credential)
    }

    /// `connection.test`: all five steps; the session is closed afterwards.
    pub async fn test_connection(
        &mut self,
        connection_id: &str,
        secret: Option<SecretString>,
    ) -> Result<ConnectionCheckReport, ConnectionError> {
        let profile = find(&self.profiles.load()?, connection_id)?.clone();
        let (endpoint, trust, authentication) = self.prepare(&profile, secret)?;
        let outcome = run_check(
            endpoint,
            &trust,
            authentication,
            profile.identity.as_ref(),
            false,
            now(),
        )
        .await;
        if let Some(gateway) = outcome.gateway {
            let _ = gateway.logout().await;
        }
        let mut report = outcome.report;
        self.count_sign_in_failures(connection_id, &mut report);
        self.record_last_check(connection_id, &report, None)?;
        Ok(report)
    }

    /// `connection.connect`: runs the check, keeps the session read-only on
    /// success and replaces any other active connection.
    pub async fn connect(
        &mut self,
        connection_id: &str,
        request: ConnectRequest,
    ) -> Result<ConnectOutcome, ConnectionError> {
        let profile = find(&self.profiles.load()?, connection_id)?.clone();
        let ConnectRequest {
            secret,
            accept_new_identity,
        } = request;
        let save_token = match (&profile.auth, &secret) {
            (
                ProfileAuth::ApiToken {
                    secret_storage: SecretStorage::OsStore,
                },
                Some(secret),
            ) => Some(SecretString::from(secret.expose_secret().to_owned())),
            _ => None,
        };
        let (endpoint, trust, authentication) = self.prepare(&profile, secret)?;
        if self.active.is_some() {
            let _ = self.disconnect().await;
        }
        let outcome = run_check(
            endpoint,
            &trust,
            authentication,
            profile.identity.as_ref(),
            accept_new_identity,
            now(),
        )
        .await;
        let mut report = outcome.report;
        self.count_sign_in_failures(connection_id, &mut report);
        let (Some(gateway), Some(observed), true) =
            (outcome.gateway, outcome.observed, report.passed())
        else {
            self.record_last_check(connection_id, &report, None)?;
            return Ok(ConnectOutcome {
                report,
                connected: false,
                context_changed: false,
                credential_save: None,
            });
        };
        let context_changed = report.identity == Some(IdentityMatch::Changed);
        let identity = StoredIdentity {
            endpoint_fingerprint: observed.endpoint_fingerprint().to_owned(),
            server_fingerprint: observed.fingerprint().to_owned(),
            server_version: observed.version().to_string(),
            verified_at: report.checked_at,
        };
        let generation =
            self.record_last_check(connection_id, &report, Some((identity, context_changed)))?;
        let credential_save = save_token.and_then(|token| {
            self.credentials
                .save(&profile.credential_ref, &token)
                .err()
                .map(CredentialState::code)
        });
        self.active = Some(ActiveConnection {
            connection_id: connection_id.to_owned(),
            context_generation: generation,
            gateway: ActiveGateway::ReadOnly(gateway),
        });
        Ok(ConnectOutcome {
            report,
            connected: true,
            context_changed,
            credential_save,
        })
    }

    /// `connection.disconnect`: `user.logout` for password sessions; the
    /// secret is erased from memory with the gateway.
    pub async fn disconnect(&mut self) -> Result<DisconnectOutcome, ConnectionError> {
        let Some(active) = self.active.take() else {
            return Ok(DisconnectOutcome {
                connection_id: None,
                logout: None,
            });
        };
        let result = match active.gateway {
            ActiveGateway::ReadOnly(gateway) => gateway.logout().await,
            ActiveGateway::ReadWrite(gateway) => gateway.logout().await,
            ActiveGateway::Switching => Ok(()),
        };
        Ok(DisconnectOutcome {
            connection_id: Some(active.connection_id),
            logout: result.err().map(|error| error.failure().code.as_str()),
        })
    }

    /// `connection.enable_writes`: only for the active connection and only
    /// with the user's confirmation. Lasts until disconnect or restart.
    pub fn enable_writes(
        &mut self,
        connection_id: &str,
        confirmed: bool,
    ) -> Result<AccessMode, ConnectionError> {
        if !confirmed {
            return Err(ConnectionError::new("writes_not_confirmed"));
        }
        let active = self.active_for(connection_id)?;
        let gateway = std::mem::replace(&mut active.gateway, ActiveGateway::Switching);
        active.gateway = match gateway {
            ActiveGateway::ReadOnly(reader) => ActiveGateway::ReadWrite(reader.enable_writes()),
            other => other,
        };
        Ok(AccessMode::ReadWrite)
    }

    /// `connection.set_read_only`: immediate, invalidates every plan of this
    /// connection (the write epoch is dropped).
    pub fn set_read_only(&mut self, connection_id: &str) -> Result<AccessMode, ConnectionError> {
        let active = self.active_for(connection_id)?;
        let gateway = std::mem::replace(&mut active.gateway, ActiveGateway::Switching);
        active.gateway = match gateway {
            ActiveGateway::ReadWrite(writer) => ActiveGateway::ReadOnly(writer.into_read_only()),
            other => other,
        };
        Ok(AccessMode::ReadOnly)
    }

    /// Active connection: profile ID, context generation and mode.
    #[must_use]
    pub fn active(&self) -> Option<(&str, u64, AccessMode)> {
        self.active.as_ref().and_then(|active| {
            active.mode().map(|mode| {
                (
                    active.connection_id.as_str(),
                    active.context_generation,
                    mode,
                )
            })
        })
    }

    /// Read access of the active connection in either mode.
    #[must_use]
    pub fn reader(&self) -> Option<&ReadOnlyGateway> {
        match &self.active.as_ref()?.gateway {
            ActiveGateway::ReadOnly(reader) => Some(reader),
            ActiveGateway::ReadWrite(writer) => Some(writer.reader()),
            ActiveGateway::Switching => None,
        }
    }

    /// Write gateway, only in read-write mode.
    #[must_use]
    pub fn writer(&self) -> Option<&ReadWriteGateway> {
        match &self.active.as_ref()?.gateway {
            ActiveGateway::ReadWrite(writer) => Some(writer),
            _ => None,
        }
    }

    fn is_active(&self, connection_id: &str) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.connection_id == connection_id)
    }

    fn active_for(
        &mut self,
        connection_id: &str,
    ) -> Result<&mut ActiveConnection, ConnectionError> {
        self.active
            .as_mut()
            .filter(|active| active.connection_id == connection_id)
            .ok_or_else(|| ConnectionError::new("not_connected"))
    }

    fn validated_input(
        &self,
        input: &ProfileInput,
    ) -> Result<(String, ProfileAuth, TrustSetting), ConnectionError> {
        validate_name(&input.name)?;
        let endpoint =
            ZabbixEndpoint::parse(&input.address).map_err(ConnectionError::from_zabbix)?;
        let auth = match &input.auth {
            AuthInput::ApiToken { store_token } => {
                if *store_token
                    && endpoint.is_plain_http()
                    && !input.confirm_plain_http_token_storage
                {
                    return Err(ConnectionError::new("plain_http_token_storage_unconfirmed"));
                }
                ProfileAuth::ApiToken {
                    secret_storage: if *store_token {
                        SecretStorage::OsStore
                    } else {
                        SecretStorage::None
                    },
                }
            }
            AuthInput::Password { username } => {
                validate_username(username)?;
                ProfileAuth::Password {
                    username: username.clone(),
                }
            }
        };
        let trust = match &input.trust {
            TrustInput::System => TrustSetting::System,
            TrustInput::CustomCa { ca_ref } => {
                // The material must exist and still be valid.
                self.trust.load(ca_ref)?;
                TrustSetting::CustomCa {
                    ca_ref: ca_ref.clone(),
                }
            }
        };
        Ok((endpoint.base_url().to_string(), auth, trust))
    }

    fn prepare(
        &self,
        profile: &ConnectionProfile,
        secret: Option<SecretString>,
    ) -> Result<(ZabbixEndpoint, TrustAnchors, Authentication), ConnectionError> {
        let endpoint = profile.endpoint()?;
        let trust = match &profile.trust {
            TrustSetting::System => TrustAnchors::system(),
            TrustSetting::CustomCa { ca_ref } => self.trust.load(ca_ref)?,
        };
        let authentication = match (&profile.auth, secret) {
            (ProfileAuth::ApiToken { .. }, Some(token)) => Authentication::ApiToken(token),
            (
                ProfileAuth::ApiToken {
                    secret_storage: SecretStorage::OsStore,
                },
                None,
            ) => Authentication::ApiToken(
                self.credentials
                    .load(&profile.credential_ref)
                    .map_err(ConnectionError::from_credential)?,
            ),
            (ProfileAuth::Password { username }, Some(password)) => {
                Authentication::UsernamePassword {
                    username: username.clone(),
                    password,
                }
            }
            (auth, None) => {
                return Err(ConnectionError::new("secret_required")
                    .with("auth_method", auth.method().as_str()));
            }
        };
        Ok((endpoint, trust, authentication))
    }

    /// Adds `consecutive_failures` to a rejected password sign-in: Zabbix
    /// blocks sign-in for 30 s after five failures (P1-02C1).
    fn count_sign_in_failures(&mut self, connection_id: &str, report: &mut ConnectionCheckReport) {
        let auth_failed = report
            .steps
            .iter()
            .any(|step| step.step == CheckStep::Auth && step.code == Some("auth_failed"));
        if !auth_failed {
            if report.passed() {
                self.sign_in_failures.remove(connection_id);
            }
            return;
        }
        let count = self
            .sign_in_failures
            .entry(connection_id.to_owned())
            .or_insert(0);
        *count = count.saturating_add(1);
        if let Some(step) = report
            .steps
            .iter_mut()
            .find(|step| step.step == CheckStep::Auth)
        {
            step.params.push(FailureParam {
                name: "consecutive_failures",
                value: count.to_string(),
            });
        }
    }

    /// Persists the outcome; on success also the identity. Returns the
    /// current context generation.
    fn record_last_check(
        &mut self,
        connection_id: &str,
        report: &ConnectionCheckReport,
        identity: Option<(StoredIdentity, bool)>,
    ) -> Result<u64, ConnectionError> {
        let mut profiles = self.profiles.load()?;
        let profile = profiles
            .iter_mut()
            .find(|profile| profile.connection_id == connection_id)
            .ok_or_else(|| ConnectionError::new("profile_not_found"))?;
        profile.last_check = Some(LastCheck {
            at: report.checked_at,
            server_version: report.server_version.clone(),
            result_code: report.result_code().map(str::to_owned),
        });
        if let Some((identity, raise_generation)) = identity {
            profile.identity = Some(identity);
            if raise_generation {
                profile.context_generation = profile.context_generation.saturating_add(1);
            }
        }
        let generation = profile.context_generation;
        self.profiles.save(&profiles)?;
        Ok(generation)
    }

    fn view(&self, profile: &ConnectionProfile) -> Result<ProfileView, ConnectionError> {
        let endpoint = profile.endpoint()?;
        let ca_fingerprints = match &profile.trust {
            TrustSetting::System => Vec::new(),
            TrustSetting::CustomCa { ca_ref } => self
                .trust
                .load(ca_ref)
                .map(|anchors| anchors.certificate_fingerprints())
                .unwrap_or_default(),
        };
        let active = self
            .active
            .as_ref()
            .filter(|active| active.connection_id == profile.connection_id);
        let state = if active.is_some() {
            ConnectionState::Connected
        } else {
            match profile
                .last_check
                .as_ref()
                .and_then(|check| check.result_code.as_deref())
            {
                Some(code) if SIGN_IN_CODES.contains(&code) => ConnectionState::NeedsSignIn,
                Some(_) => ConnectionState::LastCheckFailed,
                None => ConnectionState::NotConnected,
            }
        };
        Ok(ProfileView {
            local_id: profile.connection_id.clone(),
            name: profile.name.clone(),
            endpoint_display: endpoint.base_url().to_string(),
            api_url_display: endpoint.api_url().to_string(),
            plain_http: endpoint.is_plain_http(),
            auth_method: profile.auth.method(),
            secret_storage: profile.auth.secret_storage(),
            username: match &profile.auth {
                ProfileAuth::Password { username } => Some(username.clone()),
                ProfileAuth::ApiToken { .. } => None,
            },
            trust: profile.trust.kind(),
            ca_fingerprints,
            last_check: profile.last_check.clone(),
            state,
            mode: active.and_then(ActiveConnection::mode),
            context_generation: profile.context_generation,
        })
    }
}

impl ActiveConnection {
    fn mode(&self) -> Option<AccessMode> {
        match self.gateway {
            ActiveGateway::ReadOnly(_) => Some(AccessMode::ReadOnly),
            ActiveGateway::ReadWrite(_) => Some(AccessMode::ReadWrite),
            ActiveGateway::Switching => None,
        }
    }
}

fn find<'a>(
    profiles: &'a [ConnectionProfile],
    connection_id: &str,
) -> Result<&'a ConnectionProfile, ConnectionError> {
    profiles
        .iter()
        .find(|profile| profile.connection_id == connection_id)
        .ok_or_else(|| ConnectionError::new("profile_not_found"))
}
