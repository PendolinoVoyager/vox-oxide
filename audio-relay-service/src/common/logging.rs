use std::{fs::OpenOptions, path::PathBuf};

use tracing::level_filters::LevelFilter;
use tracing_subscriber::{Layer, layer::SubscriberExt};

use crate::common::app_config::AppConfig;

pub fn setup_tracing_subscriber(config: &AppConfig) {
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
