// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;

const IDS: [&str; 6] = ["40001", "40002", "40003", "40004", "40005", "40006"];

fn host(id: &str) -> Value {
    let name = match id {
        "40001" => "Prod A",
        "40002" => "Prod B",
        "40003" => "Pilot A",
        "40004" => "Pilot B",
        "40005" => "Pilot C",
        "40006" => "Pilot external",
        _ => panic!("unexpected host ID: {id}"),
    };
    json!({"hostid":id,"host":format!("test-{id}"),"name":name})
}

fn item(id: &str, owner: &str, key: &str, prototype: Option<&str>) -> Value {
    json!({
        "itemid": id, "hostid": owner, "key_": key, "name": key,
        "value_type": "0", "units": "", "templateid": "0",
        "flags": if prototype.is_some() { "4" } else { "0" },
        "discoveryData": prototype.map(|id| json!({"parent_itemid":id,"key_":"test.lld[{#DB}]"})),
    })
}

pub(super) fn lookup(state: &Value, method: &str, params: &Value) -> Value {
    let filter = |values: Vec<Value>, query: &str, field: &str| -> Value {
        let accepted = params[query].as_array();
        json!(
            values
                .into_iter()
                .filter(|entry| { accepted.is_none_or(|ids| ids.contains(&entry[field])) })
                .collect::<Vec<_>>()
        )
    };
    match method {
        "host.get" => {
            if params["searchWildcardsEnabled"] == true {
                assert_eq!(params["search"]["name"], json!(["Pilot*"]));
                return json!(IDS[2..].iter().map(|id| host(id)).collect::<Vec<_>>());
            }
            if params["selectParentTemplates"].is_array() && state["hosts_truncated"] == true {
                return json!(
                    (0..101)
                        .map(|n| json!({"hostid":format!("{}",60000+n),
                    "host":format!("test-{n}"), "name":"Test node"}))
                        .collect::<Vec<_>>()
                );
            }
            let hosts = IDS
                .iter()
                .filter(|id| !(state["hidden"] == true && **id == "40005"))
                .map(|id| host(id))
                .collect();
            filter(hosts, "hostids", "hostid")
        }
        "item.get" if params["searchWildcardsEnabled"] == true => {
            assert_eq!(params["search"]["name_resolved"], json!(["CPU*"]));
            assert_eq!(params["filter"]["value_type"], json!([0, 3]));
            assert_eq!(params["inheritedTags"], true);
            assert_eq!(params["webitems"], true);
            assert_eq!(params["evaltype"], 2);
            assert_eq!(
                params["tags"],
                json!([
                    {"tag":"component","operator":1,"value":"storage"},
                    {"tag":"ignored","operator":5,"value":""}
                ])
            );
            if state["items_truncated"] == true {
                return json!(
                    (0..501)
                        .map(|n| json!({"itemid":format!("{}",70000+n),
                    "hostid":"40003","name_resolved":format!("CPU {n}"),"value_type":"3"}))
                        .collect::<Vec<_>>()
                );
            }
            json!([
                {"itemid":"30301","hostid":"40003","name_resolved":"CPU selected","value_type":"0"},
                {"itemid":"30601","hostid":"40006","name_resolved":
                    if state["pattern_stale"] == true { "CPU changed" } else { "CPU external" },
                    "value_type":"3"}
            ])
        }
        "item.get" => {
            let mut all = vec![
                item("30001", "40001", "test.rate", None),
                item("30002", "40002", "test.lld[prod]", Some("51002")),
                item("30301", "40003", "test.rate", None),
                item("30501", "40005", "test.rate", None),
                item("30402", "40004", "test.lld[prod]", Some("51006")),
            ];
            if state["lld_stale"] != true {
                all.push(item("30401", "40004", "test.lld[pilot]", Some("51004")));
            }
            if params["hostids"].is_array() {
                filter(all, "hostids", "hostid")
            } else {
                filter(all, "itemids", "itemid")
            }
        }
        "itemprototype.get" => {
            let proto = |id, owner, parent| {
                json!({
                    "itemid":id,"hostid":owner,"templateid":parent,"key_":"test.lld[{#DB}]",
                    "name":"Test LLD","value_type":"0","units":""
                })
            };
            filter(
                vec![
                    proto("51002", "40002", "55000"),
                    proto("51004", "40004", "55000"),
                    proto("51006", "40004", "55001"),
                    proto("55000", "59000", "0"),
                ],
                "itemids",
                "itemid",
            )
        }
        _ => panic!("unexpected lookup method: {method}"),
    }
}

