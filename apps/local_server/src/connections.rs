// SPDX-License-Identifier: MIT OR Apache-2.0

//! Connection routes (P2-01G, P1-02H commands) behind the `/api` boundary:
//! session cookie, Origin/Host, CSRF, content type and body limits are
//! enforced by `security::protect_api` before any handler runs.
//!
//! Responses carry stable codes and safe parameters only, never prose and
//! never a secret. A secret travels once, UI → server, in a POST/PUT body
//! (sign-in, check, or a profile that stores it, O-037); it is wrapped in
//! `SecretString` right after parsing and is never echoed. Request
//! bodies are parsed here instead of by `axum::Json`, whose rejection text
//! can quote the offending JSON value.

use std::{collections::BTreeMap, fmt, sync::Arc};

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{FromRequestParts, Path},
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::sync::Mutex;
use zdashstitch_application::connections::{
    AuthInput, ConnectRequest, ConnectionCheckReport, ConnectionError, ConnectionService,
    ProfileInput, ProfileView, SecretString, TrustInput,
};

use crate::browser_transfer::{BrowserTransferService, transfer_error};
use crate::security::LocalSecurity;

/// Upper bound of a password or API token accepted from the UI.
const MAX_SECRET_BYTES: usize = 4096;

/// Shared connection use cases of this process. At most one connection is
/// active; every route waits for the previous connection command.
#[derive(Clone)]
pub struct ConnectionsHandle {
    service: Option<Arc<Mutex<ConnectionService>>>,
}

impl ConnectionsHandle {
    #[must_use]
    pub fn new(service: ConnectionService) -> Self {
        Self {
            service: Some(Arc::new(Mutex::new(service))),
        }
    }

    /// Profiles cannot be stored on this system (no configuration
    /// directory); every route answers `connections_unavailable`.
    #[must_use]
    pub fn unavailable() -> Self {
        Self { service: None }
    }

    /// The shared service, e.g. for `session.end`, which erases the secrets
    /// of the active connection.
    #[must_use]
    pub fn service(&self) -> Option<Arc<Mutex<ConnectionService>>> {
        self.service.clone()
    }
}

impl fmt::Debug for ConnectionsHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectionsHandle")
            .field("available", &self.service.is_some())
            .finish()
    }
}

pub(crate) fn routes(handle: ConnectionsHandle) -> Router<LocalSecurity> {
    Router::new()
        .route("/connections", get(list).post(create))
        .route("/connections/address-preview", post(preview_address))
        .route("/connections/trusted-ca", post(import_trusted_ca))
        .route(
            "/connections/{id}",
            get(profile).put(update).delete(delete_profile),
        )
        .route("/connections/{id}/duplicate", post(duplicate))
        .route("/connections/{id}/test", post(test))
        .route("/connections/{id}/connect", post(connect))
        .route("/connections/{id}/disconnect", post(disconnect))
        .route("/connections/{id}/enable-writes", post(enable_writes))
        .route("/connections/{id}/read-only", post(set_read_only))
        .route("/connections/{id}/forget-secret", post(forget_secret))
        .route(
            "/connections/{id}/hide-default-password-warning",
            post(hide_default_password_warning),
        )
        .layer(Extension(handle))
}

// ---------------------------------------------------------------- errors

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    params: BTreeMap<&'static str, String>,
}

fn error_response(status: StatusCode, code: &'static str) -> Response {
    (
        status,
        Json(ErrorBody {
            code,
            params: BTreeMap::new(),
        }),
    )
        .into_response()
}

fn connection_error(error: &ConnectionError) -> Response {
    let status = match error.code() {
        "profile_not_found" => StatusCode::NOT_FOUND,
        "not_connected"
        | "profile_limit_reached"
        | "credential_missing"
        | "credential_locked"
        | "credential_unavailable" => StatusCode::CONFLICT,
        "profile_store_invalid"
        | "profile_store_linked_path"
        | "profile_store_too_large"
        | "profile_store_unavailable"
        | "random_source_unavailable" => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    };
    let params = error
        .params()
        .iter()
        .map(|param| (param.name, param.value.clone()))
        .collect();
    (
        status,
        Json(ErrorBody {
            code: error.code(),
            params,
        }),
    )
        .into_response()
}

