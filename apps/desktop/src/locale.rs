// SPDX-License-Identifier: MIT OR Apache-2.0

//! Language of the launcher's native text: the language chosen in
//! «Settings → Appearance» (the settings file), and for «System» the
//! language of the operating-system user. The UI itself resolves «System»
//! in the web engine; this only covers text shown before or without it.

use std::ffi::OsString;

use zdashstitch_application::{LocalePreference, SettingsStore};

use crate::catalog::Locale;

/// The locale for `preference`; `system` is asked only for «System».
#[must_use]
pub fn resolve(preference: LocalePreference, system: impl FnOnce() -> Option<String>) -> Locale {
    match preference {
        LocalePreference::En => Locale::En,
        LocalePreference::Ru => Locale::Ru,
        LocalePreference::System => system().as_deref().map_or(Locale::En, from_tag),
    }
}

/// `ru`, `ru-RU`, `ru_RU.UTF-8` → Russian; anything else → English.
#[must_use]
pub fn from_tag(tag: &str) -> Locale {
    let language = tag.split(['-', '_', '.', '@']).next().unwrap_or_default();
    if language.eq_ignore_ascii_case("ru") {
        Locale::Ru
    } else {
        Locale::En
    }
}

/// The saved language preference; «System» when the settings file is
/// missing or unreadable (its last valid copy is used when there is one).
#[must_use]
pub fn saved_preference() -> LocalePreference {
    let Ok((store, defaults)) = SettingsStore::platform_default() else {
        return LocalePreference::System;
    };
    match store.load() {
        Ok(Some(settings)) => settings.locale,
        Ok(None) => defaults.locale,
        Err(error) => error
            .last_valid
            .map_or(LocalePreference::System, |settings| settings.locale),
    }
}

/// The locale of the launcher's native text.
#[must_use]
pub fn current() -> Locale {
    resolve(saved_preference(), system_tag)
}

/// POSIX order of the message-language variables; `C`/`POSIX` mean none.
#[must_use]
pub fn tag_from_environment(lookup: impl Fn(&str) -> Option<OsString>) -> Option<String> {
    ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| lookup(name)?.into_string().ok())
        .map(|value| value.split(':').next().unwrap_or_default().to_owned())
        .find(|value| !value.is_empty() && value != "C" && value != "POSIX")
}

#[cfg(not(windows))]
fn system_tag() -> Option<String> {
    tag_from_environment(|name| std::env::var_os(name))
}

/// The user's locale name from `HKCU\Control Panel\International`, read with
/// the system `reg.exe` (std has no locale API and the workspace denies
/// `unsafe`).
#[cfg(windows)]
fn system_tag() -> Option<String> {
    use std::os::windows::process::CommandExt as _;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let root = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute())?;
    let output = std::process::Command::new(root.join("System32").join("reg.exe"))
        .args([
            "query",
            r"HKCU\Control Panel\International",
            "/v",
            "LocaleName",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    tag_from_reg_output(&String::from_utf8_lossy(&output.stdout))
}

/// Parses `    LocaleName    REG_SZ    ru-RU` from `reg query` output.
#[must_use]
pub fn tag_from_reg_output(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        match (parts.next(), parts.next(), parts.next()) {
            (Some("LocaleName"), Some("REG_SZ"), Some(value)) => Some(value.to_owned()),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use zdashstitch_application::LocalePreference;

    use super::{from_tag, resolve, tag_from_environment, tag_from_reg_output};
    use crate::catalog::Locale;

    #[test]
    fn explicit_preference_wins_over_the_system() {
        assert_eq!(
            resolve(LocalePreference::En, || Some("ru-RU".to_owned())),
            Locale::En
        );
        assert_eq!(resolve(LocalePreference::Ru, || None), Locale::Ru);
    }

    #[test]
    fn system_preference_follows_the_operating_system() {
        assert_eq!(
            resolve(LocalePreference::System, || Some("ru_RU.UTF-8".to_owned())),
            Locale::Ru
        );
        assert_eq!(
            resolve(LocalePreference::System, || Some("de-DE".to_owned())),
            Locale::En
        );
        assert_eq!(resolve(LocalePreference::System, || None), Locale::En);
    }

    #[test]
    fn tags_are_matched_by_language() {
        for tag in ["ru", "RU", "ru-RU", "ru_RU.UTF-8", "ru@latin"] {
            assert_eq!(from_tag(tag), Locale::Ru, "{tag}");
        }
        for tag in ["", "en-US", "rus", "uk-UA", "C"] {
            assert_eq!(from_tag(tag), Locale::En, "{tag}");
        }
    }

    #[test]
    fn environment_follows_posix_priority() {
        let env = |values: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                values
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| OsString::from(value))
            }
        };
        assert_eq!(
            tag_from_environment(env(&[("LANG", "en_US.UTF-8"), ("LC_ALL", "ru_RU.UTF-8")])),
            Some("ru_RU.UTF-8".to_owned())
        );
        assert_eq!(
            tag_from_environment(env(&[("LANGUAGE", "ru:en"), ("LANG", "en_US")])),
            Some("ru".to_owned())
        );
        assert_eq!(
            tag_from_environment(env(&[("LC_ALL", "C"), ("LANG", "ru_RU")])),
            Some("ru_RU".to_owned())
        );
        assert_eq!(tag_from_environment(env(&[])), None);
    }

    #[test]
    fn registry_output_is_parsed() {
        let output = "\r\nHKEY_CURRENT_USER\\Control Panel\\International\r\n    LocaleName    REG_SZ    ru-RU\r\n\r\n";
        assert_eq!(tag_from_reg_output(output), Some("ru-RU".to_owned()));
        assert_eq!(tag_from_reg_output("ERROR: not found"), None);
    }
}
