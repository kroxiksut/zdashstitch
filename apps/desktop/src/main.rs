// SPDX-License-Identifier: MIT OR Apache-2.0

//! `zdashstitch`: the desktop launcher (O-036). See the crate documentation.

use std::{
    env,
    path::PathBuf,
    process::{ExitCode, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdout, Command},
    sync::oneshot,
};
use url::Url;
use zdashstitch_application::{MessageId, SessionEndError};
use zdashstitch_desktop::{
    catalog::Catalog,
    cli::{self, BROWSER_ENV, Command as CliCommand, Mode, USAGE},
    fallback::{self, Choice, FallbackDialog, Platform, WEBVIEW2_DOWNLOAD_URL, WindowFailure},
    locale,
    opener::open_in_browser,
    protocol::{MAX_LINE, OpenRequest, WINDOW_EXECUTABLE, WindowReport},
};
use zdashstitch_local_server::{
    LocalSecurity, end_session_on_close, end_session_on_exit,
    instance::{self, InstanceConfig},
    startup::{self, Launch, LaunchSettings, LocalServer},
};

/// How long the window may take to create its web view (a first WebView2
/// start creates its profile).
const READY_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the window may take to close after stdin was closed.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long running requests may take once the server stops.
const SERVER_STOP_TIMEOUT: Duration = Duration::from_secs(10);
/// Upper bound of waiting for the erase of an ended session before a new
/// bootstrap code is issued (as for a repeated launch).
const BOOTSTRAP_ERASE_WAIT: Duration = Duration::from_secs(30);