/// A request refused before the use case runs: a status and a code.
struct Rejection {
    status: StatusCode,
    code: &'static str,
}

impl IntoResponse for Rejection {
    fn into_response(self) -> Response {
        error_response(self.status, self.code)
    }
}

/// The `{id}` path segment of a profile route. A segment that is not valid
/// percent-encoded UTF-8 is refused with the routes' own JSON
/// `request_invalid`, not with axum's plain-text rejection.
struct ProfileId(String);

impl<S: Send + Sync> FromRequestParts<S> for ProfileId {
    type Rejection = Rejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Path::<String>::from_request_parts(parts, state)
            .await
            .map(|Path(id)| Self(id))
            .map_err(|_| Rejection {
                status: StatusCode::BAD_REQUEST,
                code: "request_invalid",
            })
    }
}

fn invalid_request() -> Response {
    error_response(StatusCode::BAD_REQUEST, "request_invalid")
}

fn service_of(handle: &ConnectionsHandle) -> Result<Arc<Mutex<ConnectionService>>, Rejection> {
    handle.service.clone().ok_or(Rejection {
        status: StatusCode::SERVICE_UNAVAILABLE,
        code: "connections_unavailable",
    })
}

/// Parses a JSON body without echoing parser details: they may quote a
/// secret. An empty body counts as `{}`.
fn parse<T: DeserializeOwned>(body: &Bytes) -> Result<T, Rejection> {
    let raw: &[u8] = if body.is_empty() { b"{}" } else { body };
    Json::<T>::from_bytes(raw)
        .map(|Json(value)| value)
        .map_err(|_| Rejection {
            status: StatusCode::BAD_REQUEST,
            code: "request_invalid",
        })
}

fn secret_of(value: Option<String>) -> Result<Option<SecretString>, Rejection> {
    match value {
        None => Ok(None),
        Some(secret) if secret.is_empty() => Ok(None),
        Some(secret) if secret.len() > MAX_SECRET_BYTES => Err(Rejection {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "secret_invalid",
        }),
        Some(secret) => Ok(Some(SecretString::from(secret))),
    }
}

// ---------------------------------------------------------------- wire types

/// No `Debug`: the struct may hold a secret until it is wrapped.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileRequest {
    name: String,
    address: String,
    auth: AuthRequest,
    trust: TrustRequest,
    #[serde(default)]
    confirm_plain_http_token_storage: bool,
    /// Password or API token typed in the profile form (O-037): stored in
    /// the operating-system store by the user's choice, otherwise kept in
    /// process memory only.
    #[serde(default)]
    secret: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