fn clone_source() -> Value {
    let mut base = dashboard("Synthetic 8.0");
    base["pages"][0]["widgets"][0]["type"] = json!("svggraph");
    base["pages"][0]["widgets"][0]["fields"] = json!([
        {"name":"ds.0.dataset_type","type":"0","value":"0"},
        {"name":"ds.0.color.0","type":"1","value":"AABBCC"},
        {"name":"ds.0.color.1","type":"1","value":"CDAA00"},
        {"name":"ds.0.itemids.0","type":"4","value":"30001"},
        {"name":"ds.0.itemids.1","type":"4","value":"30002"},
        {"name":"ds.1.hosts.0","type":"1","value":"Pilot*"},
        {"name":"ds.1.items.0","type":"1","value":"CPU*"},
        {"name":"ds.1.item_tags_evaltype","type":"0","value":"2"},
        {"name":"ds.1.item_tags.0.tag","type":"1","value":"component"},
        {"name":"ds.1.item_tags.0.operator","type":"0","value":"1"},
        {"name":"ds.1.item_tags.0.value","type":"1","value":"storage"},
        {"name":"ds.1.item_tags.1.tag","type":"1","value":"ignored"},
        {"name":"ds.1.item_tags.1.operator","type":"0","value":"5"},
        {"name":"ds.1.item_tags.1.value","type":"1","value":""}
    ]);
    base
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clone_8_0_reviews_visible_scope_and_builds_three_rows_locally() {
    let source = clone_source();
    let app = App::start(Script {
        snapshots: VecDeque::from([json!([source])]),
        list: json!({"clone_8_0": true}),
        methods: Vec::new(),
    })
    .await;
    let connection = app.connect().await;
    let (status, imported) = app
        .call(
            "POST",
            &format!("/api/server/{connection}/dashboards/501/import"),
            Some(json!({"scope":"dashboard"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{imported}");
    let id = imported["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("missing local ID"));
    let path = format!("/api/drafts/{id}/clone");
    let (status, setup) = app.call("GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{setup}");
    assert_eq!(setup["review_visible_patterns"], true);
    assert_eq!(setup["growth_samples"].as_array().map(Vec::len), Some(2));

    let mut input = json!({
        "revision":setup["revision"], "source_fingerprint":setup["source_fingerprint"],
        "name":"Synthetic Pilot", "hosts":{"40001":"40003","40002":"40004"},
        "growth":{"sample":setup["growth_samples"][0]["field"],"target_hostid":"40005"}
    });
    let (status, bindings) = app
        .call(
            "POST",
            &format!("{path}/review-bindings"),
            Some(input.clone()),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{bindings}");
    let row = &bindings["lld_rows"][0];
    assert_eq!(row["prototype_key"], "test.lld[{#DB}]");
    assert_eq!(row["candidates"].as_array().map(Vec::len), Some(1));
    assert_eq!(row["candidates"][0]["itemid"], "30401");
    input["lld_choices"] = json!([{
        "field": row["field"], "slot_id": row["slot_id"], "itemid":"30401", "fingerprint":row["fingerprint"]
    }]);

    let set_flag = |key: &str, value: bool| {
        app.script
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .list[key] = json!(value);
    };
    let pattern_path = format!("{path}/preview-pattern");
    let mut decisions = Vec::new();
    for pattern in setup["patterns"]
        .as_array()
        .unwrap_or_else(|| panic!("missing patterns"))
    {
        let request = json!({"setup":input,"field":pattern["field"],"target_pattern":pattern["source_pattern"]});
        let (status, review) = app.call("POST", &pattern_path, Some(request.clone())).await;
        assert_eq!(status, StatusCode::OK, "{review}");
        assert_eq!(review["truncated"], false);
        let matches = review["matches"]
            .as_array()
            .unwrap_or_else(|| panic!("missing matches"));
        if pattern["kind"] == "item" {
            assert_eq!(matches.len(), 2);
            assert!(
                matches
                    .iter()
                    .any(|item| item["name"] == "CPU selected" && item["outside"] == false)
            );
            assert!(
                matches
                    .iter()
                    .any(|item| item["name"] == "CPU external" && item["outside"] == true)
            );
            for flag in ["hosts_truncated", "items_truncated"] {
                set_flag(flag, true);
                let (_, rejected) = app.call("POST", &pattern_path, Some(request.clone())).await;
                assert_eq!(
                    rejected["code"], "clone_pattern_preview_truncated",
                    "{rejected}"
                );
                set_flag(flag, false);
            }
        } else {
            assert_eq!(matches.len(), 4);
        }
        decisions.push(json!({"field":pattern["field"],"decision":{
            "action":"replace", "target_pattern":pattern["source_pattern"], "fingerprint":review["fingerprint"]
        }}));
    }
    assert_eq!(decisions.len(), 2);
    let growth_path = format!("{path}/preview-growth");
    input["pattern_choices"] = json!(decisions);
    let (_, rejected) = app.call("POST", &growth_path, Some(input.clone())).await;
    assert_eq!(
        rejected["code"], "clone_pattern_decision_required",
        "{rejected}"
    );
    for decision in input["pattern_choices"]
        .as_array_mut()
        .unwrap_or_else(|| panic!("missing decisions"))
    {
        decision["decision"]["acknowledge_visible_outside"] = true.into();
    }
    for (flag, code) in [
        ("lld_stale", "clone_lld_selection_stale"),
        ("pattern_stale", "clone_pattern_preview_stale"),
        ("hidden", "clone_target_not_visible"),
    ] {
        set_flag(flag, true);
        let (_, rejected) = app.call("POST", &growth_path, Some(input.clone())).await;
        assert_eq!(rejected["code"], code, "{rejected}");
        set_flag(flag, false);
    }
    let (status, preview) = app.call("POST", &growth_path, Some(input.clone())).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["before"].as_array().map(Vec::len), Some(2));
    assert_eq!(preview["after"][0]["host"], "Pilot A");
    assert_eq!(preview["after"][1]["key"], "test.lld[pilot]");
    assert_eq!(preview["after"][2]["host"], "Pilot C");
    input["growth_fingerprint"] = preview["fingerprint"].clone();
    let (status, saved) = app.call("POST", &path, Some(input)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let clone_id = saved["clone_id"]
        .as_str()
        .unwrap_or_else(|| panic!("missing clone ID"));
    let service =
        zdashstitch_application::workcopy::WorkcopyService::new(app.storage.path().to_path_buf());
    let clone = service
        .open_clone_workcopy(clone_id)
        .unwrap_or_else(|error| panic!("{error}"));
    let fields = clone.local().raw_api_document()["pages"][0]["widgets"][0]["fields"]
        .as_array()
        .unwrap_or_else(|| panic!("missing fields"));
    assert!(
        fields
            .iter()
            .any(|field| field["name"] == "ds.0.itemids.2" && field["value"] == "30501")
    );
    assert!(fields.iter().any(|field| field["name"] == "ds.0.color.2"));
    for tag in source["pages"][0]["widgets"][0]["fields"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|field| {
            field["name"]
                .as_str()
                .is_some_and(|name| name.contains(".item_tags"))
        })
    {
        assert!(fields.contains(tag), "{tag}");
    }
    let active = app
        .connections
        .service()
        .unwrap_or_else(|| panic!("connection"));
    let guard = active.lock().await;
    let reader = guard.reader().unwrap_or_else(|| panic!("reader"));
    use zdashstitch_application::clone_mapping::{
        CloneBindingCheck, CloneCheckReason, CloneRevalidationContext,
        revalidate_clone_mapping_state,
    };
    let stored = clone.draft().mapping();
    let context = CloneRevalidationContext {
        connection_id: connection,
        server_identity_fingerprint: reader.identity().fingerprint().to_owned(),
        server_version: reader.capabilities().version.to_string(),
        local_fingerprint: clone.local().fingerprint().to_owned(),
    };
    let base = clone.draft().base();
    let checked = revalidate_clone_mapping_state(reader, base, stored, &context)
        .await
        .unwrap_or_else(|error| panic!("revalidation: {error}"));
    assert_eq!(&checked, stored);
    set_flag("pattern_stale", true);
    set_flag("lld_stale", true);
    let changed = revalidate_clone_mapping_state(reader, base, stored, &context)
        .await
        .unwrap_or_else(|error| panic!("stale revalidation: {error}"));
    assert!(changed.pattern_decisions.iter().any(|decision| {
        decision.check == CloneBindingCheck::Stale
            && decision.check_reason == Some(CloneCheckReason::PatternChanged)
    }));
    assert!(
        changed
            .mappings
            .iter()
            .flat_map(|mapping| &mapping.references)
            .flat_map(|reference| &reference.targets)
            .any(|target| {
                target.check == CloneBindingCheck::Invalid
                    && target.check_reason == Some(CloneCheckReason::CandidateNotVisible)
            })
    );
    assert!(
        saved["blocked"].is_string()
            || saved["missing_contracts"]
                .as_array()
                .is_some_and(|c| !c.is_empty()),
        "{saved}"
    );
    assert_reads_only(&app.methods());
}
