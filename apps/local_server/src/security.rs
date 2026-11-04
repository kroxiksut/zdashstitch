// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
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
use tokio::sync::{Notify, watch};
use zdashstitch_application::{
    EndSessionRequest, LocalSession, MessageId, SessionEndError, SessionGeneration,
};

use crate::browser_transfer::{
    self, BrowserTransferService, upload_route_set_id, validate_display_name,
};
use crate::session_end::SessionMemory;

pub const JSON_BODY_LIMIT: usize = 2 * 1024 * 1024;
pub const UPLOAD_BODY_LIMIT: usize = 16 * 1024 * 1024;
/// Concurrent upload bodies being buffered; bounds in-flight memory to
/// `UPLOAD_COUNT_LIMIT * UPLOAD_BODY_LIMIT` (64 MiB).
pub const UPLOAD_COUNT_LIMIT: usize = 4;
pub const CSRF_HEADER: &str = "x-zdashstitch-csrf";
const SESSION_COOKIE_PREFIX: &str = "zdashstitch_session";
const BOOTSTRAP_TTL: Duration = Duration::from_secs(120);
/// Unexchanged bootstrap codes kept at once; a request beyond it is refused.
const BOOTSTRAP_CODE_LIMIT: usize = 32;

#[derive(Clone)]
pub struct LocalSecurity {
    inner: Arc<SecurityInner>,
}

struct SecurityInner {
    port: u16,
    /// `None` once the local session was ended (P2-SEC-01E): the cookie and
    /// CSRF token of every tab are rejected from then on.
    session: Mutex<Option<SessionKeys>>,
    local_session: LocalSession,
    in_flight: Arc<InFlight>,
    memories: Mutex<Vec<Arc<dyn SessionMemory>>>,
    /// Completion of the erase task started by the last `end_session`
    /// (P2-SEC-01F): `true` once it finished. A new session is created only
    /// after that. Locked after `session`, before `bootstraps`.
    erase: Mutex<Option<watch::Receiver<bool>>>,
    bootstraps: Mutex<HashMap<[u8; 32], Instant>>,
    bootstrap_ttl: Duration,
    active_uploads: Mutex<usize>,
}

struct SessionKeys {
    session_token: String,
    session_hash: [u8; 32],
    csrf_token: String,
    csrf_hash: [u8; 32],
}

impl SessionKeys {
    fn generate() -> Result<Self, SecurityInitError> {
        let session_token = random_token()?;
        let csrf_token = random_token()?;
        Ok(Self {
            session_hash: token_hash(&session_token),
            session_token,
            csrf_hash: token_hash(&csrf_token),
            csrf_token,
        })
    }
}

/// What `end_session` hands to the handler: the requests still running
/// under the ended session and the completion signal of the erase task.
pub(crate) struct EndedSession {
    pub(crate) in_flight: Arc<InFlight>,
    pub(crate) erase: EraseCompletion,
}

/// Signals that the erase task of an ended session finished. Dropped
/// without [`EraseCompletion::finish`] (a panicking erase), the ended
/// session never gets a successor in this process.
pub(crate) struct EraseCompletion(watch::Sender<bool>);

impl EraseCompletion {
    pub(crate) fn finish(self) {
        self.0.send_replace(true);
    }
}

/// Why [`LocalSecurity::issue_bootstrap`] issued no code. Machine codes
/// only; the single-instance channel returns them to the second launch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapIssueError {
    /// The erase of the ended session did not finish within the wait.
    EraseTimeout,
    /// The erase task of the ended session stopped without finishing.
    EraseFailed,
    /// Too many unexchanged bootstrap codes are pending.
    TooManyCodes,
    /// The operating-system random source failed.
    Random,
}

impl BootstrapIssueError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::EraseTimeout => "instance.bootstrap.erase_timeout",
            Self::EraseFailed => "instance.bootstrap.erase_failed",
            Self::TooManyCodes => "instance.bootstrap.too_many_codes",
            Self::Random => "instance.bootstrap.random_failed",
        }
    }
}

