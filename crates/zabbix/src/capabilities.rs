// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{Result, ZabbixError, ZabbixVersion};

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
}

impl DashboardWriteContracts {
    pub fn item_replacement_is_verified(&self) -> bool {
        [
            self.update_one_page_with_full_fields,
            self.preserve_untouched_pages,
            self.item_widget_itemid,
            self.preserve_widget_references,
            self.reconcile_unknown_outcome,
        ]
        .into_iter()
        .all(|state| state == CapabilityState::Supported)
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
            },
        })
    }
}
