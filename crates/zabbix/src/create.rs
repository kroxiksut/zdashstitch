// SPDX-License-Identifier: MIT OR Apache-2.0

//! Closed dashboard creation path of the 7.4 adapter (P2-06B/C/D, O-019).
//!
//! A [`VerifiedDashboardCreate`] is built only by [`verify_dashboard_create`]
//! from a Local snapshot (Base + operations) of the same connection and a
//! target name. It is neither `Clone` nor deserializable, its payload is
//! never exposed, and the only way to send it is
//! [`ReadWriteGateway::create_verified_dashboard`]. The laboratory
//! contracts of LAB-02 (28.10.2026, 7.4.16) proved
//! `create_private_dashboard`, `create_copy_read_widgets` and
//! `create_reconcile_unknown_outcome`, so on 7.4.16 a copy of widget types
//! the registry knows can be verified ([`CreateContract::COPY`]);
//! `create_copy_unknown_widgets` is not proven. No user-facing route calls
//! this path.
//!
//! Payload rules (P2-06B, P1-IN-04, O-031):
//!
//! - `dashboard.create` carries the target `name`, the Local
//!   `display_period` and `auto_start`, `private: 1`, empty `users` and
//!   `userGroups`, and the pages of the [`CreateScope`]: **every** page in
//!   Local order for a dashboard draft, exactly the selected page for a page
//!   draft (owner decision 28.10.2026, item 2). `userid` is never sent: the
//!   owner is the account of the connection.
//! - A page copy carries no widget link whose provider is on another page
//!   of Local: such a `*._reference` would point at nothing in the new
//!   dashboard, so the create is refused (`create_link_outside_page`).
//! - A page carries `name`, `display_period` and all its widgets; a widget
//!   carries `type`, `name`, geometry, `view_mode` and the full `fields`
//!   array as read (`type`, `name`, `value`), so in-dashboard `reference`
//!   links stay intact. No `dashboardid`, `dashboard_pageid` or `widgetid`
//!   is sent.
//! - Widget types and field types unknown to the 7.4 registry are copied as
//!   read only within the same adapter and exact version, and only behind
//!   their own contract `create_copy_unknown_widgets` (ADR-011).
//!
//! Outcome rules (P2-06C/D): the name check before send is a hint (a
//! private dashboard of another user with the same name is invisible). It
//! compares names exactly, with case, as the server does: on PostgreSQL a
//! name that differs only in case is accepted (LAB-02);
//! exactly one request is sent; `already exists` is the final
//! [`DashboardCreateOutcome::NameTaken`], never retried and never renamed;
//! an unknown response is reconciled by reads only, by exact name and
//! content fingerprint, and only exactly one confirmed candidate counts.
//!
//! A read-only gateway has no create path:
//!
//! ```compile_fail
//! async fn no_create_path(
//!     gateway: &zdashstitch_zabbix::ReadOnlyGateway,
//!     create: &zdashstitch_zabbix::VerifiedDashboardCreate,
//! ) {
//!     gateway.create_verified_dashboard(create, || true).await;
//! }
//! ```
//!
//! Control: the same call compiles on a `ReadWriteGateway`.
//!
//! ```no_run
//! async fn create_path(
//!     gateway: &zdashstitch_zabbix::ReadWriteGateway,
//!     create: &zdashstitch_zabbix::VerifiedDashboardCreate,
//! ) {
//!     let _ = gateway.create_verified_dashboard(create, || true).await;
//! }
//! ```

use std::collections::BTreeSet;
use std::fmt::{self, Write as _};
use std::time::Duration;

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::gateway::{ReadMethod, ReadOnlyGateway, ReadWriteGateway, WriteMethod};
use crate::{
    AdapterRegistry, CreateContract, DashboardCreateContracts, Failure, FailureParam,
    PublicationContext, Result, VersionedDashboardSnapshot, WidgetFieldType, ZabbixError,
    ZabbixVersion, is_zabbix_id,
};

/// Longest dashboard name Zabbix stores (`dashboard.name`, 255 characters).
pub const MAX_DASHBOARD_NAME_CHARS: usize = 255;

/// Dashboards read per name lookup; more means the answer is truncated.
pub const MAX_NAMED_DASHBOARDS: usize = 16;

const MAX_PARAM_CHARS: usize = 64;
const READ_ATTEMPTS: usize = 3;

/// Why the adapter refused to build a create. Stable codes, no prose.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CreateRejectionCode {
    /// The publication context is offline or detached from a session.
    AuthorityOffline,
    /// The context version differs from the Local snapshot version.
    ServerVersionMismatch,
    /// No adapter is registered for the Local snapshot version.
    AdapterUnsupported,
    /// A contract the create needs is not `Supported` (param `contract`).
    ContractUnverified,
    /// A widget type or field type unknown to the registry would be copied
    /// without its contract (params `page_index`, `widget_index`,
    /// `widget_type`, `reason`).
    WidgetCopyUnproven,
    /// Empty, too long, padded or with control characters.
    NameInvalid,
    /// Zabbix refuses a dashboard without pages.
    PagesEmpty,
    /// The Local snapshot lacks a key the payload copies (param `object`;
    /// `page` when the selected page is not exactly once in Local).
    SnapshotShapeInvalid,
    /// A widget of the copied page listens to a widget on another page
    /// (params `page_index`, `widget_index`, `field`).
    LinkOutsidePage,
}

