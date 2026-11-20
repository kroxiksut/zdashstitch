// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;

fn update_request(preview: &Value) -> Value {
    json!({
        "action": "update_source",
        "revision": preview["revision"],
        "base_fingerprint": preview["base_fingerprint"],
        "local_fingerprint": preview["local_fingerprint"],
    })
}

fn confirm_request(held: &Value) -> Value {
    json!({"handle": held["handle"], "plan_id": held["plan"]["plan_id"], "confirmed": true})
}

async fn update(
    app: &mut App,
    lab: &Lab,
    connection: &str,
    path: &str,
    preview: &Value,
    mode: &Value,
) {
    let url = format!("{path}/publication/plan");
    let confirm = format!("{path}/publication/confirm");
    let input = update_request(preview);
    let options = match mode.as_str() {
        Some("app-lld" | "app-lld-hidden") => &[1, 3, 4, 5, 8, 10, 11, 13][..],
        Some("app-masks") => &[2, 3, 4, 5, 6, 7, 8][..],
        _ => &[3, 4, 5, 8][..],
    };
    for &changed in options {
        let held = composite_plan(app, &url, &input).await;
        lab.changed_evidence.store(changed, Ordering::SeqCst);
        let (_, refused) = app
            .call("POST", &confirm, Some(confirm_request(&held)))
            .await;
        lab.changed_evidence.store(0, Ordering::SeqCst);
        if changed == 11 {
            assert!(lab.role_revoked.swap(false, Ordering::SeqCst));
        }
        assert_eq!(
            refused["state"], "failed_before_send",
            "evidence {changed}: {}",
            refused["code"]
        );
        assert_eq!(refused["sent"], false);
        assert_eq!(lab.sends.load(Ordering::SeqCst), 0);
    }
    for role in [false, true] {
        let held = composite_plan(app, &url, &input).await;
        lab.stale.store(!role, Ordering::SeqCst);
        lab.role_revoked.store(role, Ordering::SeqCst);
        let (_, refused) = app
            .call("POST", &confirm, Some(confirm_request(&held)))
            .await;
        lab.stale.store(false, Ordering::SeqCst);
        lab.role_revoked.store(false, Ordering::SeqCst);
        assert_eq!(
            refused["state"],
            if role {
                "failed_before_send"
            } else {
                "conflict"
            }
        );
        assert_eq!(refused["sent"], false);
        assert_eq!(lab.sends.load(Ordering::SeqCst), 0);
    }
    let held = composite_plan(app, &url, &input).await;
    assert_eq!(held["plan"]["api_method"], "dashboard.update");
    assert_eq!(held["plan"]["write_requests"], 1);
    assert_eq!(held["plan"]["replacement_count"], 3);
    assert_eq!(held["plan"]["changed_pages"], json!([0, 1]));
    if mode == "app-masks" {
        assert!(
            held["plan"]["report"]["findings"]
                .as_array()
                .is_some_and(|findings| {
                    findings.iter().any(|finding| {
                        finding["code"] == "pattern_matches_outside_selection"
                            && finding["params"]["count"] == "2"
                    })
                })
        );
    }
    assert_ne!(
        held["plan"]["expected_fingerprint"],
        preview["local_fingerprint"]
    );
    let lose_response = matches!(
        mode.as_str(),
        Some("app-recover" | "app-lld" | "app-lld-hidden" | "app-masks")
    );
    lab.drop_response.store(lose_response, Ordering::SeqCst);
    let (status, result) = app
        .call("POST", &confirm, Some(confirm_request(&held)))
        .await;
    assert_eq!(status, StatusCode::OK, "confirm: {}", result["code"]);
    assert_eq!(result["sent"], true);
    assert_eq!(lab.sends.load(Ordering::SeqCst), 1);
    assert_eq!(
        app.call("POST", &confirm, Some(confirm_request(&held)))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    if lose_response {
        assert_eq!(result["state"], "unknown_outcome");
        assert!(result["base_advanced"].is_null());
        lab.hide_copy.store(false, Ordering::SeqCst);
        restart(app, connection).await;
        let operation = held["plan"]["plan_id"]
            .as_str()
            .unwrap_or_else(|| panic!("operation ID"));
        let reconcile = format!("/api/publication/journal/{operation}/reconcile");
        let (status, recovered) = app.call("POST", &reconcile, Some(json!({}))).await;
        assert_eq!(status, StatusCode::OK, "reconcile: {}", recovered["code"]);
        assert_eq!(recovered["state"], "reconciled_verified");
        assert_eq!(
            recovered["base_advanced"]["base_fingerprint"],
            held["plan"]["expected_fingerprint"]
        );
        assert_eq!(
            app.call("POST", &reconcile, Some(json!({}))).await.0,
            StatusCode::CONFLICT
        );
    } else {
        assert_eq!(result["state"], "verified");
        assert_eq!(
            result["base_advanced"]["base_fingerprint"],
            held["plan"]["expected_fingerprint"]
        );
    }
    assert_eq!(lab.sends.load(Ordering::SeqCst), 1);
}

async fn choose_lld(app: &App, lab: &Lab, path: &str, fixture: &Value, preview: &Value) -> Value {
    let plan = format!("{path}/publication/plan");
    let (status, _) = app.call("POST", &plan, Some(update_request(preview))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(lab.sends.load(Ordering::SeqCst), 0);
    let row = preview["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|row| row["match"] == "unresolved_lld")
        .unwrap_or_else(|| panic!("explicit LLD row"));
    let candidates = row["candidates"]
        .as_array()
        .unwrap_or_else(|| panic!("LLD candidates"));
    assert_eq!(candidates.len(), 2);
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate["itemid"] == fixture["lld_target"])
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate["itemid"] == fixture["lld_other"])
    );
    let decide = format!("{path}/replace-host/decide");
    let decision = |item: &Value| json!({"key":row["key"], "decision":{"kind":"choose_lld_instance", "itemid":item}});
    let (_, refused) = app
        .call("POST", &decide, Some(decision(&fixture["ordinary_item"])))
        .await;
    assert_eq!(refused["code"], "candidate_not_visible");
    let (status, resolved) = app
        .call("POST", &decide, Some(decision(&fixture["lld_target"])))
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "explicit LLD choice: {}",
        resolved["code"]
    );
    assert_eq!(resolved["changes"].as_array().map(Vec::len), Some(3));
    for changed in [1, 12] {
        lab.changed_evidence.store(changed, Ordering::SeqCst);
        let (status, refreshed) = app
            .call(
                "POST",
                &format!("{path}/replace-host/revalidate"),
                Some(json!({})),
            )
            .await;
        lab.changed_evidence.store(0, Ordering::SeqCst);
        assert_eq!(
            status,
            StatusCode::OK,
            "LLD revalidation: {}",
            refreshed["code"]
        );
        assert!(
            refreshed["rows"]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|fresh| {
                    fresh["key"] == row["key"]
                        && fresh["check"] == "invalid"
                        && fresh["check_reason"] == "candidate_not_visible"
                }))
        );
        assert_eq!(lab.sends.load(Ordering::SeqCst), 0);
    }
    resolved
}

