// SPDX-License-Identifier: MIT OR Apache-2.0

//! Single instance per operating-system user (P2-SEC-01F, O-017, ADR-008).
//!
//! The first launch takes an exclusive lock on `instance.lock` in a private
//! per-user directory and holds it for its lifetime, then serves a local
//! channel: a named pipe on Windows, a Unix domain socket on Linux; never
//! TCP or any other network transport. A repeated launch finds the lock
//! held, asks the running process for a fresh bootstrap URL over that
//! channel, prints it and exits without binding an HTTP server.
//!
//! The operating system releases the lock when the process ends, crashed
//! or not. The next launch that takes the lock deletes a leftover endpoint
//! file at once, before anything else, and later publishes its own. The
//! endpoint file `instance.json` names the channel and carries a random
//! proof. A repeated launch sends a fresh random nonce, and the server
//! answers with an HMAC of the proof over that nonce and the reply (wire
//! format in `protocol`), so the proof itself never travels over the
//! channel and only a holder of the endpoint file can produce a valid
//! answer. The server writes nothing on a connection before it has read a
//! complete, authenticatable request line; anything else is closed without
//! a byte.
//!
//! Platform permissions:
//!
//! - Windows: directory `%LOCALAPPDATA%\ZDashstitch\runtime` (profile ACL:
//!   the user, SYSTEM, Administrators; a host may add more ACEs, which then
//!   also apply to `instance.json`). The pipe name has a fresh 128-bit
//!   random part per run and is created with `FILE_FLAG_FIRST_PIPE_INSTANCE`
//!   and `PIPE_REJECT_REMOTE_CLIENTS` (tokio's default). tokio passes no
//!   security attributes, so the pipe gets the default descriptor: full
//!   control for the creator owner, SYSTEM and Administrators, read access
//!   for Everyone and Anonymous. Pipe names are enumerable, so any local
//!   principal can open the pipe for reading; such a client cannot write a
//!   request, so the server never sends it anything and closes it after
//!   the read timeout, and it cannot create another instance of the pipe.
//!   Setting an owner-only DACL would need `unsafe`
//!   (`create_with_security_attributes_raw`), which the workspace denies.
//! - Linux: directory `$XDG_RUNTIME_DIR/zdashstitch`, or
//!   `$XDG_DATA_HOME/zdashstitch/runtime` (`~/.local/share/...`) without a
//!   runtime directory; it must be owned by the effective user and have no
//!   group or other permissions (created `0700`). Socket and files are
//!   `0600`; the server also refuses peers of another user (`SO_PEERCRED`).
//!
//! Requests are read before they take one of a few issue permits, so idle
//! connections cannot block a repeated launch; the number of connections
//! being read is capped separately, and a connection over that cap is
//! closed at once.

mod protocol;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as platform;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as platform;

use std::{
    fmt,
    fs::{File, TryLockError},
    future::Future,
    io::{self, Read as _, Write as _},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
};

use crate::security::LocalSecurity;
use protocol::{EndpointRecord, Reply, Request};

const LOCK_FILE: &str = "instance.lock";
const ENDPOINT_FILE: &str = "instance.json";
const ENDPOINT_TEMP_FILE: &str = "instance.json.tmp";

/// Upper bound of waiting for the erase of an ended session.
const ERASE_WAIT: Duration = Duration::from_secs(30);
/// How long the server waits for the request line; a repeated launch
/// writes it right after connecting.
const READ_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a repeated launch keeps trying to reach the running instance
/// (it may still be starting, or busy).
const CONNECT_WINDOW: Duration = Duration::from_secs(5);
/// How long a repeated launch waits for the response once connected; longer
/// than [`ERASE_WAIT`].
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(40);
const CONNECT_RETRY: Duration = Duration::from_millis(50);
/// After answering, how long the server lets the client read before closing.
const CLOSE_WAIT: Duration = Duration::from_secs(1);
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// Complete requests served at once; more get `instance.busy`, which a
/// repeated launch retries within its connect window.
const CONCURRENT_REQUESTS: usize = 4;
/// Connections being read or answered at once; more are closed silently.
const CONCURRENT_CONNECTIONS: usize = 64;
/// First pause after the channel failed to accept; doubled per failure.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);
const ACCEPT_BACKOFF_MAX: Duration = Duration::from_secs(2);
/// Consecutive accept failures after which the endpoint file is removed,
/// so repeated launches fail closed until the channel recovers.
const WITHDRAW_AFTER: u32 = 3;

