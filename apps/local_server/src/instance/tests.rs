// SPDX-License-Identifier: MIT OR Apache-2.0

//! Single-instance channel: lock, endpoint takeover, protocol errors over
//! the real platform channel. Every test uses its own temporary directory
//! and a random channel name, so a running instance of the user is never
//! reached.

use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use super::*;

const PORT: u16 = 43191;
const NONCE: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

fn temp_dir() -> tempfile::TempDir {
    InstanceConfig::private_temp_dir()
}

fn security() -> LocalSecurity {
    LocalSecurity::initialize(PORT)
        .unwrap_or_else(|error| panic!("security must initialize: {error}"))
        .security
}

pub(crate) fn primary(config: &InstanceConfig) -> PrimaryLock {
    match acquire(config) {
        Ok(Acquired::Primary(lock)) => lock,
        other => panic!("expected the primary lock, got {other:?}"),
    }
}

pub(crate) fn serve(config: &InstanceConfig, security: LocalSecurity) -> InstanceServer {
    primary(config)
        .serve(security, config)
        .unwrap_or_else(|error| panic!("serve: {error}"))
}

fn endpoint(config: &InstanceConfig) -> EndpointRecord {
    read_endpoint_file(config.directory()).unwrap_or_else(|| panic!("endpoint file"))
}

