pub mod audio_manager;
pub mod audio_source;
use anyhow::{Result, anyhow};
use quinn::Connection;

use crate::{app_config::AppConfig, client_config::create_client_config};
pub async fn create_audio_connection(options: AppConfig) -> Result<Connection> {
    let client_config = create_client_config(&options)?;
    let mut endpoint = quinn::Endpoint::client(options.bind)?;
    endpoint.set_default_client_config(client_config);

    let host = options.get_host()?;
    let remote = options.get_remote_addr()?;

    let conn = endpoint
        .connect(remote, &host)?
        .await
        .map_err(|e| anyhow!("failed to connect: {}", e))?;
    tracing::info!("Connected to {host} at {remote}");
    Ok(conn)
}
