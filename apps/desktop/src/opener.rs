// SPDX-License-Identifier: MIT OR Apache-2.0

//! Opens a page in the system browser without a shell and without a new
//! dependency: `rundll32.exe url.dll,FileProtocolHandler <url>` from the
//! Windows system directory, `xdg-open <url>` on Linux. The URL is one
//! argument of a direct process start, never part of a command string.

use std::{io, process::Command};

use url::Url;

/// Opens `url` (`http` or `https` only) in the default browser. The caller
/// decides which URLs qualify (`navigation`, the loopback bootstrap URL).
pub fn open_in_browser(url: &Url) -> io::Result<()> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let mut child = browser_command(url.as_str())?.spawn()?;
    // Reap the helper so it does not linger as a zombie on Linux.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(windows)]
fn browser_command(url: &str) -> io::Result<Command> {
    let root = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
    let mut command = Command::new(root.join("System32").join("rundll32.exe"));
    command.arg("url.dll,FileProtocolHandler").arg(url);
    Ok(command)
}

#[cfg(not(windows))]
fn browser_command(url: &str) -> io::Result<Command> {
    let mut command = Command::new("xdg-open");
    command.arg(url);
    Ok(command)
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::open_in_browser;

    #[test]
    fn only_web_urls_are_opened() {
        for target in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ftp://example.com/",
        ] {
            let url = Url::parse(target).unwrap_or_else(|error| panic!("{target}: {error}"));
            assert_eq!(
                open_in_browser(&url).map_err(|error| error.kind()),
                Err(std::io::ErrorKind::InvalidInput),
                "{target}"
            );
        }
    }
}