impl CreateRejectionCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuthorityOffline => "publication_authority_offline",
            Self::ServerVersionMismatch => "server_version_mismatch",
            Self::AdapterUnsupported => "adapter_unsupported",
            Self::ContractUnverified => "write_contract_unverified",
            Self::WidgetCopyUnproven => "create_widget_copy_unproven",
            Self::NameInvalid => "dashboard_name_invalid",
            Self::PagesEmpty => "dashboard_pages_empty",
            Self::SnapshotShapeInvalid => "snapshot_shape_invalid",
            Self::LinkOutsidePage => "create_link_outside_page",
        }
    }
}

/// Adapter refusal of a create: a code plus safe parameters (positions,
/// widget types, contract codes); never names, values or secrets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateRejection {
    code: CreateRejectionCode,
    params: Vec<FailureParam>,
}

impl CreateRejection {
    fn new(code: CreateRejectionCode) -> Self {
        Self {
            code,
            params: Vec::new(),
        }
    }

    fn with(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.params.push(FailureParam {
            name,
            value: value.into(),
        });
        self
    }

    #[must_use]
    pub const fn code(&self) -> CreateRejectionCode {
        self.code
    }

    #[must_use]
    pub fn params(&self) -> &[FailureParam] {
        &self.params
    }

    #[must_use]
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|param| param.name == name)
            .map(|param| param.value.as_str())
    }
}

impl fmt::Display for CreateRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "dashboard create rejected: {}",
            self.code.as_str()
        )
    }
}

impl std::error::Error for CreateRejection {}

/// What the registry does not know about a copied widget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnknownCopyReason {
    WidgetType,
    FieldType,
}

impl UnknownCopyReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WidgetType => "widget_type",
            Self::FieldType => "field_type",
        }
    }
}

/// A widget copied as read although the registry does not know it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnknownCopyWidget {
    pub page_index: usize,
    pub widget_index: usize,
    pub widget_type: String,
    pub reason: UnknownCopyReason,
}

/// Which pages of Local a create copies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreateScope<'a> {
    /// Every page, in Local order (a dashboard draft).
    Dashboard,
    /// Exactly the page with this `dashboard_pageid` (a page draft).
    Page { dashboard_pageid: &'a str },
}

/// A consumer field of the copied page whose provider widget is on another
/// page of Local (positions in Local).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkOutsidePage {
    pub page_index: usize,
    pub widget_index: usize,
    /// The `*._reference` field name (bounded).
    pub field: String,
}

/// What a create of Local needs from the server, derived from Local (never
/// from the caller's claims). Positions are Local positions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateRequirements {
    pub contracts: BTreeSet<CreateContract>,
    /// Widgets that need `create_copy_unknown_widgets`, Local order.
    pub unknown_widgets: Vec<UnknownCopyWidget>,
    pub page_count: usize,
    pub widget_count: usize,
    /// `reference` fields (link providers) copied as read.
    pub reference_definitions: usize,
    /// `*._reference` fields (link consumers) copied as read.
    pub reference_consumers: usize,
    /// Page copy only: consumers whose provider is on another page. Any
    /// entry refuses the create.
    pub links_outside_page: Vec<LinkOutsidePage>,
}

impl CreateRequirements {
    /// Required contracts that are not `Supported`, in a stable order.
    #[must_use]
    pub fn missing_contracts(&self, contracts: &DashboardCreateContracts) -> Vec<CreateContract> {
        self.contracts
            .iter()
            .copied()
            .filter(|contract| !contracts.is_verified(*contract))
            .collect()
    }

    #[must_use]
    pub fn are_met_by(&self, contracts: &DashboardCreateContracts) -> bool {
        self.missing_contracts(contracts).is_empty()
    }
}

/// Adapter-validated `dashboard.create`. Deliberately neither `Clone` nor
/// deserializable, and its payload is never exposed.
pub struct VerifiedDashboardCreate {
    connection_id: String,
    server_version: ZabbixVersion,
    authority_fingerprint: String,
    local_fingerprint: String,
    name: String,
    create_params: Value,
    content_fingerprint: String,
    requirements: CreateRequirements,
}

impl VerifiedDashboardCreate {
    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub fn server_version(&self) -> &ZabbixVersion {
        &self.server_version
    }

    pub fn authority_fingerprint(&self) -> &str {
        &self.authority_fingerprint
    }

    /// Fingerprint of the Local snapshot the payload was built from.
    pub fn local_fingerprint(&self) -> &str {
        &self.local_fingerprint
    }

    /// Target dashboard name (user data).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Fingerprint of the expected content (name, properties, pages,
    /// widgets and fields, without server IDs); the read-back and the
    /// reconciliation compare against it.
    pub fn content_fingerprint(&self) -> &str {
        &self.content_fingerprint
    }

    pub fn required_contracts(&self) -> &BTreeSet<CreateContract> {
        &self.requirements.contracts
    }

    pub fn requirements(&self) -> &CreateRequirements {
        &self.requirements
    }
}

/// What a created dashboard must look like; also the durable key of a
/// later reconciliation (plain data: reconciliation only reads).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateExpectation {
    pub connection_id: String,
    pub server_version: ZabbixVersion,
    /// Exact target name (user data).
    pub name: String,
    pub content_fingerprint: String,
    /// `userid` of the connection account: the expected owner.
    pub owner_userid: String,
}

