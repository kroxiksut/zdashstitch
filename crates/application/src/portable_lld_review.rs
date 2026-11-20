// SPDX-License-Identifier: MIT OR Apache-2.0
//! Read-only target review. The result never grants publication authority.
mod graph_8_0;
mod sources;

use crate::workcopy::store::{LocalStore, read_bounded, write_atomic};
use crate::workcopy::{WorkcopyError, WorkcopyService};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};
use zdashstitch_core::formats::{
    Cardinality, MetricOrigin, MetricSelector, Page, ProjectRoot, RegistryState,
    TypedPortableBundle, ValueType, Widget, ZabbixWidgetFieldValue,
};
use zdashstitch_zabbix::{
    HostDetails, ItemFlags, ReadOnlyGateway, WidgetCapabilityRegistry, WidgetFieldType,
};

use self::sources::{LldSource, widget_sources};

const PATH: &[&str] = &["bindings"];
const FILE: &str = "portable-lld-v1.json";
const MAX_BYTES: usize = 1024 * 1024;
const MAX_BINDINGS: usize = 2048;
static BINDINGS_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct StoredBinding {
    local_id: String,
    server: String,
    revision: u64,
    draft_fingerprint: String,
    page_index: usize,
    widget_index: usize,
    field_index: usize,
    hostid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    itemid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    review_fingerprint: Option<String>,
}
#[derive(Default, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct BindingFile {
    format: String,
    bindings: Vec<StoredBinding>,
}

fn read_bindings(store: &LocalStore) -> Result<BindingFile, WorkcopyError> {
    let Some(dir) = store.path(PATH, false)? else {
        return Ok(BindingFile {
            format: "portable-lld-bindings/v1".into(),
            bindings: Vec::new(),
        });
    };
    let Some(raw) = read_bounded(&dir, FILE, MAX_BYTES)? else {
        return Ok(BindingFile {
            format: "portable-lld-bindings/v1".into(),
            bindings: Vec::new(),
        });
    };
    let file: BindingFile = serde_json::from_slice(&raw)
        .map_err(|_| WorkcopyError::new("portable_lld_bindings_corrupt"))?;
    if file.format != "portable-lld-bindings/v1"
        || file.bindings.len() > MAX_BINDINGS
        || file.bindings.iter().any(|b| {
            b.server.len() != 64
                || !b.server.bytes().all(|c| c.is_ascii_hexdigit())
                || b.draft_fingerprint.len() != 64
                || !b.draft_fingerprint.bytes().all(|c| c.is_ascii_hexdigit())
                || b.review_fingerprint.as_ref().is_some_and(|fingerprint| {
                    fingerprint.len() != 64 || !fingerprint.bytes().all(|c| c.is_ascii_hexdigit())
                })
                || !zdashstitch_zabbix::is_zabbix_id(&b.hostid)
                || b.itemid
                    .as_deref()
                    .is_some_and(|id| !zdashstitch_zabbix::is_zabbix_id(id))
        })
    {
        return Err(WorkcopyError::new("portable_lld_bindings_corrupt"));
    }
    Ok(file)
}

