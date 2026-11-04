// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-02E: links between widgets (`reference` definitions and
//! `<reference>._<channel>` consumers) and orphaned dependencies.
//!
//! Zabbix does not check `reference` uniqueness (P1-IN-04), so a value
//! defined by several widgets is reported as a duplicate, not resolved to
//! one of them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::model::{SourceKind, WidgetAddress, WidgetSources};
use super::resolve::ResolveScope;

/// Provider value for the dashboard-level selector (`DASHBOARD._hostid`).
pub const DASHBOARD_REFERENCE: &str = "DASHBOARD";

/// A widget that other widgets can listen to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceDefinition {
    pub widget: WidgetAddress,
    pub reference: String,
}

/// Who provides the data a consumer field listens to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinkProvider {
    /// Exactly one widget defines the reference.
    Widget(WidgetAddress),
    /// The dashboard itself (`DASHBOARD._hostid`, `DASHBOARD._timeperiod`).
    Dashboard,
    /// No widget defines the reference: an orphaned dependency.
    Missing,
    /// Several widgets define the same reference.
    Duplicate(Vec<WidgetAddress>),
}

/// One consumer field and its provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WidgetLink {
    pub consumer: WidgetAddress,
    pub field_name: String,
    pub reference: String,
    /// `hostid`, `itemid`, `timeperiod`, … (empty when not stated).
    pub channel: String,
    pub provider: LinkProvider,
}

impl WidgetLink {
    /// No widget (and not the dashboard) provides the data.
    #[must_use]
    pub const fn is_orphaned(&self) -> bool {
        matches!(self.provider, LinkProvider::Missing)
    }
}

/// Which end of a link lies outside an operation scope.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CrossingSide {
    /// The consumer is in scope but its provider is not: within the scope
    /// the consumer becomes orphaned (P1-05B).
    ProviderOutside,
    /// The provider is in scope but a consumer is not: the consumer outside
    /// keeps listening to a widget the operation changes.
    ConsumerOutside,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeCrossing {
    pub link: WidgetLink,
    pub side: CrossingSide,
}

/// All widget links of a dashboard.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WidgetLinks {
    pub definitions: Vec<ReferenceDefinition>,
    pub links: Vec<WidgetLink>,
}

impl WidgetLinks {
    pub(crate) fn from_widgets(widgets: &[WidgetSources]) -> Self {
        let mut definitions = Vec::new();
        let mut by_reference: BTreeMap<&str, Vec<WidgetAddress>> = BTreeMap::new();
        for widget in widgets {
            for source in &widget.sources {
                if let SourceKind::ReferenceDefinition { reference } = &source.kind {
                    definitions.push(ReferenceDefinition {
                        widget: widget.widget.clone(),
                        reference: reference.clone(),
                    });
                    by_reference
                        .entry(reference.as_str())
                        .or_default()
                        .push(widget.widget.clone());
                }
            }
        }
        let mut links = Vec::new();
        for widget in widgets {
            for source in &widget.sources {
                let SourceKind::WidgetReference { reference, channel } = &source.kind else {
                    continue;
                };
                let provider = if reference == DASHBOARD_REFERENCE {
                    LinkProvider::Dashboard
                } else {
                    match by_reference.get(reference.as_str()).map(Vec::as_slice) {
                        None | Some([]) => LinkProvider::Missing,
                        Some([single]) => LinkProvider::Widget(single.clone()),
                        Some(several) => LinkProvider::Duplicate(several.to_vec()),
                    }
                };
                links.push(WidgetLink {
                    consumer: widget.widget.clone(),
                    field_name: source.field.field_name.clone(),
                    reference: reference.clone(),
                    channel: channel.clone(),
                    provider,
                });
            }
        }
        Self { definitions, links }
    }

    /// Consumers whose reference no widget defines.
    pub fn orphaned(&self) -> impl Iterator<Item = &WidgetLink> {
        self.links.iter().filter(|link| link.is_orphaned())
    }

    /// References defined by more than one widget.
    #[must_use]
    pub fn duplicate_definitions(&self) -> BTreeMap<String, Vec<WidgetAddress>> {
        let mut by_reference: BTreeMap<String, Vec<WidgetAddress>> = BTreeMap::new();
        for definition in &self.definitions {
            by_reference
                .entry(definition.reference.clone())
                .or_default()
                .push(definition.widget.clone());
        }
        by_reference.retain(|_, widgets| widgets.len() > 1);
        by_reference
    }

    /// Links that cross the boundary of `scope`. A link provided by the
    /// dashboard or by nobody never crosses.
    #[must_use]
    pub fn crossing(&self, scope: &ResolveScope) -> Vec<ScopeCrossing> {
        let mut crossings = Vec::new();
        for link in &self.links {
            let providers: &[WidgetAddress] = match &link.provider {
                LinkProvider::Widget(provider) => std::slice::from_ref(provider),
                LinkProvider::Duplicate(providers) => providers,
                LinkProvider::Dashboard | LinkProvider::Missing => continue,
            };
            let consumer_in = scope.contains(&link.consumer);
            let any_provider_in = providers.iter().any(|provider| scope.contains(provider));
            let all_providers_in = providers.iter().all(|provider| scope.contains(provider));
            let side = if consumer_in && !all_providers_in {
                Some(CrossingSide::ProviderOutside)
            } else if !consumer_in && any_provider_in {
                Some(CrossingSide::ConsumerOutside)
            } else {
                None
            };
            if let Some(side) = side {
                crossings.push(ScopeCrossing {
                    link: link.clone(),
                    side,
                });
            }
        }
        crossings
    }
}
