// SPDX-License-Identifier: MIT OR Apache-2.0

//! Closed dashboard publication path of the 7.4 adapter (P0-07, P2-04B).
//!
//! A [`VerifiedDashboardUpdate`] is built only by
//! [`verify_widget_field_replacements`] from a freshly read dashboard and a
//! set of field value replacements. It is neither `Clone` nor
//! deserializable, its payload is never exposed, and the only way to send it
//! is [`ReadWriteGateway::publish_verified_dashboard_update`].
//!
//! Payload rules (P0-06 contract «update one page with full fields»,
//! extended to several pages behind `update_several_pages`):
//!
//! - `dashboard.update` carries `dashboardid` and **every** page in the read
//!   order; omitting a page would delete it.
//! - An unchanged page is sent as `{"dashboard_pageid"}` only, so the server
//!   keeps it as stored (`preserve_untouched_pages`).
//! - Replacements are addressed by page position and `widgetid`, never by
//!   the widget's position inside the page, and the payload is built again
//!   from the read made right before send: every page, widget and field is
//!   sent in the order the server returned it that time.
//! - A changed page is sent with `dashboard_pageid`, `name`,
//!   `display_period` and all its widgets, each with `widgetid`, `type`,
//!   `name`, geometry, `view_mode` and the full `fields` array copied from
//!   the read; only the `value` of each replaced field differs, keeping its
//!   JSON kind. Every widget of a changed page must be of a type whose full
//!   resend is proven ([`RESEND_PROVEN_WIDGET_TYPES`]).
//! - Dashboard properties, owner and sharing (`name`, `private`, `userid`,
//!   `users`, `userGroups`, …) are never sent.
//! - A widget of a changed page with two fields of the same `name` is
//!   refused (`field_ambiguous`). The server accepts such widgets (LAB-02),
//!   but matches stored fields by (`type`, `name`) on update, so such a
//!   resend has no proven meaning.
//!
//! Comparison rules (LAB-02, 28.10.2026): the source check before send and
//! the read-back compare [`VersionedDashboardSnapshot::fingerprint`], the
//! canonical fingerprint. Zabbix 7.4.16 returns widgets and widget fields
//! without a defined order, and a field changed by `dashboard.update` is
//! read back at another position; page order, every value and its JSON
//! kind still have to match exactly (see
//! [`crate::canonical_document`]).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};
use std::time::Duration;

use secrecy::ExposeSecret;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::gateway::{ReadMethod, ReadWriteGateway, WriteMethod};
use crate::{
    DashboardWriteContracts, FailureParam, Result, VersionedDashboardSnapshot, WriteContract,
    ZabbixCapabilities, ZabbixError, ZabbixVersion, is_zabbix_id,
};

/// Upper bound of replacements in one verified update.
pub const MAX_FIELD_REPLACEMENTS: usize = 1_024;

/// Widget types whose full read-back `fields` survive a resend on a changed
/// page (P0-06 laboratory: `item`, `svggraph`; LAB-02, 28.10.2026: classic
/// `graph`, both `source_type` 0 and 1). Any other widget on a page that
/// must be sent blocks the update until its own contract is recorded.
pub const RESEND_PROVEN_WIDGET_TYPES: [&str; 3] = ["item", "svggraph", "graph"];

const MAX_ID_DIGITS: usize = 20;
const MAX_HOST_PATTERN_CHARS: usize = 255;
const MAX_PARAM_CHARS: usize = 64;

/// One field value replacement, addressed by its page position and the
/// `widgetid` of the widget on that page, and checked against the read
/// value. The widget's position inside the page is not part of the address:
/// Zabbix 7.4.16 returns the widgets of a page in no defined order (LAB-02).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WidgetFieldReplacement {
    pub page_index: usize,
    /// `widgetid` of the widget; it must occur exactly once on the page, and
    /// on no other page.
    pub widgetid: String,
    pub field_name: String,
    /// Value read with the snapshot (`itemid`, `graphid` or a host name).
    pub expected_value: String,
    pub replacement_value: String,
}

