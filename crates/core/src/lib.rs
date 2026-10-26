// SPDX-License-Identifier: MIT OR Apache-2.0

//! Version-independent product models.
//!
//! P0-01 intentionally contains no Zabbix identifiers, transport types, or
//! dashboard editing behavior. Domain models arrive in P0-02.

pub mod formats;
mod version;

pub use version::{Version, VersionParseError, VersionRange};

/// Honest implementation stage reported by the local shell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProductStage {
    /// The repository has a runnable shell but no dashboard editor yet.
    Scaffold,
}

impl ProductStage {
    /// Stable wire label for diagnostics and the local UI.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scaffold => "scaffold",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProductStage;

    #[test]
    fn scaffold_stage_is_explicit() {
        assert_eq!(ProductStage::Scaffold.as_str(), "scaffold");
    }
}
