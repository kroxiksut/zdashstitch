// SPDX-License-Identifier: MIT OR Apache-2.0

//! Linux: Unix domain socket `instance.sock` in a `0700` directory owned by
//! the effective user; files and socket `0600`.

use std::{
    env,
    fs::{DirBuilder, File, OpenOptions, Permissions},
    io,
    os::unix::fs::{
        DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
    },
    path::{Path, PathBuf},
};

use tokio::net::{UnixListener, UnixStream};

use super::{Accept, InstanceError};

const SOCKET: &str = "instance.sock";

pub(super) type Stream = UnixStream;

pub(super) fn default_directory() -> Option<PathBuf> {
    let absolute = |name: &str| {
        env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    if let Some(runtime) = absolute("XDG_RUNTIME_DIR") {
        return Some(runtime.join("zdashstitch"));
    }
    absolute("XDG_DATA_HOME")
        .or_else(|| absolute("HOME").map(|home| home.join(".local/share")))
        .map(|data| data.join("zdashstitch").join("runtime"))
}

/// A development override passes the same checks as the default
/// directory in [`prepare_directory`].
pub(super) fn check_override(directory: &Path) -> Result<PathBuf, InstanceError> {
    Ok(directory.to_path_buf())
}

/// Effective user of this process: the owner of a file it has just created
/// with `O_EXCL` in `directory` (no `/proc`, no libc). The probe is removed
/// again; `directory` is already known to have no group or other bits.
fn effective_uid(directory: &Path) -> io::Result<u32> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|_| io::Error::from(io::ErrorKind::Other))?;
    let probe = directory.join(format!(".uid-probe-{}", super::protocol::hex(&random)));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&probe)?;
    let uid = file.metadata().map(|metadata| metadata.uid());
    drop(file);
    let removed = std::fs::remove_file(&probe);
    let uid = uid?;
    removed?;
    Ok(uid)
}

/// Creates the directory `0700`; refuses a link, another owner or any group
/// or other permission bit, so no other user can reach or plant anything.
/// The mode is checked before the ownership probe writes into it.
pub(super) fn prepare_directory(directory: &Path) -> Result<(), InstanceError> {
    if let Some(parent) = directory.parent() {
        std::fs::create_dir_all(parent).map_err(|_| InstanceError::DirectoryUnavailable)?;
    }
    match DirBuilder::new().mode(0o700).create(directory) {
        Err(error) if error.kind() != io::ErrorKind::AlreadyExists => {
            return Err(InstanceError::DirectoryUnavailable);
        }
        _ => {}
    }
    let metadata =
        std::fs::symlink_metadata(directory).map_err(|_| InstanceError::DirectoryUnavailable)?;
    if !metadata.file_type().is_dir() || metadata.mode() & 0o077 != 0 {
        return Err(InstanceError::DirectoryInsecure);
    }
    let uid = effective_uid(directory).map_err(|_| InstanceError::DirectoryInsecure)?;
    if metadata.uid() != uid {
        return Err(InstanceError::DirectoryInsecure);
    }
    Ok(())
}

pub(super) fn open_private(path: &Path, truncate: bool) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(truncate)
        .mode(0o600)
        .open(path)
}

pub(super) struct Listener {
    listener: UnixListener,
    path: PathBuf,
    uid: u32,
}

impl Listener {
    /// The lock is held, so a socket left at this path belongs to a process
    /// of this user that has ended: it is removed and bound anew. Nothing
    /// outside this user's private directory is touched.
    pub(super) fn bind(directory: &Path, _random: &str) -> io::Result<Self> {
        let path = directory.join(SOCKET);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {
                return Err(io::ErrorKind::AlreadyExists.into());
            }
            Ok(_) => std::fs::remove_file(&path)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, Permissions::from_mode(0o600))?;
        // The socket file was just created by this process.
        let uid = std::fs::symlink_metadata(&path)?.uid();
        Ok(Self {
            listener,
            path,
            uid,
        })
    }

    pub(super) fn endpoint(&self) -> &str {
        SOCKET
    }

    pub(super) fn cleanup_path(&self) -> Option<PathBuf> {
        Some(self.path.clone())
    }
}

impl Accept for Listener {
    type Stream = UnixStream;

    /// The next peer of this user; peers of another user are dropped.
    async fn accept(&mut self) -> io::Result<UnixStream> {
        loop {
            let (stream, _) = self.listener.accept().await?;
            if stream
                .peer_cred()
                .is_ok_and(|credentials| credentials.uid() == self.uid)
            {
                return Ok(stream);
            }
            eprintln!("instance: refused a peer of another user");
        }
    }
}

pub(super) async fn connect(directory: &Path, endpoint: &str) -> io::Result<UnixStream> {
    if endpoint != SOCKET {
        return Err(io::ErrorKind::InvalidData.into());
    }
    UnixStream::connect(directory.join(SOCKET)).await
}