/// Where the single-instance files live and how long each step may take.
#[derive(Clone, Debug)]
pub struct InstanceConfig {
    directory: PathBuf,
    erase_wait: Duration,
    read_timeout: Duration,
    close_wait: Duration,
    connect_window: Duration,
    response_timeout: Duration,
    max_connections: usize,
    accept_backoff: Duration,
}

impl InstanceConfig {
    /// The private per-user directory of this platform.
    pub fn platform_default() -> Result<Self, InstanceError> {
        platform::default_directory()
            .map(Self::at)
            .ok_or(InstanceError::DirectoryUnavailable)
    }

    /// An explicit directory (`ZDASHSTITCH_INSTANCE_DIR`, development). It
    /// must be absolute. On Linux it must pass the same ownership and
    /// permission checks as the default directory when acquired; on Windows,
    /// where std offers no ACL or owner check, it must lie inside
    /// `%LOCALAPPDATA%` without junctions or links on the way, otherwise
    /// [`InstanceError::DirectoryInsecure`].
    pub fn from_override(directory: PathBuf) -> Result<Self, InstanceError> {
        if !directory.is_absolute() {
            return Err(InstanceError::DirectoryUnavailable);
        }
        platform::check_override(&directory).map(Self::at)
    }

    fn at(directory: PathBuf) -> Self {
        Self {
            directory,
            erase_wait: ERASE_WAIT,
            read_timeout: READ_TIMEOUT,
            close_wait: CLOSE_WAIT,
            connect_window: CONNECT_WINDOW,
            response_timeout: RESPONSE_TIMEOUT,
            max_connections: CONCURRENT_CONNECTIONS,
            accept_backoff: ACCEPT_BACKOFF,
        }
    }

    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// A temporary directory that passes the private-directory checks
    /// (`tempfile` uses the umask, typically `0755`, on Linux).
    #[cfg(test)]
    pub(crate) fn private_temp_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap_or_else(|error| panic!("temp dir permissions: {error}"));
        }
        dir
    }

    /// Short timeouts for tests; bypasses the override checks of
    /// [`Self::from_override`] (the directory checks of [`acquire`] still
    /// apply).
    #[cfg(test)]
    pub(crate) fn for_test(directory: &Path) -> Self {
        Self {
            directory: directory.to_path_buf(),
            erase_wait: Duration::from_secs(5),
            read_timeout: Duration::from_millis(300),
            close_wait: Duration::from_millis(100),
            connect_window: Duration::from_secs(2),
            response_timeout: Duration::from_secs(10),
            max_connections: CONCURRENT_CONNECTIONS,
            accept_backoff: Duration::from_millis(1),
        }
    }
}

/// Failures as stable machine codes; no paths, names or secrets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstanceError {
    /// No per-user directory is known or it cannot be created.
    DirectoryUnavailable,
    /// The directory is a link, foreign-owned, accessible to others or (an
    /// override on Windows) outside `%LOCALAPPDATA%`.
    DirectoryInsecure,
    /// The instance lock file cannot be opened or locked.
    Lock,
    /// The local channel or the endpoint file cannot be created.
    Endpoint,
    /// The operating-system random source failed.
    Random,
    /// A repeated launch could not reach the running instance, or it closed
    /// the channel without an answer.
    Unreachable,
    /// The running instance did not answer in time.
    Timeout,
    /// The answer was not a valid response.
    InvalidResponse,
    /// The answer did not carry a valid MAC of the endpoint proof.
    Unauthenticated,
    /// The running instance refused with this code.
    Refused(String),
}

impl InstanceError {
    #[must_use]
    pub fn code(&self) -> &str {
        match self {
            Self::DirectoryUnavailable => "instance.directory.unavailable",
            Self::DirectoryInsecure => "instance.directory.insecure",
            Self::Lock => "instance.lock.failed",
            Self::Endpoint => "instance.endpoint.failed",
            Self::Random => "instance.random_failed",
            Self::Unreachable => "instance.endpoint.unreachable",
            Self::Timeout => "instance.response.timeout",
            Self::InvalidResponse => "instance.response.invalid",
            Self::Unauthenticated => "instance.response.unauthenticated",
            Self::Refused(code) => code,
        }
    }
}

impl fmt::Display for InstanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for InstanceError {}

/// Outcome of [`acquire`].
#[derive(Debug)]
pub enum Acquired {
    /// This process is the instance of the user; it must serve.
    Primary(PrimaryLock),
    /// Another live process holds the lock; ask it with
    /// [`request_bootstrap`].
    Running,
}

/// The held instance lock, released when dropped or when the process ends.
#[derive(Debug)]
pub struct PrimaryLock {
    /// Holds the operating-system lock while open.
    _file: File,
    directory: PathBuf,
}

