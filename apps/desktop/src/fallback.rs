// SPDX-License-Identifier: MIT OR Apache-2.0

//! What happens when the program window cannot start (O-036): a small
//! native message box that needs no web engine (`rfd`, already used for
//! file dialogs) explains where to get the engine and offers «I will install
//! and restart» or «Continue in the browser». Without a message box (for
//! example no `zenity` on Linux) the same text goes to stderr and the
//! browser opens.

use zdashstitch_application::MessageId;

use crate::catalog::Catalog;

/// Official WebView2 Runtime download page.
pub const WEBVIEW2_DOWNLOAD_URL: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";
/// WebKitGTK 4.1 on RED OS 8 (repository `updates`).
pub const RED_OS_INSTALL_COMMAND: &str = "sudo dnf install webkit2gtk4.1";
/// WebKitGTK 4.1 on Debian and Astra Linux 1.8.
pub const DEBIAN_INSTALL_COMMAND: &str = "sudo apt install libwebkit2gtk-4.1-0";

/// Exit code of the dynamic loader when a shared library is missing.
const LOADER_EXIT_CODE: i32 = 127;

/// Operating-system family of the install hint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Platform {
    Windows,
    Linux,
}

impl Platform {
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// Why the window did not start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowFailure {
    /// The system web engine is not installed.
    EngineMissing,
    /// Anything else: the engine could not be used, no display, the window
    /// executable is missing or did not answer in time.
    StartFailed,
}

/// Classifies a window process that ended or was given up before it
/// reported `ready`. `engine_missing` is the window's own report; on Linux
/// exit code 127 means the loader could not find a library, which for this
/// executable is WebKitGTK 4.1.
#[must_use]
pub fn classify(engine_missing: bool, exit_code: Option<i32>, platform: Platform) -> WindowFailure {
    if engine_missing || (platform == Platform::Linux && exit_code == Some(LOADER_EXIT_CODE)) {
        WindowFailure::EngineMissing
    } else {
        WindowFailure::StartFailed
    }
}

/// The user's answer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Choice {
    /// «Continue in the browser».
    Browser,
    /// «I will install and restart»: the program exits (on Windows after
    /// opening the download page).
    InstallAndRestart,
    /// No message box could be shown: print the text, open the browser.
    DialogUnavailable,
}

/// Buttons of the message box.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Buttons {
    /// Windows `MessageBox` cannot relabel buttons: «Yes» is the browser,
    /// «No» install and restart; the body says so.
    YesNo,
    /// Labelled buttons (zenity on Linux): (browser, install).
    Labelled { browser: String, install: String },
}

/// Text and buttons of the fallback message box.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FallbackDialog {
    pub title: String,
    pub body: String,
    pub buttons: Buttons,
}

impl FallbackDialog {
    #[must_use]
    pub fn new(catalog: &Catalog, failure: WindowFailure, platform: Platform) -> Self {
        let reason = match failure {
            WindowFailure::EngineMissing => MessageId::DESKTOP_FALLBACK_REASON_ENGINE_MISSING,
            WindowFailure::StartFailed => MessageId::DESKTOP_FALLBACK_REASON_START_FAILED,
        };
        let mut lines = vec![catalog.text(reason, &[]), String::new()];
        match platform {
            Platform::Windows => lines.push(catalog.text(
                MessageId::DESKTOP_FALLBACK_INSTALL_WEBVIEW2,
                &[("url", WEBVIEW2_DOWNLOAD_URL)],
            )),
            Platform::Linux => {
                lines.push(catalog.text(MessageId::DESKTOP_FALLBACK_INSTALL_WEBKITGTK, &[]));
                lines.push(catalog.text(
                    MessageId::DESKTOP_FALLBACK_INSTALL_RED_OS,
                    &[("command", RED_OS_INSTALL_COMMAND)],
                ));
                lines.push(catalog.text(
                    MessageId::DESKTOP_FALLBACK_INSTALL_DEBIAN,
                    &[("command", DEBIAN_INSTALL_COMMAND)],
                ));
                lines.push(catalog.text(MessageId::DESKTOP_FALLBACK_INSTALL_ASTRA_1_7, &[]));
            }
        }
        lines.push(catalog.text(MessageId::DESKTOP_FALLBACK_RESTART, &[]));
        let buttons = match platform {
            Platform::Windows => {
                lines.push(String::new());
                lines.push(catalog.text(MessageId::DESKTOP_FALLBACK_CHOICE_YES_NO, &[]));
                Buttons::YesNo
            }
            Platform::Linux => Buttons::Labelled {
                browser: catalog.text(MessageId::DESKTOP_FALLBACK_CHOICE_BROWSER, &[]),
                install: catalog.text(MessageId::DESKTOP_FALLBACK_CHOICE_INSTALL, &[]),
            },
        };
        Self {
            title: catalog.text(MessageId::DESKTOP_FALLBACK_TITLE, &[]),
            body: lines.join("\n"),
            buttons,
        }
    }

