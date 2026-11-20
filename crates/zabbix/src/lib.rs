// SPDX-License-Identifier: MIT OR Apache-2.0

//! Zabbix JSON-RPC transport and versioned adapter boundary.
//!
//! Network access is opt-in through an explicitly supplied endpoint and
//! credentials. Sign-in yields a `ReadOnlyGateway`, which has no write path;
//! the only writes are the closed, `VerifiedPlan`-bound dashboard update and
//! dashboard create of a `ReadWriteGateway`. The create stays unreachable
//! while `create_reconcile_unknown_outcome` is not laboratory-proven.

mod capabilities;
mod client;
mod create;
mod create_permission;
mod dashboard_access;
mod dashboards;
mod error;
mod gateway;
mod inventory;
mod lookups;
mod snapshot;
mod taxonomy;
mod transport;
mod version;
mod widget_capabilities;
mod write;

pub use capabilities::{
    AdapterProfile, AdapterRegistry, CapabilityState, CreateContract, DashboardCreateContracts,
    DashboardWriteContracts, WriteContract, ZabbixCapabilities,
};
pub use client::{Authentication, VersionDetector, ZabbixClient};
pub use create::{
    CreateExpectation, CreateFailureReason, CreateReconciliation, CreateRejection,
    CreateRejectionCode, CreateRequirements, CreateScope, CreateSendRefusal,
    DashboardCreateOutcome, LinkOutsidePage, LinkedGraphCompositionPage, LinkedGraphPage,
    MAX_DASHBOARD_NAME_CHARS, MAX_NAMED_DASHBOARDS, MAX_PORTABLE_APPEND_WIDGETS,
    MAX_PORTABLE_GRAPH_PAGES, NamedDashboards, PortableGraphChange, PortableUpdateDifference,
    PortableUpdateReadback, ReadBackVerdict, UnknownCopyReason, UnknownCopyWidget,
    VerifiedDashboardCreate, VerifiedPortableUpdate, check_created_dashboard, create_requirements,
    create_requirements_for, dashboard_comparison_content, dashboard_content_fingerprint,
    eight_zero_clone_create_requirements, expected_eight_zero_clone_access,
    is_valid_dashboard_name, portable_create_requirements, reconcile_created_dashboard,
    verify_dashboard_create, verify_dashboard_create_for, verify_portable_dashboard_create,
};
pub use dashboard_access::{
    DashboardAccessChoices, DashboardAccessSelection, DashboardAccessState, DashboardRecipient,
    SharingPermission,
};
pub use dashboards::{
    DashboardListPage, DashboardListing, DashboardPageListing, ITEM_METADATA_BATCH, ItemMetadata,
    ListedField, ListedWidget, MAX_DASHBOARD_LIST_LIMIT, MAX_DASHBOARD_SEARCH_CHARS, ValueMap,
    ValueMapping, is_zabbix_id,
};
pub use error::{Result, ZabbixError};
pub use gateway::{
    AccessMode, AuthMethod, ReadAccess, ReadMethod, ReadOnlyGateway, ReadWriteGateway,
    RoleApiAccess, ServerIdentity, SessionMethod, dashboard_write_allowlist, read_only_allowlist,
};
pub use inventory::{InventoryMethod, InventoryProbe, MethodOutcome};
pub use lookups::{
    ExactHostTag, GraphDetails, GraphPrototypeDetails, HostDetails, HostGroupListing, HostListing,
    HostObjects, HostSearchFilter, ItemDetails, ItemDiscoveryLink, ItemFlags, ItemPrototypeDetails,
    LOOKUP_BATCH, MAX_FILTER_CHARS, MAX_HOST_GROUP_SEARCH, MAX_HOST_OBJECTS, MAX_HOST_SEARCH,
    MAX_HOST_SEARCH_CHARS, SvgGraphItemMatch, SvgGraphItemTagFilter, TemplateRef,
};
pub use snapshot::{
    ADAPTER_ID_7_4, DashboardSnapshotAnalysis, DependencyKind, DependencyState, SnapshotDependency,
    SnapshotSummary, VersionedDashboardSnapshot, WidgetClassification, canonical_document,
};
pub use taxonomy::{CheckStep, Failure, FailureCode, FailureParam};
pub use transport::{EndpointRejection, TrustAnchors, TrustMaterialError, ZabbixEndpoint};
pub use version::ZabbixVersion;
pub use widget_capabilities::{
    KnownWidgetField, WidgetCapability, WidgetCapabilityFlags, WidgetCapabilityRegistry,
    WidgetFieldType,
};
pub use write::{
    DashboardPublishOutcome, MAX_FIELD_REPLACEMENTS, MAX_REPLACE_HOST_LLD_CANDIDATES,
    PublicationContext, RESEND_PROVEN_WIDGET_TYPES, UnprovenWidget, VerifiedDashboardUpdate,
    WidgetFieldReplacement, WriteRejection, WriteRejectionCode, WriteRequirements,
    verify_widget_field_replacements, write_requirements,
};

/// Adapter family targeted by the current MVP.
pub const TARGET_ADAPTER_FAMILY: &str = "7.4";

/// No user-facing write route is enabled. The internal publisher accepts only
/// adapter-validated plans and performs mandatory preflight/read-back.
pub const DASHBOARD_WRITES_ENABLED: bool = false;