/// Result of comparing one read dashboard with a [`CreateExpectation`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadBackVerdict {
    Matches,
    /// Read under another connection or version.
    ConnectionDiffers,
    NameDiffers,
    /// Pages, widgets, geometry or fields differ from the plan.
    ContentDiffers,
    /// `private` is not `1`.
    NotPrivate,
    /// `users` or `userGroups` is not empty.
    Shared,
    /// Owned by another account.
    OwnerDiffers,
    /// The read dashboard lacks a key the comparison needs.
    ShapeInvalid,
}

impl ReadBackVerdict {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Matches => "matches",
            Self::ConnectionDiffers => "connection_differs",
            Self::NameDiffers => "name_differs",
            Self::ContentDiffers => "content_differs",
            Self::NotPrivate => "not_private",
            Self::Shared => "shared",
            Self::OwnerDiffers => "owner_differs",
            Self::ShapeInvalid => "shape_invalid",
        }
    }
}

/// Dashboards visible under an exact name filter.
#[derive(Clone)]
pub struct NamedDashboards {
    pub dashboards: Vec<VersionedDashboardSnapshot>,
    /// More than [`MAX_NAMED_DASHBOARDS`] were returned.
    pub truncated: bool,
}

/// Reconciliation of a create whose outcome is unknown (P2-06D). Only
/// `Confirmed` is a result; every other variant leaves the operation
/// undetermined and the target blocked until a manual check.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CreateReconciliation {
    /// Exactly one dashboard with the exact name, the expected content,
    /// private and owned by the account.
    Confirmed {
        dashboardid: String,
        /// Snapshot fingerprint of the read dashboard (O-022 record).
        fingerprint: String,
    },
    /// No visible dashboard with the name.
    NotFound,
    /// Several dashboards with the exact name (param `candidates`).
    Ambiguous { candidates: usize },
    /// One dashboard with the name that is not the planned one.
    Mismatch { verdict: ReadBackVerdict },
    /// The read was truncated; not every candidate was seen.
    PartialVisibility,
    /// The read failed.
    ReadFailed { failure: Failure },
    /// Another server version or account than the expectation.
    ConnectionMismatch,
}

impl CreateReconciliation {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Confirmed { .. } => "reconciled_verified",
            Self::NotFound => "reconcile_not_found",
            Self::Ambiguous { .. } => "reconcile_ambiguous",
            Self::Mismatch { .. } => "reconcile_structure_mismatch",
            Self::PartialVisibility => "reconcile_partial_visibility",
            Self::ReadFailed { .. } => "reconcile_read_failed",
            Self::ConnectionMismatch => "reconcile_connection_mismatch",
        }
    }

    #[must_use]
    pub const fn is_confirmed(&self) -> bool {
        matches!(self, Self::Confirmed { .. })
    }
}

/// Why a verified create was not sent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreateSendRefusal {
    /// Endpoint, identity, version, credential or write epoch changed.
    AuthorityChanged,
    /// A required contract is not `Supported` right before send.
    ContractUnverified(CreateContract),
    /// The name check before send could not read.
    NameCheckFailed,
    /// The caller could not record `submitting` before the send.
    SubmitNotRecorded,
}

impl CreateSendRefusal {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuthorityChanged => "connection_changed",
            Self::ContractUnverified(_) => "write_contract_unverified",
            Self::NameCheckFailed => "name_check_failed",
            Self::SubmitNotRecorded => "submitting_not_recorded",
        }
    }
}

/// Why a sent create failed with a known response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CreateFailureReason {
    /// The server refused the request; nothing was created.
    ApiRejected(Failure),
    /// A dashboard was created but differs from the plan.
    ReadBackMismatch(ReadBackVerdict),
    /// A dashboard was created but could not be read back.
    ReadBackUnavailable,
}

impl CreateFailureReason {
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ApiRejected(_) => "api_rejected",
            Self::ReadBackMismatch(_) => "read_back_mismatch",
            Self::ReadBackUnavailable => "read_back_unavailable",
        }
    }
}

/// Result of [`ReadWriteGateway::create_verified_dashboard`]. Plain data for
/// the publication journal; no payload, no server answer text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DashboardCreateOutcome {
    /// Not sent.
    FailedBeforeSend(CreateSendRefusal),
    /// Final `dashboard_name_taken`: visible before send (`sent: false`) or
    /// refused with `already exists` (`sent: true`). Never retried.
    NameTaken { sent: bool },
    /// Created and read back as planned.
    Verified {
        dashboardid: String,
        fingerprint: String,
    },
    /// Sent; the response was known but the result is not the plan.
    FailedAfterResponse {
        reason: CreateFailureReason,
        /// Set when the server reported a created dashboard.
        dashboardid: Option<String>,
    },
    /// No usable response; reads found exactly the planned dashboard.
    ReconciledVerified {
        dashboardid: String,
        fingerprint: String,
    },
    /// No usable response and no confirmed candidate. Never retried.
    UnknownOutcome {
        reconciliation: CreateReconciliation,
    },
}

impl DashboardCreateOutcome {
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::FailedBeforeSend(_) => "failed_before_send",
            Self::NameTaken { .. } => "dashboard_name_taken",
            Self::Verified { .. } => "verified",
            Self::FailedAfterResponse { .. } => "failed_after_response",
            Self::ReconciledVerified { .. } => "reconciled_verified",
            Self::UnknownOutcome { .. } => "unknown_outcome",
        }
    }

    /// Whether the create request was dispatched (always at most once).
    #[must_use]
    pub const fn sent(&self) -> bool {
        match self {
            Self::FailedBeforeSend(_) => false,
            Self::NameTaken { sent } => *sent,
            _ => true,
        }
    }

    /// The created dashboard confirmed by read-back or reconciliation.
    #[must_use]
    pub fn confirmed_dashboard(&self) -> Option<(&str, &str)> {
        match self {
            Self::Verified {
                dashboardid,
                fingerprint,
            }
            | Self::ReconciledVerified {
                dashboardid,
                fingerprint,
            } => Some((dashboardid, fingerprint)),
            _ => None,
        }
    }
}