/// Takes the instance lock of `config`'s directory without waiting. The
/// new holder removes a leftover endpoint file of an ended process at
/// once, so a repeated launch racing with this start finds no stale
/// channel name to connect to.
pub fn acquire(config: &InstanceConfig) -> Result<Acquired, InstanceError> {
    if !config.directory.is_absolute() {
        return Err(InstanceError::DirectoryUnavailable);
    }
    platform::prepare_directory(&config.directory)?;
    let file = platform::open_private(&config.directory.join(LOCK_FILE), false)
        .map_err(|_| InstanceError::Lock)?;
    match file.try_lock() {
        Ok(()) => {
            remove_if_present(&config.directory.join(ENDPOINT_FILE))
                .map_err(|_| InstanceError::Endpoint)?;
            remove_if_present(&config.directory.join(ENDPOINT_TEMP_FILE))
                .map_err(|_| InstanceError::Endpoint)?;
            Ok(Acquired::Primary(PrimaryLock {
                _file: file,
                directory: config.directory.clone(),
            }))
        }
        Err(TryLockError::WouldBlock) => Ok(Acquired::Running),
        Err(TryLockError::Error(_)) => Err(InstanceError::Lock),
    }
}

/// A source of connected streams: the platform listener, or a scripted
/// one in tests.
pub(crate) trait Accept: Send + 'static {
    type Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static;

    fn accept(&mut self) -> impl Future<Output = io::Result<Self::Stream>> + Send;
}

impl PrimaryLock {
    /// Creates a new channel, publishes it in the endpoint file and serves
    /// bootstrap requests for `security` until the returned server is
    /// dropped. Must run inside the tokio runtime.
    pub fn serve(
        self,
        security: LocalSecurity,
        config: &InstanceConfig,
    ) -> Result<InstanceServer, InstanceError> {
        let name = random_hex()?;
        let listener = platform::Listener::bind(&self.directory, &name[..32])
            .map_err(|_| InstanceError::Endpoint)?;
        let endpoint = listener.endpoint().to_owned();
        let cleanup = listener.cleanup_path();
        self.serve_with(listener, endpoint, cleanup, security, config)
    }

    fn serve_with<L: Accept>(
        self,
        listener: L,
        endpoint: String,
        cleanup: Option<PathBuf>,
        security: LocalSecurity,
        config: &InstanceConfig,
    ) -> Result<InstanceServer, InstanceError> {
        let proof = random_hex()?;
        let record = EndpointRecord {
            v: protocol::VERSION,
            endpoint,
            proof: proof.clone(),
        };
        let publication = Arc::new(Publication {
            directory: self.directory.clone(),
            record: record.encode(),
            state: Mutex::default(),
        });
        publication.publish().map_err(|_| InstanceError::Endpoint)?;
        let responder = Arc::new(Responder {
            security,
            proof,
            erase_wait: config.erase_wait,
            read_timeout: config.read_timeout,
            close_wait: config.close_wait,
            permits: Arc::new(Semaphore::new(CONCURRENT_REQUESTS)),
        });
        let task = tokio::spawn(accept_loop(
            listener,
            responder,
            Arc::clone(&publication),
            Arc::new(Semaphore::new(config.max_connections)),
            config.accept_backoff,
        ));
        Ok(InstanceServer {
            task,
            publication,
            cleanup,
            _lock: self,
        })
    }
}

/// The endpoint file of this process: published at start, withdrawn while
/// the channel cannot accept, removed for good when the server stops. The
/// mutex orders these, so a stopped server never republishes.
#[derive(Debug)]
struct Publication {
    directory: PathBuf,
    record: Vec<u8>,
    state: Mutex<PublicationState>,
}

#[derive(Debug, Default)]
struct PublicationState {
    stopped: bool,
}

impl Publication {
    fn publish(&self) -> io::Result<()> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.stopped {
            return Ok(());
        }
        write_private_atomically(&self.directory, &self.record)
    }

    fn withdraw(&self) -> io::Result<()> {
        let _state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        remove_if_present(&self.directory.join(ENDPOINT_FILE))
    }

    fn stop(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.stopped = true;
        let _ = remove_if_present(&self.directory.join(ENDPOINT_FILE));
    }
}

/// The serving side; dropping it stops the channel, removes the endpoint
/// and then releases the lock.
#[derive(Debug)]
pub struct InstanceServer {
    task: JoinHandle<()>,
    publication: Arc<Publication>,
    cleanup: Option<PathBuf>,
    _lock: PrimaryLock,
}

