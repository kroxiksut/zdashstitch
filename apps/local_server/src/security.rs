// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json,
    body::{Body, HttpBody, to_bytes},
    extract::{Request, State},
    http::{
        HeaderMap, HeaderValue, Method, StatusCode,
        header::{
            CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_SECURITY_POLICY,
            CONTENT_TYPE, HOST, LOCATION, ORIGIN, REFERRER_POLICY, SET_COOKIE, TRANSFER_ENCODING,
            X_CONTENT_TYPE_OPTIONS,
        },
    },
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use zdashstitch_application::MessageId;

use crate::browser_transfer::{
    self, BrowserTransferService, upload_route_set_id, validate_display_name,
};

pub const JSON_BODY_LIMIT: usize = 2 * 1024 * 1024;
pub const UPLOAD_BODY_LIMIT: usize = 16 * 1024 * 1024;
/// Concurrent upload bodies being buffered; bounds in-flight memory to
/// `UPLOAD_COUNT_LIMIT * UPLOAD_BODY_LIMIT` (64 MiB).
pub const UPLOAD_COUNT_LIMIT: usize = 4;
pub const CSRF_HEADER: &str = "x-zdashstitch-csrf";
const SESSION_COOKIE_PREFIX: &str = "zdashstitch_session";
const BOOTSTRAP_TTL: Duration = Duration::from_secs(120);

#[derive(Clone)]
pub struct LocalSecurity {
    inner: Arc<SecurityInner>,
}

struct SecurityInner {
    port: u16,
    session_token: String,
    session_hash: [u8; 32],
    csrf_token: String,
    csrf_hash: [u8; 32],
    bootstraps: Mutex<HashMap<[u8; 32], Instant>>,
    active_uploads: Mutex<usize>,
}

pub struct InitialBootstrap {
    pub security: LocalSecurity,
    pub code: String,
}

#[derive(Debug)]
pub struct SecurityInitError;

impl std::fmt::Display for SecurityInitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("operating system random source failed")
    }
}

impl std::error::Error for SecurityInitError {}

impl LocalSecurity {
    pub fn initialize(port: u16) -> Result<InitialBootstrap, SecurityInitError> {
        Self::new_with_ttl(port, BOOTSTRAP_TTL)
    }

    fn new_with_ttl(port: u16, ttl: Duration) -> Result<InitialBootstrap, SecurityInitError> {
        let session_token = random_token()?;
        let csrf_token = random_token()?;
        let code = random_token()?;
        let security = Self {
            inner: Arc::new(SecurityInner {
                port,
                session_hash: token_hash(&session_token),
                session_token,
                csrf_hash: token_hash(&csrf_token),
                csrf_token,
                bootstraps: Mutex::new(HashMap::from([(token_hash(&code), Instant::now() + ttl)])),
                active_uploads: Mutex::new(0),
            }),
        };
        Ok(InitialBootstrap { security, code })
    }

    #[must_use]
    pub fn bootstrap_url(&self, code: &str) -> String {
        format!("http://127.0.0.1:{}/?bootstrap={code}", self.inner.port)
    }

    fn host_allowed(&self, host: &str) -> bool {
        host == format!("127.0.0.1:{}", self.inner.port)
            || host == format!("localhost:{}", self.inner.port)
    }

    fn origin_allowed(&self, origin: &str) -> bool {
        origin == format!("http://127.0.0.1:{}", self.inner.port)
            || origin == format!("http://localhost:{}", self.inner.port)
    }

    fn exchange_bootstrap(&self, code: &str) -> bool {
        let now = Instant::now();
        let code_hash = token_hash(code);
        let mut bootstraps = self
            .inner
            .bootstraps
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        bootstraps.retain(|_, expires_at| *expires_at > now);
        bootstraps.remove(&code_hash).is_some()
    }

    fn valid_session(&self, headers: &HeaderMap) -> bool {
        cookie_value(headers, &self.session_cookie_name())
            .map(token_hash)
            .is_some_and(|hash| constant_time_eq(&hash, &self.inner.session_hash))
    }

