// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{Result, WidgetCapabilityRegistry, ZabbixError, ZabbixVersion};

const VERIFIED_DASHBOARD_WRITE_PATCH_MIN: u32 = 16;
const VERIFIED_DASHBOARD_WRITE_PATCH_MAX: u32 = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityState {
    Supported,
    Experimental,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZabbixCapabilities {
    pub version: ZabbixVersion,
    pub read_inventory: CapabilityState,
    pub dashboard_write: CapabilityState,
    pub dashboard_write_contracts: DashboardWriteContracts,
    /// `dashboard.create` contracts (P2-06A), separate from the update
    /// contracts above.
    pub dashboard_create_contracts: DashboardCreateContracts,
}

/// Laboratory contracts of the closed `dashboard.create` path (P2-06,
/// O-019). Each must be `Supported` for the exact connected version before a
/// create is sent. LAB-02 (28.10.2026, 7.4.16) proved
/// `create_private_dashboard`, `copy_read_widgets` and
/// `reconcile_create_outcome`, so [`CreateContract::COPY`] is met there for a
/// copy of widget types the registry knows; `copy_unknown_widgets` is not
/// proven. No user-facing route sends a create.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DashboardCreateContracts {
    /// `dashboard.create` without `userid` makes a private dashboard owned by
    /// the caller with empty sharing; a taken name, also of a dashboard the
    /// caller cannot see, is refused with `already exists`.
    pub create_private_dashboard: CapabilityState,
    /// Every page, geometry, widget field and in-dashboard `reference` of a
    /// read dashboard is created as sent and read back identically, for
    /// widget types known to the adapter registry.
    pub copy_read_widgets: CapabilityState,
    /// The same for widget types or field types the registry does not know,
    /// within the same adapter and exact version (ADR-011).
    pub copy_unknown_widgets: CapabilityState,
    /// After an unknown outcome the created dashboard is found by exact name
    /// and content fingerprint without a second create.
    pub reconcile_create_outcome: CapabilityState,
}

/// One laboratory contract of `dashboard.create`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CreateContract {
    CreatePrivateDashboard,
    CopyReadWidgets,
    CopyUnknownWidgets,
    ReconcileCreateOutcome,
}

impl CreateContract {
    /// Contracts every create of a copied dashboard relies on.
    pub const COPY: [Self; 3] = [
        Self::CreatePrivateDashboard,
        Self::CopyReadWidgets,
        Self::ReconcileCreateOutcome,
    ];

    /// Stable code, used as the `contract` parameter of
    /// `write_contract_unverified`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CreatePrivateDashboard => "create_private_dashboard",
            Self::CopyReadWidgets => "create_copy_read_widgets",
            Self::CopyUnknownWidgets => "create_copy_unknown_widgets",
            Self::ReconcileCreateOutcome => "create_reconcile_unknown_outcome",
        }
    }
}

impl DashboardCreateContracts {
    /// Every create contract `Unsupported`: the state of every version
    /// outside the laboratory-proven patch range.
    #[must_use]
    pub const fn unproven() -> Self {
        Self {
            create_private_dashboard: CapabilityState::Unsupported,
            copy_read_widgets: CapabilityState::Unsupported,
            copy_unknown_widgets: CapabilityState::Unsupported,
            reconcile_create_outcome: CapabilityState::Unsupported,
        }
    }

    #[must_use]
    pub const fn state(&self, contract: CreateContract) -> CapabilityState {
        match contract {
            CreateContract::CreatePrivateDashboard => self.create_private_dashboard,
            CreateContract::CopyReadWidgets => self.copy_read_widgets,
            CreateContract::CopyUnknownWidgets => self.copy_unknown_widgets,
            CreateContract::ReconcileCreateOutcome => self.reconcile_create_outcome,
        }
    }

    #[must_use]
    pub fn is_verified(&self, contract: CreateContract) -> bool {
        self.state(contract) == CapabilityState::Supported
    }
}

/// Individually proven laboratory write contracts. The application must
/// require every contract used by a mutation; the server version alone is not
/// write authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DashboardWriteContracts {
    pub update_one_page_with_full_fields: CapabilityState,
    pub preserve_untouched_pages: CapabilityState,
    pub item_widget_itemid: CapabilityState,
    pub preserve_widget_references: CapabilityState,
    pub reconcile_unknown_outcome: CapabilityState,
    /// One `dashboard.update` that sends several pages with full widgets
    /// (P2-04B; LAB-02, 28.10.2026).
    pub update_several_pages: CapabilityState,
    /// `svggraph` explicit items `ds.N.itemids.M` (P2-04A; LAB-02).
    pub svggraph_dataset_itemids: CapabilityState,
    /// `svggraph` host patterns `ds.N.hosts.M` that are an exact visible
    /// host name without `*` (P2-04A; LAB-02). Masks stay refused by the
    /// adapter (`field_kind_unsupported`).
    pub svggraph_dataset_hosts: CapabilityState,
    /// Classic `graph` widget `graphid.N` (P2-04H, O-003; LAB-02).
    pub classic_graph_graphid: CapabilityState,
}

/// One laboratory write contract. A mutation lists the contracts it relies
/// on; publication requires each of them to be `Supported` for the exact
/// connected version (plan time and again before send).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum WriteContract {
    UpdateOnePageWithFullFields,
    PreserveUntouchedPages,
    PreserveWidgetReferences,
    ReconcileUnknownOutcome,
    ItemWidgetItemid,
    SvggraphDatasetItemids,
    SvggraphDatasetHosts,
    ClassicGraphGraphid,
    UpdateSeveralPages,
}

