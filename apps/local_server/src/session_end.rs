// SPDX-License-Identifier: MIT OR Apache-2.0

//! `POST /api/session/end` (P2-SEC-01E, O-032, P1-01G §5): ends the local
//! session in every tab after the user's confirmation and erases Zabbix
//! secrets and plans from memory. Refused during a write stage; never
//! interrupts a running write. Saved data on disk and the operating-system
//! credential store are not touched.

use std::{
    future::Future,
    pin::Pin,
    sync::{Mutex, PoisonError},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::{HeaderValue, StatusCode, header::SET_COOKIE},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::Deserialize;
use zdashstitch_application::{
    EndSessionRequest, SessionEndError,
    connections::{ClosedSession, ConnectionService},
};

use crate::security::{EndedSession, LocalSecurity, api_error};

/// Upper bound of the best-effort `user.logout` after the session ended.
const LOGOUT_TIMEOUT: Duration = Duration::from_secs(5);
/// How often [`end_session_on_exit`] looks whether a write stage finished.
const WRITE_POLL: Duration = Duration::from_millis(250);

/// Best-effort `user.logout` tasks that may still run; the program awaits
/// them before it exits, so closing the window still logs out (O-036).
static PENDING_LOGOUTS: Mutex<Vec<tokio::task::JoinHandle<()>>> = Mutex::new(Vec::new());

/// Future returned by [`SessionMemory::erase`].
pub type EraseFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// An in-memory holder of Zabbix secrets or publication plans. Registered
/// with [`LocalSecurity::register_session_memory`]; erased when the local
/// session ends. Erasing must be idempotent and must not touch disk or the
/// operating-system credential store.
pub trait SessionMemory: Send + Sync + 'static {
    fn erase(&self) -> EraseFuture<'_>;
}

impl SessionMemory for tokio::sync::Mutex<ConnectionService> {
    /// Takes the active connection out of the service without a network
    /// call; `user.logout` then runs in its own task with a short timeout,
    /// and the secret is dropped (zeroized) when that attempt ends.
    fn erase(&self) -> EraseFuture<'_> {
        Box::pin(async move {
            let closed = self.lock().await.erase_session_secrets();
            if let Some(closed) = closed {
                let task = tokio::spawn(best_effort_logout(closed));
                let mut pending = PENDING_LOGOUTS
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                pending.retain(|task| !task.is_finished());
                pending.push(task);
            }
        })
    }
}

/// A failure is logged by code only and otherwise ignored: the session has
/// already ended locally.
async fn best_effort_logout(closed: ClosedSession) {
    match tokio::time::timeout(LOGOUT_TIMEOUT, closed.logout()).await {
        Ok(None) => {}
        Ok(Some(code)) => eprintln!("session end: user.logout failed: {code}"),
        Err(_) => eprintln!("session end: user.logout timed out"),
    }
}

pub(crate) fn routes() -> Router<LocalSecurity> {
    Router::new().route("/session/end", post(end_session))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EndSessionBody {
    confirmed: bool,
}

async fn end_session(
    State(security): State<LocalSecurity>,
    body: Result<Json<EndSessionBody>, JsonRejection>,
) -> Response {
    let confirmed = body.is_ok_and(|Json(body)| body.confirmed);
    let ended = match security.end_session(EndSessionRequest { confirmed }) {
        Ok(ended) => ended,
        Err(error) => {
            let status = match error {
                SessionEndError::NotConfirmed => StatusCode::BAD_REQUEST,
                SessionEndError::Writing => StatusCode::CONFLICT,
            };
            return api_error(status, error.message_id());
        }
    };
    // No request of any tab is admitted from here on. Erasing runs in its
    // own task, started before this handler awaits anything: a client that
    // disconnects cancels the handler, never the erase. It erases now, then
    // once more after the requests admitted before the end (this one
    // included) have finished, so a slow request cannot leave a secret
    // behind. The response does not wait for it: a connection command may
    // hold a secret holder for a network round trip. A repeated launch gets
    // a bootstrap code for a new session only after this task finished
    // (P2-SEC-01F).
    spawn_erase(security.clone(), ended);

    let mut response = StatusCode::NO_CONTENT.into_response();
    let expired = format!(
        "{}=; Max-Age=0; HttpOnly; SameSite=Strict; Path=/",
        security.session_cookie_name()
    );
    if let Ok(value) = HeaderValue::from_str(&expired) {
        response.headers_mut().insert(SET_COOKIE, value);
    }
    response
}

/// Erases the secrets of an ended session in its own task: now, then once
/// more after the requests admitted before the end have finished.
fn spawn_erase(security: LocalSecurity, ended: EndedSession) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        security.erase_session_memory().await;
        ended.in_flight.drained().await;
        security.erase_session_memory().await;
        ended.erase.finish();
    })
}

