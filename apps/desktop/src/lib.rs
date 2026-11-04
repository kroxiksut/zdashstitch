// SPDX-License-Identifier: MIT OR Apache-2.0

//! Desktop host of the local UI (O-036, 2026-10-28).
//!
//! The launcher `zdashstitch` starts the same loopback server as the
//! browser-only binary and shows its UI in the program window: a separate
//! process `zdashstitch-window` on the system web engine (WebView2 on
//! Windows, WebKitGTK 4.1 on Linux). The window opens the one-time bootstrap
//! URL exactly like a browser does, so the session cookie, Origin/Host and
//! CSRF checks of the server stay the only security boundary. The window is
//! a separate process because on Linux a binary linked against a missing
//! WebKitGTK cannot even start; the launcher notices that and offers the
//! browser instead.
//!
//! This library holds the parts both processes share and that can be tested
//! without a window: command-line parsing, the launcher/window line protocol,
//! the navigation policy, opening the system browser, locale resolution, the
//! message catalog and the fallback decision.

pub mod catalog;
pub mod cli;
pub mod fallback;
pub mod locale;
pub mod navigation;
pub mod opener;
pub mod protocol;
