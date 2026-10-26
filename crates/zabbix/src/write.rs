// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::time::Duration;

use secrecy::ExposeSecret;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::gateway::{ReadWriteGateway, WriteMethod};
use crate::{Result, VersionedDashboardSnapshot, ZabbixCapabilities, ZabbixError, ZabbixVersion};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemFieldReplacement {
    pub page_index: usize,
    pub widget_index: usize,
    pub field_name: String,
    pub expected_itemid: String,
    pub replacement_itemid: String,
}

/// Non-secret binding to the authenticated connection and exact capability
/// profile. Offline contexts can validate a preview but cannot authorize send.
#[derive(Clone)]
pub struct PublicationContext {
    capabilities: ZabbixCapabilities,
    binding_fingerprint: String,
    live_authenticated: bool,
}

impl PublicationContext {
    pub fn capabilities(&self) -> &ZabbixCapabilities {
        &self.capabilities
    }

    #[doc(hidden)]
    pub fn offline_for_planning(capabilities: ZabbixCapabilities) -> Self {
        Self {
            capabilities,
            binding_fingerprint: "offline-unpublishable".to_owned(),
            live_authenticated: false,
        }
    }

    #[doc(hidden)]
    pub fn detached_for_testing(capabilities: ZabbixCapabilities) -> Self {
        Self {
            capabilities,
            binding_fingerprint: "detached-test-authority".to_owned(),
            live_authenticated: true,
        }
    }

    pub fn is_live_authenticated(&self) -> bool {
        self.live_authenticated
    }
}

/// Adapter-validated update. It is deliberately neither `Clone` nor
/// deserializable, and its payload is never exposed.
pub struct VerifiedDashboardUpdate {
    connection_id: String,
    server_version: ZabbixVersion,
    dashboard_id: String,
    source_fingerprint: String,
    expected_fingerprint: String,
    authority_fingerprint: String,
    update_params: Value,
    replacement_count: usize,
}

impl VerifiedDashboardUpdate {
    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub fn server_version(&self) -> &ZabbixVersion {
        &self.server_version
    }

    pub fn dashboard_id(&self) -> &str {
        &self.dashboard_id
    }

    pub fn source_fingerprint(&self) -> &str {
        &self.source_fingerprint
    }

    pub fn expected_fingerprint(&self) -> &str {
        &self.expected_fingerprint
    }

    pub const fn replacement_count(&self) -> usize {
        self.replacement_count
    }

    fn matches_expected(&self, snapshot: &VersionedDashboardSnapshot) -> bool {
        snapshot.connection_id() == self.connection_id
            && snapshot.server_version() == &self.server_version
            && snapshot.dashboard_id() == self.dashboard_id
            && snapshot.fingerprint() == self.expected_fingerprint
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DashboardPublishOutcome {
    Conflict,
    Verified,
    FailedBeforeSend,
    FailedAfterResponse,
    UnknownOutcome,
    ReconciledVerified,
    ReconciledFailed,
}

pub fn verify_item_field_replacements(
    snapshot: &VersionedDashboardSnapshot,
    context: &PublicationContext,
    replacements: &[ItemFieldReplacement],
) -> Result<VerifiedDashboardUpdate> {
    let capabilities = context.capabilities();
    if !context.live_authenticated
        || capabilities.version != *snapshot.server_version()
        || !capabilities
            .dashboard_write_contracts
            .item_replacement_is_verified()
    {
        return Err(reject(
            "exact server version and every granular write contract are required",
        ));
    }
    if replacements.is_empty() || replacements.len() > 1_024 {
        return Err(reject("replacement count must be 1..=1024"));
    }
    let page_index = replacements[0].page_index;
    if replacements
        .iter()
        .any(|replacement| replacement.page_index != page_index)
    {
        return Err(reject("one verified update may change only one page"));
    }

    let mut expected_raw = snapshot.raw_api_document().clone();
    let pages = expected_raw
        .get_mut("pages")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| reject("snapshot has no pages array"))?;
    let page = pages
        .get_mut(page_index)
        .ok_or_else(|| reject("replacement page index is out of range"))?;
    let widgets = page
        .get_mut("widgets")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| reject("replacement page has no widgets array"))?;
    for widget in &*widgets {
        if !matches!(
            string_property(widget, "type", "widget")?,
            "item" | "svggraph"
        ) {
            return Err(reject(
                "changed pages may contain only contract-proven item and svggraph widgets",
            ));
        }
    }

