// SPDX-License-Identifier: MIT OR Apache-2.0

//! Zabbix JSON-RPC transport and the Zabbix 7.4 adapter boundary.
//!
//! Network access is opt-in through an explicitly supplied endpoint and
//! credentials. Sign-in yields a `ReadOnlyGateway`, which has no write path;
//! the only write is the closed, `VerifiedPlan`-bound dashboard update of a
//! `ReadWriteGateway`.

mod capabilities;
mod client;
mod error;
mod gateway;
mod inventory;
mod snapshot;
mod taxonomy;
mod transport;
mod version;
mod widget_capabilities;
mod write;

pub use capabilities::{
    AdapterRegistry, CapabilityState, DashboardWriteContracts, ZabbixCapabilities,
};
pub use client::{Authentication, VersionDetector, ZabbixClient};
pub use error::{Result, ZabbixError};
pub use gateway::{
    AccessMode, AuthMethod, ReadAccess, ReadMethod, ReadOnlyGateway, ReadWriteGateway,
    ServerIdentity, SessionMethod, dashboard_write_allowlist, read_only_allowlist,
};
pub use inventory::{InventoryMethod, InventoryProbe, MethodOutcome};
pub use snapshot::{
    DashboardSnapshotAnalysis, DependencyKind, DependencyState, SnapshotDependency,
    SnapshotSummary, VersionedDashboardSnapshot, WidgetClassification,
};
pub use taxonomy::{CheckStep, Failure, FailureCode, FailureParam};
pub use transport::{EndpointRejection, TrustAnchors, TrustMaterialError, ZabbixEndpoint};
pub use version::ZabbixVersion;
pub use widget_capabilities::{
    KnownWidgetField, WidgetCapability, WidgetCapabilityFlags, WidgetCapabilityRegistry,
    WidgetFieldType,
};
pub use write::{
    DashboardPublishOutcome, ItemFieldReplacement, PublicationContext, VerifiedDashboardUpdate,
    verify_item_field_replacements,
};

/// Adapter family targeted by the current MVP.
pub const TARGET_ADAPTER_FAMILY: &str = "7.4";

/// No user-facing write route is enabled. The internal publisher accepts only
/// adapter-validated plans and performs mandatory preflight/read-back.
pub const DASHBOARD_WRITES_ENABLED: bool = false;