/// Why the adapter refused to build an update. Stable codes, no prose.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum WriteRejectionCode {
    /// The publication context is offline or detached from a session.
    AuthorityOffline,
    /// The context version differs from the snapshot version.
    ServerVersionMismatch,
    /// A contract the update needs is not `Supported` (param `contract`).
    ContractUnverified,
    /// A changed page holds a widget type without a proven resend (params
    /// `page_index`, `widget_index`, `widget_type`).
    ChangedPageWidgetUnproven,
    /// Zero or more than [`MAX_FIELD_REPLACEMENTS`] replacements.
    ReplacementCountInvalid,
    /// The read dashboard has an unexpected shape.
    SnapshotShapeInvalid,
    /// The page position is out of range, or no widget of the dashboard has
    /// the `widgetid`.
    WidgetNotFound,
    /// The `widgetid` is on another page, or occurs more than once on the
    /// page.
    WidgetIdentityMismatch,
    /// No field with the name on the widget.
    FieldNotFound,
    /// Several fields with the name on the widget.
    FieldAmbiguous,
    /// The field is not a replaceable host reference of a known kind
    /// (params `widget_type`, `field`, optional `field_kind`).
    FieldKindUnsupported,
    /// The read value differs from the expected one.
    SourceValueStale,
    /// The replacement value is malformed for the field kind.
    ReplacementValueInvalid,
    /// The replacement equals the read value.
    ReplacementUnchanged,
    /// The same widget field is replaced twice.
    FieldReplacedTwice,
}

impl WriteRejectionCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuthorityOffline => "publication_authority_offline",
            Self::ServerVersionMismatch => "server_version_mismatch",
            Self::ContractUnverified => "write_contract_unverified",
            Self::ChangedPageWidgetUnproven => "changed_page_widget_unproven",
            Self::ReplacementCountInvalid => "replacement_count_invalid",
            Self::SnapshotShapeInvalid => "snapshot_shape_invalid",
            Self::WidgetNotFound => "widget_not_found",
            Self::WidgetIdentityMismatch => "widget_identity_mismatch",
            Self::FieldNotFound => "field_not_found",
            Self::FieldAmbiguous => "field_ambiguous",
            Self::FieldKindUnsupported => "field_kind_unsupported",
            Self::SourceValueStale => "source_value_stale",
            Self::ReplacementValueInvalid => "replacement_value_invalid",
            Self::ReplacementUnchanged => "replacement_unchanged",
            Self::FieldReplacedTwice => "field_replaced_twice",
        }
    }
}

/// Adapter refusal: a code plus safe parameters (positions, Zabbix field
/// names, contract codes); never values, names or secrets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriteRejection {
    code: WriteRejectionCode,
    params: Vec<FailureParam>,
}

impl WriteRejection {
    fn new(code: WriteRejectionCode) -> Self {
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

    fn at(self, replacement: &WidgetFieldReplacement) -> Self {
        self.with("page_index", replacement.page_index.to_string())
            .with("field", bounded(&replacement.field_name))
    }

    /// [`Self::at`] plus the position at which the widget was read this time.
    fn at_widget(self, replacement: &WidgetFieldReplacement, widget_index: usize) -> Self {
        self.at(replacement)
            .with("widget_index", widget_index.to_string())
    }

    #[must_use]
    pub const fn code(&self) -> WriteRejectionCode {
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

impl fmt::Display for WriteRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "dashboard write rejected: {}",
            self.code.as_str()
        )
    }
}

impl std::error::Error for WriteRejection {}

/// A widget on a changed page whose full resend is not proven.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnprovenWidget {
    pub page_index: usize,
    pub widget_index: usize,
    pub widget_type: String,
}

/// What a set of replacements needs from the server, derived from the read
/// dashboard (never from the caller's claims).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriteRequirements {
    /// Every contract the update relies on.
    pub contracts: BTreeSet<WriteContract>,
    /// Pages sent with full widgets, read order.
    pub changed_pages: BTreeSet<usize>,
    /// Replacement count per field-kind contract.
    pub kinds: BTreeMap<WriteContract, usize>,
    /// Widgets of changed pages without a proven resend.
    pub unproven_widgets: Vec<UnprovenWidget>,
}

impl WriteRequirements {
    /// Required contracts that are not `Supported`, in a stable order.
    #[must_use]
    pub fn missing_contracts(&self, contracts: &DashboardWriteContracts) -> Vec<WriteContract> {
        self.contracts
            .iter()
            .copied()
            .filter(|contract| !contracts.is_verified(*contract))
            .collect()
    }

    /// Whether `contracts` allow this update.
    #[must_use]
    pub fn are_met_by(&self, contracts: &DashboardWriteContracts) -> bool {
        self.missing_contracts(contracts).is_empty() && self.unproven_widgets.is_empty()
    }
}

