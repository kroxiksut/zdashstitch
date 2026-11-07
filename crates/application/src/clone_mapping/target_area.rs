// SPDX-License-Identifier: MIT OR Apache-2.0

//! P3-02B target-area discovery and frozen host selection.
//!
//! Group, tag and name filters are read-only discovery aids. The result that
//! proceeds to mapping is always an explicit ordered host list and retains no
//! filter, so later group membership changes cannot add a host automatically.

use std::collections::BTreeSet;
use std::fmt;
use std::future::Future;

use zdashstitch_zabbix::{
    HostGroupListing, HostListing, HostObjects, HostSearchFilter, ReadOnlyGateway, ZabbixError,
    is_zabbix_id,
};

/// A visible target host with source-template coverage for the picker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetAreaCandidate {
    pub host: HostListing,
    pub source_templates_found: u32,
}

/// Read-only target search. Empty results preserve the P1-IN-08 ambiguity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetAreaSearch {
    pub hosts: Vec<TargetAreaCandidate>,
    pub truncated: bool,
    pub empty_reason: Option<&'static str>,
}

/// Only the read gateway implements this in production; fixtures implement
/// it in tests. No write gateway can enter target discovery.
pub trait TargetAreaLookup {
    fn search_host_groups(
        &self,
        search: &str,
        limit: usize,
    ) -> impl Future<Output = Result<HostObjects<HostGroupListing>, ZabbixError>> + Send;

    fn search_hosts_filtered(
        &self,
        filter: &HostSearchFilter,
        limit: usize,
    ) -> impl Future<Output = Result<HostObjects<HostListing>, ZabbixError>> + Send;
}

impl TargetAreaLookup for ReadOnlyGateway {
    fn search_host_groups(
        &self,
        search: &str,
        limit: usize,
    ) -> impl Future<Output = Result<HostObjects<HostGroupListing>, ZabbixError>> + Send {
        ReadOnlyGateway::search_host_groups(self, search, limit)
    }

    fn search_hosts_filtered(
        &self,
        filter: &HostSearchFilter,
        limit: usize,
    ) -> impl Future<Output = Result<HostObjects<HostListing>, ZabbixError>> + Send {
        ReadOnlyGateway::search_hosts_filtered(self, filter, limit)
    }
}

/// Search visible groups through `hostgroup.get` only.
pub async fn search_target_groups<L: TargetAreaLookup>(
    lookup: &L,
    search: &str,
    limit: usize,
) -> Result<HostObjects<HostGroupListing>, ZabbixError> {
    lookup.search_host_groups(search, limit).await
}

/// Search visible hosts through `host.get` only and calculate how many
/// templates of the source set are linked to each candidate.
pub async fn search_target_hosts<L: TargetAreaLookup>(
    lookup: &L,
    filter: &HostSearchFilter,
    source_templateids: &BTreeSet<String>,
    limit: usize,
) -> Result<TargetAreaSearch, ZabbixError> {
    let found = lookup.search_hosts_filtered(filter, limit).await?;
    let hosts = found
        .objects
        .into_iter()
        .map(|host| {
            let source_templates_found = u32::try_from(
                host.templates
                    .iter()
                    .filter(|template| source_templateids.contains(&template.templateid))
                    .count(),
            )
            .unwrap_or(u32::MAX);
            TargetAreaCandidate {
                host,
                source_templates_found,
            }
        })
        .collect::<Vec<_>>();
    let empty_reason = hosts.is_empty().then_some("not_found_or_no_access");
    Ok(TargetAreaSearch {
        hosts,
        truncated: found.truncated,
        empty_reason,
    })
}

/// Explicit ordered selection stored for mapping. Search filters are
/// deliberately absent: this is the complete frozen list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrozenTargetSelection {
    hosts: Vec<HostListing>,
}

impl FrozenTargetSelection {
    /// Errors: `target_hosts_empty`, `target_hostid_invalid`,
    /// `target_host_duplicate`.
    pub fn new(hosts: Vec<HostListing>) -> Result<Self, TargetSelectionError> {
        if hosts.is_empty() {
            return Err(TargetSelectionError::new("target_hosts_empty"));
        }
        let mut ids = BTreeSet::new();
        for host in &hosts {
            if !is_zabbix_id(&host.hostid) {
                return Err(TargetSelectionError::new("target_hostid_invalid"));
            }
            if !ids.insert(host.hostid.as_str()) {
                return Err(TargetSelectionError::new("target_host_duplicate"));
            }
        }
        Ok(Self { hosts })
    }

    #[must_use]
    pub fn hosts(&self) -> &[HostListing] {
        &self.hosts
    }
}

/// Stable data-only error of explicit target selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetSelectionError {
    code: &'static str,
}

impl TargetSelectionError {
    const fn new(code: &'static str) -> Self {
        Self { code }
    }

    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }
}

impl fmt::Display for TargetSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code)
    }
}

impl std::error::Error for TargetSelectionError {}
