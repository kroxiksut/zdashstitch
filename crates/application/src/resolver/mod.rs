// SPDX-License-Identifier: MIT OR Apache-2.0

//! Source resolver (P2-02): what a dashboard's widgets refer to and how each
//! reference of one host resolves on another host, with evidence.
//!
//! Three steps, the first and last pure:
//!
//! 1. [`source_model`] — widget → sources (items, graphs, hosts, `svggraph`
//!    patterns, widget links) with their address in Base (P2-02A/E);
//! 2. [`collect_source_facts`] / [`collect_target_facts`] — read-only
//!    lookups through [`ResolverLookup`] (implemented for
//!    [`zdashstitch_zabbix::ReadOnlyGateway`] only); [`host_inventory`]
//!    lists the hosts the dashboard references (P1-05A);
//! 3. [`resolve_replace_host`] — rows with independent match and
//!    applicability statuses (P1-IN-03), reasons and evidence, the
//!    `kept_on_source` list of O-003, unverifiable sources and scope
//!    crossings (P2-02B/C/D/F/G).
//!
//! Nothing here writes to Zabbix or to the local store. Results carry codes
//! and server data, never prose.

mod links;
mod lookup;
mod model;
mod pattern;
mod resolve;
mod status;

use std::fmt;

use zdashstitch_zabbix::{FailureParam, ZabbixError};

pub use links::{
    CrossingSide, DASHBOARD_REFERENCE, LinkProvider, ReferenceDefinition, ScopeCrossing,
    WidgetLink, WidgetLinks,
};
pub use lookup::{
    HostInventory, ReferencedHost, ResolverLookup, SourceFacts, TargetFacts, UnattributedSource,
    collect_source_facts, collect_target_facts, host_inventory,
};
pub use model::{
    FieldAddress, PatternPlace, Source, SourceKind, SourceModel, SvgDataSet, WidgetAddress,
    WidgetSources, WidgetSupport, source_model,
};
pub use pattern::{is_mask, name_pattern_matches, same_visible_name};
pub use resolve::{
    Evidence, GraphEvidence, ItemEvidence, ItemOrigin, KeptOnSource, MAX_LLD_CANDIDATES,
    MAX_PATTERN_CANDIDATES, PatternCoverage, ReplaceHostRequest, ReplaceHostResolution,
    ResolutionRow, ResolutionSummary, ResolveScope, SourceValue, TargetValue, UnverifiedSource,
    resolve_replace_host,
};
pub use status::{Applicability, Difference, MatchStatus, ReasonCode};

/// Reads the facts and resolves ReplaceHost in one call: source facts,
/// target facts, then [`resolve_replace_host`].
pub async fn resolve_replace_host_with<L: ResolverLookup>(
    lookup: &L,
    model: &SourceModel,
    request: &ReplaceHostRequest,
) -> Result<ReplaceHostResolution, ResolverError> {
    let source = collect_source_facts(lookup, model).await?;
    let target = collect_target_facts(
        lookup,
        model,
        &request.source_hostid,
        &request.target_hostid,
    )
    .await?;
    resolve_replace_host(model, request, &source, &target)
}

/// Failure of a resolver step: a stable code plus safe parameters. `Display`
/// shows only the code. Parameters never contain names, URLs, hostnames,
/// raw server data or secrets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolverError {
    code: &'static str,
    params: Vec<FailureParam>,
}

impl ResolverError {
    pub(crate) const fn new(code: &'static str) -> Self {
        Self {
            code,
            params: Vec::new(),
        }
    }

    /// Zabbix failures keep the connection taxonomy code (`session_expired`,
    /// `api_method_denied`, …); a malformed answer is `snapshot_shape`.
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
}

impl fmt::Display for ResolverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "source resolution failed: {}", self.code)
    }
}

impl std::error::Error for ResolverError {}

#[cfg(test)]
pub(crate) mod tests;