/// Non-secret binding to the authenticated connection and exact capability
/// profile. Offline contexts can validate a preview but cannot authorize send.
#[derive(Clone)]
pub struct PublicationContext {
    capabilities: ZabbixCapabilities,
    binding_fingerprint: String,
    live_authenticated: bool,
}

impl PublicationContext {
    pub fn capabilities(&self) -> &ZabbixCapabilities {
        &self.capabilities
    }

    /// Fingerprint of the publication authority (endpoint, server identity,
    /// version, credential and write epoch). Changes whenever any of them
    /// changes; plans compare it before send.
    pub fn binding_fingerprint(&self) -> &str {
        &self.binding_fingerprint
    }

    #[doc(hidden)]
    pub fn offline_for_planning(capabilities: ZabbixCapabilities) -> Self {
        Self {
            capabilities,
            binding_fingerprint: "offline-unpublishable".to_owned(),
            live_authenticated: false,
        }
    }

    /// A context that verifies plans but matches no gateway, so nothing
    /// built with it can be sent.
    #[doc(hidden)]
    pub fn detached_for_testing(capabilities: ZabbixCapabilities) -> Self {
        Self {
            capabilities,
            binding_fingerprint: "detached-test-authority".to_owned(),
            live_authenticated: true,
        }
    }

    pub fn is_live_authenticated(&self) -> bool {
        self.live_authenticated
    }
}

/// Adapter-validated update. It is deliberately neither `Clone` nor
/// deserializable, and its payload is never exposed.
pub struct VerifiedDashboardUpdate {
    connection_id: String,
    server_version: ZabbixVersion,
    dashboard_id: String,
    source_fingerprint: String,
    expected_fingerprint: String,
    authority_fingerprint: String,
    /// Applied again to the read right before send, so the payload carries
    /// pages, widgets and fields in the order the server returned them then.
    replacements: Vec<WidgetFieldReplacement>,
    replacement_count: usize,
    required_contracts: BTreeSet<WriteContract>,
    changed_pages: Vec<usize>,
    kinds: BTreeMap<WriteContract, usize>,
}

impl VerifiedDashboardUpdate {
    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub fn server_version(&self) -> &ZabbixVersion {
        &self.server_version
    }

    pub fn dashboard_id(&self) -> &str {
        &self.dashboard_id
    }

    pub fn source_fingerprint(&self) -> &str {
        &self.source_fingerprint
    }

    pub fn expected_fingerprint(&self) -> &str {
        &self.expected_fingerprint
    }

    pub fn authority_fingerprint(&self) -> &str {
        &self.authority_fingerprint
    }

    pub const fn replacement_count(&self) -> usize {
        self.replacement_count
    }

    pub fn required_contracts(&self) -> &BTreeSet<WriteContract> {
        &self.required_contracts
    }

    /// Page positions sent with full widgets.
    pub fn changed_pages(&self) -> &[usize] {
        &self.changed_pages
    }

    /// Replacement count per field-kind contract.
    pub fn replacement_kinds(&self) -> &BTreeMap<WriteContract, usize> {
        &self.kinds
    }

    /// Read-back check on the canonical fingerprint (module rules).
    fn matches_expected(&self, snapshot: &VersionedDashboardSnapshot) -> bool {
        snapshot.connection_id() == self.connection_id
            && snapshot.server_version() == &self.server_version
            && snapshot.dashboard_id() == self.dashboard_id
            && snapshot.fingerprint() == self.expected_fingerprint
    }

