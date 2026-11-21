// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{Result, WidgetCapabilityRegistry, ZabbixError, ZabbixVersion};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zdashstitch_core::{Version, VersionRange};

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
    pub item_background_color: CapabilityState,
    pub item_background_reset: CapabilityState,
    pub svggraph_series_color: CapabilityState,
    pub append_blueprint_item: CapabilityState,
    pub append_blueprint_graph: CapabilityState,
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
    ItemBackgroundColor,
    ItemBackgroundReset,
    SvggraphSeriesColor,
    AppendBlueprintItem,
    AppendBlueprintGraph,
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
            Self::ItemBackgroundColor => "item_background_color",
            Self::ItemBackgroundReset => "item_background_reset",
            Self::SvggraphSeriesColor => "svggraph_series_color",
            Self::AppendBlueprintItem => "append_blueprint_item",
            Self::AppendBlueprintGraph => "append_blueprint_graph",
        }
    }
}

impl DashboardWriteContracts {
    fn for_eight_zero(version: &ZabbixVersion) -> Self {
        if !is_eight_zero_item_update_version(version) {
            return Self::unproven();
        }
        // Z80-P5-03 and Z80-P2-03. These contracts apply only to the shapes
        // and live evidence accepted by the scoped 8.0.0 codec.
        Self {
            update_one_page_with_full_fields: CapabilityState::Supported,
            preserve_untouched_pages: CapabilityState::Supported,
            preserve_widget_references: CapabilityState::Supported,
            item_widget_itemid: CapabilityState::Supported,
            svggraph_dataset_itemids: CapabilityState::Supported,
            svggraph_dataset_hosts: CapabilityState::Supported,
            update_several_pages: CapabilityState::Supported,
            reconcile_unknown_outcome: CapabilityState::Supported,
            item_background_color: CapabilityState::Supported,
            item_background_reset: CapabilityState::Supported,
            svggraph_series_color: CapabilityState::Supported,
            append_blueprint_item: CapabilityState::Supported,
            append_blueprint_graph: CapabilityState::Supported,
            ..Self::unproven()
        }
    }

    #[must_use]
    pub const fn unproven() -> Self {
        Self {
            update_one_page_with_full_fields: CapabilityState::Unsupported,
            preserve_untouched_pages: CapabilityState::Unsupported,
            item_widget_itemid: CapabilityState::Unsupported,
            preserve_widget_references: CapabilityState::Unsupported,
            reconcile_unknown_outcome: CapabilityState::Unsupported,
            update_several_pages: CapabilityState::Unsupported,
            svggraph_dataset_itemids: CapabilityState::Unsupported,
            svggraph_dataset_hosts: CapabilityState::Unsupported,
            classic_graph_graphid: CapabilityState::Unsupported,
            item_background_color: CapabilityState::Unsupported,
            item_background_reset: CapabilityState::Unsupported,
            svggraph_series_color: CapabilityState::Unsupported,
            append_blueprint_item: CapabilityState::Unsupported,
            append_blueprint_graph: CapabilityState::Unsupported,
        }
    }

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
            WriteContract::ItemBackgroundColor => self.item_background_color,
            WriteContract::ItemBackgroundReset => self.item_background_reset,
            WriteContract::SvggraphSeriesColor => self.svggraph_series_color,
            WriteContract::AppendBlueprintItem => self.append_blueprint_item,
            WriteContract::AppendBlueprintGraph => self.append_blueprint_graph,
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

pub(crate) const fn is_eight_zero_item_update_version(version: &ZabbixVersion) -> bool {
    version.major == 8 && version.minor == 0 && version.patch == 0
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AdapterRegistry;

/// Version identity selected from `apiinfo.version` or an explicit offline
/// version. Selecting a profile does not authorize server reads or writes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AdapterProfile {
    #[serde(rename = "7.4")]
    V7_4,
    #[serde(rename = "8.0")]
    V8_0,
}

impl AdapterProfile {
    #[must_use]
    pub const fn adapter_id(self) -> &'static str {
        match self {
            Self::V7_4 => "zabbix-7.4",
            Self::V8_0 => "zabbix-8.0",
        }
    }

