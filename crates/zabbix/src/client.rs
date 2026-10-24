// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::transport::Transport;
use crate::{
    AdapterRegistry, InventoryMethod, InventoryProbe, MethodOutcome, Result,
    VersionedDashboardSnapshot, ZabbixCapabilities, ZabbixEndpoint, ZabbixError, ZabbixVersion,
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};

pub enum Authentication {
    ApiToken(SecretString),
    UsernamePassword {
        username: String,
        password: SecretString,
    },
}

pub struct ZabbixClient {
    transport: Transport,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct VersionDetector;

pub struct AuthenticatedClient {
    pub(crate) transport: Transport,
    pub(crate) token: SecretString,
    login_session: bool,
    capabilities: ZabbixCapabilities,
}

impl ZabbixClient {
    pub fn new(endpoint: ZabbixEndpoint) -> Result<Self> {
        Ok(Self {
            transport: Transport::new(endpoint)?,
        })
    }

    pub async fn detect_version(&self) -> Result<ZabbixVersion> {
        let raw: String = self
            .transport
            .call("apiinfo.version", json!([]), None)
            .await?;
        raw.parse()
    }

    pub async fn authenticate(self, authentication: Authentication) -> Result<AuthenticatedClient> {
        let capabilities = AdapterRegistry::resolve(VersionDetector::detect(&self).await?)?;
        let (token, login_session) = match authentication {
            Authentication::ApiToken(token) => (token, false),
            Authentication::UsernamePassword { username, password } => {
                let token: String = self
                    .transport
                    .call(
                        "user.login",
                        json!({ "username": username, "password": password.expose_secret() }),
                        None,
                    )
                    .await?;
                (SecretString::from(token), true)
            }
        };
        Ok(AuthenticatedClient {
            transport: self.transport,
            token,
            login_session,
            capabilities,
        })
    }
}

impl VersionDetector {
    pub async fn detect(client: &ZabbixClient) -> Result<ZabbixVersion> {
        client.detect_version().await
    }
}

impl AuthenticatedClient {
    pub fn capabilities(&self) -> &ZabbixCapabilities {
        &self.capabilities
    }

    pub async fn inventory(&self) -> Result<InventoryProbe> {
        Ok(InventoryProbe {
            template: self.probe(InventoryMethod::Template).await?,
            host: self.probe(InventoryMethod::Host).await?,
            item: self.probe(InventoryMethod::Item).await?,
            dashboard: self.probe(InventoryMethod::Dashboard).await?,
            graph: self.probe(InventoryMethod::Graph).await?,
        })
    }

    /// Read a bounded list of item identifiers for trusted application-side
    /// resolution. This is read-only and never returns item values or secrets.
    pub async fn readable_item_ids(&self, limit: usize) -> Result<Vec<String>> {
        if !(1..=1_000).contains(&limit) {
            return Err(ZabbixError::SnapshotShape(
                "item id read limit must be 1..=1000".to_owned(),
            ));
        }
        let raw: Vec<Value> = self
            .transport
            .call(
                "item.get",
                json!({"output": ["itemid"], "sortfield": "itemid", "limit": limit}),
                Some(&self.token),
            )
            .await?;
        raw.into_iter()
            .map(|item| {
                item.get("itemid")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        ZabbixError::SnapshotShape("item.get returned no string itemid".to_owned())
                    })
            })
            .collect()
    }

    pub async fn dashboard_snapshots(
        &self,
        connection_id: &str,
    ) -> Result<Vec<VersionedDashboardSnapshot>> {
        if connection_id.is_empty() || connection_id.len() > 128 {
            return Err(ZabbixError::SnapshotShape(
                "connection_id must contain 1..=128 bytes".to_owned(),
            ));
        }
        let raw: Vec<Value> = self
            .transport
            .call(
                "dashboard.get",
                json!({
                    "output": "extend",
                    "selectPages": "extend",
                    "selectUsers": "extend",
                    "selectUserGroups": "extend",
                    "sortfield": "dashboardid"
                }),
                Some(&self.token),
            )
            .await?;
        raw.into_iter()
            .map(|dashboard| {
                VersionedDashboardSnapshot::from_adapter_raw(
                    connection_id.to_owned(),
                    self.capabilities.version.clone(),
                    dashboard,
                )
            })
            .collect()
    }

    async fn probe(&self, method: InventoryMethod) -> Result<MethodOutcome> {
        match self
            .transport
            .call::<Value>(
                method.api_method(),
                inventory_params(method),
                Some(&self.token),
            )
            .await
        {
            Ok(value) => Ok(MethodOutcome::Available(value)),
            Err(error) if error.is_permission_denied() => Ok(MethodOutcome::PermissionDenied {
                method: method.api_method(),
                diagnostic: error.to_string(),
            }),
            Err(error) => Err(error),
        }
    }

    pub async fn logout(self) -> Result<()> {
        if self.login_session {
            let logged_out: bool = self
                .transport
                .call("user.logout", json!([]), Some(&self.token))
                .await?;
            if !logged_out {
                return Err(ZabbixError::Authentication(
                    "user.logout returned false".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

fn inventory_params(method: InventoryMethod) -> Value {
    match method {
        InventoryMethod::Template => json!({ "output": ["templateid", "host", "name"] }),
        InventoryMethod::Host => json!({ "output": ["hostid", "host", "name", "status"] }),
        InventoryMethod::Item => {
            json!({ "output": ["itemid", "hostid", "key_", "name", "value_type"], "limit": 1 })
        }
        InventoryMethod::Dashboard => {
            json!({ "output": ["dashboardid", "name", "userid", "private"], "selectPages": "extend" })
        }
        InventoryMethod::Graph => json!({ "output": ["graphid", "name"], "limit": 1 }),
    }
}
