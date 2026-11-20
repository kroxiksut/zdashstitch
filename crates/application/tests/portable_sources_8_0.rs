// SPDX-License-Identifier: MIT OR Apache-2.0

use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use axum::{Json, Router, extract::State, routing::post};
use secrecy::SecretString;
use serde_json::{Value, json};
use zdashstitch_application::{
    clone_mapping::{TargetSlot, TargetSlotId},
    offline_dashboards::{OfflineDashboard, OfflineDashboardStore},
    portable_lld_review::{PortableLldConfirmRequest, PortableLldReviewRequest},
    portable_pattern_review::{PortablePatternConfirmRequest, PortablePatternReviewRequest},
    workcopy::WorkcopyService,
};
use zdashstitch_core::formats::TypedPortableBundle;
use zdashstitch_zabbix::{
    Authentication, HostDetails, ItemFlags, MAX_HOST_SEARCH, ReadOnlyGateway, ZabbixClient,
    ZabbixEndpoint,
};

const TEMPLATE_UUID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PROTOTYPE_UUID: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn dashboard(
    mut ordinary: Value,
    prototype: Value,
    template_uuid: &str,
    mask: &str,
) -> TypedPortableBundle {
    let field = |name: &str, slot: &str, metric: &str, source_key: Option<&Value>| {
        let catalog = if source_key.is_some() {
            "discovery-metrics"
        } else {
            "ordinary-metrics"
        };
        let mut value = json!({"kind":"metric","catalog":catalog,"metric":metric,
            "hosts":{"role":"hosts","selection":"slot","slot_id":slot}});
        if let Some(key) = source_key {
            value["kind"] = json!("lld_instance");
            value["source_key"] = key.clone();
        }
        json!({"name":name,"field_type":4,"registry_state":"known","value":value})
    };
    let mask_field = |name: &str, value: &str| {
        json!({"name":name,"field_type":1,"registry_state":"known",
        "value":{"kind":"dynamic_pattern","value":value,"role":"hosts"}})
    };
    let mut proto = prototype;
    let key = proto
        .as_object_mut()
        .and_then(|object| object.remove("source_key"))
        .unwrap_or_default();
    let mut ordinary_source = json!({"kind":"manual"});
    if let Some(uuid) = ordinary
        .as_object_mut()
        .and_then(|object| object.remove("template_uuid"))
    {
        ordinary_source["template_uuid"] = uuid;
    }
    serde_json::from_value(json!({"project":{
        "format":"zdashstitch.project/v2","project_id":"demo","kind":"dashboard",
        "declared_compatibility":{"minimum":"8.0.0"},"provenance":{"kind":"local"},
        "roles":{"hosts":{"cardinality":"many","slots":["first","second"]}},
        "catalogs":[{"id":"ordinary-metrics","file":"metrics/ordinary-metrics.yaml"},
            {"id":"discovery-metrics","file":"metrics/discovery-metrics.yaml"}],
        "root":{"kind":"global","id":"demo","name":"Demo","pages":[{"id":"page","name":"Page","widgets":[
            {"type":"zabbix","id":"explicit","name":"Explicit","widget_type":"svggraph","registry_state":"known",
                "layout":{"mode":"fixed","x":0,"y":0,"w":18,"h":4},"fields":[
                    field("ds.0.itemids.0", "first", "ordinary", None),
                    field("ds.0.itemids.1", "second", "discovery", Some(&key))]},
            {"type":"zabbix","id":"masks","name":"Masks","widget_type":"svggraph","registry_state":"known",
                "layout":{"mode":"fixed","x":18,"y":0,"w":18,"h":4},"fields":[
                    mask_field("ds.0.hosts.0", "*"),mask_field("ds.0.items.0", mask),
                    mask_field("or.0.hosts.0", "*"),mask_field("or.0.items.0", mask),
                    mask_field("problemhosts.0", "*")]}
        ]}]}
    },"catalogs":{"discovery-metrics":{"format":"zdashstitch.metrics/v1","catalog_id":"discovery-metrics",
        "export_profile":"portable_with_public_keys","source":{"kind":"template_export","export_format_version":"8.0","template_uuid":template_uuid},
        "diagnostics":[],"metrics":[proto]},"ordinary-metrics":{"format":"zdashstitch.metrics/v1","catalog_id":"ordinary-metrics",
        "export_profile":"portable_with_public_keys","source":ordinary_source,"diagnostics":[],"metrics":[ordinary]}}}))
        .unwrap_or_else(|error| panic!("fixture: {error}"))
}

