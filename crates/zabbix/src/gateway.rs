// SPDX-License-Identifier: MIT OR Apache-2.0

//! Access-mode gateways (O-013, P2-01E).
//!
//! `ReadOnlyGateway` is a separate type, not a flag: it has no method that
//! sends a write, and the transport only accepts write methods together with
//! a [`WriteGrant`] that exists solely inside a `ReadWriteGateway`. Every
//! method name that can reach the server is an enum variant, so the allowlist
//! is closed at compile time.
//!
//! ```compile_fail
//! async fn no_write_path(
//!     gateway: &zdashstitch_zabbix::ReadOnlyGateway,
//!     update: &zdashstitch_zabbix::VerifiedDashboardUpdate,
//! ) {
//!     gateway.publish_verified_dashboard_update(update, || true).await;
//! }
//! ```
//!
//! ```compile_fail
//! fn no_publication_authority(gateway: &zdashstitch_zabbix::ReadOnlyGateway) {
//!     let _ = gateway.publication_context();
//! }
//! ```
//!
//! Control: the same calls compile on a `ReadWriteGateway`, so the two
//! failures above come from the missing methods, not from the snippets.
//!
//! ```no_run
//! async fn write_path(
//!     gateway: &zdashstitch_zabbix::ReadWriteGateway,
//!     update: &zdashstitch_zabbix::VerifiedDashboardUpdate,
//! ) {
//!     let _ = gateway.publication_context();
//!     let _ = gateway.publish_verified_dashboard_update(update, || true).await;
//! }
//! ```

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use secrecy::SecretString;
use serde_json::{Value, json};

use crate::taxonomy::{Failure, FailureCode};
use crate::transport::{Transport, hex_sha256};
use crate::{
    InventoryMethod, InventoryProbe, MethodOutcome, Result, VersionedDashboardSnapshot,
    ZabbixCapabilities, ZabbixEndpoint, ZabbixError, ZabbixVersion,
};

/// Unauthenticated and session-management methods.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SessionMethod {
    ApiInfoVersion,
    UserLogin,
    UserLogout,
    UserCheckAuthentication,
}

impl SessionMethod {
    pub const ALL: [Self; 4] = [
        Self::ApiInfoVersion,
        Self::UserLogin,
        Self::UserLogout,
        Self::UserCheckAuthentication,
    ];

    #[must_use]
    pub const fn api_method(self) -> &'static str {
        match self {
            Self::ApiInfoVersion => "apiinfo.version",
            Self::UserLogin => "user.login",
            Self::UserLogout => "user.logout",
            Self::UserCheckAuthentication => "user.checkAuthentication",
        }
    }
}

/// Read methods allowed by P1-IN-08.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ReadMethod {
    HostgroupGet,
    TemplategroupGet,
    HostGet,
    TemplateGet,
    ItemGet,
    GraphGet,
    ItemPrototypeGet,
    GraphPrototypeGet,
    DashboardGet,
    UserGet,
    UsergroupGet,
    ConfigurationExport,
}

impl ReadMethod {
    pub const ALL: [Self; 12] = [
        Self::HostgroupGet,
        Self::TemplategroupGet,
        Self::HostGet,
        Self::TemplateGet,
        Self::ItemGet,
        Self::GraphGet,
        Self::ItemPrototypeGet,
        Self::GraphPrototypeGet,
        Self::DashboardGet,
        Self::UserGet,
        Self::UsergroupGet,
        Self::ConfigurationExport,
    ];

    /// Methods probed by the capability step: every `get` with a cheap,
    /// bounded query. `configuration.export` needs object IDs and returns an
    /// empty export instead of an error when access is missing (P1-IN-08).
    pub const PROBED: [Self; 9] = [
        Self::HostgroupGet,
        Self::TemplategroupGet,
        Self::HostGet,
        Self::TemplateGet,
        Self::ItemGet,
        Self::GraphGet,
        Self::DashboardGet,
        Self::UserGet,
        Self::UsergroupGet,
    ];

