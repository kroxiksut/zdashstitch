// SPDX-License-Identifier: MIT OR Apache-2.0

use std::fmt;

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::json;

use crate::gateway::{AuthMethod, ReadMethod, ReadOnlyGateway, ServerIdentity, Session};
use crate::taxonomy::{Failure, FailureCode};
use crate::transport::Transport;
use crate::{
    AdapterRegistry, Result, SessionMethod, TrustAnchors, ZabbixEndpoint, ZabbixError,
    ZabbixVersion,
};

/// Credentials passed once from the UI into the Rust process (D-09). `Debug`
/// never shows the secret.
pub enum Authentication {
    ApiToken(SecretString),
    UsernamePassword {
        username: String,
        password: SecretString,
    },
}

impl Authentication {
    #[must_use]
    pub const fn method(&self) -> AuthMethod {
        match self {
            Self::ApiToken(_) => AuthMethod::ApiToken,
            Self::UsernamePassword { .. } => AuthMethod::Password,
        }
    }
}

impl fmt::Debug for Authentication {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiToken(_) => formatter.write_str("ApiToken([REDACTED])"),
            Self::UsernamePassword { .. } => {
                formatter.write_str("UsernamePassword { password: [REDACTED], .. }")
            }
        }
    }
}

pub struct ZabbixClient {
    transport: Transport,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct VersionDetector;

/// Fields of `user.checkAuthentication` needed for identity. The response
/// also carries `sessionid` and `secret`; they are never deserialized.
#[derive(Deserialize)]
struct AuthenticatedUser {
    userid: String,
    username: String,
}

impl ZabbixClient {
    /// Client that trusts the operating-system certificate store.
    pub fn new(endpoint: ZabbixEndpoint) -> Result<Self> {
        Self::with_trust(endpoint, &TrustAnchors::system())
    }

    /// Client that trusts the system store plus explicitly added CA roots.
    pub fn with_trust(endpoint: ZabbixEndpoint, trust: &TrustAnchors) -> Result<Self> {
        Ok(Self {
            transport: Transport::new(endpoint, trust)?,
        })
    }

    pub fn endpoint(&self) -> &ZabbixEndpoint {
        self.transport.endpoint()
    }

    /// Unauthenticated `apiinfo.version`: reaches the endpoint, TLS and the
    /// JSON-RPC API (P1-02D steps 1–2).
    pub async fn detect_version(&self) -> Result<ZabbixVersion> {
        let raw: String = self
            .transport
            .session(SessionMethod::ApiInfoVersion, json!([]), None)
            .await?;
        raw.parse()
    }

    /// Detects the version and signs in (steps 1–3).
    pub async fn authenticate(self, authentication: Authentication) -> Result<ReadOnlyGateway> {
        let version = VersionDetector::detect(&self).await?;
        self.sign_in(version, authentication).await
    }

    /// Signs in on an already detected version and verifies the credential on
    /// the server (P1-IN-01): an API token with `user.checkAuthentication`; a
    /// password with `user.login`, then a probe call, because MFA and a role
    /// without API access only show on the first authenticated call.
    pub async fn sign_in(
        self,
        version: ZabbixVersion,
        authentication: Authentication,
    ) -> Result<ReadOnlyGateway> {
        let capabilities = AdapterRegistry::resolve(version.clone())?;
        let auth_method = authentication.method();
        let (token, login_session, user) = match authentication {
            Authentication::ApiToken(token) => {
                let user: AuthenticatedUser = self
                    .transport
                    .session(
                        SessionMethod::UserCheckAuthentication,
                        json!({ "token": token.expose_secret() }),
                        None,
                    )
                    .await
                    .map_err(|error| sign_in_error(error, auth_method, false))?;
                (token, false, user)
            }
            Authentication::UsernamePassword { username, password } => {
                let session: String = self
                    .transport
                    .session(
                        SessionMethod::UserLogin,
                        json!({ "username": username, "password": password.expose_secret() }),
                        None,
                    )
                    .await
                    .map_err(|error| sign_in_error(error, auth_method, false))?;
                let token = SecretString::from(session);
                match self.verify_login_session(&token).await {
                    Ok(user) => (token, true, user),
                    Err(error) => {
                        // Best effort: a terminated session cannot be closed.
                        let _: Result<bool> = self
                            .transport
                            .session(SessionMethod::UserLogout, json!([]), Some(&token))
                            .await;
                        return Err(error);
                    }
                }
            }
        };
        let identity = ServerIdentity::new(
            self.transport.endpoint(),
            version,
            &user.userid,
            &user.username,
        );
        Ok(ReadOnlyGateway {
            session: Session {
                transport: self.transport,
                token,
                login_session,
                capabilities,
                identity,
                auth_method,
            },
        })
    }

    async fn verify_login_session(&self, token: &SecretString) -> Result<AuthenticatedUser> {
        let _: serde_json::Value = self
            .transport
            .read(
                ReadMethod::UserGet,
                json!({ "output": ["userid"], "limit": 1 }),
                token,
            )
            .await
            .map_err(|error| sign_in_error(error, AuthMethod::Password, true))?;
        self.transport
            .session(
                SessionMethod::UserCheckAuthentication,
                json!({ "sessionid": token.expose_secret() }),
                None,
            )
            .await
            .map_err(|error| sign_in_error(error, AuthMethod::Password, true))
    }
}

/// Attributes a server rejection during sign-in to the authentication step.
/// Transport failures keep their own classification.
fn sign_in_error(error: ZabbixError, method: AuthMethod, after_login: bool) -> ZabbixError {
    if !matches!(error, ZabbixError::Api { .. }) {
        return error;
    }
    let failure = error.failure();
    let code = match (method, failure.code) {
        // Right after a successful user.login this means an MFA account.
        (AuthMethod::Password, FailureCode::SessionExpired) if after_login => {
            FailureCode::MfaSessionTerminated
        }
        (AuthMethod::Password, FailureCode::TokenNotAuthorized) => FailureCode::AuthFailed,
        (AuthMethod::Password, FailureCode::ApiError) if !after_login => FailureCode::AuthFailed,
        (_, code) => code,
    };
    let mut classified = Failure::new(code).with("auth_method", method.as_str());
    if let Some(api_method) = failure.param("method") {
        classified = classified.with("method", api_method);
    }
    ZabbixError::SignIn(classified)
}

impl VersionDetector {
    pub async fn detect(client: &ZabbixClient) -> Result<ZabbixVersion> {
        client.detect_version().await
    }
}
