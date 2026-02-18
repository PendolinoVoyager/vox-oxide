use std::io::BufReader;
use std::net::SocketAddrV6;
use std::path::PathBuf;
use std::str::FromStr;
use std::{fs::File, net::SocketAddr};

use clap_serde_derive::{
    ClapSerde,
    clap::{self, Parser},
};
use serde::{Deserialize, Serialize};
use tracing::Level;

#[cfg(test)]
const CONFIG_PATH_ENV: &'static str = "TEST_CONFIG_PATH";

#[cfg(not(test))]
pub const CONFIG_PATH_ENV: &'static str = "ARS_CONFIG_PATH";

/// Configuration for the app.
#[derive(Parser, Deserialize, Debug, Clone)]
#[command(version, about, long_about = None)]
pub struct AppConfigArgs {
    /// stding input (unused)
    pub input: Option<Vec<String>>,

    /// Path pointing to config.yaml
    #[clap(long = "config", default_value = "config.yaml")]
    pub config_path: std::path::PathBuf,

    #[command(flatten)]
    pub config: <AppConfig as ClapSerde>::Opt,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, Copy, derive_more::FromStr, PartialEq)]
#[from_str(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    Production,
    #[default]
    Development,
}

#[derive(ClapSerde, Debug, Clone, Deserialize)]
pub struct AppConfig {
    #[clap(short = 'e', long = "environment")]
    pub environment: Environment,

    /// TLS private key in PEM format
    #[clap(short = 'k', long = "key", requires = "cert")]
    pub key: PathBuf,
    /// TLS certificate in PEM format
    #[clap(short = 'c', long = "cert", requires = "key")]
    pub cert: PathBuf,

    /// Address to listen on
    #[clap(long = "listen")]
    #[default(SocketAddr::V6(SocketAddrV6::from_str("[::1]:4433").unwrap()))]
    pub listen: SocketAddr,

    /// Maximum number of concurrent connections to allow
    #[clap(long = "connection-limit")]
    pub connection_limit: usize,

    #[clap(short, long)]
    pub log_level: String,
}

impl std::fmt::Debug for ClapSerdeOptionalAppConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClapSerdeOptionalConfig")
            .field("environment", &self.environment)
            .field("key", &self.key)
            .field("cert", &self.cert)
            .field("listen", &self.listen)
            .field("connection_limit", &self.connection_limit)
            .field("log_level", &self.log_level)
            .finish()
    }
}
/// Greeaaaaat...derive doesn't work due to macro shenanigans
impl Clone for ClapSerdeOptionalAppConfig {
    fn clone(&self) -> Self {
        Self {
            environment: self.environment.clone(),
            key: self.key.clone(),
            cert: self.cert.clone(),
            listen: self.listen.clone(),
            connection_limit: self.connection_limit.clone(),
            log_level: self.log_level.clone(),
        }
    }
}

impl AppConfig {
    /// Config takes priority from:
    /// 1. CLI commands (eg. --connection_limit 10) will always be 10 despite config.yaml saying otherwise
    /// 2. YAML config from ENV ARS_CONFIG_PATH
    /// 3. YAML config from CLI if no env is provided (--config)
    /// 4. Default config YAML file - ./config.yaml
    pub fn new() -> anyhow::Result<Self> {
        // Parse from real CLI args + env
        let mut args = AppConfigArgs::try_parse()?;
        Self::from_args(&mut args)
    }
    /// Testable constructor: accepts a pre-built AppConfigArgs so tests
    /// can bypass real CLI parsing.
    pub fn from_args(args: &mut AppConfigArgs) -> anyhow::Result<Self> {
        // Environment variable overrides the --config flag
        if let Some(path) = std::env::var_os(CONFIG_PATH_ENV) {
            println!("{:?}", &path);
            args.config_path = path.into();
        }
        match File::open(&args.config_path) {
            Ok(f) => match serde_yaml::from_reader::<_, AppConfig>(BufReader::new(f)) {
                Ok(file_config) => {
                    let cfg = AppConfig::try_from(file_config)?;
                    Ok(cfg.merge(&mut args.config))
                }
                Err(err) => Err(err.into()),
            },
            Err(open_error) => Err(open_error.into()),
        }
    }
    pub fn get_log_level(&self) -> Level {
        match self.log_level.as_str() {
            "trace" => Level::TRACE,
            "debug" => Level::DEBUG,
            "info" => Level::INFO,
            "warn" => Level::WARN,
            "error" => Level::ERROR,
            _ => Level::INFO,
        }
    }
}