    #[must_use]
    pub const fn api_method(self) -> &'static str {
        match self {
            Self::HostgroupGet => "hostgroup.get",
            Self::TemplategroupGet => "templategroup.get",
            Self::HostGet => "host.get",
            Self::TemplateGet => "template.get",
            Self::ItemGet => "item.get",
            Self::GraphGet => "graph.get",
            Self::ItemPrototypeGet => "itemprototype.get",
            Self::GraphPrototypeGet => "graphprototype.get",
            Self::DashboardGet => "dashboard.get",
            Self::UserGet => "user.get",
            Self::UsergroupGet => "usergroup.get",
            Self::ConfigurationExport => "configuration.export",
        }
    }

    fn probe_params(self) -> Value {
        let id_field = match self {
            Self::HostgroupGet | Self::TemplategroupGet => "groupid",
            Self::HostGet => "hostid",
            Self::TemplateGet => "templateid",
            Self::ItemGet => "itemid",
            Self::GraphGet => "graphid",
            Self::ItemPrototypeGet => "itemid",
            Self::GraphPrototypeGet => "graphid",
            Self::DashboardGet => "dashboardid",
            Self::UserGet => "userid",
            Self::UsergroupGet => "usrgrpid",
            Self::ConfigurationExport => return json!({}),
        };
        json!({ "output": [id_field], "limit": 1 })
    }
}

/// Write methods. Only the closed publication path uses them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WriteMethod {
    DashboardUpdate,
    /// Closed create path (`create.rs`); unreachable while the P2-06A
    /// contracts are not `Supported`.
    DashboardCreate,
}

impl WriteMethod {
    pub(crate) const fn api_method(self) -> &'static str {
        match self {
            Self::DashboardUpdate => "dashboard.update",
            Self::DashboardCreate => "dashboard.create",
        }
    }
}

/// Every method name that a `ReadOnlyGateway` session can send (P1-IN-08).
#[must_use]
pub fn read_only_allowlist() -> Vec<&'static str> {
    SessionMethod::ALL
        .iter()
        .map(|method| method.api_method())
        .chain(ReadMethod::ALL.iter().map(|method| method.api_method()))
        .chain([crate::create_permission::OWN_ROLE_METHOD])
        .collect()
}

/// Every write method name a `ReadWriteGateway` can send.
#[must_use]
pub fn dashboard_write_allowlist() -> Vec<&'static str> {
    vec![
        WriteMethod::DashboardUpdate.api_method(),
        WriteMethod::DashboardCreate.api_method(),
    ]
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AccessMode {
    ReadOnly,
    ReadWrite,
}

impl AccessMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::ReadWrite => "read_write",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AuthMethod {
    ApiToken,
    Password,
}

impl AuthMethod {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ApiToken => "api_token",
            Self::Password => "password",
        }
    }
}

/// Server identity observed after sign-in. Zabbix 7.4 exposes no server
/// UUID to ordinary users, so the identity binds the normalized API URL to
/// the account (`userid` for this user name). A cloned server with the same
/// account IDs at the same address is indistinguishable.
#[derive(Clone, Eq, PartialEq)]
pub struct ServerIdentity {
    endpoint_fingerprint: String,
    fingerprint: String,
    version: ZabbixVersion,
    account_userid: String,
}

impl ServerIdentity {
    pub(crate) fn new(
        endpoint: &ZabbixEndpoint,
        version: ZabbixVersion,
        userid: &str,
        username: &str,
    ) -> Self {
        Self {
            endpoint_fingerprint: endpoint.fingerprint(),
            fingerprint: hex_sha256(&[
                b"zdashstitch-server-identity-v1",
                endpoint.api_url().as_str().as_bytes(),
                username.as_bytes(),
                userid.as_bytes(),
            ]),
            version,
            account_userid: userid.to_owned(),
        }
    }

    /// Zabbix `userid` of the signed-in account, a server ID kept in memory
    /// only. It tells the account's own dashboards from others' (P1-03B).
    #[must_use]
    pub fn account_userid(&self) -> &str {
        &self.account_userid
    }

