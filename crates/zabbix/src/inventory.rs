// SPDX-License-Identifier: MIT OR Apache-2.0

use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InventoryMethod {
    Template,
    Host,
    Item,
    Dashboard,
    Graph,
}

impl InventoryMethod {
    pub const fn api_method(self) -> &'static str {
        match self {
            Self::Template => "template.get",
            Self::Host => "host.get",
            Self::Item => "item.get",
            Self::Dashboard => "dashboard.get",
            Self::Graph => "graph.get",
        }
    }

    pub(crate) const fn read_method(self) -> crate::ReadMethod {
        match self {
            Self::Template => crate::ReadMethod::TemplateGet,
            Self::Host => crate::ReadMethod::HostGet,
            Self::Item => crate::ReadMethod::ItemGet,
            Self::Dashboard => crate::ReadMethod::DashboardGet,
            Self::Graph => crate::ReadMethod::GraphGet,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MethodOutcome {
    Available(Value),
    PermissionDenied {
        method: &'static str,
        diagnostic: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct InventoryProbe {
    pub template: MethodOutcome,
    pub host: MethodOutcome,
    pub item: MethodOutcome,
    pub dashboard: MethodOutcome,
    pub graph: MethodOutcome,
}