/// Ends the local session because the program is closing (O-036): the same
/// path as a confirmed «End session», including the erase of Zabbix secrets
/// and the best-effort `user.logout`, awaited here so the process can exit
/// afterwards. Refused with [`SessionEndError::Writing`] during a write
/// stage; the caller keeps the window open then.
pub async fn end_session_on_close(security: &LocalSecurity) -> Result<(), SessionEndError> {
    let ended = security.end_session(EndSessionRequest { confirmed: true })?;
    let _ = spawn_erase(security.clone(), ended).await;
    finish_pending_logouts().await;
    Ok(())
}

/// Ends the local session when the program exits without a close request
/// (the window process ended on its own, or Ctrl+C): a running write stage
/// is never interrupted, so this waits until it has finished, then ends the
/// session as [`end_session_on_close`] does.
pub async fn end_session_on_exit(security: &LocalSecurity) {
    loop {
        match end_session_on_close(security).await {
            Err(SessionEndError::Writing) => tokio::time::sleep(WRITE_POLL).await,
            Ok(()) | Err(SessionEndError::NotConfirmed) => return,
        }
    }
}

/// Awaits the `user.logout` attempts started by ended sessions; each is
/// bounded by its own timeout.
async fn finish_pending_logouts() {
    let pending = std::mem::take(
        &mut *PENDING_LOGOUTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    );
    for task in pending {
        let _ = task.await;
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc, OnceLock,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use axum::{
        Extension, Router,
        body::Body,
        extract::State,
        http::{Request, StatusCode, header},
        routing::post,
    };
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tokio::sync::Notify;
    use tower::ServiceExt;
    use zdashstitch_application::{
        SessionGeneration,
        connections::{ConnectionService, MemoryCredentialStore},
    };

    use super::{EraseFuture, SessionMemory};
    use crate::{
        BootstrapIssueError, CSRF_HEADER, LocalSecurity, build_router_with_api,
        instance::{self, Acquired, InstanceConfig, InstanceServer},
    };

    const PORT: u16 = 43177;
    const CONFIRMED: &str = r#"{"confirmed":true}"#;

    /// Stands in for a holder of Zabbix secrets.
    #[derive(Default)]
    struct FakeMemory {
        holds_secret: AtomicBool,
        erased: AtomicUsize,
    }

    impl SessionMemory for FakeMemory {
        fn erase(&self) -> EraseFuture<'_> {
            Box::pin(async move {
                self.holds_secret.store(false, Ordering::SeqCst);
                self.erased.fetch_add(1, Ordering::SeqCst);
            })
        }
    }

    /// A holder whose erase waits until the test releases it.
    struct GatedMemory {
        holds_secret: AtomicBool,
        release: tokio::sync::watch::Sender<bool>,
    }

    impl GatedMemory {
        fn new() -> Self {
            Self {
                holds_secret: AtomicBool::new(true),
                release: tokio::sync::watch::Sender::new(false),
            }
        }

        fn release(&self) {
            self.release.send_replace(true);
        }

        fn holds_secret(&self) -> bool {
            self.holds_secret.load(Ordering::SeqCst)
        }
    }

    impl SessionMemory for GatedMemory {
        fn erase(&self) -> EraseFuture<'_> {
            Box::pin(async move {
                let mut released = self.release.subscribe();
                let _ = released.wait_for(|released| *released).await;
                self.holds_secret.store(false, Ordering::SeqCst);
            })
        }
    }

    /// Lets a handler signal that it started, then wait for `release`.
    #[derive(Clone, Default)]
    struct Gate {
        started: Arc<Notify>,
        release: Arc<Notify>,
    }

    struct Fixture {
        router: Router,
        security: LocalSecurity,
        memory: Arc<FakeMemory>,
        cookie: String,
        csrf: String,
    }

    impl Fixture {
        async fn new(extra: Router<LocalSecurity>) -> Self {
            let initial = LocalSecurity::initialize(PORT)
                .unwrap_or_else(|error| panic!("security must initialize: {error}"));
            let memory = Arc::new(FakeMemory::default());
            memory.holds_secret.store(true, Ordering::SeqCst);
            initial.security.register_session_memory(memory.clone());
            let security = initial.security.clone();
            let router = build_router_with_api("missing-ui", initial.security, extra);
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/?bootstrap={}", initial.code))
                        .header(header::HOST, format!("127.0.0.1:{PORT}"))
                        .body(Body::empty())
                        .unwrap_or_else(|error| panic!("request: {error}")),
                )
                .await
                .unwrap_or_else(|error| panic!("router: {error}"));
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            let cookie = response
                .headers()
                .get(header::SET_COOKIE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                .unwrap_or_else(|| panic!("bootstrap must set a cookie"))
                .to_owned();
            let (status, body) =
                send(&router, request("GET", "/api/session", &cookie, None, "")).await;
            assert_eq!(status, StatusCode::OK);
            let csrf = body["csrf_token"]
                .as_str()
                .unwrap_or_else(|| panic!("CSRF token"))
                .to_owned();
            Self {
                router,
                security,
                memory,
                cookie,
                csrf,
            }
        }

        fn post(&self, uri: &str, body: &str) -> Request<Body> {
            request("POST", uri, &self.cookie, Some(&self.csrf), body)
        }

        fn end(&self, body: &str) -> Request<Body> {
            self.post("/api/session/end", body)
        }

        async fn health(&self) -> StatusCode {
            send(
                &self.router,
                request("GET", "/api/health", &self.cookie, None, ""),
            )
            .await
            .0
        }

        fn holds_secret(&self) -> bool {
            self.memory.holds_secret.load(Ordering::SeqCst)
        }

        /// Exchanges a bootstrap code; returns the session cookie it set.
        async fn exchange(&self, code: &str) -> Option<String> {
            let response = self
                .router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/?bootstrap={code}"))
                        .header(header::HOST, format!("127.0.0.1:{PORT}"))
                        .body(Body::empty())
                        .unwrap_or_else(|error| panic!("request: {error}")),
                )
                .await
                .unwrap_or_else(|error| panic!("router: {error}"));
            if response.status() != StatusCode::SEE_OTHER {
                assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
                return None;
            }
            response
                .headers()
                .get(header::SET_COOKIE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                .map(str::to_owned)
        }

        async fn status_with(&self, cookie: &str) -> StatusCode {
            send(
                &self.router,
                request("GET", "/api/health", cookie, None, ""),
            )
            .await
            .0
        }

        async fn csrf_with(&self, cookie: &str) -> String {
            let (status, body) = send(
                &self.router,
                request("GET", "/api/session", cookie, None, ""),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            body["csrf_token"]
                .as_str()
                .unwrap_or_else(|| panic!("CSRF token"))
                .to_owned()
        }

        /// Waits for the erase task started by `session.end`.
        async fn erased_at_least(&self, times: usize) {
            tokio::time::timeout(Duration::from_secs(5), async {
                while self.holds_secret() || self.memory.erased.load(Ordering::SeqCst) < times {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap_or_else(|_| panic!("secrets must be erased {times} time(s)"));
        }
    }

    fn request(
        method: &str,
        uri: &str,
        cookie: &str,
        csrf: Option<&str>,
        body: &str,
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, format!("127.0.0.1:{PORT}"))
            .header(header::ORIGIN, format!("http://127.0.0.1:{PORT}"))
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(csrf) = csrf {
            builder = builder.header(CSRF_HEADER, csrf);
        }
        builder
            .body(Body::from(body.to_owned()))
            .unwrap_or_else(|error| panic!("request: {error}"))
    }

    async fn send(router: &Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = router
            .clone()
            .oneshot(request)
            .await
            .unwrap_or_else(|error| panic!("router: {error}"));
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .unwrap_or_else(|error| panic!("body: {error}"))
            .to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn ended_session_rejects_the_old_cookie_on_every_api_route() {
        let fixture = Fixture::new(Router::new()).await;
        for unconfirmed in ["{}", r#"{"confirmed":false}"#, "not json"] {
            let (status, body) = send(&fixture.router, fixture.end(unconfirmed)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(body["code"], "session.end.not_confirmed");
        }
        assert!(fixture.holds_secret());
        assert_eq!(fixture.health().await, StatusCode::OK);

        let response = fixture
            .router
            .clone()
            .oneshot(fixture.end(CONFIRMED))
            .await
            .unwrap_or_else(|error| panic!("router: {error}"));
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let expired = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_else(|| panic!("the cookie must be expired"));
        assert!(expired.contains("Max-Age=0"));
        assert!(!expired.contains(&fixture.cookie));
        // Secrets are erased by a task started before the response.
        fixture.erased_at_least(1).await;

        for (method, uri) in [
            ("GET", "/api/health"),
            ("GET", "/api/session"),
            ("GET", "/api/about"),
            ("GET", "/api/unknown"),
            ("POST", "/api/session/end"),
            ("POST", "/api/upload-sets"),
        ] {
            let body = if method == "POST" { CONFIRMED } else { "" };
            let (status, value) = send(
                &fixture.router,
                request(method, uri, &fixture.cookie, Some(&fixture.csrf), body),
            )
            .await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
            assert_eq!(value["code"], "http.security.session_invalid");
        }
    }

    #[test]
    fn the_connection_service_is_erased_by_end_session() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"));
        let security = LocalSecurity::initialize(PORT)
            .unwrap_or_else(|error| panic!("security must initialize: {error}"))
            .security;
        let handle = crate::ConnectionsHandle::new(ConnectionService::new(
            dir.path().to_path_buf(),
            Box::new(MemoryCredentialStore::default()),
        ));
        let _router = crate::build_router_with_connections("missing-ui", security.clone(), handle);
        // The connection service (secrets) and the publication plans
        // (P2-04F) are both erased with the session.
        assert_eq!(security.session_memory_count(), 2);
    }

    #[tokio::test]
    async fn end_session_is_protected_by_origin_csrf_and_content_type() {
        let fixture = Fixture::new(Router::new()).await;
        let mut foreign = fixture.end(CONFIRMED);
        foreign.headers_mut().insert(
            header::ORIGIN,
            header::HeaderValue::from_static("https://evil.invalid"),
        );
        let mut no_csrf = fixture.end(CONFIRMED);
        no_csrf.headers_mut().remove(CSRF_HEADER);
        let mut wrong_csrf = fixture.end(CONFIRMED);
        wrong_csrf
            .headers_mut()
            .insert(CSRF_HEADER, header::HeaderValue::from_static("wrong"));
        let mut form = fixture.end("confirmed=true");
        form.headers_mut().insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        for (rejected, expected) in [
            (foreign, StatusCode::FORBIDDEN),
            (no_csrf, StatusCode::FORBIDDEN),
            (wrong_csrf, StatusCode::FORBIDDEN),
            (form, StatusCode::UNSUPPORTED_MEDIA_TYPE),
        ] {
            assert_eq!(send(&fixture.router, rejected).await.0, expected);
        }
        // Nothing was ended or erased.
        assert!(fixture.holds_secret());
        assert_eq!(fixture.memory.erased.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.health().await, StatusCode::OK);
    }

    #[tokio::test]
    async fn end_session_is_refused_during_a_write_and_the_write_completes() {
        let gate = Gate::default();
        let handler_gate = gate.clone();
        let extra = Router::new().route(
            "/write",
            post(
                move |State(security): State<LocalSecurity>,
                      Extension(admitted): Extension<SessionGeneration>| {
                    let gate = handler_gate.clone();
                    async move {
                        let Ok(stage) = security.local_session().begin_write(admitted) else {
                            return StatusCode::UNAUTHORIZED;
                        };
                        gate.started.notify_one();
                        gate.release.notified().await;
                        drop(stage);
                        StatusCode::OK
                    }
                },
            ),
        );
        let fixture = Fixture::new(extra).await;
        let write = tokio::spawn(
            fixture
                .router
                .clone()
                .oneshot(fixture.post("/api/write", "{}")),
        );
        gate.started.notified().await;
        assert!(fixture.security.local_session().is_writing());

        let (status, body) = send(&fixture.router, fixture.end(CONFIRMED)).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["code"], "session.end.unavailable_writing");
        assert!(fixture.holds_secret());
        assert_eq!(fixture.health().await, StatusCode::OK);

        gate.release.notify_one();
        let response = write
            .await
            .unwrap_or_else(|error| panic!("write task: {error}"))
            .unwrap_or_else(|error| panic!("router: {error}"));
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!fixture.security.local_session().is_writing());

        let (status, _) = send(&fixture.router, fixture.end(CONFIRMED)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(fixture.health().await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn requests_admitted_before_the_end_cannot_write_or_leave_a_secret() {
        let gate = Gate::default();
        let handler_gate = gate.clone();
        let memory_slot: Arc<OnceLock<Arc<FakeMemory>>> = Arc::default();
        let handler_memory = memory_slot.clone();
        let extra = Router::new().route(
            "/slow-connect",
            post(
                move |State(security): State<LocalSecurity>,
                      Extension(admitted): Extension<SessionGeneration>| {
                    let gate = handler_gate.clone();
                    let memory = handler_memory.clone();
                    async move {
                        gate.started.notify_one();
                        gate.release.notified().await;
                        // A connection finishing after the end stores a secret...
                        if let Some(memory) = memory.get() {
                            memory.holds_secret.store(true, Ordering::SeqCst);
                        }
                        // ...but cannot start a write.
                        match security.local_session().begin_write(admitted) {
                            Ok(_stage) => StatusCode::OK,
                            Err(_) => StatusCode::UNAUTHORIZED,
                        }
                    }
                },
            ),
        );
        let fixture = Fixture::new(extra).await;
        let _ = memory_slot.set(fixture.memory.clone());
        let slow = tokio::spawn(
            fixture
                .router
                .clone()
                .oneshot(fixture.post("/api/slow-connect", "{}")),
        );
        gate.started.notified().await;

        let (status, _) = send(&fixture.router, fixture.end(CONFIRMED)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        fixture.erased_at_least(1).await;

        gate.release.notify_one();
        let response = slow
            .await
            .unwrap_or_else(|error| panic!("slow task: {error}"))
            .unwrap_or_else(|error| panic!("router: {error}"));
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        // The second erase runs once the admitted request has finished.
        fixture.erased_at_least(2).await;
    }

    fn instance_server(
        dir: &tempfile::TempDir,
        security: &LocalSecurity,
    ) -> (InstanceConfig, InstanceServer) {
        let config = InstanceConfig::for_test(dir.path());
        let Ok(Acquired::Primary(primary)) = instance::acquire(&config) else {
            panic!("the test directory must give the primary lock");
        };
        let server = primary
            .serve(security.clone(), &config)
            .unwrap_or_else(|error| panic!("serve: {error}"));
        (config, server)
    }

    fn code_of(url: &str) -> String {
        url.split_once("?bootstrap=")
            .map(|(_, code)| code.to_owned())
            .unwrap_or_else(|| panic!("bootstrap URL"))
    }

    /// P2-SEC-01F: after «End session» a repeated launch opens a new session
    /// in the same process, but only once the background erase finished.
    #[tokio::test]
    async fn a_repeated_launch_after_end_session_waits_for_the_erase_and_opens_a_new_session() {
        let fixture = Fixture::new(Router::new()).await;
        let gated = Arc::new(GatedMemory::new());
        fixture.security.register_session_memory(gated.clone());
        let ended_generation = fixture.security.local_session().current();
        let dir = InstanceConfig::private_temp_dir();
        let (config, _server) = instance_server(&dir, &fixture.security);

        let (status, _) = send(&fixture.router, fixture.end(CONFIRMED)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let launch = tokio::spawn({
            let config = config.clone();
            async move { instance::request_bootstrap(&config).await }
        });
        // The erase is still running: no code, no new session.
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(!launch.is_finished());
        assert!(gated.holds_secret());
        assert_eq!(fixture.health().await, StatusCode::UNAUTHORIZED);

        gated.release();
        let url = launch
            .await
            .unwrap_or_else(|error| panic!("launch task: {error}"))
            .unwrap_or_else(|error| panic!("bootstrap: {error}"));
        // Every holder was erased before the code was issued.
        assert!(!gated.holds_secret());
        assert!(!fixture.holds_secret());

        let code = code_of(&url);
        let cookie = fixture
            .exchange(&code)
            .await
            .unwrap_or_else(|| panic!("the new code must open a session"));
        assert_ne!(cookie, fixture.cookie);
        assert_eq!(fixture.status_with(&cookie).await, StatusCode::OK);
        assert_ne!(fixture.csrf_with(&cookie).await, fixture.csrf);
        assert_ne!(fixture.security.local_session().current(), ended_generation);
        // The old cookie stays rejected; the code is single-use.
        assert_eq!(fixture.health().await, StatusCode::UNAUTHORIZED);
        assert!(fixture.exchange(&code).await.is_none());
    }

    #[tokio::test]
    async fn a_bootstrap_request_is_refused_while_the_erase_hangs_past_the_wait() {
        let fixture = Fixture::new(Router::new()).await;
        let gated = Arc::new(GatedMemory::new());
        fixture.security.register_session_memory(gated.clone());
        let (status, _) = send(&fixture.router, fixture.end(CONFIRMED)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        assert_eq!(
            fixture
                .security
                .issue_bootstrap(Duration::from_millis(100))
                .await
                .err(),
            Some(BootstrapIssueError::EraseTimeout)
        );
        assert_eq!(fixture.health().await, StatusCode::UNAUTHORIZED);

        gated.release();
        let code = fixture
            .security
            .issue_bootstrap(Duration::from_secs(5))
            .await
            .unwrap_or_else(|error| panic!("bootstrap after the erase: {error:?}"));
        assert!(fixture.exchange(&code).await.is_some());
    }

    /// While a session is active, a repeated launch joins it
    /// (`session.tab.additional`): same cookie, same generation.
    #[tokio::test]
    async fn a_repeated_launch_during_an_active_session_joins_it() {
        let fixture = Fixture::new(Router::new()).await;
        let generation = fixture.security.local_session().current();
        let dir = InstanceConfig::private_temp_dir();
        let (config, _server) = instance_server(&dir, &fixture.security);

        let url = instance::request_bootstrap(&config)
            .await
            .unwrap_or_else(|error| panic!("bootstrap: {error}"));
        let cookie = fixture
            .exchange(&code_of(&url))
            .await
            .unwrap_or_else(|| panic!("the code must join the session"));
        assert_eq!(cookie, fixture.cookie);
        assert_eq!(fixture.csrf_with(&cookie).await, fixture.csrf);
        assert_eq!(fixture.security.local_session().current(), generation);
        assert!(fixture.holds_secret());

        // A code issued before «End session» dies with the session.
        let pending = fixture
            .security
            .issue_bootstrap(Duration::from_secs(1))
            .await
            .unwrap_or_else(|error| panic!("bootstrap: {error:?}"));
        let (status, _) = send(&fixture.router, fixture.end(CONFIRMED)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(fixture.exchange(&pending).await.is_none());
    }
}
