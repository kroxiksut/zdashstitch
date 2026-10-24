// SPDX-License-Identifier: MIT OR Apache-2.0

//! Read-only Zabbix JSON-RPC transport and the Zabbix 7.4 adapter boundary.
//!
//! This crate intentionally exposes no dashboard write operation. Network
//! access is opt-in through an explicitly supplied endpoint and credentials.

mod capabilities;
mod client;
mod error;
mod inventory;
mod snapshot;
mod transport;
mod version;
mod write;

pub use capabilities::{
    AdapterRegistry, CapabilityState, DashboardWriteContracts, ZabbixCapabilities,
};
pub use client::{AuthenticatedClient, Authentication, VersionDetector, ZabbixClient};
pub use error::{Result, ZabbixError};
pub use inventory::{InventoryMethod, InventoryProbe, MethodOutcome};
pub use snapshot::{
    DashboardSnapshotAnalysis, DependencyKind, DependencyState, SnapshotDependency,
    SnapshotSummary, VersionedDashboardSnapshot, WidgetClassification,
};
pub use transport::ZabbixEndpoint;
pub use version::ZabbixVersion;
pub use write::{
    DashboardPublishOutcome, ItemFieldReplacement, PublicationContext, VerifiedDashboardUpdate,
    verify_item_field_replacements,
};

/// Adapter family targeted by the current MVP.
pub const TARGET_ADAPTER_FAMILY: &str = "7.4";

/// No user-facing write route is enabled. The internal publisher accepts only
/// adapter-validated plans and performs mandatory preflight/read-back.
pub const DASHBOARD_WRITES_ENABLED: bool = false;
