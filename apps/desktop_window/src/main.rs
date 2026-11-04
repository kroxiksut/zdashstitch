// SPDX-License-Identifier: MIT OR Apache-2.0

//! `zdashstitch-window`: the program window (O-036).
//!
//! Started by the `zdashstitch` launcher, never by the user. Reads one
//! request line (bootstrap URL, first title) from stdin, shows that loopback
//! origin on the system web engine and reports on stdout; closing stdin
//! closes the window. The page is a normal client of the local server: the
//! bootstrap exchange, session cookie, Origin/Host and CSRF checks apply as
//! in a browser.
//!
//! Window properties:
//! - only `http://127.0.0.1:<port>` of the request is shown; `https://`
//!   pages elsewhere open in the system browser when clicked, everything
//!   else is refused (`zdashstitch_desktop::navigation`);
//! - `window.open` and `target=_blank` never create a window;
//! - camera, microphone, location, notifications and other permission
//!   requests are denied;
//! - developer tools and browser accelerator keys only in debug builds;
//! - in-private profile: no cookies, cache or storage kept on disk.

#![cfg_attr(not(test), windows_subsystem = "windows")]

use std::{
    cell::RefCell,
    io::{self, BufRead as _, Read as _, Write as _},
    process::ExitCode,
    rc::Rc,
    time::Instant,
};

use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    window::{Window, WindowBuilder},
};
use url::Url;
use wry::{NewWindowResponse, PermissionResponse, WebContext, WebView, WebViewBuilder};
use zdashstitch_desktop::{
    navigation::{ExternalOpenGate, Navigation, NavigationPolicy},
    opener::open_in_browser,
    protocol::{
        EXIT_BAD_REQUEST, EXIT_ENGINE_MISSING, EXIT_START_FAILED, MAX_LINE, OpenRequest,
        WindowReport, window_title,
    },
};

enum UserEvent {
    /// The launcher closed stdin.
    Close,
    /// The page title changed.
    Title(String),
}

fn main() -> ExitCode {
    let request = match read_request() {
        Some(request) => request,
        None => {
            eprintln!("window.request.invalid");
            return ExitCode::from(EXIT_BAD_REQUEST);
        }
    };
    let Ok((policy, url)) = NavigationPolicy::for_bootstrap(&request.url) else {
        eprintln!("window.request.url_refused");
        return ExitCode::from(EXIT_BAD_REQUEST);
    };
    if let Err(error) = wry::webview_version() {
        eprintln!("window.engine.missing ({error})");
        report(WindowReport::EngineMissing);
        return ExitCode::from(EXIT_ENGINE_MISSING);
    }
    match run(policy, &url, &request.title) {
        Ok(never) => match never {},
        Err(code) => {
            eprintln!("{code}");
            ExitCode::from(EXIT_START_FAILED)
        }
    }
}

/// One bounded line from stdin.
fn read_request() -> Option<OpenRequest> {
    let mut line = String::new();
    io::stdin()
        .lock()
        .take(MAX_LINE as u64)
        .read_line(&mut line)
        .ok()?;
    if !line.ends_with('\n') {
        return None;
    }
    OpenRequest::decode(line.trim_end()).ok()
}

fn report(report: WindowReport) {
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{}", report.as_line());
    let _ = stdout.flush();
}

/// Never returns on success: the event loop exits the process.
fn run(
    policy: NavigationPolicy,
    url: &Url,
    title: &str,
) -> Result<std::convert::Infallible, &'static str> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let fallback_title = window_title(title, "ZDashstitch");
    let window = WindowBuilder::new()
        .with_title(&fallback_title)
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .with_min_inner_size(LogicalSize::new(800.0, 560.0))
        .build(&event_loop)
        .map_err(|_| "window.create.failed")?;
    set_icon(&window);

    let mut context = WebContext::new(data_directory());
    let webview = build_webview(
        &window,
        &mut context,
        policy,
        url,
        event_loop.create_proxy(),
    )
    .map_err(|_| "window.webview.failed")?;
    report(WindowReport::Ready);
    watch_stdin(event_loop.create_proxy());

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        // The web view and its context live as long as the loop.
        let _ = (&webview, &context);
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => report(WindowReport::CloseRequested),
            Event::UserEvent(UserEvent::Close) => *control_flow = ControlFlow::Exit,
            Event::UserEvent(UserEvent::Title(page_title)) => {
                window.set_title(&window_title(&page_title, &fallback_title));
            }
            _ => {}
        }
    })
}