    #[must_use]
    pub fn endpoint_fingerprint(&self) -> &str {
        &self.endpoint_fingerprint
    }

    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    #[must_use]
    pub fn version(&self) -> &ZabbixVersion {
        &self.version
    }
}

impl fmt::Debug for ServerIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerIdentity")
            .field("fingerprint", &self.fingerprint)
            .field("version", &self.version.to_string())
            .finish()
    }
}

/// Result of probing one read method.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadAccess {
    Available,
    Denied,
}

impl ReadAccess {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Denied => "denied",
        }
    }
}

pub(crate) struct Session {
    pub(crate) transport: Transport,
    pub(crate) token: SecretString,
    pub(crate) login_session: bool,
    pub(crate) capabilities: ZabbixCapabilities,
    pub(crate) identity: ServerIdentity,
    pub(crate) auth_method: AuthMethod,
}

/// Authenticated read-only connection. It cannot write.
pub struct ReadOnlyGateway {
    pub(crate) session: Session,
}

impl fmt::Debug for ReadOnlyGateway {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReadOnlyGateway")
            .field("identity", &self.session.identity)
            .field("auth_method", &self.session.auth_method.as_str())
            .finish_non_exhaustive()
    }
}

impl ReadOnlyGateway {
    #[must_use]
    pub const fn mode(&self) -> AccessMode {
        AccessMode::ReadOnly
    }

    pub fn capabilities(&self) -> &ZabbixCapabilities {
        &self.session.capabilities
    }

    pub fn identity(&self) -> &ServerIdentity {
        &self.session.identity
    }

    pub fn endpoint(&self) -> &ZabbixEndpoint {
        self.session.transport.endpoint()
    }

    #[must_use]
    pub fn auth_method(&self) -> AuthMethod {
        self.session.auth_method
    }

    /// Generic allowlisted read for application use cases.
    pub async fn read(&self, method: ReadMethod, params: Value) -> Result<Value> {
        self.session
            .transport
            .read(method, params, &self.session.token)
            .await
    }

    /// Capability step (P1-02D step 5): a role without access to a method
    /// returns `No permissions to call`; data restrictions return less data,
    /// never an error (P1-IN-08). Authentication failures abort the probe.
    pub async fn probe_read_access(&self) -> Result<Vec<(ReadMethod, ReadAccess)>> {
        let mut outcomes = Vec::with_capacity(ReadMethod::PROBED.len());
        for method in ReadMethod::PROBED {
            let access = match self.read(method, method.probe_params()).await {
                Ok(_) => ReadAccess::Available,
                Err(error) if error.is_permission_denied() => ReadAccess::Denied,
                Err(error) => return Err(error),
            };
            outcomes.push((method, access));
        }
        Ok(outcomes)
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
            .session
            .transport
            .read(
                ReadMethod::ItemGet,
                json!({"output": ["itemid"], "sortfield": "itemid", "limit": limit}),
                &self.session.token,
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
            .session
            .transport
            .read(
                ReadMethod::DashboardGet,
                crate::dashboards::snapshot_read_params(),
                &self.session.token,
            )
            .await?;
        raw.into_iter()
            .map(|dashboard| {
                VersionedDashboardSnapshot::from_adapter_raw(
                    connection_id.to_owned(),
                    self.session.capabilities.version.clone(),
                    dashboard,
                )
            })
            .collect()
    }

    async fn probe(&self, method: InventoryMethod) -> Result<MethodOutcome> {
        match self
            .read(method.read_method(), inventory_params(method))
            .await
        {
            Ok(value) => Ok(MethodOutcome::Available(value)),
            Err(error) if error.is_permission_denied() => Ok(MethodOutcome::PermissionDenied {
                method: method.api_method(),
                diagnostic: error.failure().code.as_str().to_owned(),
            }),
            Err(error) => Err(error),
        }
    }

    /// Switches to read-write after the user's explicit confirmation in the
    /// application. Each switch opens a new write epoch, so publication plans
    /// built before a return to read-only can never be sent.
    #[must_use]
    pub fn enable_writes(self) -> ReadWriteGateway {
        ReadWriteGateway {
            reader: self,
            grant: WriteGrant {
                epoch: WRITE_EPOCH.fetch_add(1, Ordering::Relaxed) + 1,
            },
        }
    }

    /// Ends a `user.login` session with `user.logout`; an API token stays
    /// valid on the server. The secret is erased from memory on drop.
    pub async fn logout(self) -> Result<()> {
        if !self.session.login_session {
            return Ok(());
        }
        let logged_out: bool = self
            .session
            .transport
            .session(
                SessionMethod::UserLogout,
                json!([]),
                Some(&self.session.token),
            )
            .await?;
        if logged_out {
            Ok(())
        } else {
            Err(ZabbixError::SignIn(
                Failure::new(FailureCode::ApiError)
                    .with("method", SessionMethod::UserLogout.api_method()),
            ))
        }
    }
}

static WRITE_EPOCH: AtomicU64 = AtomicU64::new(0);

/// Proof of an explicit switch to read-write. Only this module creates it.
pub(crate) struct WriteGrant {
    epoch: u64,
}

impl WriteGrant {
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }
}