    #[must_use]
    pub const fn family(self) -> &'static str {
        match self {
            Self::V7_4 => "7.4",
            Self::V8_0 => "8.0",
        }
    }

    /// Range described by the local widget registry. It does not authorize
    /// any operation on a server in this range.
    #[must_use]
    pub const fn widget_version_range(self) -> VersionRange {
        match self {
            Self::V7_4 => VersionRange {
                minimum: Version::new(7, 4, 0),
                maximum_exclusive: None,
            },
            Self::V8_0 => VersionRange {
                minimum: Version::new(8, 0, 0),
                maximum_exclusive: Some(Version::new(8, 1, 0)),
            },
        }
    }

    #[must_use]
    pub fn widget_capabilities(self) -> WidgetCapabilityRegistry {
        match self {
            Self::V7_4 => WidgetCapabilityRegistry::v7_4(),
            Self::V8_0 => WidgetCapabilityRegistry::v8_0(),
        }
    }

    pub(crate) fn item_value_type(self, value: Option<&Value>, object: &str) -> Result<u8> {
        let maximum = match self {
            Self::V7_4 => 5,
            Self::V8_0 => 6,
        };
        value
            .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
            .and_then(|number| u8::try_from(number).ok())
            .filter(|number| *number <= maximum)
            .ok_or_else(|| ZabbixError::SnapshotShape(format!("{object} has no valid value_type")))
    }
}

impl AdapterRegistry {
    /// Structural selection only. Selecting a profile grants no operations.
    pub fn resolve_profile(version: &ZabbixVersion) -> Result<AdapterProfile> {
        match (version.major, version.minor) {
            (7, 4) => Ok(AdapterProfile::V7_4),
            (8, 0) => Ok(AdapterProfile::V8_0),
            _ => Err(ZabbixError::UnsupportedVersion(version.to_string())),
        }
    }

    /// Server-access selection. The 8.0.0 ReplaceHost update has scoped
    /// contracts and never enables the general write capability.
    pub fn resolve_server(version: ZabbixVersion) -> Result<ZabbixCapabilities> {
        match Self::resolve_profile(&version)? {
            AdapterProfile::V7_4 => Self::resolve(version),
            AdapterProfile::V8_0 => Ok(ZabbixCapabilities {
                dashboard_write_contracts: DashboardWriteContracts::for_eight_zero(&version),
                version,
                read_inventory: CapabilityState::Experimental,
                dashboard_write: CapabilityState::Unsupported,
                dashboard_create_contracts: DashboardCreateContracts::unproven(),
            }),
        }
    }

