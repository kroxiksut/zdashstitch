// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the program window may show (O-036).
//!
//! The window shows only the origin of the local server it was started
//! for: `http://127.0.0.1:<port>`. A top-level navigation or a new-window
//! request to an `https://` page elsewhere (a documentation or download
//! link the user clicked) is not shown in the window; it opens in the system
//! browser. Everything else (`file:`, `data:`, `javascript:`, other ports,
//! `localhost`, plain `http://` hosts, custom schemes) is refused.

use std::{
    net::Ipv4Addr,
    time::{Duration, Instant},
};

use url::{Host, Url};

/// Shortest interval between two pages opened in the system browser.
pub const EXTERNAL_OPEN_INTERVAL: Duration = Duration::from_secs(1);

/// Why a bootstrap URL was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapUrlError {
    Malformed,
    NotLoopback,
    NotBootstrap,
}

/// The origin the window was started for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NavigationPolicy {
    port: u16,
}

/// Decision on one navigation or new-window request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Navigation {
    /// Same origin as the local server: shown in the window.
    Allow,
    /// An `https://` page elsewhere: not shown, opened in the system browser.
    External(Url),
    /// Refused.
    Block,
}

impl NavigationPolicy {
    /// Accepts exactly the shape the server issues:
    /// `http://127.0.0.1:<port>/?bootstrap=<code>`, without user info or
    /// fragment, and returns the policy for its origin.
    pub fn for_bootstrap(url: &str) -> Result<(Self, Url), BootstrapUrlError> {
        let parsed = Url::parse(url).map_err(|_| BootstrapUrlError::Malformed)?;
        let port = match (parsed.scheme(), parsed.host(), parsed.port()) {
            ("http", Some(Host::Ipv4(Ipv4Addr::LOCALHOST)), Some(port)) if port != 0 => port,
            _ => return Err(BootstrapUrlError::NotLoopback),
        };
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(BootstrapUrlError::NotLoopback);
        }
        let mut pairs = parsed.query_pairs();
        let single_code = match (pairs.next(), pairs.next()) {
            (Some((key, code)), None) => key == "bootstrap" && valid_code(&code),
            _ => false,
        };
        if parsed.path() != "/" || parsed.fragment().is_some() || !single_code {
            return Err(BootstrapUrlError::NotBootstrap);
        }
        Ok((Self { port }, parsed))
    }

    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// Decides one navigation target as the web engine reports it.
    #[must_use]
    pub fn decide(&self, target: &str) -> Navigation {
        // Engines may report the empty initial document; it has no content.
        if target == "about:blank" {
            return Navigation::Allow;
        }
        let Ok(url) = Url::parse(target) else {
            return Navigation::Block;
        };
        if !url.username().is_empty() || url.password().is_some() {
            return Navigation::Block;
        }
        match (url.scheme(), url.host()) {
            ("http", Some(Host::Ipv4(Ipv4Addr::LOCALHOST))) if url.port() == Some(self.port) => {
                Navigation::Allow
            }
            ("https", Some(Host::Domain(_))) => Navigation::External(url),
            _ => Navigation::Block,
        }
    }
}