    let mut seen = BTreeSet::new();
    for replacement in replacements {
        if replacement.replacement_itemid.is_empty()
            || replacement.replacement_itemid.len() > 20
            || !replacement
                .replacement_itemid
                .chars()
                .all(|character| character.is_ascii_digit())
        {
            return Err(reject(
                "replacement itemid must contain 1..=20 ASCII digits",
            ));
        }
        if !seen.insert((replacement.widget_index, replacement.field_name.as_str())) {
            return Err(reject("the same widget field is replaced twice"));
        }
        let widget = widgets
            .get_mut(replacement.widget_index)
            .ok_or_else(|| reject("replacement widget index is out of range"))?;
        if string_property(widget, "type", "widget")? != "item" {
            return Err(reject(
                "only item-widget itemid fields have a proven mutation contract",
            ));
        }
        let fields = widget
            .get_mut("fields")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| reject("item widget has no fields array"))?;
        let matching = fields
            .iter_mut()
            .filter(|field| field["name"] == replacement.field_name)
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(reject("replacement must identify exactly one widget field"));
        }
        let field = matching
            .into_iter()
            .next()
            .ok_or_else(|| reject("replacement field disappeared"))?;
        if value_as_i64(&field["type"]) != Some(4) {
            return Err(reject("replacement field is not an itemid field"));
        }
        if field["value"].as_str() != Some(replacement.expected_itemid.as_str()) {
            return Err(reject("replacement source itemid is stale"));
        }
        if replacement.expected_itemid == replacement.replacement_itemid {
            return Err(reject("replacement does not change the itemid"));
        }
        field["value"] = Value::String(replacement.replacement_itemid.clone());
    }

    let expected = VersionedDashboardSnapshot::from_adapter_raw(
        snapshot.connection_id().to_owned(),
        snapshot.server_version().clone(),
        expected_raw.clone(),
    )?;
    let update_params = encode_update(snapshot.dashboard_id(), &expected_raw, page_index)?;
    Ok(VerifiedDashboardUpdate {
        connection_id: snapshot.connection_id().to_owned(),
        server_version: snapshot.server_version().clone(),
        dashboard_id: snapshot.dashboard_id().to_owned(),
        source_fingerprint: snapshot.fingerprint().to_owned(),
        expected_fingerprint: expected.fingerprint().to_owned(),
        authority_fingerprint: context.binding_fingerprint.clone(),
        update_params,
        replacement_count: replacements.len(),
    })
}

impl ReadWriteGateway {
    /// Publication authority bound to the endpoint, version, credential and
    /// the current write epoch.
    pub fn publication_context(&self) -> PublicationContext {
        let session = self.session();
        let mut digest = Sha256::new();
        digest.update(b"zdashstitch-publication-authority-v1\0");
        digest.update(session.transport.endpoint_identity().as_bytes());
        digest.update(b"\0");
        digest.update(self.capabilities().version.to_string().as_bytes());
        digest.update(b"\0");
        digest.update(session.token.expose_secret().as_bytes());
        digest.update(b"\0");
        digest.update(self.grant.epoch().to_be_bytes());
        let mut binding_fingerprint = String::with_capacity(64);
        for byte in digest.finalize() {
            let _ = write!(&mut binding_fingerprint, "{byte:02x}");
        }
        PublicationContext {
            capabilities: self.capabilities().clone(),
            binding_fingerprint,
            live_authenticated: true,
        }
    }