impl Drop for InstanceServer {
    fn drop(&mut self) {
        self.task.abort();
        self.publication.stop();
        if let Some(path) = &self.cleanup {
            let _ = remove_if_present(path);
        }
    }
}

struct Responder {
    security: LocalSecurity,
    proof: String,
    erase_wait: Duration,
    read_timeout: Duration,
    close_wait: Duration,
    permits: Arc<Semaphore>,
}

/// Accepts until aborted. A failing accept is logged and retried with
/// backoff; after [`WITHDRAW_AFTER`] failures in a row the endpoint file is
/// removed, so repeated launches fail closed instead of trusting a channel
/// name this process may no longer hold, and it is published again once a
/// connection is accepted.
async fn accept_loop<L: Accept>(
    mut listener: L,
    responder: Arc<Responder>,
    publication: Arc<Publication>,
    connections: Arc<Semaphore>,
    backoff: Duration,
) {
    let mut failures: u32 = 0;
    let mut withdrawn = false;
    loop {
        match listener.accept().await {
            Ok(stream) => {
                failures = 0;
                if withdrawn {
                    match publication.publish() {
                        Ok(()) => {
                            withdrawn = false;
                            eprintln!("instance: local channel recovered; endpoint published");
                        }
                        Err(error) => eprintln!(
                            "instance: endpoint not republished: {}",
                            io_error_kind(&error)
                        ),
                    }
                }
                match Arc::clone(&connections).try_acquire_owned() {
                    Ok(permit) => {
                        let responder = Arc::clone(&responder);
                        tokio::spawn(serve_connection(stream, responder, permit));
                    }
                    // Closed without a byte, like any unauthenticated peer.
                    Err(_) => eprintln!("instance: connection closed: too many connections"),
                }
            }
            Err(error) => {
                failures = failures.saturating_add(1);
                eprintln!(
                    "instance: local channel failed to accept: {}; retrying",
                    io_error_kind(&error)
                );
                if failures >= WITHDRAW_AFTER && !withdrawn {
                    withdrawn = true;
                    match publication.withdraw() {
                        Ok(()) => eprintln!(
                            "instance: endpoint withdrawn until the local channel recovers"
                        ),
                        Err(error) => eprintln!(
                            "instance: endpoint not withdrawn: {}",
                            io_error_kind(&error)
                        ),
                    }
                }
                tokio::time::sleep(accept_backoff(backoff, failures)).await;
            }
        }
    }
}

fn accept_backoff(base: Duration, failures: u32) -> Duration {
    let factor = 1_u32 << failures.saturating_sub(1).min(5);
    base.saturating_mul(factor).min(ACCEPT_BACKOFF_MAX)
}

/// Reads one request line without holding an issue permit and writes
/// nothing unless that line carries this protocol version and a valid
/// nonce: silent, partial, oversize or unauthenticatable requests are
/// closed without a byte, so a client that can only read learns nothing.
async fn serve_connection<S>(
    mut stream: S,
    responder: Arc<Responder>,
    _connection: OwnedSemaphorePermit,
) where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let line = tokio::time::timeout(
        responder.read_timeout,
        read_line(&mut stream, protocol::MAX_REQUEST),
    )
    .await;
    let line = match line {
        Ok(Ok(line)) => line,
        Ok(Err(LineError::Closed)) => return,
        Ok(Err(LineError::TooLarge)) => return closed_silently(protocol::TOO_LARGE),
        Err(_) => return closed_silently(protocol::TIMEOUT),
    };
    let parsed = match protocol::parse_request(&line) {
        Ok(parsed) => parsed,
        Err(code) => return closed_silently(code),
    };
    let reply = match parsed.request {
        Err(code) => Reply::Error(code),
        Ok(Request::Bootstrap) => match Arc::clone(&responder.permits).try_acquire_owned() {
            Ok(_permit) => issue(&responder).await,
            Err(_) => Reply::Error(protocol::BUSY),
        },
    };
    if let Reply::Error(code) = &reply {
        eprintln!("instance: request refused: {code}");
    }
    let line = protocol::encode_response(&responder.proof, &parsed.nonce, &reply);
    let written = tokio::time::timeout(WRITE_TIMEOUT, async {
        stream.write_all(&line).await?;
        stream.flush().await
    })
    .await;
    if !matches!(written, Ok(Ok(()))) {
        return;
    }
    // Let the client read the line before the channel closes.
    let _ = tokio::time::timeout(responder.close_wait, wait_for_close(&mut stream)).await;
}

fn closed_silently(code: &str) {
    eprintln!("instance: connection closed without an answer: {code}");
}

