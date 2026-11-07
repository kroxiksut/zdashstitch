// SPDX-License-Identifier: MIT OR Apache-2.0

use secrecy::ExposeSecret;
use serde_json::{Value, json};

use crate::{ReadOnlyGateway, Result, SessionMethod, is_zabbix_id};

pub(crate) const OWN_ROLE_METHOD: &str = "role.get";

#[derive(Eq, PartialEq)]
struct OwnRole {
    id: String,
    kind: u64,
}

fn integer(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

fn method_component(part: &str) -> bool {
    part == "*"
        || (!part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))
}

fn allows_create(rules: &Value) -> bool {
    if integer(&rules["api.access"]) != Some(1) {
        return false;
    }
    let Some(mode @ (0 | 1)) = integer(&rules["api.mode"]) else {
        return false;
    };
    let Some(methods) = rules["api"].as_array() else {
        return false;
    };
    let Some(actions) = rules["actions"].as_array() else {
        return false;
    };
    let mut edit = None;
    for action in actions {
        let Some(name) = action["name"].as_str().filter(|name| !name.is_empty()) else {
            return false;
        };
        let Some(status @ (0 | 1)) = integer(&action["status"]) else {
            return false;
        };
        if name == "edit_dashboards" && edit.replace(status).is_some() {
            return false;
        }
    }
    if edit != Some(1) {
        return false;
    }
    let mut matches = false;
    for entry in methods {
        let Some(entry) = entry.as_str() else {
            return false;
        };
        if entry != "*"
            && !entry.split_once('.').is_some_and(|(service, method)| {
                method_component(service) && method_component(method)
            })
        {
            return false;
        }
        matches |= matches!(
            entry,
            "*" | "*.*" | "dashboard.*" | "*.create" | "dashboard.create"
        );
    }
    // Zabbix 7.4.16 CLocalApiClient::isAllowedMethod permits an empty
    // method list in either mode after the API and action checks.
    methods.is_empty() || matches == (mode == 1)
}

impl ReadOnlyGateway {
    async fn own_role(&self) -> Result<Option<OwnRole>> {
        let credential = self.session.token.expose_secret();
        let params = if self.session.login_session {
            json!({"sessionid": credential})
        } else {
            json!({"token": credential})
        };
        let identity: Value = self
            .session
            .transport
            .session(SessionMethod::UserCheckAuthentication, params, None)
            .await?;
        Ok(identity["roleid"]
            .as_str()
            .filter(|id| is_zabbix_id(id))
            .and_then(|id| {
                let kind = integer(&identity["type"])?;
                (identity["userid"].as_str() == Some(self.identity().account_userid())
                    && matches!(kind, 1..=3))
                .then(|| OwnRole {
                    id: id.to_owned(),
                    kind,
                })
            }))
    }

    /// Reads only the current credential's own role. A positive result
    /// requires the complete rules and matching identity before and after
    /// the read. The role or its rules are never returned to the caller.
    pub async fn dashboard_create_permission(&self) -> Result<bool> {
        let version = &self.capabilities().version;
        if !version.is_supported_7_4() || version.patch != 16 {
            return Ok(false);
        }
        let live: String = self
            .session
            .transport
            .session(SessionMethod::ApiInfoVersion, json!([]), None)
            .await?;
        if live.parse::<crate::ZabbixVersion>()? != *version {
            return Ok(false);
        }
        let Some(role) = self.own_role().await? else {
            return Ok(false);
        };
        let rows = self
            .session
            .transport
            .read_own_role_rules(&role.id, &self.session.token)
            .await?;
        let [row] = rows.as_slice() else {
            return Ok(false);
        };
        if row["roleid"].as_str() != Some(&role.id)
            || integer(&row["type"]) != Some(role.kind)
            || !allows_create(&row["rules"])
        {
            return Ok(false);
        }
        Ok(self.own_role().await?.as_ref() == Some(&role))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_permission_requires_the_api_method_and_dashboard_action() {
        let base = json!({"api.access": "1", "api.mode": "0", "api": [],
            "actions": [{"name": "edit_dashboards", "status": "1"}]});
        assert!(allows_create(&base));
        for mask in ["*", "*.*", "dashboard.*", "*.create", "dashboard.create"] {
            let mut rules = base.clone();
            rules["api"] = json!([mask]);
            assert!(!allows_create(&rules), "{mask} in a deny list");
            rules["api.mode"] = json!(1);
            assert!(allows_create(&rules), "{mask} in an allow list");
        }
        for (field, value) in [
            ("api.access", json!(0)),
            ("api.mode", json!(2)),
            ("api", json!(["dashboard.cre*"])),
            ("api", json!([null])),
            ("actions", json!([])),
            ("actions", json!([{"name": "edit_dashboards", "status": 0}])),
            (
                "actions",
                json!([
                    {"name": "edit_dashboards", "status": 1},
                    {"name": "edit_dashboards", "status": 1}
                ]),
            ),
        ] {
            let mut rules = base.clone();
            rules[field] = value;
            assert!(!allows_create(&rules), "{field}");
        }
        let mut rules = base;
        rules["api.mode"] = json!("1");
        rules["api"] = json!(["dashboard.get"]);
        assert!(!allows_create(&rules));
    }
}