/// A target name Zabbix stores unchanged: 1..=255 characters, no control
/// characters, no leading or trailing whitespace.
#[must_use]
pub fn is_valid_dashboard_name(name: &str) -> bool {
    let count = name.chars().count();
    (1..=MAX_DASHBOARD_NAME_CHARS).contains(&count)
        && name.trim() == name
        && !name.chars().any(char::is_control)
}

struct Prepared {
    params: Value,
    content_fingerprint: String,
    requirements: CreateRequirements,
}

fn shape(object: &'static str) -> CreateRejection {
    CreateRejection::new(CreateRejectionCode::SnapshotShapeInvalid).with("object", object)
}

fn scalar(value: Option<&Value>) -> Option<&Value> {
    value.filter(|value| value.is_string() || value.is_number())
}

fn copy_keys(
    source: &Value,
    keys: &[&str],
    object: &'static str,
) -> std::result::Result<Map<String, Value>, CreateRejection> {
    let mut output = Map::new();
    for key in keys {
        let value = if *key == "name" {
            source.get(*key).filter(|value| value.is_string())
        } else {
            scalar(source.get(*key))
        };
        output.insert(
            (*key).to_owned(),
            value.cloned().ok_or_else(|| shape(object))?,
        );
    }
    Ok(output)
}

const WIDGET_KEYS: [&str; 7] = ["type", "name", "x", "y", "width", "height", "view_mode"];

fn field_type_known(value: &Value) -> bool {
    let number = value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()));
    number
        .and_then(|number| u16::try_from(number).ok())
        .is_some_and(|number| WidgetFieldType::try_from(number).is_ok())
}

/// Provider value of the dashboard-level selector (`DASHBOARD._hostid`).
const DASHBOARD_REFERENCE: &str = "DASHBOARD";

/// The reference a `*._reference` consumer value listens to
/// (`<reference>._<channel>`).
fn consumed_reference(value: &Value) -> Option<String> {
    let text = text(Some(value))?;
    let reference = text
        .split_once("._")
        .map_or(text.as_str(), |(reference, _)| reference);
    (!reference.is_empty()).then(|| reference.to_owned())
}

/// `reference` values defined by the widgets of `page`.
fn defined_references(page: &Value) -> BTreeSet<String> {
    page.get("widgets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|widget| widget.get("fields").and_then(Value::as_array))
        .flatten()
        .filter(|field| field.get("name").and_then(Value::as_str) == Some("reference"))
        .filter_map(|field| text(field.get("value")))
        .collect()
}

/// Consumers on page `selected` whose reference only another page defines.
fn links_outside_page(pages: &[Value], selected: usize) -> Vec<LinkOutsidePage> {
    let Some(page) = pages.get(selected) else {
        return Vec::new();
    };
    let here = defined_references(page);
    let elsewhere = pages
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != selected)
        .flat_map(|(_, page)| defined_references(page))
        .collect::<BTreeSet<_>>();
    let mut links = Vec::new();
    let widgets = page.get("widgets").and_then(Value::as_array);
    for (widget_index, widget) in widgets.into_iter().flatten().enumerate() {
        let fields = widget.get("fields").and_then(Value::as_array);
        for field in fields.into_iter().flatten() {
            let Some(field_name) = field.get("name").and_then(Value::as_str) else {
                continue;
            };
            if !field_name.ends_with("._reference") {
                continue;
            }
            let Some(reference) = field.get("value").and_then(consumed_reference) else {
                continue;
            };
            if reference != DASHBOARD_REFERENCE
                && !here.contains(&reference)
                && elsewhere.contains(&reference)
            {
                links.push(LinkOutsidePage {
                    page_index: selected,
                    widget_index,
                    field: bounded(field_name),
                });
            }
        }
    }
    links
}