fn metric(id: &str, key: &str, value_type: &str, units: &str, origin: &str) -> Value {
    json!({"id":id,"name":id,"selector":{"kind":"key","value":key},"value_type":value_type,"units":units,"origin":origin})
}

async fn confirm_metric(
    reader: &ReadOnlyGateway,
    service: &WorkcopyService,
    draft: &OfflineDashboard,
    field: usize,
    host: &str,
    item: &str,
) {
    let review = PortableLldReviewRequest {
        revision: draft.revision,
        page_index: 0,
        widget_index: 0,
        field_index: field,
        hostid: host.into(),
    };
    let result = service
        .review_portable_lld(reader, &draft.local_id, &review)
        .await
        .unwrap_or_else(|error| panic!("metric review: {error}"));
    assert!(
        result
            .candidates
            .iter()
            .any(|candidate| candidate.itemid == item)
    );
    service
        .confirm_portable_lld(
            reader,
            &draft.local_id,
            &PortableLldConfirmRequest {
                review,
                fingerprint: result.fingerprint,
                itemid: Some(item.into()),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("metric confirmation: {error}"));
}

async fn confirm_patterns(
    reader: &ReadOnlyGateway,
    service: &WorkcopyService,
    draft: &OfflineDashboard,
    hostids: [&str; 2],
) {
    let targets = ["first", "second"]
        .into_iter()
        .zip(hostids)
        .map(|(slot, host)| TargetSlot {
            slot_id: TargetSlotId(slot.into()),
            host: HostDetails {
                hostid: host.into(),
                host: "unused".into(),
                name: "unused".into(),
            },
        })
        .collect::<Vec<_>>();
    for field in 0..5 {
        let review = PortablePatternReviewRequest {
            revision: draft.revision,
            page_index: 0,
            widget_index: 1,
            field_index: field,
            targets: targets.clone(),
        };
        let result = service
            .preview_saved_portable_pattern(reader, &draft.local_id, &review)
            .await
            .unwrap_or_else(|error| panic!("mask preview: {error}"));
        assert!(result.visible_matches.iter().any(|entry| !entry.selected));
        let mut request = PortablePatternConfirmRequest {
            review,
            fingerprint: result.fingerprint,
            acknowledge_visible_outside: false,
        };
        let without = service
            .confirm_saved_portable_pattern(reader, &draft.local_id, &request)
            .await;
        assert!(without.is_err());
        request.acknowledge_visible_outside = true;
        service
            .confirm_saved_portable_pattern(reader, &draft.local_id, &request)
            .await
            .unwrap_or_else(|error| panic!("mask confirmation: {error}"));
    }
}

async fn rpc(State(fault): State<Arc<AtomicU8>>, Json(body): Json<Value>) -> Json<Value> {
    let fault = fault.load(Ordering::Relaxed);
    let params = &body["params"];
    let method = body["method"].as_str().unwrap_or_default();
    let count = params["countOutput"] == true;
    let is_mask = params["filter"]["value_type"].is_array();
    let host = params["hostids"][0].as_str().unwrap_or_default();
    let role_type = if fault == 1 { 2 } else { 3 };
    let result = match method {
        "apiinfo.version" => json!(if fault == 6 { "8.0.1" } else { "8.0.0" }),
        "user.checkAuthentication" => {
            json!({"userid":"3","username":"synthetic","roleid":"9","type":role_type})
        }
        "role.get" => {
            json!([{"roleid":"9","type":role_type,"rules":{"api.access":"1","api.mode":"0","api":if fault == 9 { json!(["item.get"]) } else { json!([]) }}}])
        }
        "host.get" => {
            let rows: Vec<_> = ["10", "11", "12"].into_iter().filter(|id| {
                (fault != 8 || *id != "11") && params["hostids"].as_array().is_none_or(|ids| ids.iter().any(|value| value == id))
            }).map(|id| json!({"hostid":id,"host":format!("host-{id}"),"name":format!("Host {id} fault {}", if fault == 12 { 1 } else { 0 })})).collect();
            if count {
                json!(if fault == 10 {
                    rows.len() + 1
                } else {
                    rows.len()
                })
            } else {
                json!(rows)
            }
        }
        "item.get" if is_mask => {
            assert!(params["tags"].is_null());
            if count {
                json!(if fault == 2 { 4 } else { 3 })
            } else {
                json!((0..3).map(|i| json!({"hostid":format!("{}",10+i),"itemid":format!("{}",20+i),
                "name_resolved":if fault == 7 && i == 2 { "Probe changed" } else { "Probe" },"value_type":"3"})).collect::<Vec<_>>())
            }
        }
        "item.get" => {
            let ordinary = |id: &str| {
                json!({"hostid":host,"itemid":id,"name":if fault == 3 { "Probe renamed" } else { "Probe" },
                "key_":"probe","units":"","value_type":"3","flags":"0","templateid":"0"})
            };
            let discovered = |id: &str, key: &str| {
                json!({"hostid":host,"itemid":id,"name":"Probe",
                "key_":key,"units":"","value_type":"3","flags":"4","templateid":"0",
                "discoveryData":{"parent_itemid":"30","key_":"probe[{#ID}]","status":if fault == 13 { "1" } else { "0" }}})
            };
            let rows = if host == "10" && fault == 4 {
                vec![ordinary("20"), ordinary("23")]
            } else if host == "10" {
                vec![ordinary("20")]
            } else if fault == 11 {
                vec![
                    ordinary("25"),
                    discovered("21", "probe[db]"),
                    discovered("22", "probe[db2]"),
                ]
            } else {
                vec![ordinary("25"), discovered("21", "probe[db]")]
            };
            if count {
                json!(rows.len())
            } else {
                json!(rows)
            }
        }
        "itemprototype.get" if fault == 5 => json!([]),
        "itemprototype.get" => {
            if params["itemids"] == json!(["30"]) {
                json!([{"itemid":"30","hostid":"11","name":"Prototype","key_":"probe[{#ID}]","units":"","value_type":"3","templateid":"31"}])
            } else {
                json!([{"itemid":"31","hostid":"40","templateid":"0","uuid":PROTOTYPE_UUID}])
            }
        }
        "template.get" => json!([{"templateid":"40","uuid":TEMPLATE_UUID}]),
        _ => panic!("unexpected method {method}"),
    };
    Json(json!({"jsonrpc":"2.0","id":body["id"],"result":result}))
}

async fn fixture_server() -> (ReadOnlyGateway, Arc<AtomicU8>) {
    let server = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("listener: {error}"));
    let address = server
        .local_addr()
        .unwrap_or_else(|error| panic!("listener address: {error}"));
    let fault = Arc::new(AtomicU8::new(0));
    tokio::spawn(
        axum::serve(
            server,
            Router::new()
                .route("/api_jsonrpc.php", post(rpc))
                .with_state(fault.clone()),
        )
        .into_future(),
    );
    let endpoint = ZabbixEndpoint::parse(&format!("http://{address}/api_jsonrpc.php"))
        .unwrap_or_else(|_| panic!("endpoint"));
    let reader = ZabbixClient::new(endpoint)
        .unwrap_or_else(|_| panic!("client"))
        .authenticate(Authentication::ApiToken(SecretString::from("synthetic")))
        .await
        .unwrap_or_else(|_| panic!("connection"));
    (reader, fault)
}

