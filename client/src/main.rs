//! This example demonstrates an HTTP client that requests files from a server.
//!
//! Checkout the `README.md` for guidance.

use std::time::Duration;
mod app_config;
mod client_config;

use anyhow::{Result, anyhow};
use bytes::Bytes;
use clap::Parser;
use rustls::crypto;

use crate::{app_config::AppConfig, client_config::create_client_config};

fn main() {
    crypto::CryptoProvider::install_default(crypto::aws_lc_rs::default_provider()).unwrap();
    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .finish(),
    )
    .unwrap();
    let opt = app_config::AppConfig::parse();
    let code = {
        if let Err(e) = run(opt) {
            eprintln!("ERROR: {e}");
            1
        } else {
            0
        }
    };
    ::std::process::exit(code);
}

#[tokio::main]
async fn run(options: AppConfig) -> Result<()> {
    let client_config = create_client_config(&options)?;
    let mut endpoint = quinn::Endpoint::client(options.bind)?;
    endpoint.set_default_client_config(client_config);

    let host = options.get_host()?;
    let remote = options.get_remote_addr()?;

    tracing::debug!("connecting to {host} at {remote}");
    let conn = endpoint
        .connect(remote, &host)?
        .await
        .map_err(|e| anyhow!("failed to connect: {}", e))?;

    loop {
        let res = conn.send_datagram(Bytes::copy_from_slice(b"Wowzer :D"));
        if let Err(e) = res {
            eprintln!("Error: {:?}", e);
        } else {
            println!("Sent a dgram");
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    conn.close(0u32.into(), b"done");

    // Give the server a fair chance to receive the close packet
    endpoint.wait_idle().await;

    Ok(())
}
