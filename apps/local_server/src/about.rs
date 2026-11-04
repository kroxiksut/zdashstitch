// SPDX-License-Identifier: MIT OR Apache-2.0

//! Build information for «About and licenses» and the report (P1-12 §7,
//! P2-00F): embedded at build time, read without any network access.

use axum::Json;
use serde::Serialize;

/// Product version: the single source is `[workspace.package] version` in
/// the root `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Full commit hash embedded by `build.rs`; empty when unknown (for
/// example a build from a source archive without git).
const EMBEDDED_COMMIT: &str = env!("ZDASHSTITCH_BUILD_COMMIT");

/// Commit the binary was built from, `None` when unknown.
#[must_use]
pub fn build_commit() -> Option<&'static str> {
    (!EMBEDDED_COMMIT.is_empty()).then_some(EMBEDDED_COMMIT)
}

/// `GET /api/about`: data only. Fields from P1-12 §7 not listed here
/// (platform, adapters, paths) arrive with the About screen. No build date
/// is embedded or shown: only the commit identifies the build, which keeps
/// builds reproducible (owner decision of 2026-10-26).
#[derive(Debug, Serialize)]
pub struct AboutResponse {
    pub version: &'static str,
    /// `null` renders as `about.field.build_unknown`.
    pub build_commit: Option<&'static str>,
}

pub(crate) async fn about() -> Json<AboutResponse> {
    Json(AboutResponse {
        version: VERSION,
        build_commit: build_commit(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_commit_is_a_full_hash_or_unknown() {
        if let Some(commit) = build_commit() {
            assert!(matches!(commit.len(), 40 | 64), "{commit}");
            assert!(
                commit
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            );
        }
    }

    #[test]
    fn ui_package_version_follows_the_workspace_version() {
        let ui = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui");
        for file in ["package.json", "package-lock.json"] {
            let text = std::fs::read_to_string(ui.join(file))
                .unwrap_or_else(|error| panic!("{file} must be readable: {error}"));
            let manifest: serde_json::Value = serde_json::from_str(&text)
                .unwrap_or_else(|error| panic!("{file} must be JSON: {error}"));
            assert_eq!(
                manifest["version"].as_str(),
                Some(VERSION),
                "ui/{file} version must equal [workspace.package] version"
            );
            if file == "package-lock.json" {
                assert_eq!(manifest["packages"][""]["version"].as_str(), Some(VERSION));
            }
        }
    }
}