async fn copy(app: &App, lab: &Lab, path: &str, preview: &Value) {
    let name = format!("{} HTTP private copy", lab.title);
    let input = json!({
        "action":"save_as_copy", "name":name, "revision":preview["revision"],
        "base_fingerprint":preview["base_fingerprint"], "local_fingerprint":preview["local_fingerprint"],
    });
    let held = composite_plan(app, &format!("{path}/publication/plan"), &input).await;
    assert_eq!(held["plan"]["api_method"], "dashboard.create");
    let mut confirmation = confirm_request(&held);
    confirmation["name"] = json!(name);
    let confirm = format!("{path}/publication/confirm");
    let (status, result) = app.call("POST", &confirm, Some(confirmation.clone())).await;
    assert_eq!(status, StatusCode::OK, "copy confirm: {}", result["code"]);
    assert_eq!(result["state"], "verified");
    assert!(result["record_error"].is_null());
    assert_eq!(result["clone_source_check"], "current");
    assert_eq!(
        app.call("POST", &confirm, Some(confirmation)).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(lab.sends.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the authorized 8.0.0 private ReplaceHost laboratory fixture"]
async fn lab_z80_replace_update_http() {
    let (mut app, lab, proxy_task, connection, fixture) =
        connect_lab(FixtureKind::ReplaceHost).await;
    let source = &lab.source;
    let (status, imported) = app
        .call(
            "POST",
            &format!("/api/server/{connection}/dashboards/{source}/import"),
            Some(json!({"scope":"dashboard"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "import: {}", imported["code"]);
    let local = imported["local_id"]
        .as_str()
        .unwrap_or_else(|| panic!("workcopy ID"));
    let path = format!("/api/drafts/{local}");
    let (status, preview) = app.call("POST", &format!("{path}/replace-host/calculate"), Some(json!({
        "source_hostid":fixture["ordinary_source"], "target_hostid":fixture["ordinary_target"],
        "scope":{"kind":"dashboard"}
    }))).await;
    assert_eq!(status, StatusCode::OK, "calculate: {}", preview["code"]);
    let lld = matches!(fixture["mode"].as_str(), Some("app-lld" | "app-lld-hidden"));
    assert_eq!(
        preview["changes"].as_array().map(Vec::len),
        Some(if lld { 2 } else { 3 })
    );
    let (status, refusal) = app
        .call(
            "POST",
            &format!("{path}/publication/plan"),
            Some(update_request(&preview)),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refusal["code"], "write_mode_required");
    assert_eq!(lab.sends.load(Ordering::SeqCst), 0);
    let (status, enabled) = app
        .call(
            "POST",
            &format!("/api/connections/{connection}/enable-writes"),
            Some(json!({"confirmed":true})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "enable writes: {}", enabled["code"]);
    let preview = if lld {
        choose_lld(&app, &lab, &path, &fixture, &preview).await
    } else {
        preview
    };
    if fixture["mode"] == "app-copy" {
        copy(&app, &lab, &path, &preview).await;
    } else {
        update(
            &mut app,
            &lab,
            &connection,
            &path,
            &preview,
            &fixture["mode"],
        )
        .await;
    }
    proxy_task.abort();
}