#[tokio::test]
async fn portable_sources_8_0_recheck_all_confirmed_sources_before_plan() {
    let (reader, fault) = fixture_server().await;
    let temp = tempfile::tempdir().unwrap_or_else(|_| panic!("tempdir"));
    let mut prototype = metric(
        "discovery",
        "probe[{#ID}]",
        "numeric_unsigned",
        "",
        "lld_prototype",
    );
    prototype["uuid"] = json!(PROTOTYPE_UUID);
    prototype["source_key"] = json!("source-is-only-a-hint");
    let bundle = dashboard(
        metric("ordinary", "probe", "numeric_unsigned", "", "host_item"),
        prototype,
        TEMPLATE_UUID,
        "Probe*",
    );
    let draft = OfflineDashboardStore::new(temp.path().into())
        .save_imported(bundle)
        .unwrap_or_else(|error| panic!("saved dashboard: {error}"));
    let service = WorkcopyService::new(temp.path().into());
    confirm_metric(&reader, &service, &draft, 0, "10", "20").await;
    confirm_metric(&reader, &service, &draft, 1, "11", "21").await;
    confirm_patterns(&reader, &service, &draft, ["10", "11"]).await;
    let service = WorkcopyService::new(temp.path().into());
    let evaluate =
        || service.verify_portable_lld_for_plan(&reader, &draft.local_id, draft.revision);
    let verified = evaluate()
        .await
        .unwrap_or_else(|error| panic!("fresh proof: {error}"));
    assert_eq!(verified.selection_count(), 2);
    for (mode, expected) in [
        (1, "clone_pattern_target_not_visible"),
        (2, "clone_pattern_preview_read_failed"),
        (3, "portable_lld_review_changed"),
        (4, "portable_lld_review_invalid"),
        (5, "portable_lld_identity_unavailable"),
        (6, "portable_lld_review_changed"),
        (7, "portable_pattern_review_changed"),
        (8, "portable_lld_review_invalid"),
        (9, "clone_pattern_target_not_visible"),
        (10, "clone_pattern_preview_read_failed"),
        (11, "portable_lld_review_changed"),
    ] {
        fault.store(mode, Ordering::Relaxed);
        match evaluate().await {
            Ok(_) => panic!("changed proof accepted in mode {mode}"),
            Err(error) => assert_eq!(error.code(), expected, "mode {mode}"),
        }
    }
    fault.store(0, Ordering::Relaxed);
    service
        .revalidate_portable_lld_for_plan(&reader, &verified)
        .await
        .unwrap_or_else(|error| panic!("revalidated proof: {error}"));
    assert!(
        service
            .materialize_portable_lld_dashboard(&verified)
            .is_err()
    );
    confirm_patterns(&reader, &service, &draft, ["11", "10"]).await;
    assert!(evaluate().await.is_err());
}

