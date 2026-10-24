// SPDX-License-Identifier: MIT OR Apache-2.0

//! Offline template-export parsing boundary.
//!
//! No parser is implemented in P0-01. The crate exists now so later parsers
//! cannot accidentally be placed in the transport or UI layers.

use zdashstitch_core::ProductStage;

/// Current readiness of the importer boundary.
#[must_use]
pub const fn stage() -> ProductStage {
    ProductStage::Scaffold
}