    fn valid_csrf(&self, headers: &HeaderMap) -> bool {
        headers
            .get(CSRF_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(token_hash)
            .is_some_and(|hash| constant_time_eq(&hash, &self.inner.csrf_hash))
    }

    fn reserve_upload(&self) -> Option<UploadPermit> {
        let mut accepted = self
            .inner
            .active_uploads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *accepted >= UPLOAD_COUNT_LIMIT {
            return None;
        }
        *accepted += 1;
        Some(UploadPermit {
            security: self.clone(),
        })
    }

    fn session_cookie_name(&self) -> String {
        format!("{SESSION_COOKIE_PREFIX}_{}", self.inner.port)
    }
}

struct UploadPermit {
    security: LocalSecurity,
}

impl Drop for UploadPermit {
    fn drop(&mut self) {
        let mut active = self
            .security
            .inner
            .active_uploads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *active = active.saturating_sub(1);
    }
}

#[derive(Debug, Serialize)]
pub struct ApiErrorBody {
    pub code: &'static str,
    pub params: BTreeMap<&'static str, String>,
}

pub async fn host_and_security_headers(
    State(security): State<LocalSecurity>,
    request: Request,
    next: Next,
) -> Response {
    let host = request
        .headers()
        .get(HOST)
        .and_then(|value| value.to_str().ok());
    let is_api = request.uri().path().starts_with("/api/");
    let mut response = if !host.is_some_and(|host| security.host_allowed(host)) {
        api_error(
            StatusCode::BAD_REQUEST,
            MessageId::HTTP_SECURITY_INVALID_HOST,
        )
    } else if request.uri().path() == "/" {
        if let Some(code) = bootstrap_code(request.uri().query()) {
            bootstrap_response(&security, code)
        } else {
            next.run(request).await
        }
    } else {
        next.run(request).await
    };
    add_security_headers(response.headers_mut());
    if is_api {
        add_api_headers(response.headers_mut());
    }
    response
}

/// State of the `/api` boundary middleware.
#[derive(Clone)]
pub(crate) struct ApiGuard {
    pub(crate) security: LocalSecurity,
    pub(crate) transfers: BrowserTransferService,
}

pub(crate) async fn protect_api(
    State(guard): State<ApiGuard>,
    request: Request,
    next: Next,
) -> Response {
    let security = &guard.security;
    let headers = request.headers();
    if !request_origin_allowed(security, headers) {
        return api_error_with_api_headers(
            StatusCode::FORBIDDEN,
            MessageId::HTTP_SECURITY_INVALID_ORIGIN,
        );
    }
    if !security.valid_session(headers) {
        return api_error_with_api_headers(
            StatusCode::UNAUTHORIZED,
            MessageId::HTTP_SECURITY_SESSION_INVALID,
        );
    }
    if is_mutating(request.method()) && !security.valid_csrf(headers) {
        return api_error_with_api_headers(
            StatusCode::FORBIDDEN,
            MessageId::HTTP_SECURITY_CSRF_INVALID,
        );
    }

    guard.transfers.prune_expired();
    if !is_mutating(request.method()) && request_has_body(&request) {
        return api_error_with_api_headers(
            StatusCode::PAYLOAD_TOO_LARGE,
            MessageId::HTTP_SECURITY_BODY_TOO_LARGE,
        );
    }

    let is_upload = request.uri().path().starts_with("/uploads/");
    let upload_permit = if is_upload {
        match security.reserve_upload() {
            Some(permit) => Some(permit),
            None => {
                return api_error_with_api_headers(
                    StatusCode::TOO_MANY_REQUESTS,
                    MessageId::HTTP_SECURITY_UPLOAD_COUNT_EXCEEDED,
                );
            }
        }
    } else {
        None
    };

    let request = if is_mutating(request.method()) {
        let expected_content_type = if is_upload {
            "application/octet-stream"
        } else {
            "application/json"
        };
        if media_type(headers) != Some(expected_content_type) {
            return api_error_with_api_headers(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                MessageId::HTTP_SECURITY_INVALID_CONTENT_TYPE,
            );
        }
        let declared_length = declared_body_length(&request);
        let mut limit = if is_upload {
            UPLOAD_BODY_LIMIT
        } else {
            JSON_BODY_LIMIT
        };
        if let Some(set_id) = upload_route_set_id(request.uri().path()) {
            match guard.transfers.upload_capacity(set_id, declared_length) {
                Ok(capacity) => limit = limit.min(capacity),
                Err(error) => {
                    let mut response = browser_transfer::transfer_error(error);
                    add_security_headers(response.headers_mut());
                    add_api_headers(response.headers_mut());
                    return response;
                }
            }
        }
        if declared_length.is_some_and(|length| length > limit) {
            return api_error_with_api_headers(
                StatusCode::PAYLOAD_TOO_LARGE,
                MessageId::HTTP_SECURITY_BODY_TOO_LARGE,
            );
        }
        let (parts, body) = request.into_parts();
        let bytes = match to_bytes(body, limit).await {
            Ok(bytes) => bytes,
            Err(_) => {
                return api_error_with_api_headers(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    MessageId::HTTP_SECURITY_BODY_TOO_LARGE,
                );
            }
        };
        Request::from_parts(parts, Body::from(bytes))
    } else {
        request
    };

    let mut response = next.run(request).await;
    drop(upload_permit);
    add_api_headers(response.headers_mut());
    if let Some(attachment) = response.extensions().get::<AttachmentResponse>() {
        let value = attachment.content_disposition();
        response.headers_mut().insert(CONTENT_DISPOSITION, value);
    }
    response
}

/// Length announced by `Content-Length`, or the exact body size when the
/// body knows it (in-process requests have no header).
fn declared_body_length(request: &Request) -> Option<usize> {
    match request.headers().get(CONTENT_LENGTH) {
        Some(value) => value.to_str().ok()?.trim().parse().ok(),
        None => request
            .body()
            .size_hint()
            .exact()
            .and_then(|length| usize::try_from(length).ok()),
    }
}

/// `GET`, `HEAD` and `OPTIONS` carry no body here; since the per-route body
/// limit is disabled for `/api`, any announced body is refused.
fn request_has_body(request: &Request) -> bool {
    request.headers().contains_key(TRANSFER_ENCODING)
        || request
            .headers()
            .get(CONTENT_LENGTH)
            .is_some_and(|value| value.as_bytes() != b"0")
        || request.body().size_hint().lower() > 0
}

pub async fn api_not_found() -> Response {
    api_error_with_api_headers(
        StatusCode::NOT_FOUND,
        MessageId::HTTP_SECURITY_API_NOT_FOUND,
    )
}

/// Response extension that makes the API boundary add `Content-Disposition:
/// attachment`, with a file name when one was validated.
#[derive(Clone, Debug, Default)]
pub struct AttachmentResponse {
    file_name: Option<String>,
}

impl AttachmentResponse {
    /// Attachment without a suggested file name.
    #[must_use]
    pub fn unnamed() -> Self {
        Self::default()
    }

