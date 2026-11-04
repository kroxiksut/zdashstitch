// SPDX-License-Identifier: MIT OR Apache-2.0

//! Process startup shared by the browser-only server binary and the desktop
//! launcher (O-036): single-instance lock, loopback listener, the initial
//! one-time bootstrap code and the HTTP application. Both binaries start the
//! server the same way, so the session, Origin/Host and CSRF boundary does not
//! depend on how the UI is shown.

use std::{
    env, fmt,
    future::Future,
    io,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};

use axum::Router;
use tokio::net::TcpListener;
use zdashstitch_application::connections::{ConnectionService, SystemCredentialStore};

use crate::{
    ConnectionsHandle, LocalSecurity, build_router_with_connections, default_ui_dir,
    instance::{self, Acquired, InstanceConfig, InstanceError, InstanceServer},
};

/// Where and how the local server starts: `ZDASHSTITCH_INSTANCE_DIR`,
/// `ZDASHSTITCH_PORT` and `ZDASHSTITCH_UI_DIR`.
#[derive(Clone, Debug)]
pub struct LaunchSettings {
    instance: InstanceConfig,
    port: u16,
    ui_dir: PathBuf,
}

impl LaunchSettings {
    /// Reads the process environment.
    pub fn from_env() -> Result<Self, StartupError> {
        Self::from_lookup(|name| env::var_os(name))
    }

    /// `ZDASHSTITCH_INSTANCE_DIR` (absolute) replaces the per-user directory,
    /// e.g. to run a separate development instance. On Windows it must lie
    /// inside `%LOCALAPPDATA%`; on Linux it must be a private `0700`
    /// directory. `ZDASHSTITCH_PORT` absent means an ephemeral port.
    pub fn from_lookup(
        lookup: impl Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Result<Self, StartupError> {
        let instance = match lookup("ZDASHSTITCH_INSTANCE_DIR") {
            Some(directory) => InstanceConfig::from_override(PathBuf::from(directory))?,
            None => InstanceConfig::platform_default()?,
        };
        let port = parse_port(lookup("ZDASHSTITCH_PORT"))?;
        let ui_dir = lookup("ZDASHSTITCH_UI_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(default_ui_dir);
        Ok(Self {
            instance,
            port,
            ui_dir,
        })
    }

    #[must_use]
    pub fn instance(&self) -> &InstanceConfig {
        &self.instance
    }

    #[must_use]
    pub fn ui_dir(&self) -> &Path {
        &self.ui_dir
    }
}

fn parse_port(value: Option<std::ffi::OsString>) -> Result<u16, StartupError> {
    match value {
        None => Ok(0),
        Some(value) => value
            .to_str()
            .and_then(|value| value.parse().ok())
            .ok_or(StartupError::InvalidPort),
    }
}

/// Startup failures as stable machine codes; no paths, names or secrets.
#[derive(Debug)]
pub enum StartupError {
    /// `ZDASHSTITCH_PORT` is not a port number.
    InvalidPort,
    /// The single-instance directory, lock or channel failed.
    Instance(InstanceError),
    /// The loopback listener could not be bound.
    Bind(io::ErrorKind),
    /// The operating-system random source failed.
    Random,
}

impl StartupError {
    #[must_use]
    pub fn code(&self) -> &str {
        match self {
            Self::InvalidPort => "startup.port.invalid",
            Self::Instance(error) => error.code(),
            Self::Bind(_) => "startup.bind.failed",
            Self::Random => "startup.random_failed",
        }
    }
}

impl fmt::Display for StartupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bind(kind) => write!(formatter, "{} ({kind:?})", self.code()),
            _ => formatter.write_str(self.code()),
        }
    }
}

impl std::error::Error for StartupError {}

impl From<InstanceError> for StartupError {
    fn from(error: InstanceError) -> Self {
        Self::Instance(error)
    }
}

/// Outcome of [`launch`].
pub enum Launch {
    /// This process holds the instance lock and serves the UI.
    Primary(Box<LocalServer>),
    /// Another process of this user serves; ask it with
    /// [`LaunchSettings::instance`] and [`instance::request_bootstrap`].
    Running,
}