impl WriteContract {
    /// Contracts every `dashboard.update` of a page relies on.
    pub const PAGE_UPDATE: [Self; 4] = [
        Self::UpdateOnePageWithFullFields,
        Self::PreserveUntouchedPages,
        Self::PreserveWidgetReferences,
        Self::ReconcileUnknownOutcome,
    ];

    /// Stable code, used as the `contract` parameter of
    /// `write_contract_unverified`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UpdateOnePageWithFullFields => "update_one_page_with_full_fields",
            Self::PreserveUntouchedPages => "preserve_untouched_pages",
            Self::PreserveWidgetReferences => "preserve_widget_references",
            Self::ReconcileUnknownOutcome => "reconcile_unknown_outcome",
            Self::ItemWidgetItemid => "item_widget_itemid",
            Self::SvggraphDatasetItemids => "svggraph_dataset_itemids",
            Self::SvggraphDatasetHosts => "svggraph_dataset_hosts",
            Self::ClassicGraphGraphid => "classic_graph_graphid",
            Self::UpdateSeveralPages => "update_several_pages",
        }
    }
}

impl DashboardWriteContracts {
    #[must_use]
    pub const fn state(&self, contract: WriteContract) -> CapabilityState {
        match contract {
            WriteContract::UpdateOnePageWithFullFields => self.update_one_page_with_full_fields,
            WriteContract::PreserveUntouchedPages => self.preserve_untouched_pages,
            WriteContract::PreserveWidgetReferences => self.preserve_widget_references,
            WriteContract::ReconcileUnknownOutcome => self.reconcile_unknown_outcome,
            WriteContract::ItemWidgetItemid => self.item_widget_itemid,
            WriteContract::SvggraphDatasetItemids => self.svggraph_dataset_itemids,
            WriteContract::SvggraphDatasetHosts => self.svggraph_dataset_hosts,
            WriteContract::ClassicGraphGraphid => self.classic_graph_graphid,
            WriteContract::UpdateSeveralPages => self.update_several_pages,
        }
    }

    #[must_use]
    pub fn is_verified(&self, contract: WriteContract) -> bool {
        self.state(contract) == CapabilityState::Supported
    }

    /// The P0-07 operation: `item` widget `itemid` on one page.
    pub fn item_replacement_is_verified(&self) -> bool {
        WriteContract::PAGE_UPDATE
            .into_iter()
            .chain([WriteContract::ItemWidgetItemid])
            .all(|contract| self.is_verified(contract))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AdapterRegistry;

impl AdapterRegistry {
    pub fn resolve(version: ZabbixVersion) -> Result<ZabbixCapabilities> {
        if !version.is_supported_7_4() {
            return Err(ZabbixError::UnsupportedVersion(version.to_string()));
        }
        let laboratory_contract = if (VERIFIED_DASHBOARD_WRITE_PATCH_MIN
            ..=VERIFIED_DASHBOARD_WRITE_PATCH_MAX)
            .contains(&version.patch)
        {
            CapabilityState::Supported
        } else {
            CapabilityState::Unsupported
        };
        Ok(ZabbixCapabilities {
            version,
            read_inventory: CapabilityState::Supported,
            dashboard_write: CapabilityState::Unsupported,
            dashboard_write_contracts: DashboardWriteContracts {
                update_one_page_with_full_fields: laboratory_contract,
                preserve_untouched_pages: laboratory_contract,
                item_widget_itemid: laboratory_contract,
                preserve_widget_references: laboratory_contract,
                reconcile_unknown_outcome: laboratory_contract,
                // LAB-02 (28.10.2026, 7.4.16, `lab_write_contracts_p2`):
                // read-back verified on the canonical fingerprint.
                update_several_pages: laboratory_contract,
                svggraph_dataset_itemids: laboratory_contract,
                svggraph_dataset_hosts: laboratory_contract,
                classic_graph_graphid: laboratory_contract,
            },
            dashboard_create_contracts: DashboardCreateContracts {
                // LAB-02: private by default, owner is the caller, empty
                // sharing, `already exists` also for an invisible name; a
                // read page of registered widget types is created as sent.
                create_private_dashboard: laboratory_contract,
                copy_read_widgets: laboratory_contract,
                // The server accepts any widget type, even one no module
                // provides, so it proves nothing about unknown widgets:
                // the adapter rules of ADR-011 decide, and they are not
                // recorded as a contract.
                copy_unknown_widgets: CapabilityState::Unsupported,
                // LAB-02 (`create_unknown_outcome_is_reconciled_by_reads_only`):
                // after one `dashboard.create` with the answer dropped, the
                // read-only reconciliation finds exactly one confirmed
                // candidate (also from a new session and by ID), and nothing
                // when the request was not sent.
                reconcile_create_outcome: laboratory_contract,
            },
        })
    }

    pub fn widget_capabilities(version: &ZabbixVersion) -> Result<WidgetCapabilityRegistry> {
        if !version.is_supported_7_4() {
            return Err(ZabbixError::UnsupportedVersion(version.to_string()));
        }
        Ok(WidgetCapabilityRegistry::v7_4())
    }
}