/// API requests admitted under the session and still running.
#[derive(Default)]
pub(crate) struct InFlight {
    count: AtomicUsize,
    idle: Notify,
}

impl InFlight {
    fn enter(this: &Arc<Self>) -> InFlightGuard {
        this.count.fetch_add(1, Ordering::SeqCst);
        InFlightGuard(Arc::clone(this))
    }

    /// Resolves once no admitted request is running.
    pub(crate) async fn drained(&self) {
        loop {
            let idle = self.idle.notified();
            tokio::pin!(idle);
            idle.as_mut().enable();
            if self.count.load(Ordering::SeqCst) == 0 {
                return;
            }
            idle.await;
        }
    }
}

struct InFlightGuard(Arc<InFlight>);

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        if self.0.count.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.0.idle.notify_waiters();
        }
    }
}

/// Why [`LocalSecurity::admit`] refused a request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Refusal {
    /// No session, an ended session or a wrong cookie: 401.
    Session,
    /// A mutating request without the current CSRF token: 403.
    Csrf,
}

/// A request that passed the session check.
struct Admission {
    generation: SessionGeneration,
    in_flight: InFlightGuard,
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
        let keys = SessionKeys::generate()?;
        let code = random_token()?;
        let security = Self {
            inner: Arc::new(SecurityInner {
                port,
                session: Mutex::new(Some(keys)),
                local_session: LocalSession::new(),
                in_flight: Arc::default(),
                memories: Mutex::default(),
                erase: Mutex::default(),
                bootstraps: Mutex::new(HashMap::from([(token_hash(&code), Instant::now() + ttl)])),
                bootstrap_ttl: ttl,
                active_uploads: Mutex::new(0),
            }),
        };
        Ok(InitialBootstrap { security, code })
    }

    /// A fresh one-time bootstrap code for a repeated launch (O-017,
    /// P2-SEC-01F).
    ///
    /// While a session is active, the code joins that session: the new tab
    /// uses the current local session (`session.tab.additional`). After
    /// «End session» no session exists; the code is issued only once the
    /// background erase of the ended session has finished, and then for a
    /// new session with new cookie and CSRF keys. Requests admitted under
    /// the ended session keep their old generation and cannot write. Waits
    /// at most `erase_wait` for the erase.
    pub async fn issue_bootstrap(
        &self,
        erase_wait: Duration,
    ) -> Result<String, BootstrapIssueError> {
        let deadline = tokio::time::Instant::now() + erase_wait;
        loop {
            let mut pending = {
                let mut session = lock(&self.inner.session);
                let erase = lock(&self.inner.erase).clone();
                match erase {
                    Some(erase) if session.is_none() && !*erase.borrow() => erase,
                    _ => {
                        if session.is_none() {
                            let keys =
                                SessionKeys::generate().map_err(|_| BootstrapIssueError::Random)?;
                            *session = Some(keys);
                        }
                        return self.add_bootstrap_code();
                    }
                }
            };
            match tokio::time::timeout_at(deadline, pending.wait_for(|finished| *finished)).await {
                Ok(Ok(_)) => {}
                Ok(Err(_)) => return Err(BootstrapIssueError::EraseFailed),
                Err(_) => return Err(BootstrapIssueError::EraseTimeout),
            }
        }
    }

    /// Called with the session lock held, so `end_session` cannot clear
    /// the pending codes between the session check and the insert.
    fn add_bootstrap_code(&self) -> Result<String, BootstrapIssueError> {
        let code = random_token().map_err(|_| BootstrapIssueError::Random)?;
        let now = Instant::now();
        let mut bootstraps = lock(&self.inner.bootstraps);
        bootstraps.retain(|_, expires_at| *expires_at > now);
        if bootstraps.len() >= BOOTSTRAP_CODE_LIMIT {
            return Err(BootstrapIssueError::TooManyCodes);
        }
        bootstraps.insert(token_hash(&code), now + self.inner.bootstrap_ttl);
        Ok(code)
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

    /// Redeems a one-time code for the cookie token of the session it was
    /// issued for. The session lock is taken first and held while the code
    /// is removed and the token read (the order of `issue_bootstrap` and
    /// `end_session`), so an «End session» plus a new session in between
    /// can never hand a code of the ended session the new session's cookie.
    fn exchange_bootstrap(&self, code: &str) -> Option<String> {
        let now = Instant::now();
        let code_hash = token_hash(code);
        let session = lock(&self.inner.session);
        let mut bootstraps = lock(&self.inner.bootstraps);
        bootstraps.retain(|_, expires_at| *expires_at > now);
        bootstraps.remove(&code_hash)?;
        session.as_ref().map(|keys| keys.session_token.clone())
    }

    /// Admits a request carrying the current session cookie and, when
    /// `needs_csrf`, the current CSRF token. Both tokens and the generation
    /// are checked under one session lock, so a request is either admitted
    /// before `end_session` (and counted in flight) or refused after it as
    /// [`Refusal::Session`], never as a CSRF failure.
    fn admit(&self, headers: &HeaderMap, needs_csrf: bool) -> Result<Admission, Refusal> {
        let session = lock(&self.inner.session);
        let keys = session.as_ref().ok_or(Refusal::Session)?;
        let cookie = cookie_value(headers, &self.session_cookie_name()).ok_or(Refusal::Session)?;
        if !constant_time_eq(&token_hash(cookie), &keys.session_hash) {
            return Err(Refusal::Session);
        }
        let csrf_valid = headers
            .get(CSRF_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(token_hash)
            .is_some_and(|hash| constant_time_eq(&hash, &keys.csrf_hash));
        if needs_csrf && !csrf_valid {
            return Err(Refusal::Csrf);
        }
        Ok(Admission {
            generation: self.inner.local_session.current(),
            in_flight: InFlight::enter(&self.inner.in_flight),
        })
    }

    /// Local session state shared with the publication use case: a handler
    /// starts a write stage with the [`SessionGeneration`] its request was
    /// admitted under (a request extension set by the API boundary).
    #[must_use]
    pub fn local_session(&self) -> &LocalSession {
        &self.inner.local_session
    }

    /// Registers an in-memory holder of Zabbix secrets or plans that
    /// `session.end` must erase.
    pub fn register_session_memory(&self, memory: Arc<dyn SessionMemory>) {
        lock(&self.inner.memories).push(memory);
    }

    /// `session.end`: refused without confirmation or during a write stage.
    /// Otherwise the session keys of every tab and every pending bootstrap
    /// code are dropped at once, and the erase the caller must run is
    /// recorded as pending, so no new session starts before it finished.
    pub(crate) fn end_session(
        &self,
        request: EndSessionRequest,
    ) -> Result<EndedSession, SessionEndError> {
        let mut session = lock(&self.inner.session);
        self.inner.local_session.end(request)?;
        *session = None;
        let (finished, pending) = watch::channel(false);
        *lock(&self.inner.erase) = Some(pending);
        lock(&self.inner.bootstraps).clear();
        Ok(EndedSession {
            in_flight: Arc::clone(&self.inner.in_flight),
            erase: EraseCompletion(finished),
        })
    }

    /// Erases every registered holder; idempotent.
    pub(crate) async fn erase_session_memory(&self) {
        let memories = lock(&self.inner.memories).clone();
        for memory in memories {
            memory.erase().await;
        }
    }

    fn reserve_upload(&self) -> Option<UploadPermit> {
        let mut accepted = lock(&self.inner.active_uploads);
        if *accepted >= UPLOAD_COUNT_LIMIT {
            return None;
        }
        *accepted += 1;
        Some(UploadPermit {
            security: self.clone(),
        })
    }

    pub(crate) fn session_cookie_name(&self) -> String {
        format!("{SESSION_COOKIE_PREFIX}_{}", self.inner.port)
    }
}

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

struct UploadPermit {
    security: LocalSecurity,
}

impl Drop for UploadPermit {
    fn drop(&mut self) {
        let mut active = lock(&self.security.inner.active_uploads);
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
    mut request: Request,
    next: Next,
) -> Response {
    let security = &guard.security;
    if !request_origin_allowed(security, request.headers()) {
        return api_error_with_api_headers(
            StatusCode::FORBIDDEN,
            MessageId::HTTP_SECURITY_INVALID_ORIGIN,
        );
    }
    let Admission {
        generation,
        in_flight,
    } = match security.admit(request.headers(), is_mutating(request.method())) {
        Ok(admission) => admission,
        Err(Refusal::Session) => {
            return api_error_with_api_headers(
                StatusCode::UNAUTHORIZED,
                MessageId::HTTP_SECURITY_SESSION_INVALID,
            );
        }
        Err(Refusal::Csrf) => {
            return api_error_with_api_headers(
                StatusCode::FORBIDDEN,
                MessageId::HTTP_SECURITY_CSRF_INVALID,
            );
        }
    };
    request.extensions_mut().insert(generation);
    let headers = request.headers();

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
    drop(in_flight);
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

pub async fn session(State(security): State<LocalSecurity>) -> Response {
    let csrf_token = lock(&security.inner.session)
        .as_ref()
        .map(|keys| keys.csrf_token.clone());
    match csrf_token {
        Some(csrf_token) => Json(SessionResponse { csrf_token }).into_response(),
        None => api_error(
            StatusCode::UNAUTHORIZED,
            MessageId::HTTP_SECURITY_SESSION_INVALID,
        ),
    }
}

fn bootstrap_response(security: &LocalSecurity, code: &str) -> Response {
    let Some(session_token) = security.exchange_bootstrap(code) else {
        return api_error(
            StatusCode::UNAUTHORIZED,
            MessageId::HTTP_SECURITY_BOOTSTRAP_INVALID,
        );
    };
    let cookie = format!(
        "{}={session_token}; HttpOnly; SameSite=Strict; Path=/",
        security.session_cookie_name(),
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

pub(crate) fn api_error(status: StatusCode, code: MessageId) -> Response {
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

    pub(crate) fn session_memory_count(&self) -> usize {
        lock(&self.inner.memories).len()
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

    /// Finding: a session ended between a separate cookie check and CSRF
    /// check answered 403 `csrf_invalid` instead of 401. Both are now one
    /// check under the session lock, and the session refusal wins.
    #[test]
    fn admission_checks_cookie_and_csrf_together_and_an_ended_session_is_401() {
        let security = LocalSecurity::initialize(43125)
            .unwrap_or_else(|error| panic!("security must initialize: {error}"))
            .security;
        let (cookie, csrf) = {
            let session = lock(&security.inner.session);
            let keys = session
                .as_ref()
                .unwrap_or_else(|| panic!("a new session has keys"));
            (keys.session_token.clone(), keys.csrf_token.clone())
        };
        let name = security.session_cookie_name();
        let headers = |cookie: &str, csrf: Option<&str>| {
            let mut headers = HeaderMap::new();
            let cookie = HeaderValue::from_str(&format!("{name}={cookie}"))
                .unwrap_or_else(|error| panic!("cookie header: {error}"));
            headers.insert("cookie", cookie);
            if let Some(csrf) = csrf {
                let csrf = HeaderValue::from_str(csrf)
                    .unwrap_or_else(|error| panic!("CSRF header: {error}"));
                headers.insert(CSRF_HEADER, csrf);
            }
            headers
        };
        assert!(security.admit(&headers(&cookie, Some(&csrf)), true).is_ok());
        assert!(security.admit(&headers(&cookie, None), false).is_ok());
        assert_eq!(
            security.admit(&headers(&cookie, Some("wrong")), true).err(),
            Some(Refusal::Csrf)
        );
        assert_eq!(
            security.admit(&headers(&cookie, None), true).err(),
            Some(Refusal::Csrf)
        );
        assert_eq!(
            security.admit(&headers("wrong", Some(&csrf)), true).err(),
            Some(Refusal::Session)
        );

        security
            .end_session(EndSessionRequest { confirmed: true })
            .unwrap_or_else(|error| panic!("end: {error:?}"));
        for (csrf, needs_csrf) in [
            (Some(csrf.as_str()), true),
            (Some("wrong"), true),
            (None, true),
            (None, false),
        ] {
            assert_eq!(
                security.admit(&headers(&cookie, csrf), needs_csrf).err(),
                Some(Refusal::Session)
            );
        }
    }

    /// An erase task that stops without finishing (for example a panicking
    /// holder) must not let a new session start over unerased secrets.
    #[tokio::test]
    async fn an_erase_that_never_finishes_refuses_new_sessions() {
        let security = LocalSecurity::initialize(43126)
            .unwrap_or_else(|error| panic!("security must initialize: {error}"))
            .security;
        let ended = security
            .end_session(EndSessionRequest { confirmed: true })
            .unwrap_or_else(|error| panic!("end: {error:?}"));
        drop(ended);
        for _ in 0..2 {
            assert_eq!(
                security.issue_bootstrap(Duration::from_secs(1)).await.err(),
                Some(BootstrapIssueError::EraseFailed)
            );
        }
        assert!(lock(&security.inner.session).is_none());
    }

    /// A code redeems exactly once and only the cookie of the session it
    /// was issued for; «End session» invalidates it before a new session
    /// exists. (The exchange holds the session lock while it removes the
    /// code and reads the token, so no interleaving can mix the two.)
    #[tokio::test]
    async fn a_code_redeems_only_the_cookie_of_its_own_session() {
        let initial = LocalSecurity::initialize(43128)
            .unwrap_or_else(|error| panic!("security must initialize: {error}"));
        let security = initial.security;
        let first_token = lock(&security.inner.session)
            .as_ref()
            .map(|keys| keys.session_token.clone());
        assert!(first_token.is_some());
        assert_eq!(security.exchange_bootstrap(&initial.code), first_token);
        assert_eq!(security.exchange_bootstrap(&initial.code), None);

        let stale = security
            .issue_bootstrap(Duration::ZERO)
            .await
            .unwrap_or_else(|error| panic!("issue: {error:?}"));
        security
            .end_session(EndSessionRequest { confirmed: true })
            .unwrap_or_else(|error| panic!("end: {error:?}"))
            .erase
            .finish();
        assert_eq!(security.exchange_bootstrap(&stale), None);
        let fresh = security
            .issue_bootstrap(Duration::ZERO)
            .await
            .unwrap_or_else(|error| panic!("issue: {error:?}"));
        assert_eq!(security.exchange_bootstrap(&stale), None);
        let second_token = security.exchange_bootstrap(&fresh);
        assert!(second_token.is_some());
        assert_ne!(second_token, first_token);
    }

    #[tokio::test]
    async fn pending_bootstrap_codes_are_bounded() {
        let security = LocalSecurity::initialize(43127)
            .unwrap_or_else(|error| panic!("security must initialize: {error}"))
            .security;
        for _ in 1..BOOTSTRAP_CODE_LIMIT {
            assert!(security.issue_bootstrap(Duration::ZERO).await.is_ok());
        }
        assert_eq!(
            security.issue_bootstrap(Duration::ZERO).await.err(),
            Some(BootstrapIssueError::TooManyCodes)
        );
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
