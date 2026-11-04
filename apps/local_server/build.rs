// SPDX-License-Identifier: MIT OR Apache-2.0

//! Embeds the git commit of the build (P2-00F) as `ZDASHSTITCH_BUILD_COMMIT`
//! without any network access.
//!
//! - `ZDASHSTITCH_BUILD_COMMIT` set in the build environment wins (CI,
//!   reproducible builds): a full 40- or 64-digit hex hash, or empty for
//!   "unknown". Anything else fails the build.
//! - Otherwise `git rev-parse HEAD` in this checkout; no git, no repository
//!   or any git failure yields "unknown" (an empty value).
//!
//! The script reruns only when the override, `HEAD`, the checked-out ref or
//! `packed-refs` change, not on every source edit. A dirty-tree flag is not
//! embedded: keeping it correct would require rerunning on every build.

use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

const OVERRIDE: &str = "ZDASHSTITCH_BUILD_COMMIT";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed={OVERRIDE}");
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let commit = match env::var(OVERRIDE) {
        Ok(value) => match normalized_hash(&value) {
            Some(hash) => hash,
            None if value.trim().is_empty() => String::new(),
            None => panic!("{OVERRIDE} must be a full 40- or 64-digit hex commit hash or empty"),
        },
        Err(_) => {
            watch_git_state(&manifest_dir);
            git(&manifest_dir, &["rev-parse", "HEAD"])
                .as_deref()
                .and_then(normalized_hash)
                .unwrap_or_default()
        }
    };
    println!("cargo:rustc-env={OVERRIDE}={commit}");
}

fn normalized_hash(value: &str) -> Option<String> {
    let value = value.trim();
    (matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

/// Reruns the script when the commit can change. Only existing paths are
/// listed: Cargo reruns on every build for a missing one. A branch ref that
/// exists only in `packed-refs` is watched through its directory, where the
/// next commit creates the loose file.
fn watch_git_state(manifest_dir: &Path) {
    let git_path = |name: &str| {
        git(manifest_dir, &["rev-parse", "--git-path", name]).map(|path| manifest_dir.join(path))
    };
    for name in ["HEAD", "packed-refs"] {
        if let Some(path) = git_path(name).filter(|path| path.is_file()) {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    let Some(reference) = git(manifest_dir, &["symbolic-ref", "-q", "HEAD"]) else {
        return;
    };
    if let Some(path) = git_path(&reference) {
        let watched = if path.is_file() {
            Some(path.as_path())
        } else {
            path.parent().filter(|parent| parent.is_dir())
        };
        if let Some(watched) = watched {
            println!("cargo:rerun-if-changed={}", watched.display());
        }
    }
}

fn git(directory: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}
