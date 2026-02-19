use rustls::crypto::{self};

pub mod app;
pub mod common;
pub mod vc;
use crate::common::app_config::AppConfig;

use crate::app::App;

const WELCOME_LOGO: &str = include_str!("../logo.ascii");
/// Sync entrypoint to the app with setup.
fn main() {
    rustls::crypto::CryptoProvider::install_default(crypto::aws_lc_rs::default_provider()).unwrap();
    let config = AppConfig::new().unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1);
    });

    crate::common::logging::setup_tracing_subscriber(&config);

    tracing::info!("Created app config.");
    tracing::info!("{:?}", config);

    let code = run(config);
    ::std::process::exit(code);
}

#[tokio::main]
async fn run(options: AppConfig) -> i32 {
    let app = App::new(options);
    println!("{WELCOME_LOGO}");

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