    /// `dashboard.update` params for `current`, a read whose canonical
    /// fingerprint equals the source fingerprint: the replacements are
    /// applied to it again (widgets located by `widgetid`), so every page,
    /// widget and field is sent in the order of this read. `None` when the
    /// result is not exactly the verified update (other contracts, pages or
    /// expected document); then nothing may be sent.
    fn params_for(&self, current: &VersionedDashboardSnapshot) -> Option<Value> {
        if current.connection_id() != self.connection_id
            || current.server_version() != &self.server_version
            || current.dashboard_id() != self.dashboard_id
            || current.fingerprint() != self.source_fingerprint
        {
            return None;
        }
        let Prepared {
            expected_raw,
            requirements,
        } = prepare(current, &self.replacements).ok()?;
        let changed_pages: Vec<usize> = requirements.changed_pages.iter().copied().collect();
        if requirements.contracts != self.required_contracts
            || changed_pages != self.changed_pages
            || requirements.kinds != self.kinds
            || !requirements.unproven_widgets.is_empty()
        {
            return None;
        }
        let expected = VersionedDashboardSnapshot::from_adapter_raw(
            self.connection_id.clone(),
            self.server_version.clone(),
            expected_raw,
        )
        .ok()?;
        if expected.fingerprint() != self.expected_fingerprint {
            return None;
        }
        encode_update(
            &self.dashboard_id,
            expected.raw_api_document(),
            &requirements.changed_pages,
        )
        .ok()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DashboardPublishOutcome {
    Conflict,
    Verified,
    FailedBeforeSend,
    FailedAfterResponse,
    UnknownOutcome,
    ReconciledVerified,
    ReconciledFailed,
}

/// The field kind of `name` on a `widget_type` widget, as the contract that
/// covers replacing it; `None` when the field is not replaceable.
fn field_kind(widget_type: &str, name: &str, field_type: Option<i64>) -> Option<WriteContract> {
    let index = |text: &str| {
        !text.is_empty() && text.len() <= 6 && text.bytes().all(|byte| byte.is_ascii_digit())
    };
    let parts = name.split('.').collect::<Vec<_>>();
    match (widget_type, parts.as_slice(), field_type) {
        ("item", ["itemid", n], Some(4)) if index(n) => Some(WriteContract::ItemWidgetItemid),
        ("svggraph", ["ds", n, "itemids", m], Some(4)) if index(n) && index(m) => {
            Some(WriteContract::SvggraphDatasetItemids)
        }
        ("svggraph", ["ds", n, "hosts", m], Some(1)) if index(n) && index(m) => {
            Some(WriteContract::SvggraphDatasetHosts)
        }
        ("graph", ["graphid", n], Some(6)) if index(n) => Some(WriteContract::ClassicGraphGraphid),
        _ => None,
    }
}

fn is_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID_DIGITS
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_host_name(value: &str) -> bool {
    let count = value.chars().count();
    (1..=MAX_HOST_PATTERN_CHARS).contains(&count)
        && !value.contains('*')
        && !value.chars().any(char::is_control)
}

/// Checks the values of one replacement for its field kind.
fn check_values(
    kind: WriteContract,
    replacement: &WidgetFieldReplacement,
) -> std::result::Result<(), WriteRejection> {
    let rejection = |code| WriteRejection::new(code).at(replacement);
    if kind == WriteContract::SvggraphDatasetHosts {
        if replacement.expected_value.contains('*') {
            return Err(
                rejection(WriteRejectionCode::FieldKindUnsupported).with("field_kind", "host_mask")
            );
        }
        if !is_host_name(&replacement.replacement_value) {
            return Err(rejection(WriteRejectionCode::ReplacementValueInvalid));
        }
    } else if !is_id(&replacement.replacement_value) {
        return Err(rejection(WriteRejectionCode::ReplacementValueInvalid));
    }
    if replacement.expected_value == replacement.replacement_value {
        return Err(rejection(WriteRejectionCode::ReplacementUnchanged));
    }
    Ok(())
}

/// Replaces `expected` by `replacement` in `value`, keeping its JSON kind.
fn replace_value(
    value: &mut Value,
    replacement: &WidgetFieldReplacement,
) -> std::result::Result<(), WriteRejection> {
    let rejection = |code| WriteRejection::new(code).at(replacement);
    let next = match &*value {
        Value::String(text) if *text == replacement.expected_value => {
            Value::String(replacement.replacement_value.clone())
        }
        Value::Number(number) if number.to_string() == replacement.expected_value => Value::Number(
            replacement
                .replacement_value
                .parse()
                .map_err(|_| rejection(WriteRejectionCode::ReplacementValueInvalid))?,
        ),
        _ => return Err(rejection(WriteRejectionCode::SourceValueStale)),
    };
    *value = next;
    Ok(())
}

struct Prepared {
    expected_raw: Value,
    requirements: WriteRequirements,
}

fn shape() -> WriteRejection {
    WriteRejection::new(WriteRejectionCode::SnapshotShapeInvalid)
}

fn apply_one(
    pages: &mut [Value],
    replacement: &WidgetFieldReplacement,
) -> std::result::Result<WriteContract, WriteRejection> {
    let widget_index = locate_widget(pages, replacement)?;
    let at = |code| WriteRejection::new(code).at_widget(replacement, widget_index);
    let widget = pages
        .get_mut(replacement.page_index)
        .and_then(|page| page.get_mut("widgets"))
        .and_then(Value::as_array_mut)
        .and_then(|widgets| widgets.get_mut(widget_index))
        .ok_or_else(shape)?;
    let widget_type = widget
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(shape)?
        .to_owned();
    let fields = widget
        .get_mut("fields")
        .and_then(Value::as_array_mut)
        .ok_or_else(shape)?;
    let mut matching = fields.iter_mut().filter(|field| {
        field.get("name").and_then(Value::as_str) == Some(replacement.field_name.as_str())
    });
    let field = matching
        .next()
        .ok_or_else(|| at(WriteRejectionCode::FieldNotFound))?;
    if matching.next().is_some() {
        return Err(at(WriteRejectionCode::FieldAmbiguous));
    }
    let kind = field_kind(
        &widget_type,
        &replacement.field_name,
        field.get("type").and_then(value_as_i64),
    )
    .ok_or_else(|| {
        at(WriteRejectionCode::FieldKindUnsupported).with("widget_type", bounded(&widget_type))
    })?;
    check_values(kind, replacement)?;
    let value = field.get_mut("value").ok_or_else(shape)?;
    replace_value(value, replacement)?;
    Ok(kind)
}

/// Position of the replacement's widget on its page in this read: the
/// `widgetid` must occur exactly once on the page and on no other page.
fn locate_widget(
    pages: &[Value],
    replacement: &WidgetFieldReplacement,
) -> std::result::Result<usize, WriteRejection> {
    let not_found = || WriteRejection::new(WriteRejectionCode::WidgetNotFound).at(replacement);
    let mismatch =
        || WriteRejection::new(WriteRejectionCode::WidgetIdentityMismatch).at(replacement);
    let positions = |page: &Value| -> Vec<usize> {
        page.get("widgets")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .enumerate()
            .filter(|(_, widget)| {
                widget.get("widgetid").and_then(Value::as_str)
                    == Some(replacement.widgetid.as_str())
            })
            .map(|(index, _)| index)
            .collect()
    };
    let page = pages.get(replacement.page_index).ok_or_else(not_found)?;
    let elsewhere = pages
        .iter()
        .enumerate()
        .any(|(index, other)| index != replacement.page_index && !positions(other).is_empty());
    match (positions(page).as_slice(), elsewhere) {
        ([index], false) => Ok(*index),
        ([], false) => Err(not_found()),
        _ => Err(mismatch()),
    }
}

/// Applies `replacements` to a copy of the read dashboard and derives what
/// the update needs. No capability is consulted here.
fn prepare(
    snapshot: &VersionedDashboardSnapshot,
    replacements: &[WidgetFieldReplacement],
) -> std::result::Result<Prepared, WriteRejection> {
    if replacements.is_empty() || replacements.len() > MAX_FIELD_REPLACEMENTS {
        return Err(
            WriteRejection::new(WriteRejectionCode::ReplacementCountInvalid)
                .with("count", replacements.len().to_string()),
        );
    }
    let mut expected_raw = snapshot.raw_api_document().clone();
    let pages = expected_raw
        .get_mut("pages")
        .and_then(Value::as_array_mut)
        .ok_or_else(shape)?;
    let mut seen = BTreeSet::new();
    let mut contracts: BTreeSet<WriteContract> = WriteContract::PAGE_UPDATE.into_iter().collect();
    let mut kinds = BTreeMap::new();
    let mut changed_pages = BTreeSet::new();
    for replacement in replacements {
        if !seen.insert((
            replacement.page_index,
            replacement.widgetid.as_str(),
            replacement.field_name.as_str(),
        )) {
            return Err(WriteRejection::new(WriteRejectionCode::FieldReplacedTwice).at(replacement));
        }
        let kind = apply_one(pages, replacement)?;
        contracts.insert(kind);
        *kinds.entry(kind).or_insert(0) += 1;
        changed_pages.insert(replacement.page_index);
    }
    if changed_pages.len() > 1 {
        contracts.insert(WriteContract::UpdateSeveralPages);
    }
    let mut unproven_widgets = Vec::new();
    for page_index in &changed_pages {
        let widgets = pages[*page_index]
            .get("widgets")
            .and_then(Value::as_array)
            .ok_or_else(shape)?;
        for (widget_index, widget) in widgets.iter().enumerate() {
            if let Some(field) = duplicate_field_name(widget)? {
                return Err(WriteRejection::new(WriteRejectionCode::FieldAmbiguous)
                    .with("page_index", page_index.to_string())
                    .with("widget_index", widget_index.to_string())
                    .with("field", bounded(field)));
            }
            let widget_type = widget
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(shape)?;
            if !RESEND_PROVEN_WIDGET_TYPES.contains(&widget_type) {
                unproven_widgets.push(UnprovenWidget {
                    page_index: *page_index,
                    widget_index,
                    widget_type: bounded(widget_type),
                });
            }
        }
    }
    Ok(Prepared {
        expected_raw,
        requirements: WriteRequirements {
            contracts,
            changed_pages,
            kinds,
            unproven_widgets,
        },
    })
}

/// The first field `name` that occurs twice in the widget's `fields`.
/// Zabbix 7.4.16 accepts and returns such widgets (LAB-02 probe: the same
/// name with the same and with another `type`), but `dashboard.update`
/// matches stored fields by (`type`, `name`), so a resend of such a widget
/// has no proven meaning and is refused.
fn duplicate_field_name(widget: &Value) -> std::result::Result<Option<&str>, WriteRejection> {
    let fields = widget
        .get("fields")
        .and_then(Value::as_array)
        .ok_or_else(shape)?;
    let mut seen = BTreeSet::new();
    for field in fields {
        let Some(name) = field.get("name").and_then(Value::as_str) else {
            continue;
        };
        if !seen.insert(name) {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

/// What `replacements` need from the server on this read dashboard: field
/// kinds, changed pages, contracts and widgets without a proven resend.
/// Pure; lets the application report every gap at once.
pub fn write_requirements(
    snapshot: &VersionedDashboardSnapshot,
    replacements: &[WidgetFieldReplacement],
) -> std::result::Result<WriteRequirements, WriteRejection> {
    prepare(snapshot, replacements).map(|prepared| prepared.requirements)
}

/// Builds the only sendable form of a dashboard update: `replacements`
/// applied to the freshly read `snapshot`, gated by every contract they
/// need, for the exact connected version and live authority of `context`.
pub fn verify_widget_field_replacements(
    snapshot: &VersionedDashboardSnapshot,
    context: &PublicationContext,
    replacements: &[WidgetFieldReplacement],
) -> std::result::Result<VerifiedDashboardUpdate, WriteRejection> {
    let capabilities = context.capabilities();
    if !context.live_authenticated {
        return Err(WriteRejection::new(WriteRejectionCode::AuthorityOffline));
    }
    if capabilities.version != *snapshot.server_version() {
        return Err(WriteRejection::new(
            WriteRejectionCode::ServerVersionMismatch,
        ));
    }
    let Prepared {
        expected_raw,
        requirements,
    } = prepare(snapshot, replacements)?;
    if let Some(contract) = requirements
        .missing_contracts(&capabilities.dashboard_write_contracts)
        .first()
    {
        return Err(WriteRejection::new(WriteRejectionCode::ContractUnverified)
            .with("contract", contract.as_str()));
    }
    if let Some(widget) = requirements.unproven_widgets.first() {
        return Err(
            WriteRejection::new(WriteRejectionCode::ChangedPageWidgetUnproven)
                .with("page_index", widget.page_index.to_string())
                .with("widget_index", widget.widget_index.to_string())
                .with("widget_type", widget.widget_type.clone()),
        );
    }
    // The payload is built again from the read right before send; encoding
    // it here proves that the expected document is encodable.
    encode_update(
        snapshot.dashboard_id(),
        &expected_raw,
        &requirements.changed_pages,
    )
    .map_err(|_| shape())?;
    let expected = VersionedDashboardSnapshot::from_adapter_raw(
        snapshot.connection_id().to_owned(),
        snapshot.server_version().clone(),
        expected_raw,
    )
    .map_err(|_| shape())?;
    Ok(VerifiedDashboardUpdate {
        connection_id: snapshot.connection_id().to_owned(),
        server_version: snapshot.server_version().clone(),
        dashboard_id: snapshot.dashboard_id().to_owned(),
        source_fingerprint: snapshot.fingerprint().to_owned(),
        expected_fingerprint: expected.fingerprint().to_owned(),
        authority_fingerprint: context.binding_fingerprint.clone(),
        replacements: replacements.to_vec(),
        replacement_count: replacements.len(),
        required_contracts: requirements.contracts,
        changed_pages: requirements.changed_pages.into_iter().collect(),
        kinds: requirements.kinds,
    })
}

impl ReadWriteGateway {
    /// Publication authority bound to the endpoint, server identity,
    /// version, credential and the current write epoch.
    pub fn publication_context(&self) -> PublicationContext {
        let session = self.session();
        let mut digest = Sha256::new();
        digest.update(b"zdashstitch-publication-authority-v2\0");
        digest.update(session.transport.endpoint_identity().as_bytes());
        digest.update(b"\0");
        digest.update(session.identity.fingerprint().as_bytes());
        digest.update(b"\0");
        digest.update(self.capabilities().version.to_string().as_bytes());
        digest.update(b"\0");
        digest.update(session.token.expose_secret().as_bytes());
        digest.update(b"\0");
        digest.update(self.grant.epoch().to_be_bytes());
        let mut binding_fingerprint = String::with_capacity(64);
        for byte in digest.finalize() {
            let _ = write!(&mut binding_fingerprint, "{byte:02x}");
        }
        PublicationContext {
            capabilities: self.capabilities().clone(),
            binding_fingerprint,
            live_authenticated: true,
        }
    }

    /// Whether the signed-in account may change the dashboard:
    /// `dashboard.get` with `editable` returns only dashboards the user has
    /// write permission to. A read; `false` also for a hidden dashboard.
    pub async fn dashboard_is_editable(&self, dashboardid: &str) -> Result<bool> {
        if !is_zabbix_id(dashboardid) {
            return Err(ZabbixError::SnapshotShape(
                "dashboardid must be a Zabbix ID".to_owned(),
            ));
        }
        let answer = self
            .reader()
            .read(
                ReadMethod::DashboardGet,
                json!({
                    "output": ["dashboardid"],
                    "dashboardids": [dashboardid],
                    "editable": true
                }),
            )
            .await?;
        Ok(answer.as_array().is_some_and(|dashboards| {
            dashboards.iter().any(|dashboard| {
                dashboard.get("dashboardid").and_then(Value::as_str) == Some(dashboardid)
            })
        }))
    }

    /// Mandatory closed publication path: fresh read, canonical fingerprint
    /// comparison, exactly one send, then read-back on the canonical
    /// fingerprint. There is no public send-only method.
    ///
    /// `before_send` runs at most once, after every pre-send check and
    /// immediately before the single write request; the caller makes the
    /// durable `submitting` journal record there (O-018 §6, P2-04D).
    /// `false` means the record could not be made: nothing is sent and the
    /// outcome is `FailedBeforeSend`.
    ///
    /// After the send, only a JSON-RPC error is a known failure
    /// (`FailedAfterResponse`). Transport loss, an HTTP status (a proxy may
    /// answer after the server committed) or an unusable answer leave the
    /// outcome unknown; reads alone decide it: `ReconciledVerified`,
    /// `ReconciledFailed`, or `UnknownOutcome` when no read succeeded.
    /// Nothing is ever resent.
    pub async fn publish_verified_dashboard_update<G>(
        &self,
        update: &VerifiedDashboardUpdate,
        before_send: G,
    ) -> DashboardPublishOutcome
    where
        G: FnOnce() -> bool + Send,
    {
        let current_context = self.publication_context();
        let contracts = &self.capabilities().dashboard_write_contracts;
        if !current_context.live_authenticated
            || current_context.binding_fingerprint != update.authority_fingerprint
            || self.capabilities().version != update.server_version
            || update.required_contracts.is_empty()
            || !update
                .required_contracts
                .iter()
                .all(|contract| contracts.is_verified(*contract))
        {
            return DashboardPublishOutcome::FailedBeforeSend;
        }
        let Ok(before) = self.dashboard_snapshots(update.connection_id()).await else {
            return DashboardPublishOutcome::FailedBeforeSend;
        };
        let Some(current) = unique_dashboard(&before, update.dashboard_id()) else {
            return DashboardPublishOutcome::Conflict;
        };
        if current.fingerprint() != update.source_fingerprint() {
            return DashboardPublishOutcome::Conflict;
        }
        let Some(update_params) = update.params_for(current) else {
            return DashboardPublishOutcome::FailedBeforeSend;
        };
        if !before_send() {
            return DashboardPublishOutcome::FailedBeforeSend;
        }

        let session = self.session();
        let dispatch = session
            .transport
            .write::<Value>(
                WriteMethod::DashboardUpdate,
                update_params,
                &session.token,
                &self.grant,
            )
            .await;
        let response_known = match dispatch {
            Ok(result)
                if result["dashboardids"]
                    .as_array()
                    .is_some_and(|ids| ids.iter().any(|id| id == update.dashboard_id())) =>
            {
                true
            }
            Ok(_) => false,
            Err(ZabbixError::Api { .. }) => {
                return DashboardPublishOutcome::FailedAfterResponse;
            }
            // Transport loss, HTTP status (a proxy may answer after the
            // server committed), oversized or malformed answers: unknown,
            // decided by the read-only reconciliation below (as for a
            // create; owner decision 28.10.2026, item 11).
            Err(_) => false,
        };

        let mut read_succeeded = false;
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            if let Ok(after) = self.dashboard_snapshots(update.connection_id()).await {
                read_succeeded = true;
                if unique_dashboard(&after, update.dashboard_id())
                    .is_some_and(|snapshot| update.matches_expected(snapshot))
                {
                    return if response_known {
                        DashboardPublishOutcome::Verified
                    } else {
                        DashboardPublishOutcome::ReconciledVerified
                    };
                }
            }
        }
        match (response_known, read_succeeded) {
            (true, _) => DashboardPublishOutcome::FailedAfterResponse,
            (false, true) => DashboardPublishOutcome::ReconciledFailed,
            (false, false) => DashboardPublishOutcome::UnknownOutcome,
        }
    }
}

fn unique_dashboard<'a>(
    snapshots: &'a [VersionedDashboardSnapshot],
    dashboard_id: &str,
) -> Option<&'a VersionedDashboardSnapshot> {
    let mut matching = snapshots
        .iter()
        .filter(|snapshot| snapshot.dashboard_id() == dashboard_id);
    let dashboard = matching.next()?;
    matching.next().is_none().then_some(dashboard)
}

/// `dashboard.update` params: every page in read order, changed pages with
/// full widgets, the others by ID only (see the module rules).
fn encode_update(
    dashboard_id: &str,
    raw: &Value,
    changed_pages: &BTreeSet<usize>,
) -> Result<Value> {
    let pages = raw["pages"]
        .as_array()
        .ok_or_else(|| reject("expected dashboard has no pages"))?;
    if changed_pages.iter().any(|index| *index >= pages.len()) {
        return Err(reject("changed page index is out of range"));
    }
    let encoded = pages
        .iter()
        .enumerate()
        .map(|(index, page)| {
            let page_id = string_property(page, "dashboard_pageid", "page")?;
            if !changed_pages.contains(&index) {
                return Ok(json!({"dashboard_pageid": page_id}));
            }
            let mut output = Map::new();
            for key in ["dashboard_pageid", "name", "display_period"] {
                output.insert(
                    key.to_owned(),
                    page.get(key)
                        .cloned()
                        .ok_or_else(|| reject("changed page is incomplete"))?,
                );
            }
            let widgets = page["widgets"]
                .as_array()
                .ok_or_else(|| reject("changed page has no widgets"))?
                .iter()
                .map(encode_widget)
                .collect::<Result<Vec<_>>>()?;
            output.insert("widgets".to_owned(), Value::Array(widgets));
            Ok(Value::Object(output))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"dashboardid": dashboard_id, "pages": encoded}))
}

fn encode_widget(widget: &Value) -> Result<Value> {
    let mut output = Map::new();
    for key in [
        "widgetid",
        "type",
        "name",
        "x",
        "y",
        "width",
        "height",
        "view_mode",
        "fields",
    ] {
        output.insert(
            key.to_owned(),
            widget
                .get(key)
                .cloned()
                .ok_or_else(|| reject("widget is incomplete"))?,
        );
    }
    Ok(Value::Object(output))
}

fn string_property<'a>(value: &'a Value, key: &str, context: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| reject(&format!("{context} has no string {key}")))
}

fn value_as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn bounded(text: &str) -> String {
    text.chars().take(MAX_PARAM_CHARS).collect()
}

fn reject(message: &str) -> ZabbixError {
    ZabbixError::WriteContract(message.to_owned())
}

#[cfg(test)]
mod tests;
