// SPDX-License-Identifier: MIT OR Apache-2.0

//! Copies of server dashboards (P2-05): server dashboard list, read-only
//! import into an immutable snapshot plus a workcopy, and the local draft
//! read model.
//!
//! Every server read goes through a [`zdashstitch_zabbix::ReadOnlyGateway`];
//! the service has no write path and does not accept a write gateway:
//!
//! ```compile_fail
//! async fn no_write_gateway(
//!     service: &zdashstitch_application::workcopy::WorkcopyService,
//!     writer: &zdashstitch_zabbix::ReadWriteGateway,
//!     request: &zdashstitch_application::workcopy::ImportRequest,
//! ) {
//!     let _ = service.import(writer, "conn-0", request).await;
//! }
//! ```
//!
//! Control: the same call compiles with the read-only gateway, so the
//! failure above comes from the gateway type.
//!
//! ```no_run
//! async fn read_gateway(
//!     service: &zdashstitch_application::workcopy::WorkcopyService,
//!     reader: &zdashstitch_zabbix::ReadOnlyGateway,
//!     request: &zdashstitch_application::workcopy::ImportRequest,
//! ) {
//!     let _ = service.import(reader, "conn-0", request).await;
//! }
//! ```
//!
//! The API returns stable codes, safe parameters and data; never prose.
//! Times are Unix epoch milliseconds (UTC); the HTTP layer renders them as
//! RFC 3339 UTC (O-035).

mod advance;
pub(crate) mod formats;
pub(crate) mod operations;
mod service;
pub(crate) mod store;
mod view;

use std::fmt;

use zdashstitch_zabbix::{FailureParam, ZabbixError};

pub use advance::{AdvancedBase, PublishedUpdate};
pub use formats::{
    DraftKind, ItemMetadataState, LocalMetadata, StoredItemMetadata, StoredValueMap,
    StoredValueMapping,
};
pub(crate) use service::now_ms;
pub use service::{
    AccessCounts, AccessFilter, DashboardAccess, DashboardOwner, ExistingCopy, ImportOutcome,
    ImportPreview, ImportRequest, ImportScope, MAX_DRAFT_NAME_CHARS, MAX_IMPORT_ATTEMPTS,
    MAX_ITEM_METADATA_ITEMS, MAX_LIST_PAGE, PreviewDependencies, PreviewPage, PreviewWidget,
    ServerDashboardList, ServerDashboardQuery, ServerDashboardRow, WorkcopyService,
};
pub use view::{
    CreatedDashboard, DamagedDraft, DashboardProperties, DraftDetail, DraftFreshness,
    DraftFreshnessState, DraftList, DraftLocalState, DraftOrigin, DraftPage, DraftSource,
    DraftSummary, DraftWidget, DraftWidgetCounts,
};

/// Failure of a workcopy use case: a stable code plus safe parameters.
/// `Display` shows only the code. Parameters never contain names, URLs,
/// hostnames, raw server data or secrets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkcopyError {
    code: &'static str,
    params: Vec<FailureParam>,
}

impl WorkcopyError {
    pub(crate) fn new(code: &'static str) -> Self {
        Self {
            code,
            params: Vec::new(),
        }
    }

    pub(crate) fn with(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.params.push(FailureParam {
            name,
            value: value.into(),
        });
        self
    }

    /// Resolver failures keep their code and parameters.
    pub(crate) fn from_resolver(error: &crate::resolver::ResolverError) -> Self {
        Self {
            code: error.code(),
            params: error.params().to_vec(),
        }
    }

    /// Zabbix failures keep the connection taxonomy code (`session_expired`,
    /// `api_method_denied`, `host_unreachable_or_timeout`, …).
    pub(crate) fn from_zabbix(error: &ZabbixError) -> Self {
        let failure = error.failure();
        Self {
            code: failure.code.as_str(),
            params: failure.params,
        }
    }

    #[must_use]
    pub const fn code(&self) -> &'static str {
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

impl fmt::Display for WorkcopyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "workcopy operation failed: {}", self.code)
    }
}

impl std::error::Error for WorkcopyError {}

fn is_prefixed_hex(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|hex| {
        hex.len() == 32
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// Connection profile ID (`conn-` + 32 lowercase hex), a path component.
pub(crate) fn is_connection_id(value: &str) -> bool {
    is_prefixed_hex(value, "conn-")
}

/// Workcopy ID (`wc-` + 32 lowercase hex), the draft `local_id`.
#[must_use]
pub fn is_workcopy_id(value: &str) -> bool {
    is_prefixed_hex(value, "wc-")
}

/// Snapshot fingerprint (canonical or storage): 64 lowercase hex characters.
pub(crate) fn is_fingerprint(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests;