async fn issue(responder: &Responder) -> Reply {
    match responder
        .security
        .issue_bootstrap(responder.erase_wait)
        .await
    {
        Ok(code) => Reply::Url(responder.security.bootstrap_url(&code)),
        Err(error) => Reply::Error(error.code()),
    }
}

enum LineError {
    TooLarge,
    Closed,
}

/// Reads up to the first `\n` (excluded); at most `limit` bytes including it.
async fn read_line<S>(stream: &mut S, limit: usize) -> Result<Vec<u8>, LineError>
where
    S: AsyncRead + Unpin,
{
    let mut line = Vec::with_capacity(limit.min(256));
    let mut chunk = [0_u8; 256];
    loop {
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|_| LineError::Closed)?;
        if read == 0 {
            return Err(LineError::Closed);
        }
        let chunk = &chunk[..read];
        if let Some(end) = chunk.iter().position(|byte| *byte == b'\n') {
            if line.len() + end + 1 > limit {
                return Err(LineError::TooLarge);
            }
            line.extend_from_slice(&chunk[..end]);
            return Ok(line);
        }
        line.extend_from_slice(chunk);
        if line.len() >= limit {
            return Err(LineError::TooLarge);
        }
    }
}

async fn wait_for_close<S>(stream: &mut S)
where
    S: AsyncRead + Unpin,
{
    let mut sink = [0_u8; 64];
    while matches!(stream.read(&mut sink).await, Ok(read) if read > 0) {}
}

/// A repeated launch: asks the running instance for a fresh bootstrap URL.
/// `instance.busy` and a channel closed without an answer (the server was
/// at its connection cap) are retried until the connect window ends.
pub async fn request_bootstrap(config: &InstanceConfig) -> Result<String, InstanceError> {
    let deadline = tokio::time::Instant::now() + config.connect_window;
    loop {
        let (stream, proof) = tokio::time::timeout_at(deadline, connect(config))
            .await
            .map_err(|_| InstanceError::Unreachable)?;
        let nonce = random_hex()?;
        let outcome =
            tokio::time::timeout(config.response_timeout, exchange(stream, &proof, &nonce))
                .await
                .map_err(|_| InstanceError::Timeout)?;
        let retry = match &outcome {
            Err(InstanceError::Refused(code)) => code == protocol::BUSY,
            Err(InstanceError::Unreachable) => true,
            _ => false,
        };
        if !retry || tokio::time::Instant::now() >= deadline {
            return outcome;
        }
        tokio::time::sleep(CONNECT_RETRY).await;
    }
}

/// Retries until the endpoint file names a channel that accepts: the
/// running instance may still be starting, or all pipe instances busy.
async fn connect(config: &InstanceConfig) -> (platform::Stream, String) {
    loop {
        if let Some(record) = read_endpoint_file(&config.directory)
            && let Ok(stream) = platform::connect(&config.directory, &record.endpoint).await
        {
            return (stream, record.proof);
        }
        tokio::time::sleep(CONNECT_RETRY).await;
    }
}

async fn exchange<S>(mut stream: S, proof: &str, nonce: &str) -> Result<String, InstanceError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    stream
        .write_all(&protocol::request_line(nonce))
        .await
        .map_err(|_| InstanceError::Unreachable)?;
    stream
        .flush()
        .await
        .map_err(|_| InstanceError::Unreachable)?;
    let line = match read_line(&mut stream, protocol::MAX_RESPONSE).await {
        Ok(line) => line,
        Err(LineError::Closed) => return Err(InstanceError::Unreachable),
        Err(LineError::TooLarge) => return Err(InstanceError::InvalidResponse),
    };
    protocol::parse_response(&line, proof, nonce)
}

fn read_endpoint_file(directory: &Path) -> Option<EndpointRecord> {
    let file = File::open(directory.join(ENDPOINT_FILE)).ok()?;
    let mut bytes = Vec::new();
    file.take(protocol::MAX_ENDPOINT_FILE as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    EndpointRecord::parse(&bytes)
}

/// Writes the endpoint file through a temporary file and a rename, so a
/// reader sees either no file or a complete one.
fn write_private_atomically(directory: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = directory.join(ENDPOINT_TEMP_FILE);
    remove_if_present(&temporary)?;
    let mut file = platform::open_private(&temporary, true)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temporary, directory.join(ENDPOINT_FILE))
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// 256 random bits as 64 lowercase hex digits: proofs, nonces, pipe names.
fn random_hex() -> Result<String, InstanceError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| InstanceError::Random)?;
    Ok(protocol::hex(&bytes))
}

fn io_error_kind(error: &io::Error) -> String {
    format!("{:?}", error.kind())
}

#[cfg(test)]
mod tests;