    /// Attachment with a suggested file name; a name that fails download
    /// name validation is dropped instead of reaching the header.
    #[must_use]
    pub fn named(file_name: String) -> Self {
        Self {
            file_name: validate_display_name(&file_name)
                .is_ok()
                .then_some(file_name),
        }
    }

    fn content_disposition(&self) -> HeaderValue {
        self.file_name
            .as_deref()
            .and_then(|name| HeaderValue::from_str(&content_disposition_value(name)).ok())
            .unwrap_or_else(|| HeaderValue::from_static("attachment"))
    }
}

/// RFC 6266 value: an ASCII `filename` fallback plus the exact UTF-8 name
/// as an RFC 8187 `filename*`.
fn content_disposition_value(name: &str) -> String {
    use std::fmt::Write as _;
    let fallback: String = name
        .chars()
        .map(|character| {
            if (character.is_ascii_graphic() || character == ' ')
                && !matches!(character, '"' | '\\' | '%' | ';')
            {
                character
            } else {
                '_'
            }
        })
        .collect();
    let mut encoded = String::with_capacity(name.len() * 3);
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}

#[derive(Serialize)]
pub struct SessionResponse {
    pub csrf_token: String,
}

pub async fn session(State(security): State<LocalSecurity>) -> Json<SessionResponse> {
    Json(SessionResponse {
        csrf_token: security.inner.csrf_token.clone(),
    })
}

fn bootstrap_response(security: &LocalSecurity, code: &str) -> Response {
    if !security.exchange_bootstrap(code) {
        return api_error(
            StatusCode::UNAUTHORIZED,
            MessageId::HTTP_SECURITY_BOOTSTRAP_INVALID,
        );
    }
    let cookie = format!(
        "{}={}; HttpOnly; SameSite=Strict; Path=/",
        security.session_cookie_name(),
        security.inner.session_token
    );
    let mut response = StatusCode::SEE_OTHER.into_response();
    response
        .headers_mut()
        .insert(LOCATION, HeaderValue::from_static("/"));
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(SET_COOKIE, value);
    }
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn request_origin_allowed(security: &LocalSecurity, headers: &HeaderMap) -> bool {
    match headers.get(ORIGIN).and_then(|value| value.to_str().ok()) {
        Some(origin) => security.origin_allowed(origin),
        None => {
            headers
                .get("sec-fetch-site")
                .and_then(|value| value.to_str().ok())
                == Some("same-origin")
        }
    }
}

fn media_type(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
}

fn is_mutating(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn bootstrap_code(query: Option<&str>) -> Option<&str> {
    query?.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "bootstrap" && !value.is_empty()).then_some(value)
    })
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all("cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find_map(|(key, value)| (key == name).then_some(value))
}

