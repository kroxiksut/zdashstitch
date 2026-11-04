// SPDX-License-Identifier: MIT OR Apache-2.0

//! Browser-only local server: prints the one-time bootstrap URL and serves
//! until Ctrl+C. The desktop launcher `zdashstitch` (O-036) starts the same
//! server through [`zdashstitch_local_server::startup`].

use std::{error::Error, process::ExitCode};

use zdashstitch_local_server::{
    instance::{self, InstanceConfig},
    startup::{self, Launch, LaunchSettings},
};

#[tokio::main]
async fn main() -> Result<ExitCode, Box<dyn Error>> {
    // Single instance per user (P2-SEC-01F): a repeated launch asks the
    // running process for a fresh bootstrap URL and never binds a server.
    let settings = LaunchSettings::from_env()?;
    let server = match startup::launch(&settings).await? {
        Launch::Primary(server) => server,
        Launch::Running => return Ok(ask_running_instance(settings.instance()).await),
    };
    println!("Bootstrap URL: {}", server.bootstrap_url());
    println!("UI directory: {}", server.ui_dir().display());
    server.serve(shutdown_signal()).await?;
    Ok(ExitCode::SUCCESS)
}

/// The bootstrap URL goes to stdout only, as for the first launch.
async fn ask_running_instance(config: &InstanceConfig) -> ExitCode {
    match instance::request_bootstrap(config).await {
        Ok(url) => {
            println!("ZDashstitch is already running; open it with this one-time URL.");
            println!("Bootstrap URL: {url}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!(
                "the running instance issued no bootstrap URL: {}",
                error.code()
            );
            ExitCode::FAILURE
        }
    }
}

/// Resolves on Ctrl+C. Without a Ctrl+C handler it never resolves: the
/// server keeps running instead of stopping at once.
async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        eprintln!("failed to install Ctrl+C handler: {error}");
        std::future::pending::<()>().await;
    }
}