fn main() -> ExitCode {
    let command = cli::parse(env::args_os().skip(1), env::var_os(BROWSER_ENV).as_deref());
    let mode = match command {
        Ok(CliCommand::Run(mode)) => mode,
        Ok(CliCommand::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("{}\n\n{USAGE}", error.code());
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("startup.runtime.failed ({:?})", error.kind());
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(run(mode))
}

/// Native text, resolved once at start.
struct Texts {
    catalog: Catalog,
}

impl Texts {
    fn load() -> Self {
        Self {
            catalog: Catalog::new(locale::current()),
        }
    }

    fn text(&self, id: MessageId, params: &[(&str, &str)]) -> String {
        self.catalog.text(id, params)
    }
}

async fn run(mode: Mode) -> ExitCode {
    let settings = match LaunchSettings::from_env() {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let texts = Arc::new(Texts::load());
    match startup::launch(&settings).await {
        Ok(Launch::Primary(server)) => primary(server, mode, &texts).await,
        Ok(Launch::Running) => repeated(settings.instance(), mode, &texts).await,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// This process serves. Closing the program window ends the local session
/// (as «End session» does) and stops the server.
async fn primary(server: Box<LocalServer>, mode: Mode, texts: &Arc<Texts>) -> ExitCode {
    let security = server.security();
    let bootstrap_url = server.bootstrap_url().to_owned();
    let (stop, stopped) = oneshot::channel::<()>();
    let serving = tokio::spawn(server.serve(async move {
        let _ = stopped.await;
    }));

    let outcome = match mode {
        Mode::Browser => WindowOutcome::BrowserRequested,
        Mode::Window => run_window(&bootstrap_url, texts, Some(&security)).await,
    };
    let keep_serving = match outcome {
        WindowOutcome::Closed => false,
        WindowOutcome::Ended => {
            end_session_on_exit(&security).await;
            false
        }
        WindowOutcome::BrowserRequested => {
            // The initial code may have been handed to a window that failed:
            // the browser gets a fresh one.
            let url = if mode == Mode::Browser {
                Some(bootstrap_url)
            } else {
                fresh_bootstrap_url(&security).await
            };
            if let Some(url) = url {
                open_browser(&url, texts);
            }
            true
        }
        WindowOutcome::Failed(failure) => match ask_fallback(texts, failure).await {
            Choice::InstallAndRestart => false,
            Choice::Browser | Choice::DialogUnavailable => {
                if let Some(url) = fresh_bootstrap_url(&security).await {
                    open_browser(&url, texts);
                }
                true
            }
        },
    };
    if keep_serving {
        // Browser mode: the server runs until Ctrl+C, as before.
        ctrl_c().await;
        end_session_on_exit(&security).await;
    }
    let _ = stop.send(());
    match tokio::time::timeout(SERVER_STOP_TIMEOUT, serving).await {
        Ok(Ok(Ok(()))) | Err(_) => ExitCode::SUCCESS,
        Ok(Ok(Err(error))) => {
            eprintln!("startup.serve.failed ({:?})", error.kind());
            ExitCode::FAILURE
        }
        Ok(Err(_)) => ExitCode::FAILURE,
    }
}

/// Another process serves: this launch shows a further window (or browser
/// tab) of it, like a repeated launch of the browser-only server. Closing
/// that window only closes it.
async fn repeated(config: &InstanceConfig, mode: Mode, texts: &Arc<Texts>) -> ExitCode {
    let Some(url) = running_bootstrap_url(config).await else {
        return ExitCode::FAILURE;
    };
    let outcome = match mode {
        Mode::Browser => WindowOutcome::BrowserRequested,
        Mode::Window => run_window(&url, texts, None).await,
    };
    let browser_url = match outcome {
        WindowOutcome::Closed | WindowOutcome::Ended => return ExitCode::SUCCESS,
        WindowOutcome::BrowserRequested => Some(url),
        WindowOutcome::Failed(failure) => match ask_fallback(texts, failure).await {
            Choice::InstallAndRestart => return ExitCode::SUCCESS,
            Choice::Browser | Choice::DialogUnavailable => running_bootstrap_url(config).await,
        },
    };
    match browser_url {
        Some(url) => {
            open_browser(&url, texts);
            ExitCode::SUCCESS
        }
        None => ExitCode::FAILURE,
    }
}

async fn running_bootstrap_url(config: &InstanceConfig) -> Option<String> {
    match instance::request_bootstrap(config).await {
        Ok(url) => Some(url),
        Err(error) => {
            eprintln!(
                "the running instance issued no bootstrap URL: {}",
                error.code()
            );
            None
        }
    }
}

async fn fresh_bootstrap_url(security: &LocalSecurity) -> Option<String> {
    match security.issue_bootstrap(BOOTSTRAP_ERASE_WAIT).await {
        Ok(code) => Some(security.bootstrap_url(&code)),
        Err(error) => {
            eprintln!("no bootstrap code was issued: {}", error.code());
            None
        }
    }
}

/// Opens the loopback bootstrap URL in the system browser; when that fails,
/// prints it (stdout only, never a log file) for the user to open.
fn open_browser(url: &str, texts: &Texts) {
    let opened = Url::parse(url)
        .ok()
        .is_some_and(|url| open_in_browser(&url).is_ok());
    if !opened {
        println!(
            "{}",
            texts.text(MessageId::DESKTOP_BROWSER_OPEN_FAILED, &[("url", url)])
        );
    }
}

async fn ask_fallback(texts: &Arc<Texts>, failure: WindowFailure) -> Choice {
    let platform = Platform::current();
    let dialog = FallbackDialog::new(&texts.catalog, failure, platform);
    let shown = dialog.clone();
    let choice = tokio::task::spawn_blocking(move || shown.show())
        .await
        .unwrap_or(Choice::DialogUnavailable);
    match choice {
        Choice::DialogUnavailable => {
            eprintln!("{}\n\n{}", dialog.title, dialog.body);
            eprintln!(
                "{}",
                texts.text(MessageId::DESKTOP_FALLBACK_OPENING_BROWSER, &[])
            );
        }
        Choice::InstallAndRestart if platform == Platform::Windows => {
            // The user chose to install: show the download page.
            if let Ok(url) = Url::parse(WEBVIEW2_DOWNLOAD_URL) {
                let _ = open_in_browser(&url);
            }
        }
        Choice::InstallAndRestart | Choice::Browser => {}
    }
    choice
}

/// How the program window ended.
enum WindowOutcome {
    /// The user closed it and, for the primary, the session was ended.
    Closed,
    /// It ended without a close request (crash, killed, Ctrl+C).
    Ended,
    /// The browser was asked for instead of a window.
    BrowserRequested,
    /// It did not start.
    Failed(WindowFailure),
}

/// Starts the window process with `url` and supervises it until it ends.
/// `security` is the primary's session: a close request then ends it first,
/// and is refused while a write to Zabbix runs.
async fn run_window(
    url: &str,
    texts: &Arc<Texts>,
    security: Option<&LocalSecurity>,
) -> WindowOutcome {
    let mut window = match WindowProcess::start(url, texts).await {
        Ok(window) => window,
        Err(failure) => return WindowOutcome::Failed(failure),
    };
    let dialog_open = Arc::new(AtomicBool::new(false));
    let outcome = loop {
        tokio::select! {
            report = window.next_report() => match report {
                Some(WindowReport::CloseRequested) => match security {
                    None => break WindowOutcome::Closed,
                    Some(security) => match end_session_on_close(security).await {
                        Ok(()) | Err(SessionEndError::NotConfirmed) => break WindowOutcome::Closed,
                        Err(SessionEndError::Writing) => show_close_refused(texts, &dialog_open),
                    },
                },
                Some(WindowReport::Ready | WindowReport::EngineMissing) => {}
                None => break WindowOutcome::Ended,
            },
            () = ctrl_c() => break WindowOutcome::Ended,
        }
    };
    window.close().await;
    outcome
}

/// A write is running: the window stays open and says why.
fn show_close_refused(texts: &Arc<Texts>, dialog_open: &Arc<AtomicBool>) {
    if dialog_open.swap(true, Ordering::SeqCst) {
        return;
    }
    let title = texts.text(MessageId::DESKTOP_CLOSE_WRITING_TITLE, &[]);
    let body = texts.text(MessageId::DESKTOP_CLOSE_WRITING_BODY, &[]);
    let dialog_open = Arc::clone(dialog_open);
    tokio::task::spawn_blocking(move || {
        let _ = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Info)
            .set_title(title)
            .set_description(body)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
        dialog_open.store(false, Ordering::SeqCst);
    });
}

struct WindowProcess {
    child: Child,
    reports: BufReader<ChildStdout>,
}

impl WindowProcess {
    /// Spawns the window next to this executable, hands it the request over
    /// stdin and waits for `ready`.
    async fn start(url: &str, texts: &Texts) -> Result<Self, WindowFailure> {
        let title = texts.text(MessageId::COMMON_PRODUCT_NAME, &[]);
        let request = OpenRequest::new(url.to_owned(), title)
            .encode()
            .map_err(|_| WindowFailure::StartFailed)?;
        let mut child = Command::new(window_executable().ok_or(WindowFailure::StartFailed)?)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                eprintln!("window.spawn.failed ({:?})", error.kind());
                WindowFailure::StartFailed
            })?;
        let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(WindowFailure::StartFailed);
        };
        // A window that dies at once (missing library) closes the pipe.
        let _ = stdin.write_all(&request).await;
        let _ = stdin.flush().await;
        child.stdin = Some(stdin);
        let mut process = Self {
            child,
            reports: BufReader::with_capacity(MAX_LINE, stdout),
        };
        match tokio::time::timeout(READY_TIMEOUT, process.wait_ready()).await {
            Ok(Ok(())) => Ok(process),
            Ok(Err(engine_missing)) => {
                let code = process.exit_code().await;
                Err(fallback::classify(
                    engine_missing,
                    code,
                    Platform::current(),
                ))
            }
            Err(_) => {
                eprintln!("window.ready.timeout");
                let _ = process.child.kill().await;
                Err(WindowFailure::StartFailed)
            }
        }
    }

    /// `Err(engine_missing)` when the window ended before `ready`.
    async fn wait_ready(&mut self) -> Result<(), bool> {
        let mut engine_missing = false;
        loop {
            match self.next_report().await {
                Some(WindowReport::Ready) => return Ok(()),
                Some(WindowReport::EngineMissing) => engine_missing = true,
                Some(WindowReport::CloseRequested) => {}
                None => return Err(engine_missing),
            }
        }
    }

    /// The next known report; `None` once stdout closed or a line is longer
    /// than [`MAX_LINE`] (the window then is not trusted to run on).
    async fn next_report(&mut self) -> Option<WindowReport> {
        let mut line = Vec::with_capacity(64);
        loop {
            line.clear();
            (&mut self.reports)
                .take(MAX_LINE as u64)
                .read_until(b'\n', &mut line)
                .await
                .ok()?;
            if line.last() != Some(&b'\n') {
                return None;
            }
            if let Some(report) = std::str::from_utf8(&line)
                .ok()
                .and_then(WindowReport::parse)
            {
                return Some(report);
            }
        }
    }

    async fn exit_code(&mut self) -> Option<i32> {
        match tokio::time::timeout(CLOSE_TIMEOUT, self.child.wait()).await {
            Ok(Ok(status)) => status.code(),
            _ => {
                let _ = self.child.kill().await;
                None
            }
        }
    }

    /// Closing stdin tells the window to close; it is killed if it does not.
    async fn close(mut self) {
        drop(self.child.stdin.take());
        if tokio::time::timeout(CLOSE_TIMEOUT, self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.kill().await;
        }
    }
}

fn window_executable() -> Option<PathBuf> {
    let path = env::current_exe().ok()?.with_file_name(WINDOW_EXECUTABLE);
    if path.is_file() {
        Some(path)
    } else {
        eprintln!("window.executable.missing");
        None
    }
}

/// Resolves on Ctrl+C; never when no handler can be installed.
async fn ctrl_c() {
    if tokio::signal::ctrl_c().await.is_err() {
        std::future::pending::<()>().await;
    }
}