/// Connects like a client; a Windows pipe is briefly busy until the server
/// has created the next instance.
async fn open(config: &InstanceConfig) -> platform::Stream {
    let record = endpoint(config);
    let mut attempts = 0;
    loop {
        match platform::connect(config.directory(), &record.endpoint).await {
            Ok(stream) => return stream,
            Err(error) if attempts >= 100 => panic!("connect: {error}"),
            Err(_) => {
                attempts += 1;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
}

/// Reads until the server closes; the bytes it sent before.
async fn read_to_close<S: AsyncRead + Unpin>(stream: &mut S) -> Vec<u8> {
    let mut received = Vec::new();
    let mut chunk = [0_u8; 256];
    loop {
        match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk)).await {
            Ok(Ok(0) | Err(_)) => return received,
            Ok(Ok(read)) => received.extend_from_slice(&chunk[..read]),
            Err(_) => panic!("the server did not close the channel"),
        }
    }
}

/// Sends `bytes` as they are; everything the server sent before closing.
async fn raw(config: &InstanceConfig, bytes: &[u8]) -> Vec<u8> {
    let mut stream = open(config).await;
    if !bytes.is_empty() {
        stream
            .write_all(bytes)
            .await
            .unwrap_or_else(|error| panic!("write: {error}"));
    }
    read_to_close(&mut stream).await
}

/// The single response line to `bytes`, verified with the endpoint proof.
async fn answered(config: &InstanceConfig, bytes: &[u8]) -> Value {
    let proof = endpoint(config).proof;
    let mut received = raw(config, bytes).await;
    assert_eq!(received.pop(), Some(b'\n'), "one response line");
    let value: Value =
        serde_json::from_slice(&received).unwrap_or_else(|error| panic!("response JSON: {error}"));
    assert_eq!(value["v"], protocol::VERSION);
    assert!(value.get("proof").is_none(), "the proof is never sent");
    let (kind, reply) = match (value["url"].as_str(), value["error"].as_str()) {
        (Some(url), None) => ("url", url),
        (None, Some(code)) => ("error", code),
        _ => panic!("url or error: {value}"),
    };
    assert_eq!(
        value["mac"],
        protocol::response_mac(&proof, NONCE, kind, reply)
    );
    value
}

async fn wait_until(condition: impl Fn() -> bool) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

#[test]
fn a_second_acquire_in_the_same_directory_finds_the_running_instance() {
    let dir = temp_dir();
    let config = InstanceConfig::for_test(dir.path());
    let first = primary(&config);
    assert!(matches!(acquire(&config), Ok(Acquired::Running)));
    assert!(matches!(acquire(&config), Ok(Acquired::Running)));
    drop(first);
    // The lock is released with its holder; the next launch takes over.
    let _again = primary(&config);
}

#[test]
fn a_relative_directory_is_refused() {
    let config = InstanceConfig::at(PathBuf::from("relative-instance-dir"));
    assert_eq!(
        acquire(&config).err(),
        Some(InstanceError::DirectoryUnavailable)
    );
    assert_eq!(
        InstanceConfig::from_override(PathBuf::from("relative-instance-dir")).err(),
        Some(InstanceError::DirectoryUnavailable)
    );
}

#[test]
fn taking_the_lock_removes_a_stale_endpoint_file_at_once() {
    let dir = temp_dir();
    let config = InstanceConfig::for_test(dir.path());
    std::fs::write(dir.path().join(ENDPOINT_FILE), b"stale")
        .unwrap_or_else(|error| panic!("stale endpoint: {error}"));
    std::fs::write(dir.path().join(ENDPOINT_TEMP_FILE), b"partial")
        .unwrap_or_else(|error| panic!("stale temporary file: {error}"));
    let first = primary(&config);
    assert!(!dir.path().join(ENDPOINT_FILE).exists());
    assert!(!dir.path().join(ENDPOINT_TEMP_FILE).exists());
    // A repeated launch never removes the endpoint of the running holder.
    std::fs::write(dir.path().join(ENDPOINT_FILE), b"live")
        .unwrap_or_else(|error| panic!("endpoint: {error}"));
    assert!(matches!(acquire(&config), Ok(Acquired::Running)));
    assert!(dir.path().join(ENDPOINT_FILE).exists());
    drop(first);
}

#[tokio::test]
async fn a_repeated_launch_gets_a_fresh_loopback_bootstrap_url() {
    let dir = temp_dir();
    let config = InstanceConfig::for_test(dir.path());
    let _server = serve(&config, security());
    assert!(matches!(acquire(&config), Ok(Acquired::Running)));

    let first = request_bootstrap(&config)
        .await
        .unwrap_or_else(|error| panic!("bootstrap: {error}"));
    let second = request_bootstrap(&config)
        .await
        .unwrap_or_else(|error| panic!("bootstrap: {error}"));
    assert!(protocol::is_bootstrap_url(&first), "{first}");
    assert!(first.starts_with(&format!("http://127.0.0.1:{PORT}/?bootstrap=")));
    assert_ne!(first, second);
}

#[tokio::test]
async fn unauthenticatable_silent_partial_and_oversize_requests_get_no_byte() {
    let dir = temp_dir();
    let config = InstanceConfig::for_test(dir.path());
    let _server = serve(&config, security());
    let oversize = vec![b'a'; protocol::MAX_REQUEST + 44];
    let mut oversize_line =
        format!("{{\"v\":2,\"req\":\"bootstrap\",\"nonce\":\"{NONCE}\",\"pad\":\"").into_bytes();
    oversize_line.extend(std::iter::repeat_n(b'x', protocol::MAX_REQUEST));
    oversize_line.extend_from_slice(b"\"}\n");
    let without_newline = protocol::request_line(NONCE);
    let without_newline = &without_newline[..without_newline.len() - 1];
    for bytes in [
        &b""[..],
        b"nope\n",
        b"{\"v\":2}\n",
        // Protocol version 1 (static proof echo) is no longer answered.
        b"{\"v\":1,\"req\":\"bootstrap\"}\n",
        b"{\"v\":3,\"req\":\"bootstrap\"}\n",
        b"{\"v\":2,\"req\":\"bootstrap\",\"nonce\":\"short\"}\n",
        &oversize,
        &oversize_line,
        without_newline,
    ] {
        assert_eq!(
            raw(&config, bytes).await,
            Vec::<u8>::new(),
            "{}",
            String::from_utf8_lossy(bytes)
        );
    }
    // The channel still serves valid requests afterwards.
    let response = answered(&config, &protocol::request_line(NONCE)).await;
    assert!(
        response["url"]
            .as_str()
            .is_some_and(protocol::is_bootstrap_url)
    );
}

#[tokio::test]
async fn a_refused_request_with_a_nonce_gets_an_authenticated_code() {
    let dir = temp_dir();
    let config = InstanceConfig::for_test(dir.path());
    let _server = serve(&config, security());
    for (line, code) in [
        (
            format!("{{\"v\":2,\"req\":\"shutdown\",\"nonce\":\"{NONCE}\"}}\n"),
            protocol::UNKNOWN_REQUEST,
        ),
        (
            format!("{{\"v\":2,\"req\":\"bootstrap\",\"nonce\":\"{NONCE}\",\"x\":1}}\n"),
            protocol::MALFORMED,
        ),
    ] {
        let response = answered(&config, line.as_bytes()).await;
        assert_eq!(response["error"], code, "{line}");
        assert!(response.get("url").is_none());
    }
}

#[tokio::test]
async fn idle_connections_do_not_block_a_repeated_launch() {
    let dir = temp_dir();
    let config = InstanceConfig::for_test(dir.path());
    let _server = serve(&config, security());
    // More idle connections than requests served at once (the old design
    // answered the fifth with `instance.busy` at once).
    let mut idle = Vec::new();
    for _ in 0..CONCURRENT_REQUESTS * 2 {
        idle.push(open(&config).await);
    }
    let url = request_bootstrap(&config)
        .await
        .unwrap_or_else(|error| panic!("bootstrap beside idle connections: {error}"));
    assert!(protocol::is_bootstrap_url(&url));
    drop(idle);
}

#[tokio::test]
async fn a_repeated_launch_retries_past_the_connection_cap() {
    let dir = temp_dir();
    let mut config = InstanceConfig::for_test(dir.path());
    config.max_connections = 2;
    // A generous window keeps the test stable on loaded CI runners; the idle
    // connections still free their slots after the short test read timeout.
    config.connect_window = Duration::from_secs(15);
    let _server = serve(&config, security());
    let mut idle = Vec::new();
    for _ in 0..config.max_connections {
        idle.push(open(&config).await);
    }
    // The capped connection is closed without a byte; the client retries
    // until the idle ones time out (300 ms) within its connect window.
    let started = std::time::Instant::now();
    let url = request_bootstrap(&config)
        .await
        .unwrap_or_else(|error| panic!("bootstrap after the cap: {error}"));
    assert!(protocol::is_bootstrap_url(&url));
    assert!(started.elapsed() < config.connect_window);
    drop(idle);
}

#[tokio::test]
async fn a_stale_endpoint_of_an_ended_process_is_taken_over() {
    let dir = temp_dir();
    let config = InstanceConfig::for_test(dir.path());
    // What a crashed instance leaves behind: an endpoint file naming a
    // channel nobody serves (and, on Linux, the socket file itself).
    #[cfg(windows)]
    let stale = r"\\.\pipe\zdashstitch-00000000000000000000000000000000";
    #[cfg(unix)]
    let stale = "instance.sock";
    #[cfg(unix)]
    std::fs::write(dir.path().join("instance.sock"), b"")
        .unwrap_or_else(|error| panic!("stale socket: {error}"));
    let record = EndpointRecord {
        v: protocol::VERSION,
        endpoint: stale.to_owned(),
        proof: "a".repeat(64),
    };
    std::fs::write(dir.path().join(ENDPOINT_FILE), record.encode())
        .unwrap_or_else(|error| panic!("stale endpoint: {error}"));

    let mut unreachable = config.clone();
    unreachable.connect_window = Duration::from_millis(300);
    assert_eq!(
        request_bootstrap(&unreachable).await.err(),
        Some(InstanceError::Unreachable)
    );

    let server = serve(&config, security());
    assert_ne!(endpoint(&config).proof, "a".repeat(64));
    let url = request_bootstrap(&config)
        .await
        .unwrap_or_else(|error| panic!("bootstrap after takeover: {error}"));
    assert!(protocol::is_bootstrap_url(&url));

    // A clean stop removes the endpoint and releases the lock.
    drop(server);
    assert!(!dir.path().join(ENDPOINT_FILE).exists());
    let _next = primary(&config);
}

#[tokio::test]
async fn a_response_without_a_mac_of_the_endpoint_proof_is_not_trusted() {
    let dir = temp_dir();
    let config = InstanceConfig::for_test(dir.path());
    let _server = serve(&config, security());
    // The endpoint file names the live channel but another proof, as for
    // a channel that someone else serves under a known name.
    let mut record = endpoint(&config);
    record.proof = "b".repeat(64);
    std::fs::write(dir.path().join(ENDPOINT_FILE), record.encode())
        .unwrap_or_else(|error| panic!("endpoint: {error}"));
    assert_eq!(
        request_bootstrap(&config).await.err(),
        Some(InstanceError::Unauthenticated)
    );
}

/// Accepts what the test sends: errors or in-memory streams.
struct Scripted(tokio::sync::mpsc::UnboundedReceiver<io::Result<tokio::io::DuplexStream>>);

impl Accept for Scripted {
    type Stream = tokio::io::DuplexStream;

    async fn accept(&mut self) -> io::Result<tokio::io::DuplexStream> {
        match self.0.recv().await {
            Some(next) => next,
            None => std::future::pending().await,
        }
    }
}

#[tokio::test]
async fn accept_failures_withdraw_the_endpoint_until_the_channel_recovers() {
    let dir = temp_dir();
    let config = InstanceConfig::for_test(dir.path());
    let (script, accepted) = tokio::sync::mpsc::unbounded_channel();
    let server = primary(&config)
        .serve_with(
            Scripted(accepted),
            "scripted".to_owned(),
            None,
            security(),
            &config,
        )
        .unwrap_or_else(|error| panic!("serve: {error}"));
    let file = dir.path().join(ENDPOINT_FILE);
    let proof = endpoint(&config).proof;

    // Fewer failures than the threshold keep the endpoint.
    for _ in 1..WITHDRAW_AFTER {
        let _ = script.send(Err(io::ErrorKind::Other.into()));
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(file.exists());
    let _ = script.send(Err(io::ErrorKind::Other.into()));
    assert!(
        wait_until(|| !file.exists()).await,
        "withdrawn after the failures"
    );

    // The loop kept running: the next connection is served and the
    // endpoint is published again.
    let (mut client, server_side) = tokio::io::duplex(1024);
    let _ = script.send(Ok(server_side));
    client
        .write_all(&protocol::request_line(NONCE))
        .await
        .unwrap_or_else(|error| panic!("write: {error}"));
    let line = read_line(&mut client, protocol::MAX_RESPONSE)
        .await
        .unwrap_or_else(|_| panic!("response line"));
    let url = protocol::parse_response(&line, &proof, NONCE)
        .unwrap_or_else(|error| panic!("response: {error}"));
    assert!(protocol::is_bootstrap_url(&url));
    assert!(wait_until(|| file.exists()).await, "published again");

    // A stopped server never republishes.
    drop(server);
    assert!(!file.exists());
}

#[test]
fn accept_backoff_doubles_up_to_its_cap() {
    let base = Duration::from_millis(100);
    assert_eq!(accept_backoff(base, 1), base);
    assert_eq!(accept_backoff(base, 2), base * 2);
    assert_eq!(accept_backoff(base, 4), base * 8);
    assert_eq!(accept_backoff(base, 40), ACCEPT_BACKOFF_MAX);
}

#[cfg(windows)]
#[tokio::test]
async fn a_read_only_pipe_client_receives_nothing() {
    let dir = temp_dir();
    let config = InstanceConfig::for_test(dir.path());
    let _server = serve(&config, security());
    let record = endpoint(&config);
    // Everyone may open the default-DACL pipe for reading (reviewer's
    // experiment: `PipeDirection.In`); such a client cannot send a request.
    let mut reader = tokio::net::windows::named_pipe::ClientOptions::new()
        .write(false)
        .open(&record.endpoint)
        .unwrap_or_else(|error| panic!("read-only connect: {error}"));
    assert_eq!(read_to_close(&mut reader).await, Vec::<u8>::new());
}

#[cfg(windows)]
#[tokio::test]
async fn a_pipe_name_that_already_exists_is_never_joined_by_the_server() {
    let random = "0123456789abcdef0123456789abcdef";
    let _squatted = tokio::net::windows::named_pipe::ServerOptions::new()
        .create(format!(r"\\.\pipe\zdashstitch-{random}"))
        .unwrap_or_else(|error| panic!("squat: {error}"));
    assert!(platform::Listener::bind(temp_dir().path(), random).is_err());
}

#[cfg(windows)]
#[test]
fn a_windows_override_must_lie_inside_local_app_data() {
    // `%TEMP%` usually lies inside `%LOCALAPPDATA%`; a directory under the
    // volume root or the parent of `%LOCALAPPDATA%` does not.
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("LOCALAPPDATA"));
    let outside = local
        .parent()
        .map(|parent| parent.join("zdashstitch-override-test-outside"))
        .unwrap_or_else(|| panic!("parent of LOCALAPPDATA"));
    assert_eq!(
        InstanceConfig::from_override(outside.clone()).err(),
        Some(InstanceError::DirectoryInsecure)
    );
    assert!(!outside.exists(), "nothing is created outside");
    let shared = PathBuf::from(r"C:\temp\zdashstitch-override-test-shared");
    assert_eq!(
        InstanceConfig::from_override(shared.clone()).err(),
        Some(InstanceError::DirectoryInsecure)
    );
    assert!(!shared.exists(), "nothing is created outside");
    assert_eq!(
        InstanceConfig::from_override(local.join("..").join("x")).err(),
        Some(InstanceError::DirectoryInsecure)
    );
    assert_eq!(
        InstanceConfig::from_override(local.clone()).err(),
        Some(InstanceError::DirectoryInsecure),
        "the base itself is not a private subdirectory"
    );

    let inside = tempfile::Builder::new()
        .prefix("zdashstitch-override-test-")
        .tempdir_in(&local)
        .unwrap_or_else(|error| panic!("temp dir in LOCALAPPDATA: {error}"));
    let config = InstanceConfig::from_override(inside.path().join("runtime"))
        .unwrap_or_else(|error| panic!("override inside: {error}"));
    let _lock = primary(&config);

    // A junction inside `%LOCALAPPDATA%` that points outside is refused.
    let target = tempfile::tempdir().unwrap_or_else(|error| panic!("target: {error}"));
    let junction = inside.path().join("junction");
    let created = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&junction)
        .arg(target.path())
        .output()
        .is_ok_and(|output| output.status.success());
    if created && !target.path().starts_with(&local) {
        assert_eq!(
            InstanceConfig::from_override(junction.join("runtime")).err(),
            Some(InstanceError::DirectoryInsecure)
        );
    }
    let _ = std::fs::remove_dir(&junction);
}