/// A bound, not yet serving local server of the primary instance.
pub struct LocalServer {
    listener: TcpListener,
    address: SocketAddr,
    router: Router,
    security: LocalSecurity,
    bootstrap_url: String,
    ui_dir: PathBuf,
    _instance: InstanceServer,
}

/// Takes the single-instance lock (P2-SEC-01F). The primary binds the
/// loopback listener, creates the local session with its first one-time
/// bootstrap code, starts answering repeated launches and builds the HTTP
/// application. A repeated launch binds nothing. Must run inside the tokio
/// runtime.
pub async fn launch(settings: &LaunchSettings) -> Result<Launch, StartupError> {
    let primary = match instance::acquire(&settings.instance)? {
        Acquired::Primary(primary) => primary,
        Acquired::Running => return Ok(Launch::Running),
    };
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, settings.port))
        .await
        .map_err(|error| StartupError::Bind(error.kind()))?;
    let address = listener
        .local_addr()
        .map_err(|error| StartupError::Bind(error.kind()))?;
    let initial = LocalSecurity::initialize(address.port()).map_err(|_| StartupError::Random)?;
    let bootstrap_url = initial.security.bootstrap_url(&initial.code);
    let instance_server = primary.serve(initial.security.clone(), &settings.instance)?;
    let connections = match ConnectionService::platform_default(Box::new(SystemCredentialStore)) {
        Ok(service) => ConnectionsHandle::new(service),
        Err(error) => {
            eprintln!("connection profiles are unavailable: {}", error.code());
            ConnectionsHandle::unavailable()
        }
    };
    let router = build_router_with_connections(
        settings.ui_dir.clone(),
        initial.security.clone(),
        connections,
    );
    Ok(Launch::Primary(Box::new(LocalServer {
        listener,
        address,
        router,
        security: initial.security,
        bootstrap_url,
        ui_dir: settings.ui_dir.clone(),
        _instance: instance_server,
    })))
}

impl LocalServer {
    /// The first one-time bootstrap URL of the session. Hand it to exactly
    /// one UI host (window or browser); never write it to a log file.
    #[must_use]
    pub fn bootstrap_url(&self) -> &str {
        &self.bootstrap_url
    }

    /// The bound loopback address.
    #[must_use]
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// The local session boundary, e.g. to issue a further bootstrap code
    /// or to end the session when the program window closes.
    #[must_use]
    pub fn security(&self) -> LocalSecurity {
        self.security.clone()
    }

    #[must_use]
    pub fn ui_dir(&self) -> &Path {
        &self.ui_dir
    }

    /// Serves until `shutdown` resolves, then stops accepting and lets the
    /// running requests finish. Repeated launches are answered until this
    /// returns; the instance lock is released afterwards.
    pub async fn serve(
        self,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> io::Result<()> {
        let Self {
            listener,
            router,
            _instance,
            ..
        } = self;
        let result = axum::serve(listener, router)
            .with_graceful_shutdown(shutdown)
            .await;
        drop(_instance);
        result
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{StartupError, parse_port};

    #[test]
    fn absent_port_requests_an_ephemeral_listener() {
        assert_eq!(parse_port(None).ok(), Some(0));
    }

    #[test]
    fn explicit_port_is_used() {
        assert_eq!(parse_port(Some(OsString::from("4173"))).ok(), Some(4173));
    }

    #[test]
    fn invalid_port_is_refused_with_a_code() {
        for value in ["", "http", "65536", "-1"] {
            let error = parse_port(Some(OsString::from(value)));
            assert!(matches!(error, Err(StartupError::InvalidPort)), "{value}");
        }
        assert_eq!(StartupError::InvalidPort.code(), "startup.port.invalid");
    }

    #[test]
    fn relative_instance_directory_is_refused() {
        let error = super::LaunchSettings::from_lookup(|name| {
            (name == "ZDASHSTITCH_INSTANCE_DIR").then(|| OsString::from("relative/dir"))
        });
        assert_eq!(
            error.err().map(|error| error.code().to_owned()),
            Some("instance.directory.unavailable".to_owned())
        );
    }
}
