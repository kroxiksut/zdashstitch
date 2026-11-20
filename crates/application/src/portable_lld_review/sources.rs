// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::BTreeMap;

use zdashstitch_core::formats::{
    Cardinality, HostSelection, ItemExpansion, Role, Selection, Widget, ZabbixWidgetFieldValue,
};

use crate::workcopy::WorkcopyError;

#[derive(Clone, Copy)]
pub(super) struct MetricSource<'a> {
    pub catalog: &'a str,
    pub metric: &'a str,
    pub lld: LldSource,
    pub source_key: Option<&'a str>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum LldSource {
    Ordinary,
    Instance,
    Catalog,
}

pub(super) struct Source<'a> {
    pub field_index: usize,
    pub field_name: Option<&'a str>,
    pub role: &'a str,
    pub slot: Option<&'a str>,
    pub metric: Option<MetricSource<'a>>,
    pub typed: bool,
    pub color: Option<&'a str>,
}

fn invalid() -> WorkcopyError {
    WorkcopyError::new("portable_lld_review_invalid")
}

fn selected_slots<'a>(
    roles: &'a BTreeMap<String, Role>,
    role: &str,
    selection: Selection,
    slot: Option<&'a str>,
    multiple: bool,
) -> Result<Vec<Option<&'a str>>, WorkcopyError> {
    let role = roles.get(role).ok_or_else(invalid)?;
    match selection {
        Selection::Slot
            if role.slots.as_ref().is_some_and(|slots| {
                slot.is_some_and(|id| slots.iter().any(|slot| slot == id))
            }) =>
        {
            Ok(vec![slot])
        }
        Selection::One | Selection::All if role.cardinality == Cardinality::One => Ok(vec![
            role.slots
                .as_ref()
                .and_then(|slots| slots.first())
                .map(String::as_str),
        ]),
        Selection::All if multiple => {
            let slots = role
                .slots
                .as_ref()
                .filter(|slots| !slots.is_empty())
                .ok_or_else(invalid)?;
            Ok(slots.iter().map(|slot| Some(slot.as_str())).collect())
        }
        _ => Err(invalid()),
    }
}

pub(super) fn widget_sources<'a>(
    roles: &'a BTreeMap<String, Role>,
    widget: &'a Widget,
) -> Result<Vec<Source<'a>>, WorkcopyError> {
    let mut sources = Vec::new();
    let mut add_typed = |catalog: &'a str,
                         metric: &'a str,
                         hosts: &'a HostSelection,
                         multiple,
                         color: Option<&'a str>| {
        let slots = selected_slots(
            roles,
            &hosts.role,
            hosts.selection,
            hosts.slot_id.as_deref(),
            multiple,
        )?;
        for slot in slots {
            if sources.len() >= super::MAX_BINDINGS {
                return Err(WorkcopyError::new("storage_limit_reached"));
            }
            sources.push(Source {
                field_index: sources.len(),
                field_name: None,
                role: &hosts.role,
                slot,
                metric: Some(MetricSource {
                    catalog,
                    metric,
                    lld: LldSource::Catalog,
                    source_key: None,
                }),
                typed: true,
                color,
            });
        }
        Ok::<(), WorkcopyError>(())
    };
    match widget {
        Widget::Graph { series, .. } => {
            for series in series {
                add_typed(
                    &series.metric.catalog,
                    &series.metric.metric,
                    &series.hosts,
                    true,
                    series.color.as_deref(),
                )?;
            }
        }
        Widget::ItemValue {
            metric,
            hosts,
            expansion,
            ..
        } => {
            add_typed(
                &metric.catalog,
                &metric.metric,
                hosts,
                *expansion == ItemExpansion::WidgetPerHost,
                None,
            )?;
        }
        Widget::Zabbix { fields, .. } => {
            for (field_index, field) in fields.iter().enumerate() {
                let (role, selection, slot, metric) = match &field.value {
                    ZabbixWidgetFieldValue::Metric {
                        catalog,
                        metric,
                        hosts,
                    } => (
                        hosts.role.as_str(),
                        hosts.selection,
                        hosts.slot_id.as_deref(),
                        Some(MetricSource {
                            catalog,
                            metric,
                            lld: LldSource::Ordinary,
                            source_key: None,
                        }),
                    ),
                    ZabbixWidgetFieldValue::LldInstance {
                        catalog,
                        metric,
                        hosts,
                        source_key,
                    } => (
                        hosts.role.as_str(),
                        hosts.selection,
                        hosts.slot_id.as_deref(),
                        Some(MetricSource {
                            catalog,
                            metric,
                            lld: LldSource::Instance,
                            source_key: Some(source_key),
                        }),
                    ),
                    ZabbixWidgetFieldValue::Host {
                        role,
                        selection,
                        slot_id,
                    } => (role.as_str(), *selection, slot_id.as_deref(), None),
                    _ => continue,
                };
                let slot = selected_slots(roles, role, selection, slot, false)?[0];
                sources.push(Source {
                    field_index,
                    field_name: Some(&field.name),
                    role,
                    slot,
                    metric,
                    typed: false,
                    color: None,
                });
            }
        }
    }
    Ok(sources)
}
