// SPDX-License-Identifier: MIT OR Apache-2.0

//! P2-03B: structural diff «база → черновик» of a `replace_host` operation
//! as data — per page, widget, `svggraph` series and field: changed,
//! untouched, unresolved or kept on the source host.

use serde_json::Value;

use super::state::{ChangeState, RowView};
use crate::resolver::{ResolveScope, SourceModel};

/// Field counts by effect.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChangeCounts {
    pub changed: u32,
    pub unresolved: u32,
    pub kept_on_source: u32,
}

impl ChangeCounts {
    fn add(&mut self, state: ChangeState) {
        let counter = match state {
            ChangeState::Changed => &mut self.changed,
            ChangeState::Unresolved => &mut self.unresolved,
            ChangeState::KeptOnSource => &mut self.kept_on_source,
            ChangeState::Untouched => return,
        };
        *counter = counter.saturating_add(1);
    }

    fn merge(&mut self, other: Self) {
        self.changed = self.changed.saturating_add(other.changed);
        self.unresolved = self.unresolved.saturating_add(other.unresolved);
        self.kept_on_source = self.kept_on_source.saturating_add(other.kept_on_source);
    }

    /// Aggregate state: unresolved wins over changed, changed over kept.
    #[must_use]
    pub const fn state(&self) -> ChangeState {
        if self.unresolved > 0 {
            ChangeState::Unresolved
        } else if self.changed > 0 {
            ChangeState::Changed
        } else if self.kept_on_source > 0 {
            ChangeState::KeptOnSource
        } else {
            ChangeState::Untouched
        }
    }
}

/// Counts of objects (widgets, series, pages) by aggregate state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StateCounts {
    pub total: u32,
    pub changed: u32,
    pub untouched: u32,
    pub unresolved: u32,
    pub kept_on_source: u32,
}

impl StateCounts {
    fn add(&mut self, state: ChangeState) {
        self.total = self.total.saturating_add(1);
        let counter = match state {
            ChangeState::Changed => &mut self.changed,
            ChangeState::Untouched => &mut self.untouched,
            ChangeState::Unresolved => &mut self.unresolved,
            ChangeState::KeptOnSource => &mut self.kept_on_source,
        };
        *counter = counter.saturating_add(1);
    }

    fn merge(&mut self, other: Self) {
        self.total = self.total.saturating_add(other.total);
        self.changed = self.changed.saturating_add(other.changed);
        self.untouched = self.untouched.saturating_add(other.untouched);
        self.unresolved = self.unresolved.saturating_add(other.unresolved);
        self.kept_on_source = self.kept_on_source.saturating_add(other.kept_on_source);
    }
}

/// One `svggraph` data set (`ds.<n>`): a series group of the graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SeriesDiff {
    /// `<n>` of `ds.<n>.*`.
    pub dataset_index: u32,
    pub state: ChangeState,
    pub fields: ChangeCounts,
    /// Names of the fields of this data set that have rows.
    pub field_names: Vec<String>,
    /// Base values (server data).
    pub host_patterns: Vec<String>,
    pub item_patterns: Vec<String>,
    pub itemids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WidgetDiff {
    pub widget_index: usize,
    pub widgetid: Option<String>,
    pub widget_type: String,
    /// Server data; may be empty.
    pub name: String,
    pub in_scope: bool,
    pub state: ChangeState,
    pub fields: ChangeCounts,
    /// References of the source host in this widget; empty when untouched.
    pub rows: Vec<RowView>,
    /// `svggraph` data sets in index order; empty for other types.
    pub series: Vec<SeriesDiff>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageDiff {
    pub page_index: usize,
    pub dashboard_pageid: Option<String>,
    /// Server data.
    pub name: String,
    pub state: ChangeState,
    pub widgets: Vec<WidgetDiff>,
    /// Widgets of the page by state («{changed} из {total} виджетов изменены»).
    pub widget_counts: StateCounts,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiffSummary {
    pub pages: StateCounts,
    pub widgets: StateCounts,
    pub series: StateCounts,
    pub fields: ChangeCounts,
}

/// The diff of the draft scope: every page and widget of the draft, with
/// rows only where the source host is referenced.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructuralDiff {
    pub pages: Vec<PageDiff>,
    pub summary: DiffSummary,
}

fn dataset_of(field: &str) -> Option<u32> {
    let mut parts = field.split('.');
    if parts.next() != Some("ds") {
        return None;
    }
    let index = parts.next()?;
    if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    index.parse().ok()
}

/// Builds the diff. `draft_page` limits it to one page of Base (page
/// drafts); `rows` come from [`super::state`].
pub(crate) fn structural_diff(
    model: &SourceModel,
    raw: &Value,
    rows: &[RowView],
    scope: &ResolveScope,
    draft_page: Option<usize>,
) -> StructuralDiff {
    let raw_pages = raw
        .get("pages")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut pages = Vec::new();
    let mut summary = DiffSummary::default();
    for (page_index, raw_page) in raw_pages.iter().enumerate() {
        if draft_page.is_some_and(|selected| selected != page_index) {
            continue;
        }
        let mut widgets = Vec::new();
        let mut widget_counts = StateCounts::default();
        for widget in model
            .widgets
            .iter()
            .filter(|widget| widget.widget.page_index == page_index)
        {
            let widget_rows: Vec<RowView> = rows
                .iter()
                .filter(|row| {
                    row.key.page_index == page_index
                        && row.key.widget_index == widget.widget.widget_index
                })
                .cloned()
                .collect();
            let mut fields = ChangeCounts::default();
            for row in &widget_rows {
                fields.add(row.state);
            }
            let series: Vec<SeriesDiff> = widget
                .datasets
                .iter()
                .map(|dataset| {
                    let mut series_fields = ChangeCounts::default();
                    let mut field_names = Vec::new();
                    for row in widget_rows
                        .iter()
                        .filter(|row| dataset_of(&row.key.field) == Some(dataset.index))
                    {
                        series_fields.add(row.state);
                        field_names.push(row.key.field.clone());
                    }
                    SeriesDiff {
                        dataset_index: dataset.index,
                        state: series_fields.state(),
                        fields: series_fields,
                        field_names,
                        host_patterns: dataset.host_patterns.clone(),
                        item_patterns: dataset.item_patterns.clone(),
                        itemids: dataset.itemids.clone(),
                    }
                })
                .collect();
            for item in &series {
                summary.series.add(item.state);
            }
            let state = fields.state();
            widget_counts.add(state);
            summary.fields.merge(fields);
            widgets.push(WidgetDiff {
                widget_index: widget.widget.widget_index,
                widgetid: widget.widget.widgetid.clone(),
                widget_type: widget.widget_type.clone(),
                name: widget.name.clone(),
                in_scope: scope.contains(&widget.widget),
                state,
                fields,
                rows: widget_rows,
                series,
            });
        }
        let state = if widget_counts.unresolved > 0 {
            ChangeState::Unresolved
        } else if widget_counts.changed > 0 {
            ChangeState::Changed
        } else if widget_counts.kept_on_source > 0 {
            ChangeState::KeptOnSource
        } else {
            ChangeState::Untouched
        };
        summary.pages.add(state);
        summary.widgets.merge(widget_counts);
        pages.push(PageDiff {
            page_index,
            dashboard_pageid: raw_page
                .get("dashboard_pageid")
                .and_then(Value::as_str)
                .map(str::to_owned),
            name: raw_page
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            state,
            widgets,
            widget_counts,
        });
    }
    StructuralDiff { pages, summary }
}
