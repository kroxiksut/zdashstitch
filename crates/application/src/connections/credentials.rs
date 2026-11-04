// SPDX-License-Identifier: MIT OR Apache-2.0

//! Credential store (P2-01D). An API token or, since O-037, a password is
//! stored only when the user chooses so for the profile. Store errors are
//! reduced to three states and never carry platform messages or secret
//! bytes.

use std::collections::HashMap;
use std::sync::Mutex;

use secrecy::{ExposeSecret, SecretString};

/// Service name of every entry; the account part is the profile's opaque
/// `credential_ref`, which reveals neither server nor user.
pub const CREDENTIAL_SERVICE: &str = "ZDashstitch";

/// Why a stored secret is not available.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CredentialState {
    /// No entry exists (never saved or forgotten).
    Missing,
    /// The store exists but is locked or access was denied.
    Locked,
    /// No usable store on this system or the store failed.
    Unavailable,
}

impl CredentialState {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Missing => "credential_missing",
            Self::Locked => "credential_locked",
            Self::Unavailable => "credential_unavailable",
        }
    }
}

pub trait CredentialStore: Send + Sync {
    /// Whether the store can be used at all.
    fn availability(&self) -> Result<(), CredentialState>;
    fn save(&self, reference: &str, secret: &SecretString) -> Result<(), CredentialState>;
    fn load(&self, reference: &str) -> Result<SecretString, CredentialState>;
    /// Removes the local entry. A missing entry is not an error. This never
    /// revokes the token on the Zabbix server.
    fn delete(&self, reference: &str) -> Result<(), CredentialState>;
}

/// Operating-system store through `keyring` 4: Windows Credential Manager,
/// Secret Service over pure-Rust D-Bus on Linux.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemCredentialStore;

impl SystemCredentialStore {
    fn entry(reference: &str) -> Result<keyring::Entry, CredentialState> {
        keyring::Entry::new(CREDENTIAL_SERVICE, reference).map_err(map_keyring_error)
    }
}

impl CredentialStore for SystemCredentialStore {
    fn availability(&self) -> Result<(), CredentialState> {
        keyring::Entry::store_status()
            .as_ref()
            .map(|_| ())
            .map_err(|_| CredentialState::Unavailable)
    }

    fn save(&self, reference: &str, secret: &SecretString) -> Result<(), CredentialState> {
        Self::entry(reference)?
            .set_password(secret.expose_secret())
            .map_err(map_keyring_error)
    }

    fn load(&self, reference: &str) -> Result<SecretString, CredentialState> {
        Self::entry(reference)?
            .get_password()
            .map(SecretString::from)
            .map_err(map_keyring_error)
    }

    fn delete(&self, reference: &str) -> Result<(), CredentialState> {
        match Self::entry(reference)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(map_keyring_error(error)),
        }
    }
}

/// Reduces a store error to a state. The error is dropped here: some
/// variants carry raw secret bytes and platform text.
fn map_keyring_error(error: keyring::Error) -> CredentialState {
    match error {
        keyring::Error::NoEntry => CredentialState::Missing,
        keyring::Error::NoStorageAccess(_) => CredentialState::Locked,
        _ => CredentialState::Unavailable,
    }
}

/// In-memory store for tests. `fail_with` simulates a locked or missing
/// system store.
#[derive(Default)]
pub struct MemoryCredentialStore {
    entries: Mutex<HashMap<String, SecretString>>,
    failure: Mutex<Option<CredentialState>>,
}

impl MemoryCredentialStore {
    pub fn fail_with(&self, state: Option<CredentialState>) {
        *self
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = state;
    }

    #[must_use]
    pub fn contains(&self, reference: &str) -> bool {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(reference)
    }

    fn check(&self) -> Result<(), CredentialState> {
        match *self
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            Some(state) => Err(state),
            None => Ok(()),
        }
    }
}

