use anyhow::anyhow;
use std::{
    net::{SocketAddr, ToSocketAddrs},
    path::PathBuf,
};

use clap::Parser;

/// HTTP/0.9 over QUIC client
#[derive(Parser, Debug, Clone)]
#[clap(name = "client")]
pub struct AppConfig {
    #[clap(long = "url", default_value = "quic://[::1]:4433")]
    pub url: url::Url,

    /// Override hostname used for certificate verification
    #[clap(long = "host")]
    pub host: Option<String>,

    /// Certificate path
    #[clap(long = "pem", default_value = "../dev-certs/dev-ca.pem")]
    pub cert_path: Option<PathBuf>,

    /// Address to bind on
    #[clap(long = "bind", default_value = "[::]:0")]
    pub bind: SocketAddr,
}

impl AppConfig {
    pub fn get_host(&self) -> anyhow::Result<String> {
        let url_host = strip_ipv6_brackets(self.url.host_str().unwrap());

        Ok(self.host.as_deref().unwrap_or(url_host).to_owned())
    }
    pub fn get_remote_addr(&self) -> anyhow::Result<SocketAddr> {
        let url_host = strip_ipv6_brackets(self.url.host_str().unwrap());

        (url_host, self.url.port().unwrap_or(4433))
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| anyhow!("couldn't resolve to an address"))
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