    pub fn resolve(version: ZabbixVersion) -> Result<ZabbixCapabilities> {
        if Self::resolve_profile(&version)? != AdapterProfile::V7_4 {
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
                ..DashboardWriteContracts::unproven()
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
        Ok(Self::resolve_profile(version)?.widget_capabilities())
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;

    fn version(raw: &str) -> ZabbixVersion {
        raw.parse()
            .unwrap_or_else(|error| panic!("test version {raw}: {error}"))
    }

    #[test]
    fn profiles_are_exact_and_never_fall_back() {
        assert_eq!(
            AdapterRegistry::resolve_profile(&version("7.4.16"))
                .unwrap_or_else(|error| panic!("test adapter: {error}")),
            AdapterProfile::V7_4
        );
        assert_eq!(
            AdapterRegistry::resolve_profile(&version("8.0.0"))
                .unwrap_or_else(|error| panic!("test adapter: {error}")),
            AdapterProfile::V8_0
        );
        assert_eq!(AdapterProfile::V8_0.adapter_id(), "zabbix-8.0");
        for raw in ["7.0.19", "7.5.0", "8.1.0", "9.0.0"] {
            assert!(matches!(
                AdapterRegistry::resolve_profile(&version(raw)),
                Err(ZabbixError::UnsupportedVersion(_))
            ));
        }
        for raw in ["8.0.0alpha1", "8.0.0rc1", "8.0.0-rc1"] {
            assert!(matches!(
                raw.parse::<ZabbixVersion>(),
                Err(ZabbixError::InvalidVersion(_))
            ));
        }
    }

    #[test]
    fn eight_zero_profile_keeps_general_writes_closed_with_scoped_contracts() {
        let exact = version("8.0.0");
        let registry = AdapterRegistry::widget_capabilities(&exact)
            .unwrap_or_else(|error| panic!("test adapter: {error}"));
        assert_eq!(registry.adapter(), "8.0");
        assert!(registry.find("item").is_some());
        assert!(registry.capabilities().iter().all(|kind| {
            kind.flags.read && !kind.flags.edit && !kind.flags.create && !kind.flags.preview
        }));
        let capabilities = AdapterRegistry::resolve_server(exact.clone())
            .unwrap_or_else(|error| panic!("test adapter: {error}"));
        assert_eq!(capabilities.read_inventory, CapabilityState::Experimental);
        assert_eq!(capabilities.dashboard_write, CapabilityState::Unsupported);
        assert!(
            capabilities
                .dashboard_write_contracts
                .is_verified(WriteContract::ItemWidgetItemid)
        );
        assert!(
            capabilities
                .dashboard_write_contracts
                .item_replacement_is_verified()
        );
        assert!(
            capabilities
                .dashboard_write_contracts
                .is_verified(WriteContract::PreserveWidgetReferences)
        );
        assert!(
            capabilities
                .dashboard_write_contracts
                .is_verified(WriteContract::SvggraphDatasetItemids)
        );
        assert!(
            capabilities
                .dashboard_write_contracts
                .is_verified(WriteContract::UpdateSeveralPages)
        );
        assert!(
            !capabilities
                .dashboard_write_contracts
                .is_verified(WriteContract::ClassicGraphGraphid)
        );
        assert!(
            capabilities
                .dashboard_write_contracts
                .is_verified(WriteContract::SvggraphDatasetHosts)
        );
        assert_eq!(
            AdapterRegistry::resolve_server(version("8.0.1"))
                .unwrap_or_else(|error| panic!("neighboring patch: {error}"))
                .dashboard_write_contracts,
            DashboardWriteContracts::unproven()
        );
        assert!(
            !capabilities
                .dashboard_create_contracts
                .is_verified(CreateContract::CreatePrivateDashboard)
        );
        assert!(matches!(
            AdapterRegistry::resolve(exact),
            Err(ZabbixError::UnsupportedVersion(_))
        ));
    }

    #[test]
    fn seven_four_contracts_remain_separate() {
        let capabilities = AdapterRegistry::resolve_server(version("7.4.16"))
            .unwrap_or_else(|error| panic!("test adapter: {error}"));
        assert_eq!(capabilities.read_inventory, CapabilityState::Supported);
        assert!(
            capabilities
                .dashboard_write_contracts
                .is_verified(WriteContract::ItemWidgetItemid)
        );
        assert!(
            capabilities
                .dashboard_create_contracts
                .is_verified(CreateContract::CreatePrivateDashboard)
        );
        assert_eq!(
            AdapterRegistry::widget_capabilities(&version("7.4.16"))
                .unwrap_or_else(|error| panic!("test adapter: {error}"))
                .capabilities()
                .len(),
            32
        );
        assert!(
            !AdapterRegistry::resolve_server(version("7.4.17"))
                .unwrap_or_else(|error| panic!("test adapter: {error}"))
                .dashboard_write_contracts
                .is_verified(WriteContract::ItemWidgetItemid)
        );
    }
}
