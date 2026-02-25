use std::{net::SocketAddr, path::PathBuf};

use clap::Parser;

/// HTTP/0.9 over QUIC client
#[derive(Parser, Debug, Clone)]
#[clap(name = "client")]
pub struct AppConfig {
    #[clap(long = "remote", default_value = "[::1]:4433")]
    pub audio_service_address: SocketAddr,

    /// Override hostname used for certificate verification
    #[clap(long = "host")]
    pub host: Option<String>,

    /// Certificate path
    #[clap(long = "pem", default_value = "../dev-certs/dev-ca.pem")]
    pub cert_path: Option<PathBuf>,

    /// Address to bind on
    #[clap(long = "bind", default_value = "[::]:0")]
    pub bind: SocketAddr,
    /// Log file to write to. Default points to /dev/null or $null on Windows
    #[clap(long = "log-file", short, default_value = if cfg!(target_family="windows") {r"c:\nul"} else {r"/dev/null"})]
    pub log_file: PathBuf,
    #[clap(long, default_value = "info")]
    pub log_level: String,
}

impl AppConfig {
    pub fn get_host(&self) -> anyhow::Result<String> {
        let url_host = strip_ipv6_brackets(&self.audio_service_address.ip().to_string()).to_owned();

        Ok(self.host.as_deref().unwrap_or(&url_host).to_owned())
    }
    pub fn get_remote_addr(&self) -> SocketAddr {
        self.audio_service_address
    }
}

fn strip_ipv6_brackets(host: &str) -> &str {
    // An ipv6 url looks like eg https://[::1]:4433/Cargo.toml, wherein the host [::1] is the
    // ipv6 address ::1 wrapped in brackets, per RFC 2732. This strips those.
    if host.starts_with('[') && host.ends_with(']') {
        &host[1..host.len() - 1]
    } else {
        host
    }
}