/// Builds the `dashboard.create` params from the pages of `scope` in Local
/// and derives what the create needs. Pure; no capability is consulted.
fn prepare(
    local: &VersionedDashboardSnapshot,
    scope: CreateScope<'_>,
    name: &str,
) -> std::result::Result<Prepared, CreateRejection> {
    if !is_valid_dashboard_name(name) {
        return Err(CreateRejection::new(CreateRejectionCode::NameInvalid));
    }
    let registry = AdapterRegistry::widget_capabilities(local.server_version())
        .map_err(|_| CreateRejection::new(CreateRejectionCode::AdapterUnsupported))?;
    let raw = local.raw_api_document();
    let all_pages = raw
        .get("pages")
        .and_then(Value::as_array)
        .ok_or_else(|| shape("dashboard"))?;
    if all_pages.is_empty() {
        return Err(CreateRejection::new(CreateRejectionCode::PagesEmpty));
    }
    let (pages, links_outside_page) = match scope {
        CreateScope::Dashboard => (all_pages.iter().enumerate().collect::<Vec<_>>(), Vec::new()),
        CreateScope::Page { dashboard_pageid } => {
            let mut matching = all_pages.iter().enumerate().filter(|(_, page)| {
                page.get("dashboard_pageid").and_then(Value::as_str) == Some(dashboard_pageid)
            });
            let (Some(selected), None) = (matching.next(), matching.next()) else {
                return Err(shape("page"));
            };
            (vec![selected], links_outside_page(all_pages, selected.0))
        }
    };
    let properties = copy_keys(raw, &["display_period", "auto_start"], "dashboard")?;
    let mut requirements = CreateRequirements {
        contracts: CreateContract::COPY.into_iter().collect(),
        unknown_widgets: Vec::new(),
        page_count: pages.len(),
        widget_count: 0,
        reference_definitions: 0,
        reference_consumers: 0,
        links_outside_page,
    };
    let mut encoded_pages = Vec::with_capacity(pages.len());
    for (page_index, page) in pages {
        let mut output = copy_keys(page, &["name", "display_period"], "page")?;
        let widgets = page
            .get("widgets")
            .and_then(Value::as_array)
            .ok_or_else(|| shape("page"))?;
        let mut encoded_widgets = Vec::with_capacity(widgets.len());
        for (widget_index, widget) in widgets.iter().enumerate() {
            let mut encoded = copy_keys(widget, &WIDGET_KEYS, "widget")?;
            let widget_type = widget
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| shape("widget"))?;
            let mut unknown = registry
                .find(widget_type)
                .is_none()
                .then_some(UnknownCopyReason::WidgetType);
            let fields = widget
                .get("fields")
                .and_then(Value::as_array)
                .ok_or_else(|| shape("widget"))?;
            let mut encoded_fields = Vec::with_capacity(fields.len());
            for field in fields {
                let field_name = field
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| shape("field"))?;
                let field_type = scalar(field.get("type")).ok_or_else(|| shape("field"))?;
                let value = scalar(field.get("value")).ok_or_else(|| shape("field"))?;
                if !field_type_known(field_type) {
                    unknown.get_or_insert(UnknownCopyReason::FieldType);
                }
                if field_name == "reference" {
                    requirements.reference_definitions += 1;
                } else if field_name.ends_with("._reference") {
                    requirements.reference_consumers += 1;
                }
                encoded_fields.push(json!({
                    "type": field_type, "name": field_name, "value": value
                }));
            }
            if let Some(reason) = unknown {
                requirements.unknown_widgets.push(UnknownCopyWidget {
                    page_index,
                    widget_index,
                    widget_type: bounded(widget_type),
                    reason,
                });
            }
            encoded.insert("fields".to_owned(), Value::Array(encoded_fields));
            encoded_widgets.push(Value::Object(encoded));
        }
        requirements.widget_count += widgets.len();
        output.insert("widgets".to_owned(), Value::Array(encoded_widgets));
        encoded_pages.push(Value::Object(output));
    }
    if !requirements.unknown_widgets.is_empty() {
        requirements
            .contracts
            .insert(CreateContract::CopyUnknownWidgets);
    }
    let mut params = Map::new();
    params.insert("name".to_owned(), Value::String(name.to_owned()));
    params.extend(properties);
    params.insert("private".to_owned(), json!(1));
    params.insert("users".to_owned(), json!([]));
    params.insert("userGroups".to_owned(), json!([]));
    params.insert("pages".to_owned(), Value::Array(encoded_pages));
    let params = Value::Object(params);
    let content_fingerprint = content_fingerprint(&params).ok_or_else(|| shape("dashboard"))?;
    Ok(Prepared {
        params,
        content_fingerprint,
        requirements,
    })
}

/// What creating `local` under `name` needs from the server: contracts,
/// widgets the registry does not know, counts. Pure; lets the application
/// report every gap at once.
pub fn create_requirements(
    local: &VersionedDashboardSnapshot,
    name: &str,
) -> std::result::Result<CreateRequirements, CreateRejection> {
    create_requirements_for(local, CreateScope::Dashboard, name)
}

/// [`create_requirements`] of the pages of `scope`; a page copy also lists
/// its links to other pages.
pub fn create_requirements_for(
    local: &VersionedDashboardSnapshot,
    scope: CreateScope<'_>,
    name: &str,
) -> std::result::Result<CreateRequirements, CreateRejection> {
    prepare(local, scope, name).map(|prepared| prepared.requirements)
}

/// Builds the only sendable form of a dashboard create: all pages and
/// widgets of `local` without server IDs, private, under `name`, gated by
/// every create contract it needs, for the exact connected version and live
/// authority of `context`.
pub fn verify_dashboard_create(
    local: &VersionedDashboardSnapshot,
    context: &PublicationContext,
    name: &str,
) -> std::result::Result<VerifiedDashboardCreate, CreateRejection> {
    verify_dashboard_create_for(local, CreateScope::Dashboard, context, name)
}

