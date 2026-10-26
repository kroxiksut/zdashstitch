// SPDX-License-Identifier: MIT OR Apache-2.0

//! Atomic, secret-free storage of connection profiles and trusted CA
//! material (P2-01H, P2-01C). Both files live in the application
//! configuration directory and are written through a temporary file, `fsync`
//! and rename, without following links.

use std::io::Read;
use std::path::PathBuf;

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use serde::{Deserialize, Serialize};
use zdashstitch_zabbix::TrustAnchors;

use super::ConnectionError;
use super::profile::{ConnectionProfile, is_ca_reference};
use crate::settings::{
    SettingsError, cap_metadata_is_reparse, check_existing_components,
    ensure_directory_without_links, reject_link_if_present, remove_stale_temporaries, replace_file,
    sync_directory,
};

const PROFILES_FILE: &str = "connections.yaml";
const PROFILES_FORMAT: &str = "zdashstitch-connections/v0";
const MAX_PROFILES_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_PROFILES: usize = 256;
const TRUST_DIRECTORY: &str = "trusted-ca";
const MAX_TRUST_FILE_BYTES: usize = 512 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProfilesFile {
    format: String,
    profiles: Vec<ConnectionProfile>,
}

fn storage_error(error: SettingsError) -> ConnectionError {
    match error {
        SettingsError::LinkedPath => ConnectionError::new("profile_store_linked_path"),
        SettingsError::FileTooLarge => ConnectionError::new("profile_store_too_large"),
        SettingsError::InvalidYaml { .. } | SettingsError::InvalidFormat => {
            ConnectionError::new("profile_store_invalid")
        }
        _ => ConnectionError::new("profile_store_unavailable"),
    }
}

/// `connections.yaml` in the configuration directory.
#[derive(Clone, Debug)]
pub struct ProfileRepository {
    directory: PathBuf,
}