    /// Mandatory closed publication path: fresh read, fingerprint comparison,
    /// exactly one send, then read-back. There is no public send-only method.
    pub async fn publish_verified_dashboard_update(
        &self,
        update: &VerifiedDashboardUpdate,
    ) -> DashboardPublishOutcome {
        let current_context = self.publication_context();
        if !current_context.live_authenticated
            || current_context.binding_fingerprint != update.authority_fingerprint
            || self.capabilities().version != update.server_version
            || !self
                .capabilities()
                .dashboard_write_contracts
                .item_replacement_is_verified()
        {
            return DashboardPublishOutcome::FailedBeforeSend;
        }
        let Ok(before) = self.dashboard_snapshots(update.connection_id()).await else {
            return DashboardPublishOutcome::FailedBeforeSend;
        };
        let Some(current) = unique_dashboard(&before, update.dashboard_id()) else {
            return DashboardPublishOutcome::Conflict;
        };
        if current.fingerprint() != update.source_fingerprint() {
            return DashboardPublishOutcome::Conflict;
        }

        let session = self.session();
        let dispatch = session
            .transport
            .write::<Value>(
                WriteMethod::DashboardUpdate,
                update.update_params.clone(),
                &session.token,
                &self.grant,
            )
            .await;
        let response_known = match dispatch {
            Ok(result)
                if result["dashboardids"]
                    .as_array()
                    .is_some_and(|ids| ids.iter().any(|id| id == update.dashboard_id())) =>
            {
                true
            }
            Ok(_) => false,
            Err(ZabbixError::Api { .. } | ZabbixError::HttpStatus(_)) => {
                return DashboardPublishOutcome::FailedAfterResponse;
            }
            Err(_) => false,
        };

        let mut read_succeeded = false;
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            if let Ok(after) = self.dashboard_snapshots(update.connection_id()).await {
                read_succeeded = true;
                if unique_dashboard(&after, update.dashboard_id())
                    .is_some_and(|snapshot| update.matches_expected(snapshot))
                {
                    return if response_known {
                        DashboardPublishOutcome::Verified
                    } else {
                        DashboardPublishOutcome::ReconciledVerified
                    };
                }
            }
        }
        match (response_known, read_succeeded) {
            (true, _) => DashboardPublishOutcome::FailedAfterResponse,
            (false, true) => DashboardPublishOutcome::ReconciledFailed,
            (false, false) => DashboardPublishOutcome::UnknownOutcome,
        }
    }
}

fn unique_dashboard<'a>(
    snapshots: &'a [VersionedDashboardSnapshot],
    dashboard_id: &str,
) -> Option<&'a VersionedDashboardSnapshot> {
    let mut matching = snapshots
        .iter()
        .filter(|snapshot| snapshot.dashboard_id() == dashboard_id);
    let dashboard = matching.next()?;
    matching.next().is_none().then_some(dashboard)
}

fn encode_update(dashboard_id: &str, raw: &Value, changed_page: usize) -> Result<Value> {
    let pages = raw["pages"]
        .as_array()
        .ok_or_else(|| reject("expected dashboard has no pages"))?;
    let encoded = pages
        .iter()
        .enumerate()
        .map(|(index, page)| {
            let page_id = string_property(page, "dashboard_pageid", "page")?;
            if index != changed_page {
                return Ok(json!({"dashboard_pageid": page_id}));
            }
            let mut output = Map::new();
            for key in ["dashboard_pageid", "name", "display_period"] {
                output.insert(
                    key.to_owned(),
                    page.get(key)
                        .cloned()
                        .ok_or_else(|| reject("changed page is incomplete"))?,
                );
            }
            let widgets = page["widgets"]
                .as_array()
                .ok_or_else(|| reject("changed page has no widgets"))?
                .iter()
                .map(encode_widget)
                .collect::<Result<Vec<_>>>()?;
            output.insert("widgets".to_owned(), Value::Array(widgets));
            Ok(Value::Object(output))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"dashboardid": dashboard_id, "pages": encoded}))
}