#[tokio::test]
async fn typed_source_addresses_bind_all_slots_and_direct_host_references() {
    let (reader, fault) = fixture_server().await;
    let temp = tempfile::tempdir().unwrap_or_else(|_| panic!("tempdir"));
    let mut prototype = metric(
        "discovery",
        "probe[{#ID}]",
        "numeric_unsigned",
        "",
        "lld_prototype",
    );
    prototype["uuid"] = json!(PROTOTYPE_UUID);
    prototype["source_key"] = json!("hint");
    let mut bundle = serde_json::to_value(dashboard(
        metric("ordinary", "probe", "numeric_unsigned", "", "host_item"),
        prototype,
        TEMPLATE_UUID,
        "Probe*",
    ))
    .unwrap_or_default();
    let selector = json!({"role":"hosts","selection":"all"});
    let ordinary = json!({"catalog":"ordinary-metrics","metric":"ordinary"});
    bundle["project"]["root"]["pages"][0]["widgets"] = json!([
        {"type":"graph","id":"graph","title":"Graph","expansion":"series_per_host","series":[
            {"metric":ordinary,"hosts":selector},
            {"metric":{"catalog":"discovery-metrics","metric":"discovery"},"hosts":{"role":"hosts","selection":"slot","slot_id":"second"}}
        ],"layout":{"mode":"fixed","x":0,"y":0,"w":18,"h":4}},
        {"type":"item_value","id":"values","title":"Values","expansion":"widget_per_host","metric":ordinary,"hosts":selector,
            "layout":{"mode":"auto","w":12,"h":4}},
        {"type":"zabbix","id":"direct","name":"Direct","widget_type":"item","registry_state":"known",
            "layout":{"mode":"fixed","x":18,"y":0,"w":18,"h":4},"fields":[
                {"name":"override_hostid.0","field_type":3,"registry_state":"known","value":{"kind":"host","role":"hosts","selection":"slot","slot_id":"first"}}
            ]}
    ]);
    let bundle =
        serde_json::from_value(bundle).unwrap_or_else(|error| panic!("typed fixture: {error}"));
    let draft = OfflineDashboardStore::new(temp.path().into())
        .save_imported(bundle)
        .unwrap_or_else(|error| panic!("save typed draft: {error}"));
    let service = WorkcopyService::new(temp.path().into());
    let entries = service
        .portable_source_entries(&draft.local_id, draft.revision)
        .unwrap_or_else(|error| panic!("typed entries: {error}"));
    let entries = serde_json::to_value(entries).unwrap_or_default();
    let sources = [
        (0, 0, "10", Some("20")),
        (0, 1, "11", Some("25")),
        (0, 2, "11", Some("21")),
        (1, 0, "10", Some("20")),
        (1, 1, "11", Some("25")),
        (2, 0, "10", None),
    ];
    assert_eq!(entries.as_array().map(Vec::len), Some(sources.len()));
    for (index, (widget_index, field_index, hostid, itemid)) in sources.into_iter().enumerate() {
        assert_eq!(entries[index]["widget_index"], widget_index);
        assert_eq!(entries[index]["field_index"], field_index);
        let request = PortableLldReviewRequest {
            revision: draft.revision,
            page_index: 0,
            widget_index,
            field_index,
            hostid: hostid.into(),
        };
        let preview = service
            .review_portable_lld(&reader, &draft.local_id, &request)
            .await
            .unwrap_or_else(|error| panic!("typed preview {index}: {error}"));
        assert_eq!(preview.host.is_some(), itemid.is_none());
        service
            .confirm_portable_lld(
                &reader,
                &draft.local_id,
                &PortableLldConfirmRequest {
                    review: request,
                    itemid: itemid.map(str::to_owned),
                    fingerprint: preview.fingerprint,
                },
            )
            .await
            .unwrap_or_else(|error| panic!("typed confirmation {index}: {error}"));
    }
    let restarted = WorkcopyService::new(temp.path().into());
    let proof = restarted
        .verify_portable_lld_for_plan(&reader, &draft.local_id, draft.revision)
        .await
        .unwrap_or_else(|error| panic!("typed proof: {error}"));
    assert_eq!(proof.selection_count(), 6);
    assert!(
        restarted
            .portable_source_entries(&draft.local_id, draft.revision + 1)
            .is_err()
    );
    for change in [4, 5, 8, 12, 13] {
        fault.store(change, Ordering::Relaxed);
        assert!(
            restarted
                .revalidate_portable_lld_for_plan(&reader, &proof)
                .await
                .is_err()
        );
    }
}

