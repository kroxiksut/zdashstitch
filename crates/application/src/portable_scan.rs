// SPDX-License-Identifier: MIT OR Apache-2.0
//! Bounded, local candidate detection. This is not an anonymization guarantee.

use serde::Serialize;
use serde_json::Value;
use std::net::IpAddr;
use zdashstitch_core::formats::{ProjectRoot, RegistryState, TypedPortableBundle, Widget};
use zdashstitch_zabbix::{WidgetCapabilityRegistry, WidgetFieldType};

const LIMIT: usize = 256;

#[derive(Serialize)]
pub struct Candidate {
    pub kind: &'static str,
    pub location: String,
    pub evidence: &'static str,
}

#[derive(Serialize)]
pub struct CandidateScan {
    pub findings: Vec<Candidate>,
    pub truncated: bool,
    pub unknown_objects: usize,
    pub credential_candidates: usize,
}

pub fn scan(bundle: &TypedPortableBundle) -> Result<CandidateScan, serde_json::Error> {
    let value = serde_json::to_value(bundle)?;
    let mut result = CandidateScan {
        findings: Vec::new(),
        truncated: false,
        unknown_objects: 0,
        credential_candidates: 0,
    };
    visit(&value, "", &mut result);
    let registry = WidgetCapabilityRegistry::v7_4();
    let pages: Vec<_> = match &bundle.project.root {
        ProjectRoot::Page(page) => vec![page],
        ProjectRoot::Dashboard(dashboard) => dashboard.pages.iter().collect(),
    };
    for (page_index, page) in pages.iter().enumerate() {
        for (widget_index, widget) in page.widgets.iter().enumerate() {
            let Widget::Zabbix {
                widget_type,
                registry_state: RegistryState::Known,
                fields,
                ..
            } = widget
            else {
                continue;
            };
            let capability = registry.find(widget_type);
            let unverified = capability.is_none_or(|capability| {
                capability.fields.is_empty()
                    || fields.iter().any(|field| {
                        field.registry_state == RegistryState::Known
                            && WidgetFieldType::try_from(field.field_type)
                                .map_or(true, |kind| !capability.knows_field(&field.name, kind))
                    })
            });
            if unverified {
                result.unknown_objects = result.unknown_objects.saturating_add(1);
                let location = match &bundle.project.root {
                    ProjectRoot::Page(_) => format!("/project/root/widgets/{widget_index}"),
                    ProjectRoot::Dashboard(_) => {
                        format!("/project/root/pages/{page_index}/widgets/{widget_index}")
                    }
                };
                add(&mut result, "unknown", &location);
            }
        }
    }
    Ok(result)
}

fn add(result: &mut CandidateScan, kind: &'static str, location: &str) {
    if result.findings.len() < LIMIT {
        result.findings.push(Candidate {
            kind,
            location: location.to_owned(),
            evidence: match kind {
                "unknown" => "unanalysed",
                "metric_key" | "identifier" | "free_text" | "macro" => "structural",
                _ => "heuristic",
            },
        });
    } else {
        result.truncated = true;
    }
}

fn has_credential_marker(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let markers = [
        "password=",
        "passwd=",
        "api_key=",
        "apikey=",
        "token=",
        "password:",
        "passwd:",
        "api_key:",
        "apikey:",
        "token:",
        "\"password\":",
        "\"passwd\":",
        "\"api_key\":",
        "\"apikey\":",
        "\"token\":",
        "'password':",
        "'token':",
        "authorization: bearer ",
        "-----begin private key-----",
        "-----begin rsa private key-----",
        "-----begin openssh private key-----",
        "-----begin ec private key-----",
    ];
    if markers.iter().any(|marker| lower.contains(marker)) {
        return true;
    }
    lower.split_whitespace().any(|part| {
        part.split_once("://")
            .and_then(|(_, tail)| tail.split('/').next())
            .and_then(|authority| authority.rsplit_once('@'))
            .is_some_and(|(userinfo, _)| userinfo.contains(':'))
    })
}