fn encode_widget(widget: &Value) -> Result<Value> {
    let mut output = Map::new();
    for key in [
        "widgetid",
        "type",
        "name",
        "x",
        "y",
        "width",
        "height",
        "view_mode",
        "fields",
    ] {
        output.insert(
            key.to_owned(),
            widget
                .get(key)
                .cloned()
                .ok_or_else(|| reject("widget is incomplete"))?,
        );
    }
    Ok(Value::Object(output))
}

fn string_property<'a>(value: &'a Value, key: &str, context: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| reject(&format!("{context} has no string {key}")))
}

fn value_as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn reject(message: &str) -> ZabbixError {
    ZabbixError::WriteContract(message.to_owned())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::AdapterRegistry;

    fn snapshot(raw: Value, version: &str) -> VersionedDashboardSnapshot {
        VersionedDashboardSnapshot::from_adapter_raw(
            "test-connection".to_owned(),
            version
                .parse()
                .unwrap_or_else(|error| panic!("valid version: {error}")),
            raw,
        )
        .unwrap_or_else(|error| panic!("valid snapshot: {error}"))
    }

    fn dashboard(widget_type: &str) -> Value {
        json!({
            "dashboardid": "7", "name": "contract",
            "pages": [{
                "dashboard_pageid": "11", "name": "page", "display_period": "0",
                "widgets": [{
                    "widgetid": "13", "type": widget_type, "name": "source",
                    "x": "0", "y": "0", "width": "12", "height": "3", "view_mode": "0",
                    "fields": [
                        {"type": "4", "name": "itemid.0", "value": "101"},
                        {"type": "1", "name": "reference", "value": "ABCDE"}
                    ]
                }]
            }],
            "users": [], "userGroups": []
        })
    }

    fn replacement() -> ItemFieldReplacement {
        ItemFieldReplacement {
            page_index: 0,
            widget_index: 0,
            field_name: "itemid.0".to_owned(),
            expected_itemid: "101".to_owned(),
            replacement_itemid: "202".to_owned(),
        }
    }

    #[test]
    fn exact_contract_encodes_one_page_without_exposing_raw_payload() {
        let source = snapshot(dashboard("item"), "7.4.16");
        let capabilities = AdapterRegistry::resolve(source.server_version().clone())
            .unwrap_or_else(|error| panic!("supported adapter: {error}"));
        let context = PublicationContext::detached_for_testing(capabilities);
        let update = verify_item_field_replacements(&source, &context, &[replacement()])
            .unwrap_or_else(|error| panic!("verified update: {error}"));
        assert_eq!(update.replacement_count(), 1);
        assert_ne!(update.source_fingerprint(), update.expected_fingerprint());
        assert_eq!(
            update.update_params["pages"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(
            update.update_params["pages"][0]["widgets"][0]["fields"][0]["value"],
            "202"
        );
    }

    #[test]
    fn unproven_versions_unknown_widgets_and_stale_fields_fail_closed() {
        let unproven = snapshot(dashboard("item"), "7.4.17");
        let capabilities = AdapterRegistry::resolve(unproven.server_version().clone())
            .unwrap_or_else(|error| panic!("read adapter exists: {error}"));
        let context = PublicationContext::detached_for_testing(capabilities);
        assert!(verify_item_field_replacements(&unproven, &context, &[replacement()]).is_err());

        let unknown = snapshot(dashboard("vendor.private"), "7.4.16");
        let capabilities = AdapterRegistry::resolve(unknown.server_version().clone())
            .unwrap_or_else(|error| panic!("write contract exists: {error}"));
        let context = PublicationContext::detached_for_testing(capabilities);
        assert!(verify_item_field_replacements(&unknown, &context, &[replacement()]).is_err());

        let source = snapshot(dashboard("item"), "7.4.16");
        let mut stale = replacement();
        stale.expected_itemid = "999".to_owned();
        assert!(verify_item_field_replacements(&source, &context, &[stale]).is_err());
    }
}