/// The server issues 256-bit codes as URL-safe text; anything else is not
/// a code of this server.
fn valid_code(code: &str) -> bool {
    (16..=128).contains(&code.len())
        && code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// Limits how often the window hands pages to the system browser, so a page
/// cannot flood the desktop with browser windows.
#[derive(Debug, Default)]
pub struct ExternalOpenGate {
    last: Option<Instant>,
}

impl ExternalOpenGate {
    /// `true` when a page may be opened at `now`.
    pub fn admit(&mut self, now: Instant) -> bool {
        if self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) < EXTERNAL_OPEN_INTERVAL)
        {
            return false;
        }
        self.last = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{BootstrapUrlError, ExternalOpenGate, Navigation, NavigationPolicy};

    const CODE: &str = "0123456789abcdef0123456789abcdef";

    fn policy() -> NavigationPolicy {
        match NavigationPolicy::for_bootstrap(&format!("http://127.0.0.1:4317/?bootstrap={CODE}")) {
            Ok((policy, _)) => policy,
            Err(error) => panic!("bootstrap URL refused: {error:?}"),
        }
    }

    #[test]
    fn server_bootstrap_url_is_accepted() {
        assert_eq!(policy().port(), 4317);
    }

    #[test]
    fn other_start_urls_are_refused() {
        let refused = [
            ("not a url", BootstrapUrlError::Malformed),
            (
                "https://127.0.0.1:4317/?bootstrap=x",
                BootstrapUrlError::NotLoopback,
            ),
            (
                "http://localhost:4317/?bootstrap=x",
                BootstrapUrlError::NotLoopback,
            ),
            (
                "http://127.0.0.2:4317/?bootstrap=x",
                BootstrapUrlError::NotLoopback,
            ),
            (
                "http://[::1]:4317/?bootstrap=x",
                BootstrapUrlError::NotLoopback,
            ),
            (
                "http://127.0.0.1/?bootstrap=x",
                BootstrapUrlError::NotLoopback,
            ),
            (
                "http://user@127.0.0.1:4317/?bootstrap=x",
                BootstrapUrlError::NotLoopback,
            ),
            ("http://127.0.0.1:4317/", BootstrapUrlError::NotBootstrap),
            (
                "http://127.0.0.1:4317/other?bootstrap=x",
                BootstrapUrlError::NotBootstrap,
            ),
            (
                "http://127.0.0.1:4317/?bootstrap=",
                BootstrapUrlError::NotBootstrap,
            ),
            (
                "http://127.0.0.1:4317/?bootstrap=short",
                BootstrapUrlError::NotBootstrap,
            ),
            (
                "http://127.0.0.1:4317/?bootstrap=a%20b0123456789abcdef",
                BootstrapUrlError::NotBootstrap,
            ),
            (
                "http://127.0.0.1:4317/?a=1&bootstrap=x",
                BootstrapUrlError::NotBootstrap,
            ),
            (
                "http://127.0.0.1:4317/?bootstrap=x#frag",
                BootstrapUrlError::NotBootstrap,
            ),
        ];
        for (url, expected) in refused {
            let url = url.replace("=x", &format!("={CODE}"));
            assert_eq!(
                NavigationPolicy::for_bootstrap(&url).err(),
                Some(expected),
                "{url}"
            );
        }
    }

    #[test]
    fn same_origin_is_shown_in_the_window() {
        let policy = policy();
        for target in [
            "http://127.0.0.1:4317/",
            "http://127.0.0.1:4317/drafts/abc?tab=1#top",
            &format!("http://127.0.0.1:4317/?bootstrap={CODE}"),
            "about:blank",
        ] {
            assert_eq!(policy.decide(target), Navigation::Allow, "{target}");
        }
    }

    #[test]
    fn https_pages_elsewhere_open_in_the_system_browser() {
        let decision = policy().decide("https://developer.microsoft.com/microsoft-edge/webview2/");
        assert!(
            matches!(decision, Navigation::External(url) if url.host_str() == Some("developer.microsoft.com"))
        );
    }

    #[test]
    fn everything_else_is_blocked() {
        let policy = policy();
        for target in [
            "http://127.0.0.1:4318/",
            "http://localhost:4317/",
            "http://example.com/",
            "https://127.0.0.1:4317/",
            "https://user:secret@example.com/",
            "file:///etc/passwd",
            "data:text/html,<script>1</script>",
            "javascript:alert(1)",
            "blob:http://127.0.0.1:4317/uuid",
            "ftp://example.com/",
            "zdashstitch://open",
            "about:config",
            "",
        ] {
            assert_eq!(policy.decide(target), Navigation::Block, "{target}");
        }
    }

    #[test]
    fn external_pages_are_rate_limited() {
        let mut gate = ExternalOpenGate::default();
        let start = Instant::now();
        assert!(gate.admit(start));
        assert!(!gate.admit(start + Duration::from_millis(500)));
        assert!(gate.admit(start + Duration::from_millis(1000)));
    }
}