/// [`verify_dashboard_create`] of the pages of `scope`: a page draft
/// creates a new dashboard of exactly its page. A link of that page to a
/// widget on another page refuses the create (`create_link_outside_page`).
pub fn verify_dashboard_create_for(
    local: &VersionedDashboardSnapshot,
    scope: CreateScope<'_>,
    context: &PublicationContext,
    name: &str,
) -> std::result::Result<VerifiedDashboardCreate, CreateRejection> {
    if !context.is_live_authenticated() {
        return Err(CreateRejection::new(CreateRejectionCode::AuthorityOffline));
    }
    let capabilities = context.capabilities();
    if capabilities.version != *local.server_version() {
        return Err(CreateRejection::new(
            CreateRejectionCode::ServerVersionMismatch,
        ));
    }
    let Prepared {
        params,
        content_fingerprint,
        requirements,
    } = prepare(local, scope, name)?;
    if let Some(link) = requirements.links_outside_page.first() {
        return Err(CreateRejection::new(CreateRejectionCode::LinkOutsidePage)
            .with("page_index", link.page_index.to_string())
            .with("widget_index", link.widget_index.to_string())
            .with("field", link.field.clone()));
    }
    if let Some(contract) = requirements
        .missing_contracts(&capabilities.dashboard_create_contracts)
        .first()
    {
        return Err(match (contract, requirements.unknown_widgets.first()) {
            (CreateContract::CopyUnknownWidgets, Some(widget)) => {
                CreateRejection::new(CreateRejectionCode::WidgetCopyUnproven)
                    .with("page_index", widget.page_index.to_string())
                    .with("widget_index", widget.widget_index.to_string())
                    .with("widget_type", widget.widget_type.clone())
                    .with("reason", widget.reason.as_str())
            }
            _ => CreateRejection::new(CreateRejectionCode::ContractUnverified)
                .with("contract", contract.as_str()),
        });
    }
    Ok(VerifiedDashboardCreate {
        connection_id: local.connection_id().to_owned(),
        server_version: local.server_version().clone(),
        authority_fingerprint: context.binding_fingerprint().to_owned(),
        local_fingerprint: local.fingerprint().to_owned(),
        name: name.to_owned(),
        create_params: params,
        content_fingerprint,
        requirements,
    })
}