enum AuthRequest {
    ApiToken {
        store_token: bool,
    },
    Password {
        username: String,
        #[serde(default)]
        store_password: bool,
    },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum TrustRequest {
    System,
    CustomCa { ca_ref: String },
}

impl ProfileRequest {
    /// Splits the request into the secret-free input and the wrapped secret.
    fn into_input(self) -> Result<(ProfileInput, Option<SecretString>), Rejection> {
        let secret = secret_of(self.secret)?;
        let input = ProfileInput {
            name: self.name,
            address: self.address,
            auth: match self.auth {
                AuthRequest::ApiToken { store_token } => AuthInput::ApiToken { store_token },
                AuthRequest::Password {
                    username,
                    store_password,
                } => AuthInput::Password {
                    username,
                    store_password,
                },
            },
            trust: match self.trust {
                TrustRequest::System => TrustInput::System,
                TrustRequest::CustomCa { ca_ref } => TrustInput::CustomCa { ca_ref },
            },
            confirm_plain_http_token_storage: self.confirm_plain_http_token_storage,
        };
        Ok((input, secret))
    }
}

/// No `Debug`: the struct holds a secret until it is wrapped.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SecretRequest {
    #[serde(default)]
    secret: Option<String>,
    #[serde(default)]
    accept_new_identity: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NameRequest {
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmRequest {
    confirmed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyRequest {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddressRequest {
    address: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustImportRequest {
    upload_set_id: String,
}

/// Non-secret profile data for the interface (P1-02H).
#[derive(Serialize)]
struct ProfileBody {
    local_id: String,
    name: String,
    endpoint_display: String,
    api_url_display: String,
    plain_http: bool,
    auth_method: &'static str,
    secret_storage: &'static str,
    username: Option<String>,
    trust: &'static str,
    ca_ref: Option<String>,
    ca_fingerprints: Vec<String>,
    last_check: Option<LastCheckBody>,
    state: &'static str,
    mode: Option<&'static str>,
    context_generation: u64,
    default_password_warning_hidden: bool,
    secret_in_memory: bool,
}

/// RFC 3339 UTC form of a Unix-seconds timestamp (decision O-035): the
/// interface parses it and formats it in the user's locale and time zone; the
/// server never sends a formatted date. `None` only outside years 0..=9999.
fn rfc3339_utc(seconds: u64) -> Option<String> {
    let seconds = i64::try_from(seconds).ok()?;
    OffsetDateTime::from_unix_timestamp(seconds)
        .ok()?
        .format(&Rfc3339)
        .ok()
}

#[derive(Serialize)]
struct LastCheckBody {
    /// RFC 3339 UTC.
    at: Option<String>,
    server_version: Option<String>,
    result_code: Option<String>,
}

impl From<ProfileView> for ProfileBody {
    fn from(view: ProfileView) -> Self {
        Self {
            local_id: view.local_id,
            name: view.name,
            endpoint_display: view.endpoint_display,
            api_url_display: view.api_url_display,
            plain_http: view.plain_http,
            auth_method: view.auth_method.as_str(),
            secret_storage: view.secret_storage.as_str(),
            username: view.username,
            trust: view.trust,
            ca_ref: view.ca_ref,
            ca_fingerprints: view.ca_fingerprints,
            last_check: view.last_check.map(|check| LastCheckBody {
                at: rfc3339_utc(check.at),
                server_version: check.server_version,
                result_code: check.result_code,
            }),
            state: view.state.as_str(),
            mode: view.mode.map(|mode| mode.as_str()),
            context_generation: view.context_generation,
            default_password_warning_hidden: view.default_password_warning_hidden,
            secret_in_memory: view.secret_in_memory,
        }
    }
}

#[derive(Serialize)]
struct ActiveBody {
    local_id: String,
    mode: &'static str,
    context_generation: u64,
}

#[derive(Serialize)]
struct ListBody {
    profiles: Vec<ProfileBody>,
    active: Option<ActiveBody>,
}

#[derive(Serialize)]
struct StepBody {
    step: &'static str,
    status: &'static str,
    code: Option<&'static str>,
    params: BTreeMap<&'static str, String>,
}

#[derive(Serialize)]
struct ReadAccessBody {
    method: &'static str,
    access: &'static str,
}

#[derive(Serialize)]
struct ReportBody {
    steps: Vec<StepBody>,
    auth_method: &'static str,
    server_version: Option<String>,
    identity: Option<&'static str>,
    read_access: Vec<ReadAccessBody>,
    /// RFC 3339 UTC.
    checked_at: Option<String>,
    result_code: Option<&'static str>,
    passed: bool,
    /// Sign-in with the public default account (O-037); never the values.
    default_password_warning: bool,
}

impl From<&ConnectionCheckReport> for ReportBody {
    fn from(report: &ConnectionCheckReport) -> Self {
        Self {
            steps: report
                .steps
                .iter()
                .map(|step| StepBody {
                    step: step.step.as_str(),
                    status: step.status.as_str(),
                    code: step.code,
                    params: step
                        .params
                        .iter()
                        .map(|param| (param.name, param.value.clone()))
                        .collect(),
                })
                .collect(),
            auth_method: report.auth_method.as_str(),
            server_version: report.server_version.clone(),
            identity: report.identity.map(|identity| identity.as_str()),
            read_access: report
                .read_access
                .iter()
                .map(|(method, access)| ReadAccessBody {
                    method: method.api_method(),
                    access: access.as_str(),
                })
                .collect(),
            checked_at: rfc3339_utc(report.checked_at),
            result_code: report.result_code(),
            passed: report.passed(),
            default_password_warning: report.default_password_warning,
        }
    }
}

#[derive(Serialize)]
struct UpdateBody {
    profile: ProfileBody,
    context_changed: bool,
}

#[derive(Serialize)]
struct ConnectBody {
    report: ReportBody,
    connected: bool,
    context_changed: bool,
    credential_save: Option<&'static str>,
    profile: Option<ProfileBody>,
}

#[derive(Serialize)]
struct TestBody {
    report: ReportBody,
    profile: Option<ProfileBody>,
}

#[derive(Serialize)]
struct DisconnectBody {
    logout: Option<&'static str>,
    profile: Option<ProfileBody>,
}

#[derive(Serialize)]
struct DeleteBody {
    local_credential: Option<&'static str>,
}

#[derive(Serialize)]
struct ModeBody {
    mode: &'static str,
    profile: Option<ProfileBody>,
}

#[derive(Serialize)]
struct PreviewBody {
    base_url: String,
    api_url: String,
    plain_http: bool,
}

#[derive(Serialize)]
struct TrustBody {
    ca_ref: String,
    certificate_fingerprints: Vec<String>,
}

#[derive(Serialize)]
struct DoneBody {
    done: bool,
}

fn fresh_profile(service: &ConnectionService, id: &str) -> Option<ProfileBody> {
    service.profile(id).ok().map(ProfileBody::from)
}

// ---------------------------------------------------------------- handlers

async fn list(Extension(handle): Extension<ConnectionsHandle>) -> Response {
    let service = match service_of(&handle) {
        Ok(service) => service,
        Err(rejection) => return rejection.into_response(),
    };
    let service = service.lock().await;
    match service.list_profiles() {
        Ok(profiles) => Json(ListBody {
            profiles: profiles.into_iter().map(ProfileBody::from).collect(),
            active: service
                .active()
                .map(|(local_id, context_generation, mode)| ActiveBody {
                    local_id: local_id.to_owned(),
                    mode: mode.as_str(),
                    context_generation,
                }),
        })
        .into_response(),
        Err(error) => connection_error(&error),
    }
}

async fn profile(
    Extension(handle): Extension<ConnectionsHandle>,
    ProfileId(id): ProfileId,
) -> Response {
    let service = match service_of(&handle) {
        Ok(service) => service,
        Err(rejection) => return rejection.into_response(),
    };
    let service = service.lock().await;
    match service.profile(&id) {
        Ok(view) => Json(ProfileBody::from(view)).into_response(),
        Err(error) => connection_error(&error),
    }
}

async fn preview_address(body: Bytes) -> Response {
    let request: AddressRequest = match parse(&body) {
        Ok(request) => request,
        Err(rejection) => return rejection.into_response(),
    };
    match ConnectionService::preview_address(&request.address) {
        Ok((base_url, api_url, plain_http)) => Json(PreviewBody {
            base_url,
            api_url,
            plain_http,
        })
        .into_response(),
        Err(error) => connection_error(&error),
    }
}

async fn create(Extension(handle): Extension<ConnectionsHandle>, body: Bytes) -> Response {
    let service = match service_of(&handle) {
        Ok(service) => service,
        Err(rejection) => return rejection.into_response(),
    };
    let (input, secret) = match parse::<ProfileRequest>(&body).and_then(ProfileRequest::into_input)
    {
        Ok(parsed) => parsed,
        Err(rejection) => return rejection.into_response(),
    };
    let mut service = service.lock().await;
    match service.create_profile_with_secret(&input, secret) {
        Ok(view) => (StatusCode::CREATED, Json(ProfileBody::from(view))).into_response(),
        Err(error) => connection_error(&error),
    }
}

async fn update(
    Extension(handle): Extension<ConnectionsHandle>,
    ProfileId(id): ProfileId,
    body: Bytes,
) -> Response {
    let service = match service_of(&handle) {
        Ok(service) => service,
        Err(rejection) => return rejection.into_response(),
    };
    let (input, secret) = match parse::<ProfileRequest>(&body).and_then(ProfileRequest::into_input)
    {
        Ok(parsed) => parsed,
        Err(rejection) => return rejection.into_response(),
    };
    let mut service = service.lock().await;
    match service
        .update_profile_with_secret(&id, &input, secret)
        .await
    {
        Ok(update) => Json(UpdateBody {
            profile: ProfileBody::from(update.view),
            context_changed: update.context_changed,
        })
        .into_response(),
        Err(error) => connection_error(&error),
    }
}

async fn delete_profile(
    Extension(handle): Extension<ConnectionsHandle>,
    ProfileId(id): ProfileId,
    body: Bytes,
) -> Response {
    let service = match service_of(&handle) {
        Ok(service) => service,
        Err(rejection) => return rejection.into_response(),
    };
    if let Err(rejection) = parse::<EmptyRequest>(&body) {
        return rejection.into_response();
    }
    let mut service = service.lock().await;
    match service.delete_profile(&id).await {
        Ok(outcome) => Json(DeleteBody {
            local_credential: outcome.local_credential,
        })
        .into_response(),
        Err(error) => connection_error(&error),
    }
}

async fn duplicate(
    Extension(handle): Extension<ConnectionsHandle>,
    ProfileId(id): ProfileId,
    body: Bytes,
) -> Response {
    let service = match service_of(&handle) {
        Ok(service) => service,
        Err(rejection) => return rejection.into_response(),
    };
    let request: NameRequest = match parse(&body) {
        Ok(request) => request,
        Err(rejection) => return rejection.into_response(),
    };
    let mut service = service.lock().await;
    match service.duplicate_profile(&id, &request.name) {
        Ok(view) => (StatusCode::CREATED, Json(ProfileBody::from(view))).into_response(),
        Err(error) => connection_error(&error),
    }
}

async fn test(
    Extension(handle): Extension<ConnectionsHandle>,
    ProfileId(id): ProfileId,
    body: Bytes,
) -> Response {
    let service = match service_of(&handle) {
        Ok(service) => service,
        Err(rejection) => return rejection.into_response(),
    };
    let request: SecretRequest = match parse(&body) {
        Ok(request) => request,
        Err(rejection) => return rejection.into_response(),
    };
    if request.accept_new_identity {
        return invalid_request();
    }
    let secret = match secret_of(request.secret) {
        Ok(secret) => secret,
        Err(rejection) => return rejection.into_response(),
    };
    let mut service = service.lock().await;
    match service.test_connection(&id, secret).await {
        Ok(report) => Json(TestBody {
            report: ReportBody::from(&report),
            profile: fresh_profile(&service, &id),
        })
        .into_response(),
        Err(error) => connection_error(&error),
    }
}

async fn connect(
    Extension(handle): Extension<ConnectionsHandle>,
    ProfileId(id): ProfileId,
    body: Bytes,
) -> Response {
    let service = match service_of(&handle) {
        Ok(service) => service,
        Err(rejection) => return rejection.into_response(),
    };
    let request: SecretRequest = match parse(&body) {
        Ok(request) => request,
        Err(rejection) => return rejection.into_response(),
    };
    let accept_new_identity = request.accept_new_identity;
    let secret = match secret_of(request.secret) {
        Ok(secret) => secret,
        Err(rejection) => return rejection.into_response(),
    };
    let mut service = service.lock().await;
    let request = ConnectRequest {
        secret,
        accept_new_identity,
    };
    match service.connect(&id, request).await {
        Ok(outcome) => Json(ConnectBody {
            report: ReportBody::from(&outcome.report),
            connected: outcome.connected,
            context_changed: outcome.context_changed,
            credential_save: outcome.credential_save,
            profile: fresh_profile(&service, &id),
        })
        .into_response(),
        Err(error) => connection_error(&error),
    }
}

async fn disconnect(
    Extension(handle): Extension<ConnectionsHandle>,
    ProfileId(id): ProfileId,
    body: Bytes,
) -> Response {
    let service = match service_of(&handle) {
        Ok(service) => service,
        Err(rejection) => return rejection.into_response(),
    };
    if let Err(rejection) = parse::<EmptyRequest>(&body) {
        return rejection.into_response();
    }
    let mut service = service.lock().await;
    if service.active().map(|(active, _, _)| active) != Some(id.as_str()) {
        return error_response(StatusCode::CONFLICT, "not_connected");
    }
    match service.disconnect().await {
        Ok(outcome) => Json(DisconnectBody {
            logout: outcome.logout,
            profile: fresh_profile(&service, &id),
        })
        .into_response(),
        Err(error) => connection_error(&error),
    }
}

async fn enable_writes(
    Extension(handle): Extension<ConnectionsHandle>,
    ProfileId(id): ProfileId,
    body: Bytes,
) -> Response {
    let service = match service_of(&handle) {
        Ok(service) => service,
        Err(rejection) => return rejection.into_response(),
    };
    let request: ConfirmRequest = match parse(&body) {
        Ok(request) => request,
        Err(rejection) => return rejection.into_response(),
    };
    let mut service = service.lock().await;
    match service.enable_writes(&id, request.confirmed) {
        Ok(mode) => Json(ModeBody {
            mode: mode.as_str(),
            profile: fresh_profile(&service, &id),
        })
        .into_response(),
        Err(error) => connection_error(&error),
    }
}

async fn set_read_only(
    Extension(handle): Extension<ConnectionsHandle>,
    ProfileId(id): ProfileId,
    body: Bytes,
) -> Response {
    let service = match service_of(&handle) {
        Ok(service) => service,
        Err(rejection) => return rejection.into_response(),
    };
    if let Err(rejection) = parse::<EmptyRequest>(&body) {
        return rejection.into_response();
    }
    let mut service = service.lock().await;
    match service.set_read_only(&id) {
        Ok(mode) => Json(ModeBody {
            mode: mode.as_str(),
            profile: fresh_profile(&service, &id),
        })
        .into_response(),
        Err(error) => connection_error(&error),
    }
}

/// `credential.forget`: removes the stored token or password (O-037).
async fn forget_secret(
    Extension(handle): Extension<ConnectionsHandle>,
    ProfileId(id): ProfileId,
    body: Bytes,
) -> Response {
    let service = match service_of(&handle) {
        Ok(service) => service,
        Err(rejection) => return rejection.into_response(),
    };
    if let Err(rejection) = parse::<EmptyRequest>(&body) {
        return rejection.into_response();
    }
    let mut service = service.lock().await;
    match service.forget_secret(&id) {
        Ok(()) => Json(DoneBody { done: true }).into_response(),
        Err(error) => connection_error(&error),
    }
}

/// «Do not show again for this connection» of the default-password warning.
async fn hide_default_password_warning(
    Extension(handle): Extension<ConnectionsHandle>,
    ProfileId(id): ProfileId,
    body: Bytes,
) -> Response {
    let service = match service_of(&handle) {
        Ok(service) => service,
        Err(rejection) => return rejection.into_response(),
    };
    if let Err(rejection) = parse::<EmptyRequest>(&body) {
        return rejection.into_response();
    }
    let mut service = service.lock().await;
    match service.hide_default_password_warning(&id) {
        Ok(view) => Json(ProfileBody::from(view)).into_response(),
        Err(error) => connection_error(&error),
    }
}

/// Browser-upload path of a trusted CA bundle (O-023 fallback): the UI
/// uploads one PEM file into an upload set and names the set here.
async fn import_trusted_ca(
    Extension(handle): Extension<ConnectionsHandle>,
    Extension(transfers): Extension<BrowserTransferService>,
    body: Bytes,
) -> Response {
    let service = match service_of(&handle) {
        Ok(service) => service,
        Err(rejection) => return rejection.into_response(),
    };
    let request: TrustImportRequest = match parse(&body) {
        Ok(request) => request,
        Err(rejection) => return rejection.into_response(),
    };
    let files = match transfers.take_upload_set(&request.upload_set_id) {
        Ok(files) => files,
        Err(error) => return transfer_error(error),
    };
    let [file] = files.as_slice() else {
        return error_response(StatusCode::UNPROCESSABLE_ENTITY, "trust_upload_invalid");
    };
    let service = service.lock().await;
    match service.import_trusted_ca(&file.bytes) {
        Ok(imported) => (
            StatusCode::CREATED,
            Json(TrustBody {
                ca_ref: imported.ca_ref,
                certificate_fingerprints: imported.certificate_fingerprints,
            }),
        )
            .into_response(),
        Err(error) => connection_error(&error),
    }
}

#[cfg(test)]
#[path = "connections_tests.rs"]
mod tests;
