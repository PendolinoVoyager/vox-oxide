//! This example demonstrates an HTTP client that requests files from a server.
//!
//! Checkout the `README.md` for guidance.

mod app_config;
mod client_config;
use anyhow::{Result, anyhow};
use clap::Parser;
use rustls::crypto;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{Layer, fmt, layer::SubscriberExt};

use crate::{app::App, audio::audio_manager};

mod app;
mod audio;

#[tokio::main]
async fn main() -> Result<()> {
    crypto::CryptoProvider::install_default(crypto::aws_lc_rs::default_provider()).unwrap();
    let opt = app_config::AppConfig::parse();
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(&opt.log_file)?;

    let subscriber = tracing_subscriber::Registry::default().with(
        // stdout layer, to view everything in the console
        fmt::layer()
            .compact()
            .with_writer(log_file)
            .with_ansi(false)
            .with_filter(LevelFilter::from_level(tracing::Level::INFO)),
    );
    tracing::subscriber::set_global_default(subscriber)?;

    tracing::info!("App starting up...");

    color_eyre::install().map_err(|e| anyhow!(e))?;
    let audio_manager = audio_manager::AudioManager::new(opt.clone());
    let mut app = App::new(audio_manager, opt);
    ratatui::run(|terminal| app.run(terminal))?;
    Ok(())
}
