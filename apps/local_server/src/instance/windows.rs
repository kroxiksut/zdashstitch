// SPDX-License-Identifier: MIT OR Apache-2.0

//! Windows: named pipe `\\.\pipe\zdashstitch-<32 hex>` and a directory
//! under `%LOCALAPPDATA%`. See the module documentation of `instance` for
//! the pipe's security descriptor.

use std::{
    env,
    fs::{File, OpenOptions},
    io,
    os::windows::fs::MetadataExt as _,
    path::{Component, Path, PathBuf},
    time::Duration,
};

use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};

use super::{Accept, InstanceError};

const PIPE_PREFIX: &str = r"\\.\pipe\zdashstitch-";
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
/// Pause after a client left before its connection completed, so a peer
/// that connects and closes in a loop cannot spin the accept loop.
const ACCEPT_RETRY: Duration = Duration::from_millis(10);

pub(super) type Stream = NamedPipeClient;

fn local_app_data() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

pub(super) fn default_directory() -> Option<PathBuf> {
    local_app_data().map(|base| base.join("ZDashstitch").join("runtime"))
}

fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// A development override (`ZDASHSTITCH_INSTANCE_DIR`) is accepted only
/// strictly inside `%LOCALAPPDATA%`, whose profile ACL keeps other users
/// out; std offers no owner or ACL check without `unsafe`, so a shared
/// directory such as one under `C:\temp` is refused instead. The nearest
/// existing ancestor is checked before anything is created; afterwards the
/// canonical path (junctions and links resolved) must still lie inside the
/// canonical `%LOCALAPPDATA%`, and no existing component below it may be a
/// reparse point. Returns the canonical directory.
pub(super) fn check_override(directory: &Path) -> Result<PathBuf, InstanceError> {
    if directory
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(InstanceError::DirectoryInsecure);
    }
    let base = local_app_data().ok_or(InstanceError::DirectoryUnavailable)?;
    let base = std::fs::canonicalize(base).map_err(|_| InstanceError::DirectoryUnavailable)?;
    let mut existing = directory;
    while !existing.exists() {
        existing = existing.parent().ok_or(InstanceError::DirectoryInsecure)?;
    }
    let existing =
        std::fs::canonicalize(existing).map_err(|_| InstanceError::DirectoryUnavailable)?;
    if !existing.starts_with(&base) {
        return Err(InstanceError::DirectoryInsecure);
    }
    std::fs::create_dir_all(directory).map_err(|_| InstanceError::DirectoryUnavailable)?;
    let canonical =
        std::fs::canonicalize(directory).map_err(|_| InstanceError::DirectoryUnavailable)?;
    let relative = canonical
        .strip_prefix(&base)
        .map_err(|_| InstanceError::DirectoryInsecure)?;
    if relative.as_os_str().is_empty() {
        return Err(InstanceError::DirectoryInsecure);
    }
    let mut current = base.clone();
    for component in relative.components() {
        current.push(component);
        let metadata =
            std::fs::symlink_metadata(&current).map_err(|_| InstanceError::DirectoryUnavailable)?;
        if is_reparse_point(&metadata) {
            return Err(InstanceError::DirectoryInsecure);
        }
    }
    Ok(canonical)
}

/// Creates the directory; refuses a link or junction at its place.
pub(super) fn prepare_directory(directory: &Path) -> Result<(), InstanceError> {
    std::fs::create_dir_all(directory).map_err(|_| InstanceError::DirectoryUnavailable)?;
    let metadata =
        std::fs::symlink_metadata(directory).map_err(|_| InstanceError::DirectoryUnavailable)?;
    if !metadata.is_dir() || is_reparse_point(&metadata) {
        return Err(InstanceError::DirectoryInsecure);
    }
    Ok(())
}

/// Opens a file in the private directory, which carries the profile ACL.
pub(super) fn open_private(path: &Path, truncate: bool) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(truncate)
        .open(path)
}

pub(super) struct Listener {
    name: String,
    /// The instance waiting for the next client; `None` only after creating
    /// a fresh instance failed, until [`Accept::accept`] succeeds in it.
    next: Option<NamedPipeServer>,
}

fn create(name: &str, first: bool) -> io::Result<NamedPipeServer> {
    ServerOptions::new()
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        .create(name)
}

impl Listener {
    /// The first instance fails if the name exists, so a pipe created by
    /// anyone else is never joined.
    pub(super) fn bind(_directory: &Path, random: &str) -> io::Result<Self> {
        let name = format!("{PIPE_PREFIX}{random}");
        let next = Some(create(&name, true)?);
        Ok(Self { name, next })
    }

    pub(super) fn endpoint(&self) -> &str {
        &self.name
    }

    /// Nothing on disk: the pipe disappears with its last handle.
    pub(super) fn cleanup_path(&self) -> Option<PathBuf> {
        None
    }
}

impl Accept for Listener {
    type Stream = NamedPipeServer;

    /// A connected instance. A fresh instance is created as soon as one is
    /// handed out, so the name stays owned by this process; if that fails,
    /// the error surfaces on the next call and the accept loop retries.
    async fn accept(&mut self) -> io::Result<NamedPipeServer> {
        loop {
            let pending = match self.next.take() {
                Some(pending) => pending,
                None => create(&self.name, false)?,
            };
            match pending.connect().await {
                Ok(()) => {
                    self.next = create(&self.name, false).ok();
                    return Ok(pending);
                }
                // The client went away before the connection completed: the
                // same instance waits for the next one, or a fresh one is
                // created before the broken one is closed.
                Err(_) => {
                    if pending.disconnect().is_ok() {
                        self.next = Some(pending);
                    } else {
                        self.next = Some(create(&self.name, false)?);
                    }
                    tokio::time::sleep(ACCEPT_RETRY).await;
                }
            }
        }
    }
}

pub(super) async fn connect(_directory: &Path, endpoint: &str) -> io::Result<NamedPipeClient> {
    let random = endpoint
        .strip_prefix(PIPE_PREFIX)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
    if random.len() != 32
        || !random
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(io::ErrorKind::InvalidData.into());
    }
    ClientOptions::new().open(endpoint)
}
