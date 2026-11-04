// SPDX-License-Identifier: MIT OR Apache-2.0

//! Command line and environment of the launcher.

use std::ffi::{OsStr, OsString};

/// Environment variable that forces the browser: `1` browser, `0` window.
pub const BROWSER_ENV: &str = "ZDASHSTITCH_BROWSER";

/// Developer-facing usage text (English, like every diagnostic).
pub const USAGE: &str = "\
Usage: zdashstitch [--browser]

Starts ZDashstitch and opens it in the program window.
  --browser   open the system browser instead of the program window
  --help      print this text

Environment: ZDASHSTITCH_BROWSER=1 has the same effect as --browser.";

/// Where the UI is shown.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    /// The program window on the system web engine (default).
    Window,
    /// The system browser.
    Browser,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    Run(Mode),
    Help,
}

/// Why the command line was refused; machine codes only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CliError {
    /// An argument the launcher does not know (never echoed: it could be a
    /// pasted secret).
    UnknownArgument,
    /// `ZDASHSTITCH_BROWSER` is neither `0` nor `1`.
    InvalidBrowserEnv,
}

impl CliError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnknownArgument => "cli.argument.unknown",
            Self::InvalidBrowserEnv => "cli.env.browser_invalid",
        }
    }
}

/// Parses the arguments (without the program name) and the value of
/// [`BROWSER_ENV`]. `--browser` wins over the environment.
pub fn parse(
    arguments: impl IntoIterator<Item = OsString>,
    browser_env: Option<&OsStr>,
) -> Result<Command, CliError> {
    let mut browser_flag = false;
    for argument in arguments {
        match argument.to_str() {
            Some("--browser") => browser_flag = true,
            Some("--help" | "-h") => return Ok(Command::Help),
            _ => return Err(CliError::UnknownArgument),
        }
    }
    let env_mode = match browser_env.map(OsStr::to_str) {
        None => Mode::Window,
        Some(Some("1")) => Mode::Browser,
        Some(Some("0" | "")) => Mode::Window,
        Some(_) => return Err(CliError::InvalidBrowserEnv),
    };
    Ok(Command::Run(if browser_flag {
        Mode::Browser
    } else {
        env_mode
    }))
}

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};

    use super::{CliError, Command, Mode, parse};

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn window_is_the_default() {
        assert_eq!(parse(args(&[]), None), Ok(Command::Run(Mode::Window)));
        assert_eq!(
            parse(args(&[]), Some(OsStr::new("0"))),
            Ok(Command::Run(Mode::Window))
        );
        assert_eq!(
            parse(args(&[]), Some(OsStr::new(""))),
            Ok(Command::Run(Mode::Window))
        );
    }

    #[test]
    fn flag_or_environment_selects_the_browser() {
        assert_eq!(
            parse(args(&["--browser"]), None),
            Ok(Command::Run(Mode::Browser))
        );
        assert_eq!(
            parse(args(&[]), Some(OsStr::new("1"))),
            Ok(Command::Run(Mode::Browser))
        );
        assert_eq!(
            parse(args(&["--browser"]), Some(OsStr::new("0"))),
            Ok(Command::Run(Mode::Browser))
        );
    }

    #[test]
    fn help_is_recognised() {
        assert_eq!(parse(args(&["--help"]), None), Ok(Command::Help));
        assert_eq!(parse(args(&["-h"]), None), Ok(Command::Help));
    }

    #[test]
    fn unknown_input_is_refused() {
        assert_eq!(
            parse(args(&["--window"]), None),
            Err(CliError::UnknownArgument)
        );
        assert_eq!(
            parse(args(&["http://127.0.0.1:1/?bootstrap=x"]), None),
            Err(CliError::UnknownArgument)
        );
        assert_eq!(
            parse(args(&[]), Some(OsStr::new("yes"))),
            Err(CliError::InvalidBrowserEnv)
        );
        assert_eq!(
            parse(args(&["--browser"]), Some(OsStr::new("true"))),
            Err(CliError::InvalidBrowserEnv)
        );
    }
}
