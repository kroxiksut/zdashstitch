// SPDX-License-Identifier: MIT OR Apache-2.0

//! Line protocol between the launcher and the window process.
//!
//! The launcher writes one [`OpenRequest`] JSON line to the window's stdin;
//! a pipe of this parent and child only, so the one-time bootstrap URL never
//! appears on a command line or in the environment. Closing stdin tells the
//! window to close. The window answers on stdout with [`WindowReport`]
//! lines. Nothing else travels: the window never talks to the server other
//! than as a browser would.

use serde::{Deserialize, Serialize};

/// Version of [`OpenRequest`].
pub const VERSION: u32 = 1;
/// Upper bound of one line in either direction, newline included.
pub const MAX_LINE: usize = 8 * 1024;
/// Upper bound of a window title, in characters.
pub const MAX_TITLE_CHARS: usize = 200;

/// Window file name next to the launcher.
#[cfg(windows)]
pub const WINDOW_EXECUTABLE: &str = "zdashstitch-window.exe";
#[cfg(not(windows))]
pub const WINDOW_EXECUTABLE: &str = "zdashstitch-window";

/// Exit code of the window process when the request line is invalid.
pub const EXIT_BAD_REQUEST: u8 = 2;
/// Exit code after [`WindowReport::EngineMissing`].
pub const EXIT_ENGINE_MISSING: u8 = 3;
/// Exit code when the window or the web view could not be created.
pub const EXIT_START_FAILED: u8 = 4;

/// What the window opens.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpenRequest {
    pub v: u32,
    /// The loopback bootstrap URL; see `navigation::NavigationPolicy`.
    pub url: String,
    /// The first window title; the page title replaces it once loaded.
    pub title: String,
}

/// Why a request line was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestError {
    TooLong,
    Malformed,
    UnsupportedVersion,
}

impl OpenRequest {
    #[must_use]
    pub fn new(url: String, title: String) -> Self {
        Self {
            v: VERSION,
            url,
            title,
        }
    }

    /// The line to write, newline included.
    pub fn encode(&self) -> Result<Vec<u8>, RequestError> {
        let mut line = serde_json::to_vec(self).map_err(|_| RequestError::Malformed)?;
        line.push(b'\n');
        if line.len() > MAX_LINE {
            return Err(RequestError::TooLong);
        }
        Ok(line)
    }

    /// Parses one line without its newline.
    pub fn decode(line: &str) -> Result<Self, RequestError> {
        if line.len() >= MAX_LINE {
            return Err(RequestError::TooLong);
        }
        let request: Self = serde_json::from_str(line).map_err(|_| RequestError::Malformed)?;
        if request.v != VERSION {
            return Err(RequestError::UnsupportedVersion);
        }
        Ok(request)
    }
}

/// A line the window writes to stdout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowReport {
    /// The web view exists and loads the bootstrap URL.
    Ready,
    /// The system web engine is not installed (WebView2 Runtime missing).
    EngineMissing,
    /// The user asked to close the window; it stays open until the launcher
    /// closes stdin.
    CloseRequested,
}

impl WindowReport {
    #[must_use]
    pub const fn as_line(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::EngineMissing => "engine-missing",
            Self::CloseRequested => "close-requested",
        }
    }

    /// Parses one line; surrounding whitespace (`\r`) is ignored.
    #[must_use]
    pub fn parse(line: &str) -> Option<Self> {
        match line.trim() {
            "ready" => Some(Self::Ready),
            "engine-missing" => Some(Self::EngineMissing),
            "close-requested" => Some(Self::CloseRequested),
            _ => None,
        }
    }
}

/// A page title made safe for the native title bar: control characters
/// dropped, at most [`MAX_TITLE_CHARS`] characters, `fallback` when empty.
#[must_use]
pub fn window_title(title: &str, fallback: &str) -> String {
    let clean: String = title
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_TITLE_CHARS)
        .collect();
    let clean = clean.trim();
    if clean.is_empty() {
        fallback.to_owned()
    } else {
        clean.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_LINE, OpenRequest, RequestError, WindowReport, window_title};

    #[test]
    fn request_round_trips() {
        let request = OpenRequest::new(
            "http://127.0.0.1:4000/?bootstrap=abc".to_owned(),
            "ZDashstitch".to_owned(),
        );
        let line = request.encode().unwrap_or_default();
        assert_eq!(line.last(), Some(&b'\n'));
        let text = String::from_utf8(line).unwrap_or_default();
        assert_eq!(OpenRequest::decode(text.trim_end()), Ok(request));
    }

    #[test]
    fn invalid_requests_are_refused() {
        assert_eq!(OpenRequest::decode("{}"), Err(RequestError::Malformed));
        assert_eq!(
            OpenRequest::decode(r#"{"v":1,"url":"u","title":"t","extra":1}"#),
            Err(RequestError::Malformed)
        );
        assert_eq!(
            OpenRequest::decode(r#"{"v":2,"url":"u","title":"t"}"#),
            Err(RequestError::UnsupportedVersion)
        );
        assert_eq!(
            OpenRequest::decode(&"x".repeat(MAX_LINE)),
            Err(RequestError::TooLong)
        );
        let huge = OpenRequest::new("u".repeat(MAX_LINE), String::new());
        assert_eq!(huge.encode(), Err(RequestError::TooLong));
    }

    #[test]
    fn reports_round_trip() {
        for report in [
            WindowReport::Ready,
            WindowReport::EngineMissing,
            WindowReport::CloseRequested,
        ] {
            assert_eq!(WindowReport::parse(report.as_line()), Some(report));
        }
        assert_eq!(WindowReport::parse("ready\r"), Some(WindowReport::Ready));
        assert_eq!(WindowReport::parse("open http://x"), None);
    }

    #[test]
    fn titles_are_cleaned() {
        assert_eq!(window_title("Drafts\n\u{7}", "Z"), "Drafts");
        assert_eq!(window_title("  ", "Z"), "Z");
        assert_eq!(window_title(&"a".repeat(500), "Z").chars().count(), 200);
    }
}
