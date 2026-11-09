// SPDX-License-Identifier: MIT OR Apache-2.0

//! Process-local ownership of a local page editor. Never exported with a draft.

use crate::workcopy::WorkcopyError;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

const TTL: Duration = Duration::from_secs(90);
const MAX_SESSIONS: usize = 2_000;

struct Lease {
    token: String,
    expires: Instant,
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum EditAccess {
    Editing { token: String },
    ReadOnly,
}

#[derive(Default)]
pub struct PageEditSessions {
    leases: Mutex<BTreeMap<(PathBuf, String), Lease>>,
}

impl PageEditSessions {
    pub fn acquire(
        &self,
        root: PathBuf,
        id: String,
        take_over: bool,
    ) -> Result<EditAccess, WorkcopyError> {
        let mut leases = self.leases.lock().map_err(|_| failed())?;
        let now = Instant::now();
        leases.retain(|_, lease| lease.expires > now);
        let key = (root, id);
        if !take_over && leases.contains_key(&key) {
            return Ok(EditAccess::ReadOnly);
        }
        if !leases.contains_key(&key) && leases.len() >= MAX_SESSIONS {
            return Err(WorkcopyError::new("storage_limit_reached"));
        }
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| WorkcopyError::new("random_source_unavailable"))?;
        let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        leases.insert(
            key,
            Lease {
                token: token.clone(),
                expires: now + TTL,
            },
        );
        Ok(EditAccess::Editing { token })
    }

    /// The ownership guard covers the entire atomic update; a takeover cannot
    /// pass a checked writer before that writer commits.
    pub fn with_owner<T>(
        &self,
        root: PathBuf,
        id: String,
        token: Option<&str>,
        action: impl FnOnce() -> Result<T, WorkcopyError>,
    ) -> Result<T, WorkcopyError> {
        let leases = self.leases.lock().map_err(|_| failed())?;
        let lease = leases.get(&(root, id)).ok_or_else(lost)?;
        if lease.expires <= Instant::now() || token != Some(lease.token.as_str()) {
            return Err(lost());
        }
        action()
    }

    pub fn renew(&self, root: PathBuf, id: String, token: &str) -> Result<(), WorkcopyError> {
        let mut leases = self.leases.lock().map_err(|_| failed())?;
        let lease = leases.get_mut(&(root, id)).ok_or_else(lost)?;
        if lease.expires <= Instant::now() || lease.token != token {
            return Err(lost());
        }
        lease.expires = Instant::now() + TTL;
        Ok(())
    }

    pub fn release(&self, root: PathBuf, id: String, token: &str) -> Result<(), WorkcopyError> {
        let mut leases = self.leases.lock().map_err(|_| failed())?;
        let key = (root, id);
        if leases.get(&key).is_some_and(|lease| lease.token == token) {
            leases.remove(&key);
        }
        Ok(())
    }
}

fn failed() -> WorkcopyError {
    WorkcopyError::new("storage_unavailable")
}
fn lost() -> WorkcopyError {
    WorkcopyError::new("offline_page_edit_lost")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn token(access: EditAccess) -> String {
        match access {
            EditAccess::Editing { token } => token,
            _ => panic!("editing"),
        }
    }
    #[test]
    fn takeover_blocks_old_writer_and_expiry_does_not_reacquire_silently() {
        let sessions = PageEditSessions::default();
        let root = PathBuf::from("synthetic-root");
        let id = "pg-synthetic".to_owned();
        let first = token(
            sessions
                .acquire(root.clone(), id.clone(), false)
                .unwrap_or_else(|error| panic!("{error}")),
        );
        assert!(matches!(
            sessions.acquire(root.clone(), id.clone(), false),
            Ok(EditAccess::ReadOnly)
        ));
        let second = token(
            sessions
                .acquire(root.clone(), id.clone(), true)
                .unwrap_or_else(|error| panic!("{error}")),
        );
        let mut wrote = false;
        assert!(
            sessions
                .with_owner(root.clone(), id.clone(), Some(&first), || {
                    wrote = true;
                    Ok(())
                })
                .is_err()
        );
        assert!(!wrote);
        assert!(sessions.renew(root.clone(), id.clone(), &first).is_err());
        assert!(
            sessions
                .with_owner(root.clone(), id.clone(), Some(&second), || Ok(()))
                .is_ok()
        );
        sessions
            .leases
            .lock()
            .unwrap_or_else(|error| panic!("{error}"))
            .get_mut(&(root.clone(), id.clone()))
            .unwrap_or_else(|| panic!("lease"))
            .expires = Instant::now() - Duration::from_secs(1);
        assert!(sessions.renew(root.clone(), id.clone(), &second).is_err());
        assert!(
            sessions
                .with_owner(root.clone(), id.clone(), Some(&second), || Ok(()))
                .is_err()
        );
        assert!(matches!(
            sessions.acquire(root.clone(), id.clone(), false),
            Ok(EditAccess::Editing { .. })
        ));
    }
    #[test]
    fn release_from_a_revoked_tab_never_releases_the_new_owner() {
        let sessions = PageEditSessions::default();
        let root = PathBuf::from("synthetic-root");
        let id = "pg-synthetic".to_owned();
        let first = token(
            sessions
                .acquire(root.clone(), id.clone(), false)
                .unwrap_or_else(|error| panic!("{error}")),
        );
        let second = token(
            sessions
                .acquire(root.clone(), id.clone(), true)
                .unwrap_or_else(|error| panic!("{error}")),
        );
        assert!(sessions.release(root.clone(), id.clone(), &first).is_ok());
        assert!(
            sessions
                .with_owner(root.clone(), id.clone(), Some(&second), || Ok(()))
                .is_ok()
        );
        assert!(sessions.release(root.clone(), id.clone(), &second).is_ok());
        assert!(matches!(
            sessions.acquire(root, id, false),
            Ok(EditAccess::Editing { .. })
        ));
    }
}
