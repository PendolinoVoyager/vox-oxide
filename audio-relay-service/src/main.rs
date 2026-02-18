//! This example demonstrates an HTTP server that serves files from a directory.
//!
//! Checkout the `README.md` for guidance.
use rustls::crypto::{self};

pub mod app;
pub mod app_config;
use app_config::AppConfig;

use crate::app::App;
/// Sync entrypoint to the app with setup.
fn main() {
    rustls::crypto::CryptoProvider::install_default(crypto::aws_lc_rs::default_provider()).unwrap();
    let config = AppConfig::new().unwrap_or_else(|e| {
        tracing::error!("{}", e);
        std::process::exit(1);
    });
    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_max_level(config.get_log_level())
            .finish(),
    )
    .unwrap();

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