impl ProfileRepository {
    #[must_use]
    pub fn at(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub fn load(&self) -> Result<Vec<ConnectionProfile>, ConnectionError> {
        if !self.directory.exists() {
            return Ok(Vec::new());
        }
        check_existing_components(&self.directory).map_err(storage_error)?;
        let directory = Dir::open_ambient_dir(&self.directory, ambient_authority())
            .map_err(|_| ConnectionError::new("profile_store_unavailable"))?;
        let Some(raw) = read_bounded(&directory, PROFILES_FILE, MAX_PROFILES_BYTES)? else {
            return Ok(Vec::new());
        };
        let file: ProfilesFile = serde_yaml_ng::from_slice(&raw)
            .map_err(|_| ConnectionError::new("profile_store_invalid"))?;
        if file.format != PROFILES_FORMAT || file.profiles.len() > MAX_PROFILES {
            return Err(ConnectionError::new("profile_store_invalid"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for profile in &file.profiles {
            profile.validate()?;
            if !seen.insert(profile.connection_id.as_str()) {
                return Err(ConnectionError::new("profile_store_invalid"));
            }
        }
        Ok(file.profiles)
    }

    /// Replaces the whole file atomically.
    pub fn save(&self, profiles: &[ConnectionProfile]) -> Result<(), ConnectionError> {
        if profiles.len() > MAX_PROFILES {
            return Err(
                ConnectionError::new("profile_limit_reached").with("max", MAX_PROFILES.to_string())
            );
        }
        for profile in profiles {
            profile.validate()?;
        }
        let file = ProfilesFile {
            format: PROFILES_FORMAT.to_owned(),
            profiles: profiles.to_vec(),
        };
        let raw = serde_yaml_ng::to_string(&file)
            .map_err(|_| ConnectionError::new("profile_store_invalid"))?
            .into_bytes();
        if raw.len() > MAX_PROFILES_BYTES {
            return Err(ConnectionError::new("profile_store_too_large"));
        }
        ensure_directory_without_links(&self.directory).map_err(storage_error)?;
        let directory = Dir::open_ambient_dir(&self.directory, ambient_authority())
            .map_err(|_| ConnectionError::new("profile_store_unavailable"))?;
        reject_link_if_present(&directory, PROFILES_FILE).map_err(storage_error)?;
        remove_stale_temporaries(&directory);
        replace_file(&directory, PROFILES_FILE, &raw).map_err(storage_error)?;
        sync_directory(&self.directory).map_err(storage_error)
    }
}

/// Trusted CA bundles, content-addressed as `trusted-ca/<ca_ref>.pem`.
/// A profile stores only the reference.
#[derive(Clone, Debug)]
pub struct TrustStore {
    directory: PathBuf,
}

/// Result of importing CA material.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedTrust {
    pub ca_ref: String,
    /// SHA-256 of each certificate, as shown to the user (P1-02F).
    pub certificate_fingerprints: Vec<String>,
}

impl TrustStore {
    #[must_use]
    pub fn at(configuration_directory: PathBuf) -> Self {
        Self {
            directory: configuration_directory.join(TRUST_DIRECTORY),
        }
    }

    /// Validates a PEM bundle and stores its canonical form.
    pub fn import(&self, pem: &[u8]) -> Result<ImportedTrust, ConnectionError> {
        let anchors = TrustAnchors::with_pem_bundle(pem).map_err(ConnectionError::from_zabbix)?;
        let Some(fingerprint) = anchors.fingerprint() else {
            return Err(ConnectionError::new("trust_material_invalid").with("reason", "empty"));
        };
        let ca_ref = format!("sha256-{fingerprint}");
        ensure_directory_without_links(&self.directory).map_err(storage_error)?;
        let directory = Dir::open_ambient_dir(&self.directory, ambient_authority())
            .map_err(|_| ConnectionError::new("profile_store_unavailable"))?;
        let name = format!("{ca_ref}.pem");
        reject_link_if_present(&directory, &name).map_err(storage_error)?;
        replace_file(&directory, &name, anchors.to_pem().as_bytes()).map_err(storage_error)?;
        sync_directory(&self.directory).map_err(storage_error)?;
        Ok(ImportedTrust {
            ca_ref,
            certificate_fingerprints: anchors.certificate_fingerprints(),
        })
    }

    /// Loads and re-validates stored material; the content must still match
    /// its reference.
    pub fn load(&self, ca_ref: &str) -> Result<TrustAnchors, ConnectionError> {
        if !is_ca_reference(ca_ref) {
            return Err(ConnectionError::new("trust_material_missing"));
        }
        if !self.directory.exists() {
            return Err(ConnectionError::new("trust_material_missing"));
        }
        check_existing_components(&self.directory).map_err(storage_error)?;
        let directory = Dir::open_ambient_dir(&self.directory, ambient_authority())
            .map_err(|_| ConnectionError::new("trust_material_missing"))?;
        let raw = read_bounded(&directory, &format!("{ca_ref}.pem"), MAX_TRUST_FILE_BYTES)?
            .ok_or_else(|| ConnectionError::new("trust_material_missing"))?;
        let anchors = TrustAnchors::with_pem_bundle(&raw).map_err(ConnectionError::from_zabbix)?;
        if anchors
            .fingerprint()
            .map(|hex| format!("sha256-{hex}"))
            .as_deref()
            != Some(ca_ref)
        {
            return Err(ConnectionError::new("trust_material_invalid").with("reason", "changed"));
        }
        Ok(anchors)
    }
}

/// Reads a regular file without following links, at most `limit` bytes.
fn read_bounded(
    directory: &Dir,
    name: &str,
    limit: usize,
) -> Result<Option<Vec<u8>>, ConnectionError> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = match directory.open_with(name, &options) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ConnectionError::new("profile_store_linked_path")),
    };
    let metadata = file
        .metadata()
        .map_err(|_| ConnectionError::new("profile_store_unavailable"))?;
    if !metadata.is_file() || cap_metadata_is_reparse(&metadata) {
        return Err(ConnectionError::new("profile_store_linked_path"));
    }
    let mut raw = Vec::new();
    Read::by_ref(&mut file)
        .take(u64::try_from(limit).unwrap_or(u64::MAX) + 1)
        .read_to_end(&mut raw)
        .map_err(|_| ConnectionError::new("profile_store_unavailable"))?;
    if raw.len() > limit {
        return Err(ConnectionError::new("profile_store_too_large"));
    }
    Ok(Some(raw))
}