impl std::fmt::Debug for MemoryCredentialStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MemoryCredentialStore")
            .finish_non_exhaustive()
    }
}

impl CredentialStore for MemoryCredentialStore {
    fn availability(&self) -> Result<(), CredentialState> {
        self.check()
    }

    fn save(&self, reference: &str, secret: &SecretString) -> Result<(), CredentialState> {
        self.check()?;
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                reference.to_owned(),
                SecretString::from(secret.expose_secret().to_owned()),
            );
        Ok(())
    }

    fn load(&self, reference: &str) -> Result<SecretString, CredentialState> {
        self.check()?;
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(reference)
            .map(|secret| SecretString::from(secret.expose_secret().to_owned()))
            .ok_or(CredentialState::Missing)
    }

    fn delete(&self, reference: &str) -> Result<(), CredentialState> {
        self.check()?;
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(reference);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyring_errors_map_to_three_states_without_payload() {
        assert_eq!(
            map_keyring_error(keyring::Error::NoEntry),
            CredentialState::Missing
        );
        assert_eq!(
            map_keyring_error(keyring::Error::BadEncoding(b"secret-bytes".to_vec())),
            CredentialState::Unavailable
        );
        assert_eq!(
            map_keyring_error(keyring::Error::NoDefaultStore),
            CredentialState::Unavailable
        );
        assert_eq!(
            map_keyring_error(keyring::Error::TooLong("user".to_owned(), 10)),
            CredentialState::Unavailable
        );
    }

    #[test]
    fn memory_store_round_trip_and_failure_states() {
        let store = MemoryCredentialStore::default();
        let reference = "cred-00000000000000000000000000000001";
        assert_eq!(store.load(reference).err(), Some(CredentialState::Missing));
        store
            .save(reference, &SecretString::from("token-value".to_owned()))
            .unwrap_or_else(|state| panic!("save: {}", state.code()));
        assert_eq!(
            store
                .load(reference)
                .map(|secret| secret.expose_secret().to_owned())
                .ok()
                .as_deref(),
            Some("token-value")
        );
        assert!(!format!("{store:?}").contains("token-value"));
        store.fail_with(Some(CredentialState::Locked));
        assert_eq!(store.load(reference).err(), Some(CredentialState::Locked));
        store.fail_with(None);
        store
            .delete(reference)
            .unwrap_or_else(|state| panic!("delete: {}", state.code()));
        store
            .delete(reference)
            .unwrap_or_else(|state| panic!("second delete: {}", state.code()));
        assert!(!store.contains(reference));
    }

    /// Writes, reads and deletes one throw-away entry in the real system
    /// store of this machine. Opt-in because CI has no unlocked store.
    #[test]
    #[ignore = "touches the operating-system credential store; set ZDASHSTITCH_CREDENTIAL_STORE_ACK=1"]
    fn system_store_round_trip() {
        assert_eq!(
            std::env::var("ZDASHSTITCH_CREDENTIAL_STORE_ACK").as_deref(),
            Ok("1")
        );
        let store = SystemCredentialStore;
        store
            .availability()
            .unwrap_or_else(|state| panic!("store: {}", state.code()));
        let reference = "cred-p201selftest00000000000000000";
        let _ = store.delete(reference);
        assert_eq!(store.load(reference).err(), Some(CredentialState::Missing));
        store
            .save(reference, &SecretString::from("p201-self-test".to_owned()))
            .unwrap_or_else(|state| panic!("save: {}", state.code()));
        let loaded = store
            .load(reference)
            .unwrap_or_else(|state| panic!("load: {}", state.code()));
        assert_eq!(loaded.expose_secret(), "p201-self-test");
        store
            .delete(reference)
            .unwrap_or_else(|state| panic!("delete: {}", state.code()));
        assert_eq!(store.load(reference).err(), Some(CredentialState::Missing));
        println!("system credential store: save/load/delete ok");
    }
}