fn save_binding(store: &LocalStore, binding: StoredBinding) -> Result<(), WorkcopyError> {
    let _guard = BINDINGS_LOCK
        .lock()
        .map_err(|_| WorkcopyError::new("storage_write_failed"))?;
    let mut file = read_bindings(store)?;
    file.bindings.retain(|old| {
        !(old.local_id == binding.local_id
            && old.server == binding.server
            && old.page_index == binding.page_index
            && old.widget_index == binding.widget_index
            && old.field_index == binding.field_index)
    });
    if file.bindings.len() >= MAX_BINDINGS {
        return Err(WorkcopyError::new("storage_limit_reached"));
    }
    file.bindings.push(binding);
    let raw = serde_json::to_vec(&file)
        .map_err(|_| WorkcopyError::new("portable_lld_bindings_corrupt"))?;
    if raw.len() > MAX_BYTES {
        return Err(WorkcopyError::new("storage_limit_reached"));
    }
    let dir = store
        .path(PATH, true)?
        .ok_or_else(|| WorkcopyError::new("storage_unavailable"))?;
    write_atomic(&dir, FILE, &raw)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableLldReviewRequest {
    pub revision: u64,
    pub page_index: usize,
    pub widget_index: usize,
    pub field_index: usize,
    pub hostid: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableLldConfirmRequest {
    pub review: PortableLldReviewRequest,
    #[serde(default)]
    pub itemid: Option<String>,
    pub fingerprint: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableLldReopenRequest {
    pub revision: u64,
    pub page_index: usize,
    pub widget_index: usize,
    pub field_index: usize,
}
#[derive(serde::Serialize)]
pub struct PortableLldReopened {
    pub review: PortableLldReview,
    pub hostid: String,
    pub itemid: Option<String>,
}
#[derive(Clone, serde::Serialize)]
pub struct PortableLldCandidate {
    pub itemid: String,
    pub name: String,
    pub key: String,
}
#[derive(serde::Serialize)]
pub struct PortableLldReview {
    pub candidates: Vec<PortableLldCandidate>,
    pub fingerprint: String,
    pub draft_fingerprint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<HostDetails>,
    #[serde(skip)]
    require_saved_fingerprint: bool,
}

impl PortableLldReview {
    fn accepts(&self, itemid: Option<&str>) -> bool {
        match itemid {
            Some(id) => {
                self.host.is_none()
                    && self
                        .candidates
                        .iter()
                        .any(|candidate| candidate.itemid == id)
            }
            None => self.host.is_some(),
        }
    }
}

#[derive(serde::Serialize)]
pub struct PortableSourceEntry {
    page_index: usize,
    widget_index: usize,
    field_index: usize,
    page_name: String,
    widget_name: String,
    field_name: Option<String>,
    role: String,
    slot: Option<String>,
    metric_name: Option<String>,
    source_key: Option<String>,
}

/// Non-deserializable evidence for all LLD fields in one saved dashboard.
/// This is not a publication plan and never grants write authority.
///
/// ```compile_fail
/// let _proof = zdashstitch_application::portable_lld_review::VerifiedPortableLldEvidence {};
/// ```
///
/// ```compile_fail
/// let _: zdashstitch_application::portable_lld_review::VerifiedPortableLldEvidence =
///     serde_json::from_str("{}").unwrap();
/// ```
pub struct VerifiedPortableLldEvidence {
    local_id: String,
    revision: u64,
    server: String,
    version: zdashstitch_zabbix::ZabbixVersion,
    visibility_role: Option<String>,
    draft_fingerprint: String,
    selections: Vec<VerifiedSelection>,
    patterns: Vec<crate::portable_pattern_review::SavedPattern>,
    selected_single_host_patterns: BTreeSet<(usize, usize, usize)>,
    scope: Option<PortableGraphSelection>,
}

#[derive(Clone)]
pub(crate) struct PortableGraphSelection {
    pub(crate) page_index: usize,
    pub(crate) widgets: BTreeSet<usize>,
}

impl PortableGraphSelection {
    fn contains(&self, page: usize, widget: usize) -> bool {
        self.page_index == page && self.widgets.contains(&widget)
    }
}

/// Data-only materialization; not a plan and cannot authorize a write.
pub struct MaterializedPortableDashboard {
    document: serde_json::Value,
}

impl MaterializedPortableDashboard {
    /// Server-specific in-memory data; never a portable package or write grant.
    #[must_use]
    pub fn as_document(&self) -> &serde_json::Value {
        &self.document
    }

    #[must_use]
    pub fn page_count(&self) -> usize {
        self.document["pages"].as_array().map_or(0, Vec::len)
    }
}

#[derive(PartialEq, Eq)]
struct VerifiedSelection {
    page_index: usize,
    widget_index: usize,
    field_index: usize,
    hostid: String,
    itemid: Option<String>,
    review_fingerprint: String,
    require_saved_fingerprint: bool,
}

impl VerifiedPortableLldEvidence {
    pub(crate) fn draft_fingerprint(&self) -> &str {
        &self.draft_fingerprint
    }
    #[must_use]
    pub fn selection_count(&self) -> usize {
        self.selections.len()
    }
}

impl WorkcopyService {
    pub(crate) fn load_portable_source_draft(
        &self,
        local_id: &str,
    ) -> Result<(u64, TypedPortableBundle), WorkcopyError> {
        let root = self.store.root_path();
        if local_id.starts_with("pg-") {
            let page = crate::offline_pages::OfflinePageStore::new(root)
                .get(local_id)?
                .ok_or_else(|| WorkcopyError::new("offline_page_not_found"))?;
            Ok((page.revision, page.bundle))
        } else if local_id.starts_with("db-") {
            let dashboard = crate::offline_dashboards::OfflineDashboardStore::new(root)
                .get(local_id)?
                .ok_or_else(|| WorkcopyError::new("offline_dashboard_not_found"))?;
            Ok((dashboard.revision, dashboard.bundle))
        } else {
            Err(WorkcopyError::new("portable_lld_review_invalid"))
        }
    }

    /// Return binding addresses for the saved revision without contacting a server.
    pub fn portable_source_entries(
        &self,
        local_id: &str,
        revision: u64,
    ) -> Result<Vec<PortableSourceEntry>, WorkcopyError> {
        let invalid = || WorkcopyError::new("portable_lld_review_invalid");
        let (actual, bundle) = self.load_portable_source_draft(local_id)?;
        if actual != revision {
            return Err(WorkcopyError::new("workcopy_revision_conflict"));
        }
        bundle.validate().map_err(|_| invalid())?;
        let mut entries = Vec::new();
        for (page_index, page) in portable_pages(&bundle).iter().enumerate() {
            for (widget_index, widget) in page.widgets.iter().enumerate() {
                let widget_name = match widget {
                    Widget::Graph { title, .. } | Widget::ItemValue { title, .. } => title,
                    Widget::Zabbix { name, .. } => name,
                };
                for source in widget_sources(&bundle.project.roles, widget)? {
                    if entries.len() >= MAX_BINDINGS {
                        return Err(WorkcopyError::new("storage_limit_reached"));
                    }
                    let metric = source
                        .metric
                        .map(|reference| {
                            bundle
                                .catalogs
                                .get(reference.catalog)
                                .and_then(|catalog| {
                                    catalog
                                        .metrics
                                        .iter()
                                        .find(|metric| metric.id == reference.metric)
                                })
                                .ok_or_else(invalid)
                        })
                        .transpose()?;
                    entries.push(PortableSourceEntry {
                        page_index,
                        widget_index,
                        field_index: source.field_index,
                        page_name: page.name.clone(),
                        widget_name: widget_name.clone(),
                        field_name: source.field_name.map(str::to_owned),
                        role: source.role.to_owned(),
                        slot: source.slot.map(str::to_owned),
                        metric_name: metric.map(|metric| metric.name.clone()),
                        source_key: source
                            .metric
                            .and_then(|metric| metric.source_key.map(str::to_owned)),
                    });
                }
            }
        }
        Ok(entries)
    }

    /// Recheck local selections synchronously at journal admission, after the
    /// asynchronous reads and before any write is allowed to start.
    pub(crate) fn portable_lld_binding_state_matches(
        &self,
        evidence: &VerifiedPortableLldEvidence,
    ) -> bool {
        let Ok(_guard) = BINDINGS_LOCK.lock() else {
            return false;
        };
        let Ok(file) = read_bindings(&self.store) else {
            return false;
        };
        let selections_match = evidence.selections.iter().all(|selection| {
            let mut matches = file.bindings.iter().filter(|binding| {
                binding.local_id == evidence.local_id
                    && binding.server == evidence.server
                    && binding.page_index == selection.page_index
                    && binding.widget_index == selection.widget_index
                    && binding.field_index == selection.field_index
            });
            let Some(binding) = matches.next() else {
                return false;
            };
            matches.next().is_none()
                && binding.revision == evidence.revision
                && binding.draft_fingerprint == evidence.draft_fingerprint
                && binding.hostid == selection.hostid
                && binding.itemid == selection.itemid
                && (!selection.require_saved_fingerprint
                    || binding.review_fingerprint.as_ref() == Some(&selection.review_fingerprint))
        });
        selections_match && self.portable_pattern_state_matches(&evidence.patterns)
    }
    pub(crate) fn portable_draft_matches(
        &self,
        local_id: &str,
        revision: u64,
        fingerprint: &str,
    ) -> bool {
        self.load_portable_source_draft(local_id)
            .ok()
            .is_some_and(|(actual_revision, bundle)| {
                actual_revision == revision
                    && serde_json::to_vec(&bundle)
                        .ok()
                        .is_some_and(|bytes| format!("{:x}", Sha256::digest(bytes)) == fingerprint)
            })
    }
    /// Materialize only fixed-layout, known item/clock widgets, with LLD
    /// references supplied by the closed evidence. All other cases fail shut.
    /// The output does not carry authority to create or update a dashboard.
    pub fn materialize_portable_lld_dashboard(
        &self,
        evidence: &VerifiedPortableLldEvidence,
    ) -> Result<MaterializedPortableDashboard, WorkcopyError> {
        use zdashstitch_core::formats::{Layout, RegistryState};
        let invalid = || WorkcopyError::new("portable_materialization_unsupported");
        if !evidence.version.is_supported_7_4() {
            return Err(invalid());
        }
        let draft = crate::offline_dashboards::OfflineDashboardStore::new(self.store.root_path())
            .get(&evidence.local_id)?
            .ok_or_else(invalid)?;
        let digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&draft.bundle).map_err(|_| invalid())?)
        );
        if draft.revision != evidence.revision || digest != evidence.draft_fingerprint {
            return Err(WorkcopyError::new("workcopy_revision_conflict"));
        }
        draft.bundle.validate().map_err(|_| invalid())?;
        let ProjectRoot::Dashboard(dashboard) = &draft.bundle.project.root else {
            return Err(invalid());
        };
        let registry = zdashstitch_zabbix::WidgetCapabilityRegistry::v7_4();
        let mut pages = Vec::with_capacity(dashboard.pages.len());
        let mut used = 0usize;
        for (page_index, page) in dashboard.pages.iter().enumerate() {
            let mut widgets = Vec::with_capacity(page.widgets.len());
            for (widget_index, widget) in page.widgets.iter().enumerate() {
                let Widget::Zabbix {
                    widget_type,
                    name,
                    registry_state: RegistryState::Known,
                    fields,
                    layout: Layout::Fixed { x, y, w, h },
                    ..
                } = widget
                else {
                    return Err(invalid());
                };
                if !matches!(widget_type.as_str(), "item" | "clock") {
                    return Err(invalid());
                }
                let mut compatible = widget.clone();
                if let Widget::Zabbix { fields, .. } = &mut compatible {
                    for field in fields {
                        if let ZabbixWidgetFieldValue::LldInstance {
                            catalog,
                            metric,
                            hosts,
                            ..
                        } = &field.value
                        {
                            field.value = ZabbixWidgetFieldValue::Metric {
                                catalog: catalog.clone(),
                                metric: metric.clone(),
                                hosts: hosts.clone(),
                            };
                        }
                    }
                }
                crate::validate_portable_widget_for_create(&compatible, &registry)
                    .map_err(|_| invalid())?;
                let mut encoded_fields = Vec::with_capacity(fields.len());
                let mut names = std::collections::BTreeSet::new();
                for (field_index, field) in fields.iter().enumerate() {
                    if !names.insert(&field.name) {
                        return Err(invalid());
                    }
                    if field.name == "reference" || field.name.ends_with("._reference") {
                        return Err(invalid());
                    }
                    let value = match &field.value {
                        ZabbixWidgetFieldValue::Integer { value } => {
                            serde_json::json!(value.to_string())
                        }
                        ZabbixWidgetFieldValue::String { value } => serde_json::json!(value),
                        ZabbixWidgetFieldValue::LldInstance { .. }
                        | ZabbixWidgetFieldValue::Metric { .. } => {
                            let selection = evidence
                                .selections
                                .iter()
                                .find(|s| {
                                    s.page_index == page_index
                                        && s.widget_index == widget_index
                                        && s.field_index == field_index
                                })
                                .ok_or_else(invalid)?;
                            used += 1;
                            serde_json::json!(selection.itemid.as_deref().ok_or_else(invalid)?)
                        }
                        _ => return Err(invalid()),
                    };
                    encoded_fields.push(serde_json::json!({"type": field.field_type.to_string(), "name": field.name, "value": value}));
                }
                widgets.push(serde_json::json!({"type":widget_type,"name":name,"x":x.to_string(),"y":y.to_string(),"width":w.to_string(),"height":h.to_string(),"view_mode":"0","fields":encoded_fields}));
            }
            pages
                .push(serde_json::json!({"name":page.name,"display_period":"0","widgets":widgets}));
        }
        if used != evidence.selections.len() {
            return Err(invalid());
        }
        Ok(MaterializedPortableDashboard {
            document: serde_json::json!({"name":dashboard.name,"display_period":"30","auto_start":"1","pages":pages}),
        })
    }
    /// Revalidate the portable data sources of a saved dashboard. The resulting
    /// evidence is scoped to one exact draft and identity, and is not a plan.
    pub async fn verify_portable_lld_for_plan(
        &self,
        reader: &ReadOnlyGateway,
        local_id: &str,
        revision: u64,
    ) -> Result<VerifiedPortableLldEvidence, WorkcopyError> {
        self.verify_portable_lld_for_selection(reader, local_id, revision, None)
            .await
    }

    pub(crate) async fn verify_portable_lld_for_selection(
        &self,
        reader: &ReadOnlyGateway,
        local_id: &str,
        revision: u64,
        scope: Option<PortableGraphSelection>,
    ) -> Result<VerifiedPortableLldEvidence, WorkcopyError> {
        let (actual_revision, bundle) = self.load_portable_source_draft(local_id)?;
        if actual_revision != revision {
            return Err(WorkcopyError::new("workcopy_revision_conflict"));
        }
        bundle
            .validate()
            .map_err(|_| WorkcopyError::new("portable_lld_review_invalid"))?;
        let pages = portable_pages(&bundle);
        if scope.as_ref().is_some_and(|scope| {
            scope.widgets.is_empty()
                || !pages.get(scope.page_index).is_some_and(|page| {
                    scope
                        .widgets
                        .iter()
                        .all(|index| *index < page.widgets.len())
                })
        }) {
            return Err(WorkcopyError::new("portable_materialization_unsupported"));
        }
        let selected = |page, widget| {
            scope
                .as_ref()
                .is_none_or(|scope| scope.contains(page, widget))
        };
        let version = &reader.capabilities().version;
        let strict = version.major == 8;
        let registry = strict.then(WidgetCapabilityRegistry::v8_0);
        check_portable_version(reader).await?;
        let mut selections = Vec::new();
        let mut role_hosts = BTreeMap::new();
        let mut typed_metrics = BTreeMap::new();
        let mut pattern_roles = BTreeMap::new();
        let mut patterns = Vec::new();
        let mut selected_single_host_patterns = BTreeSet::new();
        let has_patterns = strict
            && pages.iter().enumerate().any(|(page_index, page)| {
                page.widgets.iter().enumerate().any(|(widget_index, widget)| {
                selected(page_index, widget_index) &&
                matches!(widget, Widget::Zabbix { fields, .. } if fields.iter().any(|field| {
                    matches!(field.value, ZabbixWidgetFieldValue::DynamicPattern { .. })
                }))
            })
            });
        let visibility = if has_patterns {
            Some(complete_pattern_visibility(reader).await?)
        } else {
            None
        };
        let digest = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&bundle)
                    .map_err(|_| WorkcopyError::new("portable_lld_review_invalid"))?
            )
        );
        for (page_index, page) in pages.iter().enumerate() {
            for (widget_index, widget) in page.widgets.iter().enumerate() {
                if !selected(page_index, widget_index) {
                    continue;
                }
                if let Widget::Zabbix {
                    widget_type,
                    fields,
                    registry_state,
                    ..
                } = widget
                {
                    for (field_index, field) in fields.iter().enumerate() {
                        if strict
                            && (*registry_state != RegistryState::Known
                                || field.registry_state != RegistryState::Known
                                || matches!(field.value, ZabbixWidgetFieldValue::Unresolved { .. }))
                        {
                            return Err(WorkcopyError::new("portable_materialization_unsupported"));
                        }
                        if let Some(registry) = &registry {
                            let field_type = WidgetFieldType::try_from(field.field_type).ok();
                            let source = match field.value {
                                ZabbixWidgetFieldValue::Metric { .. }
                                | ZabbixWidgetFieldValue::LldInstance { .. } => {
                                    Some(WidgetFieldType::Item)
                                }
                                ZabbixWidgetFieldValue::DynamicPattern { .. } => {
                                    Some(WidgetFieldType::String)
                                }
                                ZabbixWidgetFieldValue::Host { .. } => Some(WidgetFieldType::Host),
                                ZabbixWidgetFieldValue::String { .. }
                                    if widget_type == "svggraph"
                                        && crate::compatibility::is_svggraph_pattern_field(
                                            &field.name,
                                        ) =>
                                {
                                    return Err(WorkcopyError::new(
                                        "portable_materialization_unsupported",
                                    ));
                                }
                                _ => None,
                            };
                            if let Some(source) = source
                                && (field_type != Some(source)
                                    || !registry.find(widget_type).is_some_and(|widget| {
                                        widget.knows_field(&field.name, source)
                                    }))
                            {
                                return Err(WorkcopyError::new(
                                    "portable_materialization_unsupported",
                                ));
                            }
                        }
                        if let ZabbixWidgetFieldValue::DynamicPattern { role, .. } = &field.value {
                            let (saved, current) = self
                                .verify_saved_pattern_for_plan(
                                    reader,
                                    local_id,
                                    &crate::portable_pattern_review::PortablePatternReopenRequest {
                                        revision,
                                        page_index,
                                        widget_index,
                                        field_index,
                                    },
                                )
                                .await?;
                            if matches!(current.visible_matches.as_slice(), [host]
                                if host.selected && host.itemid.is_none())
                            {
                                selected_single_host_patterns.insert((
                                    page_index,
                                    widget_index,
                                    field_index,
                                ));
                            }
                            let mut targets = saved
                                .review
                                .targets
                                .iter()
                                .map(|slot| (slot.slot_id.0.clone(), slot.host.hostid.clone()))
                                .collect::<Vec<_>>();
                            targets.sort();
                            if pattern_roles
                                .insert(role.clone(), targets.clone())
                                .is_some_and(|old| old != targets)
                            {
                                return Err(WorkcopyError::new("portable_pattern_review_changed"));
                            }
                            for (slot, host) in &targets {
                                let singleton = bundle
                                    .project
                                    .roles
                                    .get(role)
                                    .is_some_and(|r| r.cardinality == Cardinality::One);
                                let key = (
                                    role.clone(),
                                    if singleton { None } else { Some(slot.clone()) },
                                );
                                if role_hosts
                                    .insert(key, host.clone())
                                    .is_some_and(|old| old != *host)
                                {
                                    return Err(WorkcopyError::new(
                                        "portable_pattern_review_changed",
                                    ));
                                }
                            }
                            patterns.push(saved);
                        }
                    }
                }
                for source in widget_sources(&bundle.project.roles, widget)? {
                    if selections.len() >= MAX_BINDINGS {
                        return Err(WorkcopyError::new("storage_limit_reached"));
                    }
                    let field_index = source.field_index;
                    let reopened = self
                        .reopen_portable_lld(
                            reader,
                            local_id,
                            &PortableLldReopenRequest {
                                revision,
                                page_index,
                                widget_index,
                                field_index,
                            },
                        )
                        .await?;
                    let singleton = bundle
                        .project
                        .roles
                        .get(source.role)
                        .is_some_and(|r| r.cardinality == Cardinality::One);
                    let key = (
                        source.role.to_owned(),
                        if singleton {
                            None
                        } else {
                            source.slot.map(str::to_owned)
                        },
                    );
                    if role_hosts
                        .insert(key, reopened.hostid.clone())
                        .is_some_and(|previous| previous != reopened.hostid)
                    {
                        return Err(WorkcopyError::new("portable_lld_review_changed"));
                    }
                    if reopened.review.draft_fingerprint != digest {
                        return Err(WorkcopyError::new("workcopy_revision_conflict"));
                    }
                    if source.typed
                        && let Some(metric) = source.metric
                        && typed_metrics
                            .insert(
                                (source.role, source.slot, metric.catalog, metric.metric),
                                reopened.itemid.clone(),
                            )
                            .is_some_and(|previous| previous != reopened.itemid)
                    {
                        return Err(WorkcopyError::new("portable_lld_review_changed"));
                    }
                    let require_saved_fingerprint = reopened.review.require_saved_fingerprint;
                    selections.push(VerifiedSelection {
                        page_index,
                        widget_index,
                        field_index,
                        hostid: reopened.hostid,
                        itemid: reopened.itemid,
                        review_fingerprint: reopened.review.fingerprint,
                        require_saved_fingerprint,
                    });
                }
            }
        }
        if strict
            || selections
                .iter()
                .any(|selection| selection.require_saved_fingerprint)
        {
            let mut host_roles = BTreeSet::new();
            for ((role, slot), host) in &role_hosts {
                if !host_roles.insert((role, host))
                    || pattern_roles.get(role).is_some_and(|targets| {
                        !targets.iter().any(|(pattern_slot, pattern_host)| {
                            slot.as_ref().is_none_or(|slot| slot == pattern_slot)
                                && host == pattern_host
                        })
                    })
                {
                    return Err(WorkcopyError::new("portable_lld_review_changed"));
                }
            }
        }
        if let Some(role) = &visibility
            && &complete_pattern_visibility(reader).await? != role
        {
            return Err(WorkcopyError::new("clone_pattern_target_not_visible"));
        }
        check_portable_version(reader).await?;
        let (after_revision, after_bundle) = self.load_portable_source_draft(local_id)?;
        if after_revision != revision || after_bundle != bundle {
            return Err(WorkcopyError::new("workcopy_revision_conflict"));
        }
        let evidence = VerifiedPortableLldEvidence {
            local_id: local_id.into(),
            revision,
            server: reader.identity().fingerprint().into(),
            version: version.clone(),
            visibility_role: visibility,
            draft_fingerprint: digest,
            selections,
            patterns,
            selected_single_host_patterns,
            scope,
        };
        if !self.portable_lld_binding_state_matches(&evidence) {
            return Err(WorkcopyError::new("portable_lld_review_changed"));
        }
        Ok(evidence)
    }

    /// Repeat the entire read-only proof and reject changed selections or data.
    pub async fn revalidate_portable_lld_for_plan(
        &self,
        reader: &ReadOnlyGateway,
        evidence: &VerifiedPortableLldEvidence,
    ) -> Result<(), WorkcopyError> {
        if reader.identity().fingerprint() != evidence.server
            || reader.capabilities().version != evidence.version
        {
            return Err(WorkcopyError::new("portable_lld_review_changed"));
        }
        let fresh = self
            .verify_portable_lld_for_selection(
                reader,
                &evidence.local_id,
                evidence.revision,
                evidence.scope.clone(),
            )
            .await?;
        if fresh.draft_fingerprint != evidence.draft_fingerprint
            || fresh.visibility_role != evidence.visibility_role
            || fresh.patterns != evidence.patterns
            || fresh.selected_single_host_patterns != evidence.selected_single_host_patterns
            || fresh.selections != evidence.selections
        {
            return Err(WorkcopyError::new("portable_lld_review_changed"));
        }
        Ok(())
    }
    /// Re-read a saved selection on the currently active server. A stored
    /// selection is a hint only, and cannot authorize a write.
    pub async fn reopen_portable_lld(
        &self,
        reader: &ReadOnlyGateway,
        local_id: &str,
        request: &PortableLldReopenRequest,
    ) -> Result<PortableLldReopened, WorkcopyError> {
        let find = || -> Result<StoredBinding, WorkcopyError> {
            let _guard = BINDINGS_LOCK
                .lock()
                .map_err(|_| WorkcopyError::new("storage_unavailable"))?;
            let file = read_bindings(&self.store)?;
            let mut found = file.bindings.into_iter().filter(|b| {
                b.local_id == local_id
                    && b.server == reader.identity().fingerprint()
                    && b.revision == request.revision
                    && b.page_index == request.page_index
                    && b.widget_index == request.widget_index
                    && b.field_index == request.field_index
            });
            let binding = found
                .next()
                .ok_or_else(|| WorkcopyError::new("portable_lld_binding_missing"))?;
            if found.next().is_some() {
                return Err(WorkcopyError::new("portable_lld_bindings_corrupt"));
            }
            Ok(binding)
        };
        let saved = find()?;
        let review = self
            .review_portable_lld(
                reader,
                local_id,
                &PortableLldReviewRequest {
                    revision: saved.revision,
                    page_index: saved.page_index,
                    widget_index: saved.widget_index,
                    field_index: saved.field_index,
                    hostid: saved.hostid.clone(),
                },
            )
            .await?;
        if find()? != saved
            || review.draft_fingerprint != saved.draft_fingerprint
            || (review.require_saved_fingerprint
                && saved.review_fingerprint.as_ref() != Some(&review.fingerprint))
            || !review.accepts(saved.itemid.as_deref())
        {
            return Err(WorkcopyError::new("portable_lld_review_changed"));
        }
        Ok(PortableLldReopened {
            review,
            hostid: saved.hostid,
            itemid: saved.itemid,
        })
    }
    pub async fn review_portable_lld(
        &self,
        reader: &ReadOnlyGateway,
        local_id: &str,
        request: &PortableLldReviewRequest,
    ) -> Result<PortableLldReview, WorkcopyError> {
        let invalid = || WorkcopyError::new("portable_lld_review_invalid");
        let strict = reader.capabilities().version.major == 8;
        check_portable_version(reader).await?;
        let load = || self.load_portable_source_draft(local_id);
        let (revision, bundle) = load()?;
        if revision != request.revision {
            return Err(WorkcopyError::new("workcopy_revision_conflict"));
        }
        bundle.validate().map_err(|_| invalid())?;
        let widget = portable_pages(&bundle)
            .get(request.page_index)
            .and_then(|page| page.widgets.get(request.widget_index))
            .ok_or_else(invalid)?;
        let source = widget_sources(&bundle.project.roles, widget)?
            .into_iter()
            .find(|source| source.field_index == request.field_index)
            .ok_or_else(invalid)?;
        if let Widget::Zabbix {
            fields,
            registry_state,
            ..
        } = widget
            && strict
            && (*registry_state != RegistryState::Known
                || fields[request.field_index].registry_state != RegistryState::Known)
        {
            return Err(invalid());
        }
        let hosts = reader
            .hosts_by_ids(std::slice::from_ref(&request.hostid))
            .await
            .map_err(|_| invalid())?;
        let [host] = hosts.as_slice() else {
            return Err(invalid());
        };
        if host.hostid != request.hostid {
            return Err(invalid());
        }
        let Some(target) = source.metric else {
            if !matches!(reader.matches_live_version().await, Ok(true)) {
                return Err(WorkcopyError::new("portable_lld_review_changed"));
            }
            let (after_revision, after) = load()?;
            if after_revision != revision || after != bundle {
                return Err(WorkcopyError::new("workcopy_revision_conflict"));
            }
            let draft_bytes = serde_json::to_vec(&bundle).map_err(|_| invalid())?;
            let bytes = serde_json::to_vec(&(
                "portable-host-review-v1",
                reader.identity().fingerprint(),
                reader.capabilities().version.to_string(),
                revision,
                &bundle,
                request.page_index,
                request.widget_index,
                request.field_index,
                host,
            ))
            .map_err(|_| invalid())?;
            return Ok(PortableLldReview {
                candidates: Vec::new(),
                fingerprint: format!("{:x}", Sha256::digest(bytes)),
                draft_fingerprint: format!("{:x}", Sha256::digest(draft_bytes)),
                host: Some(host.clone()),
                require_saved_fingerprint: true,
            });
        };
        let catalog = bundle.catalogs.get(target.catalog).ok_or_else(invalid)?;
        let metric = catalog
            .metrics
            .iter()
            .find(|m| {
                m.id == target.metric
                    && match m.origin {
                        MetricOrigin::LldPrototype => target.lld != LldSource::Ordinary,
                        MetricOrigin::HostItem | MetricOrigin::TemplateItem => {
                            target.lld != LldSource::Instance
                        }
                        MetricOrigin::DiscoveredItem => false,
                    }
            })
            .ok_or_else(invalid)?;
        let is_lld = metric.origin == MetricOrigin::LldPrototype;
        let prototype_key = match &metric.selector {
            MetricSelector::Key(key) => key.as_str(),
            MetricSelector::Alias(_) if !is_lld && (strict || source.typed) => "",
            MetricSelector::Alias(_) => return Err(invalid()),
        };
        let template_uuid = catalog.source.template_uuid.as_deref();
        let metric_uuid = metric.uuid.as_deref();
        if (is_lld || metric.origin == MetricOrigin::TemplateItem)
            && (template_uuid.is_none() || metric_uuid.is_none())
        {
            return Err(WorkcopyError::new("portable_lld_identity_unavailable"));
        }
        let items = reader
            .portable_host_items(&request.hostid)
            .await
            .map_err(|_| invalid())?;
        if items.truncated {
            return Err(WorkcopyError::new("portable_lld_inventory_truncated"));
        }
        let expected_type = match metric.value_type {
            ValueType::NumericFloat => 0,
            ValueType::Character => 1,
            ValueType::Log => 2,
            ValueType::NumericUnsigned => 3,
            ValueType::Text => 4,
            ValueType::Binary => 5,
            ValueType::Json => 6,
        };
        let mut seen = BTreeSet::new();
        let check_complete = strict || source.typed;
        let mut candidates = Vec::new();
        let mut provenance = BTreeMap::new();
        let mut checked_prototypes = BTreeMap::<String, bool>::new();
        let unverified = || WorkcopyError::new("portable_lld_identity_unavailable");
        for item in items.objects {
            if strict && (!seen.insert(item.itemid.clone()) || item.hostid != request.hostid) {
                return Err(invalid());
            }
            if !is_lld {
                let origin_matches = match metric.origin {
                    MetricOrigin::HostItem => item.templateid.is_none(),
                    MetricOrigin::TemplateItem => item.templateid.is_some(),
                    _ => false,
                };
                if item.hostid == request.hostid
                    && item.flags == ItemFlags::Plain
                    && match &metric.selector {
                        MetricSelector::Key(key) => item.key == *key,
                        MetricSelector::Alias(alias) => item.name == *alias,
                    }
                    && item.value_type == expected_type
                    && item.units == metric.units
                    && origin_matches
                {
                    if metric.origin == MetricOrigin::TemplateItem {
                        let identity = reader
                            .portable_item_identity(&item.itemid)
                            .await
                            .map_err(|_| invalid())?;
                        if check_complete && identity.is_none() {
                            return Err(unverified());
                        }
                        if !identity.is_some_and(|(template, item)| {
                            template_uuid
                                .is_some_and(|expected| template.eq_ignore_ascii_case(expected))
                                && metric_uuid
                                    .is_some_and(|expected| item.eq_ignore_ascii_case(expected))
                        }) {
                            continue;
                        }
                    }
                    if check_complete {
                        provenance.insert(item.itemid.clone(), item.templateid);
                    }
                    candidates.push(PortableLldCandidate {
                        itemid: item.itemid,
                        name: item.name,
                        key: item.key,
                    });
                }
                continue;
            }
            if item.flags != ItemFlags::Discovered || item.hostid != request.hostid {
                continue;
            }
            let Some(link) = &item.discovery else {
                if strict {
                    return Err(unverified());
                }
                continue;
            };
            if strict && link.prototype_key.is_empty() {
                return Err(unverified());
            }
            if link.prototype_key != prototype_key {
                continue;
            }
            if strict {
                match link.currently_discovered {
                    Some(true) => {}
                    Some(false) => continue,
                    None => return Err(unverified()),
                }
            }
            if item.value_type != expected_type || (strict && item.units != metric.units) {
                continue;
            }
            let confirmed = if let Some(value) = checked_prototypes.get(&link.parent_itemid) {
                *value
            } else {
                if checked_prototypes.len() >= 128 {
                    return Err(WorkcopyError::new("portable_lld_inventory_truncated"));
                }
                let prototypes = reader
                    .item_prototypes_by_ids(std::slice::from_ref(&link.parent_itemid))
                    .await
                    .map_err(|_| invalid())?;
                if strict
                    && !matches!(prototypes.as_slice(), [proto]
                    if proto.itemid == link.parent_itemid && proto.hostid == item.hostid && proto.key == prototype_key)
                {
                    return Err(unverified());
                }
                let mut confirmed = false;
                if let [proto] = prototypes.as_slice()
                    && proto.itemid == link.parent_itemid
                    && proto.hostid == item.hostid
                    && proto.key == prototype_key
                    && proto.value_type == expected_type
                    && (!strict || proto.units == metric.units)
                {
                    let identity = reader
                        .portable_prototype_identity(&proto.itemid)
                        .await
                        .map_err(|_| invalid())?;
                    if strict && identity.is_none() {
                        return Err(unverified());
                    }
                    confirmed = identity.is_some_and(|(t, m)| {
                        template_uuid.is_some_and(|expected| t.eq_ignore_ascii_case(expected))
                            && metric_uuid.is_some_and(|expected| m.eq_ignore_ascii_case(expected))
                    });
                }
                checked_prototypes.insert(link.parent_itemid.clone(), confirmed);
                confirmed
            };
            if !confirmed {
                continue;
            }
            if strict {
                provenance.insert(item.itemid.clone(), Some(link.parent_itemid.clone()));
            }
            candidates.push(PortableLldCandidate {
                itemid: item.itemid,
                name: item.name,
                key: item.key,
            });
        }
        if check_complete && !is_lld && candidates.len() > 1 {
            return Err(invalid());
        }
        candidates.sort_by(|a, b| (&a.key, &a.itemid).cmp(&(&b.key, &b.itemid)));
        check_portable_version(reader).await?;
        let (after_revision, after) = load()?;
        if after_revision != revision || after != bundle {
            return Err(WorkcopyError::new("workcopy_revision_conflict"));
        }
        let bytes = serde_json::to_vec(&(
            "portable-lld-review-v1",
            reader.identity().fingerprint(),
            reader.capabilities().version.to_string(),
            revision,
            &bundle,
            request.page_index,
            request.widget_index,
            request.field_index,
            &request.hostid,
            &candidates,
            (strict || source.typed).then_some((host, &provenance)),
        ))
        .map_err(|_| invalid())?;
        let draft_bytes = serde_json::to_vec(&bundle).map_err(|_| invalid())?;
        Ok(PortableLldReview {
            candidates,
            fingerprint: format!("{:x}", Sha256::digest(bytes)),
            draft_fingerprint: format!("{:x}", Sha256::digest(draft_bytes)),
            host: None,
            require_saved_fingerprint: strict || source.typed,
        })
    }
    pub async fn confirm_portable_lld(
        &self,
        reader: &ReadOnlyGateway,
        local_id: &str,
        request: &PortableLldConfirmRequest,
    ) -> Result<PortableLldReview, WorkcopyError> {
        let review = self
            .review_portable_lld(reader, local_id, &request.review)
            .await?;
        if request.fingerprint != review.fingerprint || !review.accepts(request.itemid.as_deref()) {
            return Err(WorkcopyError::new("portable_lld_review_changed"));
        }
        save_binding(
            &self.store,
            StoredBinding {
                local_id: local_id.to_owned(),
                server: reader.identity().fingerprint().to_owned(),
                revision: request.review.revision,
                draft_fingerprint: review.draft_fingerprint.clone(),
                page_index: request.review.page_index,
                widget_index: request.review.widget_index,
                field_index: request.review.field_index,
                hostid: request.review.hostid.clone(),
                itemid: request.itemid.clone(),
                review_fingerprint: review
                    .require_saved_fingerprint
                    .then_some(review.fingerprint.clone()),
            },
        )?;
        Ok(review)
    }
}

fn portable_pages(bundle: &TypedPortableBundle) -> &[Page] {
    match &bundle.project.root {
        ProjectRoot::Page(page) => std::slice::from_ref(page),
        ProjectRoot::Dashboard(dashboard) => &dashboard.pages,
    }
}

fn bundle_fingerprint(bundle: &TypedPortableBundle) -> Result<String, WorkcopyError> {
    let bytes = serde_json::to_vec(bundle)
        .map_err(|_| WorkcopyError::new("portable_lld_review_invalid"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

async fn check_portable_version(reader: &ReadOnlyGateway) -> Result<(), WorkcopyError> {
    if reader.capabilities().version.major == 8
        && !matches!(reader.matches_live_version().await, Ok(true))
    {
        return Err(WorkcopyError::new("portable_lld_review_changed"));
    }
    Ok(())
}

async fn complete_pattern_visibility(reader: &ReadOnlyGateway) -> Result<String, WorkcopyError> {
    reader
        .portable_pattern_inventory_role()
        .await
        .ok()
        .flatten()
        .ok_or_else(|| WorkcopyError::new("clone_pattern_target_not_visible"))
}