    /// Interprets the message-box result. `rfd` reports a message box that
    /// could not be shown (zenity missing, `MessageBoxW` failed) as plain
    /// `Cancel`; labelled buttons come back as `Custom(label)`.
    #[must_use]
    pub fn choice(&self, result: &rfd::MessageDialogResult) -> Choice {
        use rfd::MessageDialogResult as Result;

        match (&self.buttons, result) {
            (Buttons::YesNo, Result::Yes) => Choice::Browser,
            (Buttons::YesNo, Result::No) => Choice::InstallAndRestart,
            (Buttons::Labelled { browser, .. }, Result::Custom(label)) if label == browser => {
                Choice::Browser
            }
            (Buttons::Labelled { install, .. }, Result::Custom(label)) if label == install => {
                Choice::InstallAndRestart
            }
            _ => Choice::DialogUnavailable,
        }
    }

    /// Shows the message box; blocks until it is answered.
    #[must_use]
    pub fn show(&self) -> Choice {
        let buttons = match &self.buttons {
            Buttons::YesNo => rfd::MessageButtons::YesNo,
            Buttons::Labelled { browser, install } => {
                rfd::MessageButtons::OkCancelCustom(browser.clone(), install.clone())
            }
        };
        let result = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title(&self.title)
            .set_description(&self.body)
            .set_buttons(buttons)
            .show();
        self.choice(&result)
    }
}

#[cfg(test)]
mod tests {
    use rfd::MessageDialogResult;

    use super::{
        Buttons, Choice, DEBIAN_INSTALL_COMMAND, FallbackDialog, Platform, RED_OS_INSTALL_COMMAND,
        WEBVIEW2_DOWNLOAD_URL, WindowFailure, classify,
    };
    use crate::catalog::{Catalog, Locale};

    #[test]
    fn engine_detection() {
        assert_eq!(
            classify(true, Some(3), Platform::Windows),
            WindowFailure::EngineMissing
        );
        assert_eq!(
            classify(false, Some(127), Platform::Linux),
            WindowFailure::EngineMissing
        );
        assert_eq!(
            classify(false, Some(127), Platform::Windows),
            WindowFailure::StartFailed
        );
        assert_eq!(
            classify(false, Some(101), Platform::Linux),
            WindowFailure::StartFailed
        );
        assert_eq!(
            classify(false, None, Platform::Linux),
            WindowFailure::StartFailed
        );
    }

    #[test]
    fn windows_dialog_points_to_webview2() {
        let dialog = FallbackDialog::new(
            &Catalog::new(Locale::En),
            WindowFailure::EngineMissing,
            Platform::Windows,
        );
        assert!(dialog.body.contains(WEBVIEW2_DOWNLOAD_URL));
        assert!(dialog.body.contains("«Yes»"));
        assert_eq!(dialog.buttons, Buttons::YesNo);
        assert_eq!(dialog.choice(&MessageDialogResult::Yes), Choice::Browser);
        assert_eq!(
            dialog.choice(&MessageDialogResult::No),
            Choice::InstallAndRestart
        );
        assert_eq!(
            dialog.choice(&MessageDialogResult::Cancel),
            Choice::DialogUnavailable
        );
    }

    #[test]
    fn linux_dialog_lists_distribution_commands() {
        let dialog = FallbackDialog::new(
            &Catalog::new(Locale::Ru),
            WindowFailure::StartFailed,
            Platform::Linux,
        );
        assert!(dialog.body.starts_with("Окно программы не запустилось."));
        assert!(dialog.body.contains(RED_OS_INSTALL_COMMAND));
        assert!(dialog.body.contains(DEBIAN_INSTALL_COMMAND));
        assert!(dialog.body.contains("Astra Linux 1.7"));
        let Buttons::Labelled { browser, install } = dialog.buttons.clone() else {
            panic!("labelled buttons expected");
        };
        assert_eq!(browser, "Продолжить в браузере");
        assert_eq!(install, "Установлю и перезапущу");
        assert_eq!(
            dialog.choice(&MessageDialogResult::Custom(browser)),
            Choice::Browser
        );
        assert_eq!(
            dialog.choice(&MessageDialogResult::Custom(install)),
            Choice::InstallAndRestart
        );
        // zenity missing: rfd reports a plain `Cancel`.
        assert_eq!(
            dialog.choice(&MessageDialogResult::Cancel),
            Choice::DialogUnavailable
        );
        assert_eq!(
            dialog.choice(&MessageDialogResult::Custom("other".to_owned())),
            Choice::DialogUnavailable
        );
    }
}