fn visit(value: &Value, path: &str, result: &mut CandidateScan) {
    match value {
        Value::Object(fields) => {
            if path == "/catalogs" || path == "/project/roles" {
                for (index, (_, value)) in fields.iter().enumerate() {
                    if path == "/project/roles" {
                        add(result, "identifier", &format!("{path}/{index}/id"));
                    }
                    visit(value, &format!("{path}/{index}"), result);
                }
                return;
            }
            if fields.get("registry_state").and_then(Value::as_str) == Some("unknown") {
                result.unknown_objects = result.unknown_objects.saturating_add(1);
                add(result, "unknown", path);
            }
            if fields.get("kind").and_then(Value::as_str) == Some("key")
                && fields.get("value").is_some_and(Value::is_string)
            {
                add(result, "metric_key", &format!("{path}/value"));
            }
            if fields.get("kind").and_then(Value::as_str) == Some("alias")
                && fields.get("value").is_some_and(Value::is_string)
            {
                add(result, "identifier", &format!("{path}/value"));
            }
            if let (Some(name), Some(field_value)) = (
                fields.get("name").and_then(Value::as_str),
                fields.get("value").and_then(Value::as_object),
            ) && matches!(
                field_value.get("kind").and_then(Value::as_str),
                Some("string" | "dynamic_pattern")
            ) && field_value
                .get("value")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty())
            {
                let parts: Vec<_> = name.split('.').collect();
                let number = |value: &str| {
                    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
                };
                let item_pattern = match parts.as_slice() {
                    ["ds" | "or", dataset, "items", index] => number(dataset) && number(index),
                    ["columns", index, "item"] => number(index),
                    _ => false,
                };
                add(
                    result,
                    if item_pattern {
                        "metric_key"
                    } else {
                        "free_text"
                    },
                    &format!("{path}/value/value"),
                );
            }
            for (key, value) in fields {
                if matches!(
                    key.as_str(),
                    "name" | "title" | "description" | "display_name" | "template_name" | "units"
                ) && value.as_str().is_some_and(|value| !value.is_empty())
                {
                    add(result, "free_text", &format!("{path}/{key}"));
                }
                if matches!(
                    key.as_str(),
                    "template_uuid"
                        | "uuid"
                        | "id"
                        | "project_id"
                        | "catalog_id"
                        | "source_project_id"
                        | "source_dashboard_id"
                        | "source_page_id"
                ) && value.as_str().is_some_and(|value| !value.is_empty())
                {
                    add(result, "identifier", &format!("{path}/{key}"));
                }
                // Object maps may use user-provided catalog or role names as
                // keys. Never copy those names into a finding or log.
                let safe_key = match key.as_str() {
                    "project"
                    | "root"
                    | "pages"
                    | "widgets"
                    | "fields"
                    | "value"
                    | "name"
                    | "title"
                    | "display_name"
                    | "catalogs"
                    | "metrics"
                    | "source"
                    | "selector"
                    | "roles"
                    | "slots"
                    | "description"
                    | "template_name"
                    | "units"
                    | "export_format_version"
                    | "source_server_version"
                    | "template_uuid"
                    | "uuid"
                    | "id"
                    | "project_id"
                    | "catalog_id"
                    | "source_project_id"
                    | "source_dashboard_id"
                    | "source_page_id" => key.as_str(),
                    _ => "*",
                };
                visit(value, &format!("{path}/{safe_key}"), result);
            }
        }
        Value::Array(items) => {
            for (index, value) in items.iter().enumerate() {
                if path.starts_with("/project/roles/")
                    && path.ends_with("/slots")
                    && value.as_str().is_some_and(|value| !value.is_empty())
                {
                    add(result, "identifier", &format!("{path}/{index}"));
                }
                visit(value, &format!("{path}/{index}"), result);
            }
        }
        Value::String(value) => {
            if has_credential_marker(value) {
                result.credential_candidates = result.credential_candidates.saturating_add(1);
                add(result, "credential", path);
            }
            if value.contains("://") {
                add(result, "url", path);
            }
            if value.split_whitespace().any(|token| {
                token.contains('@')
                    && token
                        .split('@')
                        .nth(1)
                        .is_some_and(|part| part.contains('.'))
            }) {
                add(result, "email", path);
            }
            if value
                .split(|ch: char| !(ch.is_ascii_hexdigit() || ch == '.' || ch == ':'))
                .filter(|token| !token.is_empty())
                .any(|token| token.parse::<IpAddr>().is_ok())
            {
                add(result, "ip", path);
            }
            if value.contains("{$") || value.contains("{#") {
                add(result, "macro", path);
            }
            if value.contains('*') {
                add(result, "mask", path);
            }
            if value.split_whitespace().any(|token| {
                let token = token.trim_matches(|ch: char| {
                    matches!(ch, '(' | ')' | '[' | ']' | '"' | '\'' | ',' | ';')
                });
                token.starts_with('/')
                    || (token.len() >= 3
                        && token.as_bytes()[0].is_ascii_alphabetic()
                        && token.as_bytes()[1] == b':'
                        && matches!(token.as_bytes()[2], b'/' | b'\\'))
            }) {
                add(result, "path", path);
            }
            if value
                .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '.' || ch == '-'))
                .any(|token| {
                    let Some((head, tail)) = token.rsplit_once('.') else {
                        return false;
                    };
                    !head.is_empty()
                        && tail.len() >= 2
                        && tail.chars().all(|ch| ch.is_ascii_alphabetic())
                        && token.parse::<IpAddr>().is_err()
                })
            {
                add(result, "fqdn", path);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn candidates_do_not_return_values_and_limit_is_bounded() {
        assert!(has_credential_marker(r#"{"password":"synthetic"}"#));
        assert!(has_credential_marker(
            "https://example:synthetic@example.invalid/"
        ));
        assert!(has_credential_marker("-----BEGIN EC PRIVATE KEY-----"));
        assert!(!has_credential_marker("{$TOKEN}"));
        let mut result = CandidateScan {
            findings: Vec::new(),
            truncated: false,
            unknown_objects: 0,
            credential_candidates: 0,
        };
        let value = serde_json::json!({"name":"sensitive-label", "secret-key-example":["https://example.invalid/private", "name@example.invalid", "192.0.2.1", "{$TOKEN}", "password=never-display-this", "node-*", "/private/path", {"registry_state":"unknown"}, {"kind":"key", "value":"custom.metric"}, {"template_uuid":"0123456789abcdef0123456789abcdef"}]});
        visit(&value, "", &mut result);
        assert!(result.findings.iter().any(|item| item.kind == "url"));
        assert!(result.findings.iter().any(|item| item.kind == "email"));
        assert!(result.findings.iter().any(|item| item.kind == "ip"));
        assert!(result.findings.iter().any(|item| item.kind == "macro"));
        assert!(result.findings.iter().any(|item| item.kind == "unknown"));
        assert!(
            result
                .findings
                .iter()
                .any(|item| item.kind == "unknown" && item.evidence == "unanalysed")
        );
        assert!(
            result
                .findings
                .iter()
                .any(|item| item.kind == "credential" && item.evidence == "heuristic")
        );
        assert!(
            result
                .findings
                .iter()
                .any(|item| item.kind == "identifier" && item.evidence == "structural")
        );
        assert!(result.findings.iter().any(|item| item.kind == "credential"));
        assert!(result.findings.iter().any(|item| item.kind == "metric_key"));
        assert!(result.findings.iter().any(|item| item.kind == "path"));
        assert!(result.findings.iter().any(|item| item.kind == "mask"));
        assert!(result.findings.iter().any(|item| item.kind == "fqdn"));
        assert!(result.findings.iter().any(|item| item.kind == "identifier"));
        assert!(result.findings.iter().any(|item| item.kind == "free_text"));
        let output =
            serde_json::to_string(&result).unwrap_or_else(|error| panic!("serialize: {error}"));
        assert!(!output.contains("example.invalid"));
        assert!(!output.contains("secret-key-example"));
        assert!(!output.contains("never-display-this"));
        assert!(!output.contains("sensitive-label"));
        assert!(!output.contains("0123456789abcdef0123456789abcdef"));
        let mut widget = CandidateScan {
            findings: Vec::new(),
            truncated: false,
            unknown_objects: 0,
            credential_candidates: 0,
        };
        visit(
            &serde_json::json!({"name":"ds.0.items.0","value":{"kind":"string","value":"private.metric[*]"}}),
            "/widget",
            &mut widget,
        );
        assert!(
            widget
                .findings
                .iter()
                .any(|item| item.kind == "metric_key" && item.location == "/widget/value/value")
        );
        assert!(
            !serde_json::to_string(&widget)
                .unwrap_or_default()
                .contains("private.metric")
        );
        let mut catalogs = CandidateScan {
            findings: Vec::new(),
            truncated: false,
            unknown_objects: 0,
            credential_candidates: 0,
        };
        visit(
            &serde_json::json!({"private-z": {"metrics":[{"name":"private-value"}]}, "private-a": {"metrics":[{"name":"private-other"}]}}),
            "/catalogs",
            &mut catalogs,
        );
        assert!(
            catalogs
                .findings
                .iter()
                .any(|item| item.location == "/catalogs/0/metrics/0/name")
        );
        assert!(
            catalogs
                .findings
                .iter()
                .any(|item| item.location == "/catalogs/1/metrics/0/name")
        );
        assert!(
            !serde_json::to_string(&catalogs)
                .unwrap_or_default()
                .contains("private-")
        );
        let mut roles = CandidateScan {
            findings: Vec::new(),
            truncated: false,
            unknown_objects: 0,
            credential_candidates: 0,
        };
        visit(
            &serde_json::json!({"private-role":{"description":"private-description","slots":["private-slot"]}}),
            "/project/roles",
            &mut roles,
        );
        assert!(
            roles
                .findings
                .iter()
                .any(|item| item.location == "/project/roles/0/id")
        );
        assert!(
            roles
                .findings
                .iter()
                .any(|item| item.location == "/project/roles/0/slots/0")
        );
        assert!(
            !serde_json::to_string(&roles)
                .unwrap_or_default()
                .contains("private-")
        );
        let mut bounded = CandidateScan {
            findings: Vec::new(),
            truncated: false,
            unknown_objects: 0,
            credential_candidates: 0,
        };
        visit(
            &serde_json::json!(vec!["https://example.invalid"; LIMIT + 1]),
            "",
            &mut bounded,
        );
        assert_eq!(bounded.findings.len(), LIMIT);
        assert!(bounded.truncated);
        visit(
            &serde_json::json!({"registry_state":"unknown"}),
            "",
            &mut bounded,
        );
        assert_eq!(bounded.unknown_objects, 1);
        visit(
            &serde_json::json!("password=not-returned"),
            "",
            &mut bounded,
        );
        assert_eq!(bounded.credential_candidates, 1);
        assert_eq!(bounded.findings.len(), LIMIT);
    }
}
