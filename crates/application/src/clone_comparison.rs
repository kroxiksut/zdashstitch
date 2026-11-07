// SPDX-License-Identifier: MIT OR Apache-2.0

//! Read-only comparison of a pinned Clone and the copy identified by its journal.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Value, json};
use zdashstitch_zabbix::{
    AdapterRegistry, CreateContract, ReadBackVerdict, ReadOnlyGateway, VersionedDashboardSnapshot,
    create_requirements, dashboard_comparison_content, is_zabbix_id,
};

use crate::clone_dashboard::{CloneReadBackVerdict, SavedCloneWorkcopy};
use crate::clone_preflight::kept_sources;
use crate::create_dashboard::expectation_of;
use crate::publish_journal::{JournalEntry, JournalState, JournalTarget, OperationKind};
use crate::workcopy::WorkcopyError;

#[derive(Serialize)]
pub struct CloneComparison {
    operation_id: Option<String>,
    observation: Observation,
    verdict: Option<&'static str>,
    access: [Option<Access>; 3],
    rows: Vec<Row>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Observation {
    Pending,
    Unavailable,
    VerifiedReadBack,
    ChangedSinceReadBack,
    Unverified,
}

#[derive(Serialize)]
struct Access {
    private: Option<bool>,
    shared: Option<bool>,
    owner_matches: Option<bool>,
}

#[derive(Serialize)]
struct Row {
    kind: &'static str,
    page: Option<usize>,
    at: Option<[String; 2]>,
    field: Option<String>,
    change: &'static str,
    unresolved: bool,
    kept: bool,
    observed_differs: bool,
    source: Option<Cell>,
    expected: Option<Cell>,
    observed: Option<Cell>,
}

#[derive(Serialize)]
struct Cell {
    label: Option<String>,
    widget_type: Option<String>,
    geometry: Option<[String; 4]>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Position {
    page: usize,
    x: String,
    y: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Key {
    Dashboard,
    Page(usize),
    Widget(Position, usize),
    Field(Position, usize, String, String, usize),
}

struct Object {
    cell: Cell,
    content: Value,
}

fn raw_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn array(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

fn position(page: usize, widget: &Value) -> Position {
    Position {
        page,
        x: raw_text(&widget["x"]).unwrap_or_default(),
        y: raw_text(&widget["y"]).unwrap_or_default(),
    }
}

fn object(label: Option<String>, content: Value) -> Object {
    Object {
        cell: Cell {
            label,
            widget_type: None,
            geometry: None,
        },
        content,
    }
}

fn binding_value(widget_type: &str, kind: &str, field: &str, value: &str) -> Option<String> {
    let pattern = widget_type == "svggraph"
        && kind == "1"
        && (field.starts_with("problemhosts.")
            || (field.starts_with("ds.") || field.starts_with("or."))
                && (field.contains(".hosts.") || field.contains(".items.")));
    let reference = kind == "1" && (field == "reference" || field.ends_with("._reference"));
    let object_id = matches!(kind, "3" | "4" | "5" | "6" | "7" | "8") && is_zabbix_id(value);
    (pattern || reference || object_id).then(|| value.chars().take(255).collect())
}

fn objects(document: &Value) -> BTreeMap<Key, Object> {
    let mut result = BTreeMap::from([(
        Key::Dashboard,
        object(
            raw_text(&document["name"]),
            json!([
                document["name"],
                document["display_period"],
                document["auto_start"]
            ]),
        ),
    )]);
    for (page_index, page) in array(&document["pages"]).iter().enumerate() {
        result.insert(
            Key::Page(page_index),
            object(
                raw_text(&page["name"]),
                json!([page["name"], page["display_period"]]),
            ),
        );
        let mut duplicates = BTreeMap::new();
        for widget in array(&page["widgets"]) {
            let at = position(page_index, widget);
            let occurrence = duplicates.entry(at.clone()).or_insert(0);
            let index = *occurrence;
            *occurrence += 1;
            let kind = widget["type"].as_str().unwrap_or_default();
            result.insert(
                Key::Widget(at.clone(), index),
                Object {
                    cell: Cell {
                        label: raw_text(&widget["name"]),
                        widget_type: Some(kind.to_owned()),
                        geometry: Some(
                            ["x", "y", "width", "height"]
                                .map(|key| raw_text(&widget[key]).unwrap_or_default()),
                        ),
                    },
                    content: widget.clone(),
                },
            );
            let mut occurrences = BTreeMap::new();
            for field in array(&widget["fields"]) {
                let (Some(kind_code), Some(name), Some(value)) =
                    (field[0].as_str(), field[1].as_str(), field[2].as_str())
                else {
                    continue;
                };
                let occurrence = occurrences.entry((name, kind_code)).or_insert(0);
                result.insert(
                    Key::Field(
                        at.clone(),
                        index,
                        name.to_owned(),
                        kind_code.to_owned(),
                        *occurrence,
                    ),
                    object(binding_value(kind, kind_code, name, value), field.clone()),
                );
                *occurrence += 1;
            }
        }
    }
    result
}

fn position_at(document: &Value, page: usize, widget: usize) -> Option<Position> {
    array(&array(&document["pages"]).get(page)?["widgets"])
        .get(widget)
        .map(|value| position(page, value))
}

fn access(snapshot: &VersionedDashboardSnapshot, owner: Option<&str>) -> Access {
    let raw = snapshot.raw_api_document();
    Access {
        private: raw_text(&raw["private"]).map(|value| value == "1"),
        shared: raw["users"]
            .as_array()
            .zip(raw["userGroups"].as_array())
            .map(|(users, groups)| !users.is_empty() || !groups.is_empty()),
        owner_matches: raw_text(&raw["userid"])
            .as_deref()
            .zip(owner)
            .map(|(actual, expected)| actual == expected),
    }
}

fn matches_entry(branch: &SavedCloneWorkcopy, entry: &JournalEntry) -> bool {
    let intent = branch.draft().intent();
    let source = intent.source();
    entry.kind == OperationKind::Create
        && entry.intent == "clone_dashboard"
        && entry.source.as_ref().is_some_and(|binding| {
            binding.local_id == branch.local_id() && binding.revision == intent.mapping_revision()
        })
        && entry.source_fingerprint == source.local_fingerprint
        && entry.connection_id == source.connection_id
        && entry.server_identity == intent.server_identity_fingerprint()
        && entry.server_version == intent.server_version()
        && matches!(&entry.target, JournalTarget::NewDashboard { name } if name == intent.target_name())
}

async fn observe(
    branch: &SavedCloneWorkcopy,
    entry: Option<&JournalEntry>,
    reader: Option<&ReadOnlyGateway>,
) -> (
    Observation,
    Option<&'static str>,
    Option<VersionedDashboardSnapshot>,
) {
    let Some(entry) = entry.filter(|entry| entry.sends == 1) else {
        return (Observation::Pending, None, None);
    };
    let intent = branch.draft().intent();
    let verified = matches!(
        entry.state,
        JournalState::Verified | JournalState::ReconciledVerified
    );
    let id = if verified {
        &entry.created_dashboardid
    } else {
        &entry.reported_dashboardid
    };
    let Some((id, reader)) = id.as_deref().zip(reader).filter(|(id, reader)| {
        is_zabbix_id(id)
            && *id != intent.source().dashboardid
            && reader.identity().fingerprint() == intent.server_identity_fingerprint()
            && reader.capabilities().version == *branch.local().server_version()
    }) else {
        return (Observation::Unavailable, None, None);
    };
    let Ok(expectation) = expectation_of(entry) else {
        return (Observation::Unavailable, None, None);
    };
    let Ok(Some(snapshot)) = reader.dashboard_snapshot(&entry.connection_id, id).await else {
        return (Observation::Unavailable, None, None);
    };
    if snapshot.dashboard_id() != id || dashboard_comparison_content(&snapshot).is_none() {
        return (Observation::Unavailable, None, None);
    }
    let CloneReadBackVerdict::Copy(verdict) = intent.compare_read_back(&expectation, &snapshot)
    else {
        return (Observation::Unavailable, None, None);
    };
    let state = if !verified {
        Observation::Unverified
    } else if entry.observed_fingerprint.as_deref() == Some(snapshot.fingerprint())
        && verdict == ReadBackVerdict::Matches
    {
        Observation::VerifiedReadBack
    } else {
        Observation::ChangedSinceReadBack
    };
    (state, Some(verdict.as_str()), Some(snapshot))
}

impl SavedCloneWorkcopy {
    /// Returns display-only data. The current copy is read only when its
    /// journal entry and the authenticated connection both match this branch.
    pub async fn comparison(
        &self,
        entries: &[JournalEntry],
        reader: Option<&ReadOnlyGateway>,
    ) -> Result<CloneComparison, WorkcopyError> {
        let invalid = || WorkcopyError::new("clone_comparison_shape");
        let base = self.draft().base();
        let local = self.local();
        let intent = self.draft().intent();
        let entry = entries
            .iter()
            .filter(|entry| matches_entry(self, entry))
            .max_by_key(|entry| entry.confirmed_at_ms);
        let mut expected_doc = dashboard_comparison_content(local).ok_or_else(invalid)?;
        expected_doc["name"] = json!(intent.target_name());
        let mut source = objects(&dashboard_comparison_content(base).ok_or_else(invalid)?);
        let mut expected = objects(&expected_doc);
        let (observation, verdict, snapshot) = observe(self, entry, reader).await;
        let mut observed = snapshot
            .as_ref()
            .and_then(dashboard_comparison_content)
            .map_or_else(BTreeMap::new, |document| objects(&document));
        let owner = entry.and_then(|entry| entry.owner_userid.as_deref());
        let kept: BTreeSet<_> = kept_sources(base, self)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|kept| {
                let address = kept.field;
                let at = position_at(
                    base.raw_api_document(),
                    address.widget.page_index,
                    address.widget.widget_index,
                )?;
                Some((at, address.field_name))
            })
            .collect();
        let requirements = create_requirements(local, intent.target_name())
            .map_err(|error| WorkcopyError::new(error.code().as_str()))?;
        let capabilities =
            AdapterRegistry::resolve(local.server_version().clone()).map_err(|_| invalid())?;
        let unknown: BTreeSet<_> = if requirements
            .missing_contracts(&capabilities.dashboard_create_contracts)
            .contains(&CreateContract::CopyUnknownWidgets)
        {
            requirements
                .unknown_widgets
                .iter()
                .filter_map(|widget| {
                    position_at(
                        local.raw_api_document(),
                        widget.page_index,
                        widget.widget_index,
                    )
                })
                .collect()
        } else {
            BTreeSet::new()
        };
        let keys: BTreeSet<_> = source
            .keys()
            .chain(expected.keys())
            .chain(observed.keys())
            .cloned()
            .collect();
        let mut rows = Vec::new();
        for key in keys {
            let (before, after, remote) = (
                source.remove(&key),
                expected.remove(&key),
                observed.remove(&key),
            );
            let (kind, page, at, field, kept, unresolved) = match key {
                Key::Dashboard => ("dashboard", None, None, None, false, false),
                Key::Page(page) => ("page", Some(page + 1), None, None, false, false),
                Key::Widget(at, _) => {
                    let unresolved = unknown.contains(&at);
                    (
                        "widget",
                        Some(at.page + 1),
                        Some([at.x, at.y]),
                        None,
                        false,
                        unresolved,
                    )
                }
                Key::Field(at, _, name, _, _) => {
                    let kept = kept.contains(&(at.clone(), name.clone()));
                    (
                        "field",
                        Some(at.page + 1),
                        Some([at.x, at.y]),
                        Some(name),
                        kept,
                        false,
                    )
                }
            };
            let changed = before.as_ref().map(|object| &object.content)
                != after.as_ref().map(|object| &object.content);
            let observed_differs = snapshot.is_some()
                && after.as_ref().map(|object| &object.content)
                    != remote.as_ref().map(|object| &object.content);
            if kind == "field" && !changed && !kept && !observed_differs {
                continue;
            }
            let change = if before.is_none() && after.is_some() {
                "added"
            } else if changed {
                "changed"
            } else {
                "untouched"
            };
            rows.push(Row {
                kind,
                page,
                at,
                field,
                change,
                unresolved: unresolved || observed_differs,
                kept,
                observed_differs,
                source: before.map(|object| object.cell),
                expected: after.map(|object| object.cell),
                observed: remote.map(|object| object.cell),
            });
        }
        Ok(CloneComparison {
            operation_id: entry.map(|entry| entry.operation_id.clone()),
            observation,
            verdict,
            access: [
                Some(access(base, None)),
                Some(Access {
                    private: Some(true),
                    shared: Some(false),
                    owner_matches: Some(true),
                }),
                snapshot.as_ref().map(|snapshot| access(snapshot, owner)),
            ],
            rows,
        })
    }
}
