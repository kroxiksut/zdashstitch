// SPDX-License-Identifier: MIT OR Apache-2.0

//! Version-scoped O-031 owner and sharing evidence for global dashboards.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{ReadMethod, ReadOnlyGateway, Result, is_zabbix_id, transport::hex_sha256};

const MAX_RECIPIENTS: usize = 500;
pub(crate) const CONTRACT: &str = "dashboard_access_8_0_0";

pub(crate) const fn exact_version(version: &crate::ZabbixVersion) -> bool {
    version.major == 8 && version.minor == 0 && version.patch == 0
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SharingPermission {
    ReadOnly,
    ReadWrite,
}

impl SharingPermission {
    const fn wire(self) -> &'static str {
        match self {
            Self::ReadOnly => "2",
            Self::ReadWrite => "3",
        }
    }

    fn parse(value: &Value) -> Option<Self> {
        match value.as_u64().or_else(|| value.as_str()?.parse().ok())? {
            2 => Some(Self::ReadOnly),
            3 => Some(Self::ReadWrite),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DashboardRecipient {
    pub id: String,
    pub permission: SharingPermission,
}

/// User selection only. Only the adapter can turn this into sendable proof.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DashboardAccessSelection {
    pub private: Option<bool>,
    pub owner: Option<String>,
    pub users: Option<Vec<DashboardRecipient>>,
    pub groups: Option<Vec<DashboardRecipient>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SharingChoice {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DashboardAccessChoices {
    pub can_choose_owner: bool,
    pub users: Vec<SharingChoice>,
    pub groups: Vec<SharingChoice>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SharingRole {
    pub(crate) roleid: String,
    pub(crate) super_admin: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DashboardAccessState {
    pub owner: String,
    pub private: bool,
    pub users: Vec<DashboardRecipient>,
    pub groups: Vec<DashboardRecipient>,
}

impl DashboardAccessState {
    pub(crate) fn read(raw: &Value) -> Option<Self> {
        let owner = raw["userid"].as_str().filter(|id| valid_id(id))?.to_owned();
        let private = match raw["private"]
            .as_u64()
            .or_else(|| raw["private"].as_str()?.parse().ok())?
        {
            0 => false,
            1 => true,
            _ => return None,
        };
        Some(Self {
            owner,
            private,
            users: read_recipients(&raw["users"], "userid")?,
            groups: read_recipients(&raw["userGroups"], "usrgrpid")?,
        })
    }

    pub(crate) fn bind_create_fingerprint(&self, content: &str) -> Option<String> {
        let access = serde_json::to_vec(self).ok()?;
        Some(hex_sha256(&[
            b"zdashstitch-create-access-v1",
            content.as_bytes(),
            &access,
        ]))
    }
}

impl crate::VersionedDashboardSnapshot {
    pub fn dashboard_access_state(&self) -> Option<DashboardAccessState> {
        exact_version(self.server_version())
            .then(|| DashboardAccessState::read(self.raw_api_document()))
            .flatten()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct VerifiedAccess {
    role: SharingRole,
    selection: DashboardAccessSelection,
    pub(crate) state: DashboardAccessState,
}

impl VerifiedAccess {
    pub(crate) async fn recheck(&self, reader: &ReadOnlyGateway) -> bool {
        matches!(reader.eight_zero_sharing_role().await, Ok(Some(role)) if role == self.role)
            && matches!(reader.recipients_visible(&self.selection).await, Ok(true))
            && matches!(reader.eight_zero_sharing_role().await, Ok(Some(role)) if role == self.role)
    }

    /// Apply only the explicitly selected wire fields. Omitted lists must
    /// stay omitted even when the account can see only part of them.
    pub(crate) fn apply(&self, params: &mut Value) {
        if self.selection.private.is_some() {
            params["private"] = json!(if self.state.private { "1" } else { "0" });
        }
        if self.selection.owner.is_some() {
            params["userid"] = json!(self.state.owner);
        }
        if self.selection.users.is_some() {
            params["users"] = wire_recipients(&self.state.users, "userid");
        }
        if self.selection.groups.is_some() {
            params["userGroups"] = wire_recipients(&self.state.groups, "usrgrpid");
        }
    }
}

fn valid_id(id: &str) -> bool {
    is_zabbix_id(id) && !id.starts_with('0')
}

fn normalize(recipients: &[DashboardRecipient]) -> Option<Vec<DashboardRecipient>> {
    // Sort by string to make wire order irrelevant to evidence and recovery.
    let unique = recipients
        .iter()
        .map(|entry| valid_id(&entry.id).then_some((&entry.id, entry.permission)))
        .collect::<Option<BTreeMap<_, _>>>()?;
    if unique.len() != recipients.len() {
        return None;
    }
    Some(
        unique
            .into_iter()
            .map(|(id, permission)| DashboardRecipient {
                id: id.clone(),
                permission,
            })
            .collect(),
    )
}

fn read_recipients(list: &Value, key: &str) -> Option<Vec<DashboardRecipient>> {
    let recipients = list
        .as_array()?
        .iter()
        .map(|row| {
            Some(DashboardRecipient {
                id: row[key].as_str()?.to_owned(),
                permission: SharingPermission::parse(&row["permission"])?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    normalize(&recipients)
}

fn wire_recipients(entries: &[DashboardRecipient], key: &str) -> Value {
    json!(
        entries
            .iter()
            .map(|entry| json!({(key): entry.id, "permission": entry.permission.wire()}))
            .collect::<Vec<_>>()
    )
}

fn choose_entries(
    selection: Option<&[DashboardRecipient]>,
    before: &[DashboardRecipient],
) -> Option<Vec<DashboardRecipient>> {
    match selection {
        Some(entries) if entries.len() <= MAX_RECIPIENTS => normalize(entries),
        Some(_) => None,
        None => Some(before.to_vec()),
    }
}

impl ReadOnlyGateway {
    pub async fn dashboard_access_choices(&self) -> Result<Option<DashboardAccessChoices>> {
        let Some(role) = self.eight_zero_sharing_role().await? else {
            return Ok(None);
        };
        let (users, users_truncated) = self
            .sharing_choices(ReadMethod::UserGet, "userid", "username")
            .await?;
        let (groups, groups_truncated) = self
            .sharing_choices(ReadMethod::UsergroupGet, "usrgrpid", "name")
            .await?;
        if self.eight_zero_sharing_role().await?.as_ref() != Some(&role) {
            return Ok(None);
        }
        Ok(Some(DashboardAccessChoices {
            can_choose_owner: role.super_admin,
            users,
            groups,
            truncated: users_truncated || groups_truncated,
        }))
    }

    async fn sharing_choices(
        &self,
        method: ReadMethod,
        key: &str,
        name: &str,
    ) -> Result<(Vec<SharingChoice>, bool)> {
        let rows = self
            .read_inventory_rows(
                method,
                json!({"output":[key,name],"limit":MAX_RECIPIENTS + 1}),
            )
            .await?;
        let truncated = rows.len() > MAX_RECIPIENTS;
        let choices = rows
            .into_iter()
            .take(MAX_RECIPIENTS)
            .filter_map(|row| {
                Some(SharingChoice {
                    id: row[key].as_str().filter(|id| valid_id(id))?.to_owned(),
                    name: row[name]
                        .as_str()
                        .filter(|name| !name.is_empty())?
                        .to_owned(),
                })
            })
            .collect();
        Ok((choices, truncated))
    }

    async fn visible_ids(
        &self,
        method: ReadMethod,
        key: &str,
        filter: &str,
        mut ids: Vec<&str>,
    ) -> Result<bool> {
        ids.sort_unstable();
        ids.dedup();
        if ids.is_empty() {
            return Ok(true);
        }
        let rows = self
            .read_inventory_rows(method, json!({"output":[key],(filter):ids}))
            .await?;
        let Some(mut visible) = rows
            .iter()
            .map(|row| row[key].as_str())
            .collect::<Option<Vec<_>>>()
        else {
            return Ok(false);
        };
        visible.sort_unstable();
        Ok(visible == ids)
    }

    async fn recipients_visible(&self, selection: &DashboardAccessSelection) -> Result<bool> {
        let userids = selection
            .users
            .iter()
            .flatten()
            .map(|entry| entry.id.as_str())
            .chain(selection.owner.as_deref())
            .collect();
        let groupids = selection
            .groups
            .iter()
            .flatten()
            .map(|entry| entry.id.as_str())
            .collect();
        Ok(self
            .visible_ids(ReadMethod::UserGet, "userid", "userids", userids)
            .await?
            && self
                .visible_ids(ReadMethod::UsergroupGet, "usrgrpid", "usrgrpids", groupids)
                .await?)
    }

    pub(crate) async fn verify_dashboard_access(
        &self,
        selection: &DashboardAccessSelection,
        before: Option<&DashboardAccessState>,
    ) -> Result<Option<VerifiedAccess>> {
        let Some(role) = self.eight_zero_sharing_role().await? else {
            return Ok(None);
        };
        // An Admin can read only part of an existing ACL. Sending either
        // list would replace hidden entries and could remove their own
        // write grant. Do not send userid even when it names the caller.
        if !role.super_admin
            && (selection.owner.is_some()
                || (before.is_some() && (selection.users.is_some() || selection.groups.is_some())))
        {
            return Ok(None);
        }
        let default = DashboardAccessState {
            owner: self.identity().account_userid().to_owned(),
            private: true,
            users: Vec::new(),
            groups: Vec::new(),
        };
        let is_update = before.is_some();
        let before = before.unwrap_or(&default);
        let owner = selection.owner.as_ref().unwrap_or(&before.owner);
        if !valid_id(owner) {
            return Ok(None);
        }
        let Some(users) = choose_entries(selection.users.as_deref(), &before.users) else {
            return Ok(None);
        };
        let Some(groups) = choose_entries(selection.groups.as_deref(), &before.groups) else {
            return Ok(None);
        };
        let state = DashboardAccessState {
            owner: owner.clone(),
            private: selection.private.unwrap_or(before.private),
            users,
            groups,
        };
        if is_update && &state == before {
            return Ok(None);
        }
        let mut selection = selection.clone();
        if is_update {
            selection.private = selection
                .private
                .filter(|_| state.private != before.private);
            selection.owner = selection.owner.filter(|_| state.owner != before.owner);
            selection.users = selection.users.filter(|_| state.users != before.users);
            selection.groups = selection.groups.filter(|_| state.groups != before.groups);
        }
        if !self.recipients_visible(&selection).await?
            || self.eight_zero_sharing_role().await?.as_ref() != Some(&role)
        {
            return Ok(None);
        }
        Ok(Some(VerifiedAccess {
            role,
            selection,
            state,
        }))
    }
}