fn text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// Canonical content of a create payload or of a read dashboard: name,
/// properties, pages in order, widgets and fields sorted, scalars as text,
/// server IDs, owner and sharing left out.
fn canonical_content(document: &Value) -> Option<Value> {
    let pages = document
        .get("pages")?
        .as_array()?
        .iter()
        .map(|page| {
            let mut widgets = page
                .get("widgets")?
                .as_array()?
                .iter()
                .map(|widget| {
                    let mut fields = widget
                        .get("fields")?
                        .as_array()?
                        .iter()
                        .map(|field| {
                            Some(json!([
                                text(field.get("type"))?,
                                field.get("name")?.as_str()?,
                                text(field.get("value"))?,
                            ]))
                        })
                        .collect::<Option<Vec<_>>>()?;
                    fields.sort_by_cached_key(Value::to_string);
                    let mut canonical = Map::new();
                    for key in WIDGET_KEYS {
                        canonical.insert(key.to_owned(), Value::String(text(widget.get(key))?));
                    }
                    canonical.insert("fields".to_owned(), Value::Array(fields));
                    Some(Value::Object(canonical))
                })
                .collect::<Option<Vec<_>>>()?;
            widgets.sort_by_cached_key(Value::to_string);
            Some(json!({
                "name": text(page.get("name"))?,
                "display_period": text(page.get("display_period"))?,
                "widgets": widgets,
            }))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(json!({
        "name": text(document.get("name"))?,
        "display_period": text(document.get("display_period"))?,
        "auto_start": text(document.get("auto_start"))?,
        "pages": pages,
    }))
}

fn content_fingerprint(document: &Value) -> Option<String> {
    let canonical = serde_json::to_vec(&canonical_content(document)?).ok()?;
    let mut digest = Sha256::new();
    digest.update(b"zdashstitch-create-content-v1\0");
    digest.update(&canonical);
    let mut output = String::with_capacity(64);
    for byte in digest.finalize() {
        write!(&mut output, "{byte:02x}").ok()?;
    }
    Some(output)
}

/// Content fingerprint of a read dashboard, comparable with
/// [`VerifiedDashboardCreate::content_fingerprint`]; `None` for a shape the
/// comparison does not understand.
#[must_use]
pub fn dashboard_content_fingerprint(snapshot: &VersionedDashboardSnapshot) -> Option<String> {
    content_fingerprint(snapshot.raw_api_document())
}

/// Compares a read dashboard with the expectation: connection and version,
/// exact name, content, privacy and owner (P2-06D read-back). Pure.
#[must_use]
pub fn check_created_dashboard(
    snapshot: &VersionedDashboardSnapshot,
    expectation: &CreateExpectation,
) -> ReadBackVerdict {
    if snapshot.connection_id() != expectation.connection_id
        || *snapshot.server_version() != expectation.server_version
    {
        return ReadBackVerdict::ConnectionDiffers;
    }
    let raw = snapshot.raw_api_document();
    if raw.get("name").and_then(Value::as_str) != Some(expectation.name.as_str()) {
        return ReadBackVerdict::NameDiffers;
    }
    let Some(fingerprint) = content_fingerprint(raw) else {
        return ReadBackVerdict::ShapeInvalid;
    };
    if fingerprint != expectation.content_fingerprint {
        return ReadBackVerdict::ContentDiffers;
    }
    let (Some(private), Some(users), Some(groups), Some(owner)) = (
        text(raw.get("private")),
        raw.get("users").and_then(Value::as_array),
        raw.get("userGroups").and_then(Value::as_array),
        text(raw.get("userid")),
    ) else {
        return ReadBackVerdict::ShapeInvalid;
    };
    if private != "1" {
        return ReadBackVerdict::NotPrivate;
    }
    if !users.is_empty() || !groups.is_empty() {
        return ReadBackVerdict::Shared;
    }
    if owner != expectation.owner_userid {
        return ReadBackVerdict::OwnerDiffers;
    }
    ReadBackVerdict::Matches
}

/// Reconciliation by exact name and content fingerprint on one read (P2-06D).
/// Pure: only exactly one candidate that passes
/// [`check_created_dashboard`] is `Confirmed`.
#[must_use]
pub fn reconcile_created_dashboard(
    read: std::result::Result<&NamedDashboards, &Failure>,
    expectation: &CreateExpectation,
) -> CreateReconciliation {
    let named = match read {
        Ok(named) => named,
        Err(failure) => {
            return CreateReconciliation::ReadFailed {
                failure: failure.clone(),
            };
        }
    };
    if named.truncated {
        return CreateReconciliation::PartialVisibility;
    }
    let candidates = named
        .dashboards
        .iter()
        .filter(|snapshot| {
            snapshot
                .raw_api_document()
                .get("name")
                .and_then(Value::as_str)
                == Some(expectation.name.as_str())
        })
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [] => CreateReconciliation::NotFound,
        [candidate] => match check_created_dashboard(candidate, expectation) {
            ReadBackVerdict::Matches => CreateReconciliation::Confirmed {
                dashboardid: candidate.dashboard_id().to_owned(),
                fingerprint: candidate.fingerprint().to_owned(),
            },
            verdict => CreateReconciliation::Mismatch { verdict },
        },
        several => CreateReconciliation::Ambiguous {
            candidates: several.len(),
        },
    }
}

impl ReadOnlyGateway {
    /// Dashboards with exactly `name` (`dashboard.get` `filter`), with
    /// pages, widgets and sharing. At most [`MAX_NAMED_DASHBOARDS`]; an
    /// empty answer does not prove the name is free (P1-IN-04: another
    /// user's private dashboard is invisible).
    ///
    /// Only dashboards whose name equals `name` byte for byte are kept, so
    /// a database collation that matches case-insensitively cannot report
    /// a case variant as taken: name uniqueness of `dashboard.create` is
    /// case-sensitive on PostgreSQL (LAB-02, 28.10.2026). `truncated`
    /// counts the unfiltered answer.
    pub async fn dashboards_named(
        &self,
        connection_id: &str,
        name: &str,
    ) -> Result<NamedDashboards> {
        if connection_id.is_empty() || connection_id.len() > 128 {
            return Err(ZabbixError::SnapshotShape(
                "connection_id must contain 1..=128 bytes".to_owned(),
            ));
        }
        if !is_valid_dashboard_name(name) {
            return Err(ZabbixError::SnapshotShape(
                "dashboard name is not a valid target name".to_owned(),
            ));
        }
        let mut params = crate::dashboards::snapshot_read_params();
        params["filter"] = json!({ "name": [name] });
        params["limit"] = json!(MAX_NAMED_DASHBOARDS + 1);
        let raw: Vec<Value> = self
            .session
            .transport
            .read(ReadMethod::DashboardGet, params, &self.session.token)
            .await?;
        let truncated = raw.len() > MAX_NAMED_DASHBOARDS;
        let dashboards = raw
            .into_iter()
            .take(MAX_NAMED_DASHBOARDS)
            .filter(|document| document.get("name").and_then(Value::as_str) == Some(name))
            .map(|document| {
                VersionedDashboardSnapshot::from_adapter_raw(
                    connection_id.to_owned(),
                    self.session.capabilities.version.clone(),
                    document,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(NamedDashboards {
            dashboards,
            truncated,
        })
    }

    /// Read-only reconciliation of a create, also after a restart: by the
    /// reported `dashboardid` when one is known, otherwise by exact name and
    /// content fingerprint. Never writes.
    pub async fn reconcile_dashboard_create(
        &self,
        expectation: &CreateExpectation,
        known_dashboardid: Option<&str>,
    ) -> CreateReconciliation {
        if self.capabilities().version != expectation.server_version
            || self.identity().account_userid() != expectation.owner_userid
        {
            return CreateReconciliation::ConnectionMismatch;
        }
        if let Some(dashboardid) = known_dashboardid {
            return match self
                .dashboard_snapshot(&expectation.connection_id, dashboardid)
                .await
            {
                Ok(None) => CreateReconciliation::NotFound,
                Ok(Some(snapshot)) => match check_created_dashboard(&snapshot, expectation) {
                    ReadBackVerdict::Matches => CreateReconciliation::Confirmed {
                        dashboardid: snapshot.dashboard_id().to_owned(),
                        fingerprint: snapshot.fingerprint().to_owned(),
                    },
                    verdict => CreateReconciliation::Mismatch { verdict },
                },
                Err(error) => CreateReconciliation::ReadFailed {
                    failure: error.failure(),
                },
            };
        }
        match self
            .dashboards_named(&expectation.connection_id, &expectation.name)
            .await
        {
            Ok(named) => reconcile_created_dashboard(Ok(&named), expectation),
            Err(error) => reconcile_created_dashboard(Err(&error.failure()), expectation),
        }
    }
}

impl ReadWriteGateway {
    /// The expectation of `create` on this connection; record it before
    /// send so that an unknown outcome can be reconciled after a restart.
    pub fn create_expectation(&self, create: &VerifiedDashboardCreate) -> CreateExpectation {
        CreateExpectation {
            connection_id: create.connection_id.clone(),
            server_version: create.server_version.clone(),
            name: create.name.clone(),
            content_fingerprint: create.content_fingerprint.clone(),
            owner_userid: self.reader().identity().account_userid().to_owned(),
        }
    }

    /// Mandatory closed create path: authority and contracts, a name check
    /// (a hint), exactly one `dashboard.create`, then read-back or
    /// read-only reconciliation. There is no public send-only method.
    ///
    /// `before_send` runs at most once, after every pre-send check and
    /// immediately before the single write request; the caller makes the
    /// durable `submitting` journal record there (O-018 §6, P2-04D) with
    /// [`Self::create_expectation`] as the reconciliation key. `false`
    /// means the record could not be made: nothing is sent.
    pub async fn create_verified_dashboard<G>(
        &self,
        create: &VerifiedDashboardCreate,
        before_send: G,
    ) -> DashboardCreateOutcome
    where
        G: FnOnce() -> bool + Send,
    {
        let context = self.publication_context();
        if !context.is_live_authenticated()
            || context.binding_fingerprint() != create.authority_fingerprint
            || self.capabilities().version != create.server_version
        {
            return DashboardCreateOutcome::FailedBeforeSend(CreateSendRefusal::AuthorityChanged);
        }
        let contracts = &self.capabilities().dashboard_create_contracts;
        // The copy contracts are always required, whatever the plan lists.
        if let Some(contract) = CreateContract::COPY
            .iter()
            .chain(create.requirements.contracts.iter())
            .find(|contract| !contracts.is_verified(**contract))
        {
            return DashboardCreateOutcome::FailedBeforeSend(
                CreateSendRefusal::ContractUnverified(*contract),
            );
        }
        let expectation = self.create_expectation(create);
        match self
            .reader()
            .dashboards_named(&create.connection_id, &create.name)
            .await
        {
            Ok(named) if named.dashboards.is_empty() && !named.truncated => {}
            Ok(_) => return DashboardCreateOutcome::NameTaken { sent: false },
            Err(_) => {
                return DashboardCreateOutcome::FailedBeforeSend(
                    CreateSendRefusal::NameCheckFailed,
                );
            }
        }
        if !before_send() {
            return DashboardCreateOutcome::FailedBeforeSend(CreateSendRefusal::SubmitNotRecorded);
        }

        let session = self.session();
        let dispatch = session
            .transport
            .write::<Value>(
                WriteMethod::DashboardCreate,
                create.create_params.clone(),
                &session.token,
                &self.grant,
            )
            .await;
        match dispatch {
            Ok(result) => match created_id(&result) {
                Some(dashboardid) => self.read_back(&expectation, dashboardid).await,
                None => self.reconcile_unknown(&expectation).await,
            },
            Err(error) if is_name_taken(&error, &create.name) => {
                DashboardCreateOutcome::NameTaken { sent: true }
            }
            Err(error @ ZabbixError::Api { .. }) => DashboardCreateOutcome::FailedAfterResponse {
                reason: CreateFailureReason::ApiRejected(error.failure()),
                dashboardid: None,
            },
            // Transport loss, HTTP status (a proxy may answer after the
            // server committed), oversized or malformed answers: unknown.
            Err(_) => self.reconcile_unknown(&expectation).await,
        }
    }

    async fn read_back(
        &self,
        expectation: &CreateExpectation,
        dashboardid: String,
    ) -> DashboardCreateOutcome {
        for attempt in 0..READ_ATTEMPTS {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            if let Ok(Some(snapshot)) = self
                .reader()
                .dashboard_snapshot(&expectation.connection_id, &dashboardid)
                .await
            {
                return match check_created_dashboard(&snapshot, expectation) {
                    ReadBackVerdict::Matches => DashboardCreateOutcome::Verified {
                        dashboardid,
                        fingerprint: snapshot.fingerprint().to_owned(),
                    },
                    verdict => DashboardCreateOutcome::FailedAfterResponse {
                        reason: CreateFailureReason::ReadBackMismatch(verdict),
                        dashboardid: Some(dashboardid),
                    },
                };
            }
        }
        DashboardCreateOutcome::FailedAfterResponse {
            reason: CreateFailureReason::ReadBackUnavailable,
            dashboardid: Some(dashboardid),
        }
    }

    async fn reconcile_unknown(&self, expectation: &CreateExpectation) -> DashboardCreateOutcome {
        let mut last = CreateReconciliation::NotFound;
        for attempt in 0..READ_ATTEMPTS {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            last = self
                .reader()
                .reconcile_dashboard_create(expectation, None)
                .await;
            if let CreateReconciliation::Confirmed {
                dashboardid,
                fingerprint,
            } = last
            {
                return DashboardCreateOutcome::ReconciledVerified {
                    dashboardid,
                    fingerprint,
                };
            }
        }
        DashboardCreateOutcome::UnknownOutcome {
            reconciliation: last,
        }
    }
}

/// The single `dashboardid` of a `dashboard.create` result.
fn created_id(result: &Value) -> Option<String> {
    match result.get("dashboardids")?.as_array()?.as_slice() {
        [id] => id.as_str().filter(|id| is_zabbix_id(id)).map(str::to_owned),
        _ => None,
    }
}

/// `Dashboard "<name>" already exists.` for exactly the target name
/// (P1-IN-04). Any other refusal stays a generic API rejection.
fn is_name_taken(error: &ZabbixError, name: &str) -> bool {
    let ZabbixError::Api { message, data, .. } = error else {
        return false;
    };
    let expected = format!("Dashboard \"{name}\" already exists.");
    data.as_deref() == Some(expected.as_str()) || *message == expected
}

fn bounded(text: &str) -> String {
    text.chars().take(MAX_PARAM_CHARS).collect()
}

#[cfg(test)]
mod tests;
