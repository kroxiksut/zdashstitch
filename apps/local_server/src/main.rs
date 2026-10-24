// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{env, error::Error, net::Ipv4Addr, path::PathBuf};

use tokio::net::TcpListener;
use zdashstitch_local_server::{build_router, default_ui_dir};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let port = configured_port()?;
    let ui_dir = env::var_os("ZDASHSTITCH_UI_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(default_ui_dir);
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
    let address = listener.local_addr()?;

    println!("ZDashstitch scaffold: http://{address}");
    println!("UI directory: {}", ui_dir.display());
    axum::serve(listener, build_router(ui_dir))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        eprintln!("failed to install Ctrl+C handler: {error}");
    }
}

fn configured_port() -> Result<u16, Box<dyn Error>> {
    parse_port(env::var("ZDASHSTITCH_PORT"))
}

fn parse_port(value: Result<String, env::VarError>) -> Result<u16, Box<dyn Error>> {
    match value {
        Ok(value) => Ok(value.parse()?),
        Err(env::VarError::NotPresent) => Ok(0),
        Err(error) => Err(Box::new(error)),
    }
}

#[cfg(test)]
mod tests {
    use std::env::VarError;

    use super::parse_port;

    #[test]
    fn absent_port_requests_an_ephemeral_listener() {
        assert_eq!(parse_port(Err(VarError::NotPresent)).ok(), Some(0));
    }
}