#[cfg(unix)]
#[test]
fn a_directory_open_to_others_is_refused() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = temp_dir();
    let open = dir.path().join("open");
    std::fs::create_dir(&open).unwrap_or_else(|error| panic!("dir: {error}"));
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|error| panic!("chmod: {error}"));
    assert_eq!(
        acquire(&InstanceConfig::for_test(&open)).err(),
        Some(InstanceError::DirectoryInsecure)
    );
    assert_eq!(
        std::fs::read_dir(&open).map(Iterator::count).ok(),
        Some(0),
        "nothing is written into a refused directory"
    );
    let link = dir.path().join("link");
    let private = dir.path().join("private");
    std::fs::create_dir(&private).unwrap_or_else(|error| panic!("dir: {error}"));
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700))
        .unwrap_or_else(|error| panic!("chmod: {error}"));
    std::os::unix::fs::symlink(&private, &link).unwrap_or_else(|error| panic!("link: {error}"));
    assert_eq!(
        acquire(&InstanceConfig::for_test(&link)).err(),
        Some(InstanceError::DirectoryInsecure)
    );
}

#[cfg(unix)]
#[test]
fn the_ownership_probe_leaves_nothing_behind() {
    let dir = temp_dir();
    let _lock = primary(&InstanceConfig::for_test(dir.path()));
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap_or_else(|error| panic!("read dir: {error}"))
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from(LOCK_FILE)]);
}

#[cfg(unix)]
#[tokio::test]
async fn the_socket_and_files_are_private() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = temp_dir();
    let config = InstanceConfig::for_test(dir.path());
    let _server = serve(&config, security());
    for name in ["instance.sock", ENDPOINT_FILE, LOCK_FILE] {
        let mode = std::fs::metadata(dir.path().join(name))
            .unwrap_or_else(|error| panic!("{name}: {error}"))
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "{name}: {mode:o}");
    }
}
