use std::{fs::OpenOptions, path::PathBuf};

use rustls::crypto::{self};

pub mod app;
pub mod app_config;
use app_config::AppConfig;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{Layer, layer::SubscriberExt};

use crate::app::App;
/// Sync entrypoint to the app with setup.
fn main() {
    rustls::crypto::CryptoProvider::install_default(crypto::aws_lc_rs::default_provider()).unwrap();
    let config = AppConfig::new().unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1);
    });

    setup_tracing_subscriber(&config);

    tracing::info!("Created app config.");
    tracing::info!("{:?}", config);

    let code = run(config);
    ::std::process::exit(code);
}

#[tokio::main]
async fn run(options: AppConfig) -> i32 {
    let app = App::new(options);
    if let Ok(logo) = tokio::fs::read_to_string("logo.ascii").await {
        print!("{logo}\n");
    }

    match app.run().await {
        Ok(_) => {
            tracing::info!("App exited normally");
            0
        }
        Err(e) => {
            tracing::error!("App exited unexpectedly: {e}");
            1
        }
    }
}

fn setup_tracing_subscriber(config: &AppConfig) {
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_ansi(true)
        .with_filter(LevelFilter::from_level(config.get_log_level()));

    let console_layer = console_subscriber::ConsoleLayer::builder()
        .with_default_env()
        .spawn();

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(if let Some(f) = &config.log_file {
            f.clone()
        } else {
            PathBuf::from("/dev/null")
        })
        .expect("Cannot open log file");
    let file_layer = tracing_subscriber::fmt::layer()
        .compact()
        .with_ansi(config.log_file.is_none())
        .with_writer(file)
        .with_filter(LevelFilter::from_level(config.get_log_level()));

    let registry = tracing_subscriber::registry()
        .with(console_layer)
        .with(stdout_layer)
        .with(file_layer);

    tracing::subscriber::set_global_default(registry).unwrap();

    tracing::debug!("Set up tracing subscriber");
}