fn random_token() -> Result<String, SecurityInitError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| SecurityInitError)?;
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(token, "{byte:02x}");
    }
    Ok(token)
}

fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn constant_time_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn api_error(status: StatusCode, code: MessageId) -> Response {
    (
        status,
        Json(ApiErrorBody {
            code: code.as_str(),
            params: BTreeMap::new(),
        }),
    )
        .into_response()
}

fn api_error_with_api_headers(status: StatusCode, code: MessageId) -> Response {
    let mut response = api_error(status, code);
    add_security_headers(response.headers_mut());
    add_api_headers(response.headers_mut());
    response
}

fn add_security_headers(headers: &mut HeaderMap) {
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'self'; frame-ancestors 'none'"),
    );
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
}

fn add_api_headers(headers: &mut HeaderMap) {
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
}

#[cfg(test)]
impl LocalSecurity {
    pub fn expired_for_test(port: u16) -> Result<InitialBootstrap, SecurityInitError> {
        Self::new_with_ttl(port, Duration::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_permits_are_bounded_concurrently_and_reusable() {
        let security = LocalSecurity::initialize(43123)
            .unwrap_or_else(|error| panic!("security must initialize: {error}"))
            .security;
        let permits: Vec<_> = (0..UPLOAD_COUNT_LIMIT)
            .map(|_| {
                security
                    .reserve_upload()
                    .unwrap_or_else(|| panic!("permit below limit must be available"))
            })
            .collect();
        assert!(security.reserve_upload().is_none());
        drop(permits);
        assert!(security.reserve_upload().is_some());
    }

    #[test]
    fn session_cookie_name_is_scoped_to_the_listener_port() {
        let first = LocalSecurity::initialize(43123)
            .unwrap_or_else(|error| panic!("security must initialize: {error}"))
            .security;
        let second = LocalSecurity::initialize(43124)
            .unwrap_or_else(|error| panic!("security must initialize: {error}"))
            .security;
        assert_ne!(first.session_cookie_name(), second.session_cookie_name());
    }

    #[test]
    fn attachment_header_carries_an_ascii_fallback_and_an_exact_utf8_name() {
        assert_eq!(
            AttachmentResponse::named("Панель Prod 100%.zip".to_owned()).content_disposition(),
            HeaderValue::from_static(
                "attachment; filename=\"______ Prod 100_.zip\"; \
                 filename*=UTF-8''%D0%9F%D0%B0%D0%BD%D0%B5%D0%BB%D1%8C%20Prod%20100%25.zip"
            )
        );
        assert_eq!(
            AttachmentResponse::named("draft.zip".to_owned()).content_disposition(),
            HeaderValue::from_static(
                "attachment; filename=\"draft.zip\"; filename*=UTF-8''draft.zip"
            )
        );
        for unsafe_name in ["a\r\nSet-Cookie: x.zip", "a\";b.zip", "", "a/b.zip"] {
            assert_eq!(
                AttachmentResponse::named(unsafe_name.to_owned()).content_disposition(),
                HeaderValue::from_static("attachment")
            );
        }
        assert_eq!(
            AttachmentResponse::unnamed().content_disposition(),
            HeaderValue::from_static("attachment")
        );
    }
}