fn build_webview(
    window: &Window,
    context: &mut WebContext,
    policy: NavigationPolicy,
    url: &Url,
    proxy: EventLoopProxy<UserEvent>,
) -> wry::Result<WebView> {
    let gate = Rc::new(RefCell::new(ExternalOpenGate::default()));
    let navigation_gate = Rc::clone(&gate);
    let builder = WebViewBuilder::new_with_web_context(context)
        .with_url(url.as_str())
        .with_devtools(cfg!(debug_assertions))
        .with_incognito(true)
        .with_navigation_handler(move |target| match policy.decide(&target) {
            Navigation::Allow => true,
            Navigation::External(external) => {
                open_external(&navigation_gate, &external);
                false
            }
            Navigation::Block => false,
        })
        .with_new_window_req_handler(move |target, _features| {
            if let Navigation::External(external) = policy.decide(&target) {
                open_external(&gate, &external);
            }
            NewWindowResponse::Deny
        })
        .with_permission_handler(|_| PermissionResponse::Deny)
        .with_document_title_changed_handler(move |page_title| {
            let _ = proxy.send_event(UserEvent::Title(page_title));
        });

    #[cfg(windows)]
    {
        use wry::WebViewBuilderExtWindows as _;
        builder
            .with_browser_accelerator_keys(cfg!(debug_assertions))
            .build(window)
    }
    #[cfg(not(windows))]
    {
        use tao::platform::unix::WindowExtUnix as _;
        use wry::WebViewBuilderExtUnix as _;
        let vbox = window
            .default_vbox()
            .ok_or(wry::Error::UnsupportedWindowHandle)?;
        builder.build_gtk(vbox)
    }
}

/// A clicked `https://` page elsewhere opens in the system browser, at most
/// once per interval.
fn open_external(gate: &RefCell<ExternalOpenGate>, url: &Url) {
    if gate.borrow_mut().admit(Instant::now()) && open_in_browser(url).is_err() {
        eprintln!("window.external.open_failed");
    }
}

/// Closing stdin (or the launcher ending) closes the window.
fn watch_stdin(proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        let mut sink = [0_u8; 256];
        let mut stdin = io::stdin().lock();
        while matches!(stdin.read(&mut sink), Ok(read) if read > 0) {}
        let _ = proxy.send_event(UserEvent::Close);
    });
}

/// The WebView2 user data folder: `%LOCALAPPDATA%\ZDashstitch\webview2`
/// (otherwise WebView2 writes next to the executable). The in-private
/// profile keeps no cookies or storage there. WebKitGTK needs none.
fn data_directory() -> Option<std::path::PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(|path| path.join("ZDashstitch").join("webview2"))
}

/// The application icon `assets/brand/icons/zdashstitch.ico` (in-house
/// artwork, not Tabler): next to the executable, else in the source tree.
/// Linux takes the icon from the desktop entry (P6-01).
#[cfg(windows)]
fn set_icon(window: &Window) {
    use tao::{platform::windows::IconExtWindows as _, window::Icon};

    const ICON: &str = "zdashstitch.ico";
    let beside = std::env::current_exe()
        .ok()
        .map(|path| path.with_file_name(ICON));
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/brand/icons")
        .join(ICON);
    let icon = beside
        .into_iter()
        .chain([source])
        .find(|path| path.is_file())
        .and_then(|path| Icon::from_path(path, None).ok());
    window.set_window_icon(icon);
}

#[cfg(not(windows))]
fn set_icon(_window: &Window) {}
