use std::sync::Arc;

use quinn::{ServerConfig, crypto::rustls::QuicServerConfig};
use rustls::pki_types::PrivateKeyDer;

use crate::common::app_config::AppConfig;

pub fn create_server_config(
    // AppConfig will probably be used when new options are added
    _app_config: &AppConfig,
    certs: Vec<rustls::pki_types::CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> anyhow::Result<ServerConfig> {
    let mut server_crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    server_crypto.alpn_protocols = vec![b"hq-29".to_vec()];

    let mut server_config =
        quinn::ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(server_crypto)?));
    let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
    // No unidirectional streams are needed.
    transport_config.max_concurrent_uni_streams(0_u8.into());
    // Big buffer just in case... there shouldn't be many simultaneous conenctions on one ars anyway
    transport_config.datagram_receive_buffer_size(Some(1024 * 5));

    // streams for auth... receive_window needs to be at least auth request struct long
    transport_config.max_concurrent_bidi_streams(5_u8.into());
    transport_config.stream_receive_window(1024_u32.into());
    tracing::debug!("Created server config: {:?}", server_config);
    Ok(server_config)
}