/// Read-write connection: everything a reader can do plus the closed,
/// `VerifiedPlan`-bound dashboard publication path (see `write.rs`).
pub struct ReadWriteGateway {
    reader: ReadOnlyGateway,
    pub(crate) grant: WriteGrant,
}

impl fmt::Debug for ReadWriteGateway {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReadWriteGateway")
            .field("reader", &self.reader)
            .finish_non_exhaustive()
    }
}

impl ReadWriteGateway {
    #[must_use]
    pub const fn mode(&self) -> AccessMode {
        AccessMode::ReadWrite
    }

    pub fn reader(&self) -> &ReadOnlyGateway {
        &self.reader
    }

    pub fn capabilities(&self) -> &ZabbixCapabilities {
        self.reader.capabilities()
    }

    pub async fn dashboard_snapshots(
        &self,
        connection_id: &str,
    ) -> Result<Vec<VersionedDashboardSnapshot>> {
        self.reader.dashboard_snapshots(connection_id).await
    }

    /// Returns to read-only; the write epoch is dropped with the grant.
    #[must_use]
    pub fn into_read_only(self) -> ReadOnlyGateway {
        self.reader
    }

    pub async fn logout(self) -> Result<()> {
        self.reader.logout().await
    }

    pub(crate) fn session(&self) -> &Session {
        &self.reader.session
    }

    /// Unit tests only: marks laboratory contracts as proven on a mock
    /// transport. Not compiled into any other build.
    #[cfg(test)]
    pub(crate) fn set_write_contracts_for_test(
        &mut self,
        contracts: crate::DashboardWriteContracts,
    ) {
        self.reader.session.capabilities.dashboard_write_contracts = contracts;
    }

    /// Unit tests only: marks `dashboard.create` contracts as proven on a
    /// mock transport. Not compiled into any other build.
    #[cfg(test)]
    pub(crate) fn set_create_contracts_for_test(
        &mut self,
        contracts: crate::DashboardCreateContracts,
    ) {
        self.reader.session.capabilities.dashboard_create_contracts = contracts;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_allowlist_is_p1_in_08_plus_scoped_own_role() {
        let mut actual = read_only_allowlist();
        actual.sort_unstable();
        let mut expected = vec![
            "apiinfo.version",
            "user.login",
            "user.logout",
            "user.checkAuthentication",
            "hostgroup.get",
            "templategroup.get",
            "host.get",
            "template.get",
            "item.get",
            "graph.get",
            "itemprototype.get",
            "graphprototype.get",
            "dashboard.get",
            "user.get",
            "usergroup.get",
            "configuration.export",
            "role.get",
        ];
        expected.sort_unstable();
        assert_eq!(actual, expected);
        assert!(
            ReadMethod::ALL
                .iter()
                .all(|method| method.api_method() != "role.get")
        );
        assert!(
            actual
                .iter()
                .all(|method| method.ends_with(".get") || !method.contains("create"))
        );
        assert_eq!(
            dashboard_write_allowlist(),
            vec!["dashboard.update", "dashboard.create"]
        );
    }
}