#[tokio::test]
#[ignore = "requires the explicitly authorized read-only 8.0.0 laboratory"]
async fn lab_portable_sources_8_0() {
    let setting =
        |name| std::env::var(name).unwrap_or_else(|_| panic!("missing laboratory setting"));
    assert_eq!(
        setting("ZDASHSTITCH_P5_01_LAB_ACK"),
        "read-only-authorized-8.0.0"
    );
    let url = setting("ZDASHSTITCH_P5_01_LAB_URL");
    let endpoint = ZabbixEndpoint::parse(&url).unwrap_or_else(|_| panic!("endpoint"));
    assert_eq!(endpoint.api_url().host_str(), Some("127.0.0.1"));
    let reader = ZabbixClient::new(endpoint)
        .unwrap_or_else(|_| panic!("client"))
        .authenticate(Authentication::UsernamePassword {
            username: setting("ZDASHSTITCH_P5_01_LAB_USERNAME"),
            password: SecretString::from(setting("ZDASHSTITCH_P5_01_LAB_PASSWORD")),
        })
        .await
        .unwrap_or_else(|_| panic!("laboratory authentication"));
    assert_eq!(reader.capabilities().version.to_string(), "8.0.0");
    let targets = reader
        .search_hosts("", MAX_HOST_SEARCH)
        .await
        .unwrap_or_else(|_| panic!("lab host inventory"));
    assert!(!targets.truncated && targets.objects.len() > 2);
    let mut inventory = Vec::new();
    for host in targets.objects {
        let items = reader
            .portable_host_items(&host.hostid)
            .await
            .unwrap_or_else(|_| panic!("lab item inventory"));
        assert!(!items.truncated);
        inventory.extend(items.objects);
    }
    let mut ordinary = None;
    for item in inventory.iter().filter(|item| {
        item.flags == ItemFlags::Plain && item.key == "agent.ping" && item.value_type == 3
    }) {
        let mut catalog = metric(
            "ordinary",
            &item.key,
            "numeric_unsigned",
            &item.units,
            "host_item",
        );
        if item.templateid.is_some() {
            let Some((template, uuid)) = reader
                .portable_item_identity(&item.itemid)
                .await
                .unwrap_or_else(|_| panic!("lab item identity"))
            else {
                continue;
            };
            catalog["template_uuid"] = json!(template);
            catalog["uuid"] = json!(uuid);
            catalog["origin"] = json!("template_item");
        }
        ordinary = Some((item, catalog));
        break;
    }
    let (ordinary, ordinary_metric) =
        ordinary.unwrap_or_else(|| panic!("ordinary lab fixture absent"));
    let mut discovered = None;
    for item in inventory.iter().filter(|item| {
        item.hostid != ordinary.hostid
            && matches!(item.value_type, 0 | 3)
            && item.flags == ItemFlags::Discovered
    }) {
        let Some(link) = item
            .discovery
            .as_ref()
            .filter(|link| link.currently_discovered == Some(true))
        else {
            continue;
        };
        let identity = reader
            .portable_prototype_identity(&link.parent_itemid)
            .await
            .unwrap_or_else(|_| panic!("lab prototype identity"));
        if let Some(identity) = identity {
            discovered = Some((item, identity));
            break;
        }
    }
    let (discovered, (template_uuid, item_uuid)) =
        discovered.unwrap_or_else(|| panic!("LLD lab fixture absent"));
    let link = discovered
        .discovery
        .as_ref()
        .unwrap_or_else(|| panic!("LLD link"));
    let mut prototype = metric(
        "discovery",
        &link.prototype_key,
        if discovered.value_type == 3 {
            "numeric_unsigned"
        } else {
            "numeric_float"
        },
        &discovered.units,
        "lld_prototype",
    );
    prototype["uuid"] = json!(item_uuid);
    prototype["source_key"] = json!("source-is-only-a-hint");
    let bundle = dashboard(ordinary_metric, prototype, &template_uuid, "*ping*");
    let temp = tempfile::tempdir().unwrap_or_else(|_| panic!("tempdir"));
    let draft = OfflineDashboardStore::new(temp.path().into())
        .save_imported(bundle)
        .unwrap_or_else(|error| panic!("lab draft: {error}"));
    let service = WorkcopyService::new(temp.path().into());
    confirm_metric(
        &reader,
        &service,
        &draft,
        0,
        &ordinary.hostid,
        &ordinary.itemid,
    )
    .await;
    confirm_metric(
        &reader,
        &service,
        &draft,
        1,
        &discovered.hostid,
        &discovered.itemid,
    )
    .await;
    confirm_patterns(
        &reader,
        &service,
        &draft,
        [&ordinary.hostid, &discovered.hostid],
    )
    .await;
    let service = WorkcopyService::new(temp.path().into());
    let proof = service
        .verify_portable_lld_for_plan(&reader, &draft.local_id, draft.revision)
        .await
        .unwrap_or_else(|error| panic!("lab proof: {error}"));
    assert_eq!(proof.selection_count(), 2);
    service
        .revalidate_portable_lld_for_plan(&reader, &proof)
        .await
        .unwrap_or_else(|error| panic!("lab recheck: {error}"));
    reader
        .logout()
        .await
        .unwrap_or_else(|_| panic!("lab logout"));
}
