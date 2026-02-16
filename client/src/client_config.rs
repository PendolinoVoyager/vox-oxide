use crate::app_config::AppConfig;
use quinn::crypto::rustls::QuicClientConfig;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;
use std::sync::Arc;

#[cfg(not(debug_assertions))]
const CERT: &[u8] = include_bytes!(env!("EMBEDDED_CERT_PATH"));

/// Create a Quic server config.
/// It will load certificates etc.
pub fn create_client_config(config: &AppConfig) -> Result<quinn::ClientConfig, anyhow::Error> {
    let mut roots = rustls::RootCertStore::empty();

    let certs = {
        #[cfg(debug_assertions)]
        {
            tracing::info!("Using file certificate.");
            match &config.cert_path {
                Some(cert) => {
                    CertificateDer::pem_file_iter(cert)?.collect::<Result<Vec<_>, _>>()?
                }
                None => panic!("Certificate path not provided and not embedded into binary"),
            }
        }
        #[cfg(not(debug_assertions))]
        {
            tracing::info!("Using embedded certificate.");
            CertificateDer::pem_reader_iter(&CERT[..]).collect::<Result<Vec<_>, _>>()?
        }
    };

    for cert in certs {
        roots.add(cert)?;
    }

    let mut client_crypto = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    client_crypto.alpn_protocols = [b"hq-29"].iter().map(|&x| x.into()).collect();

    Ok(quinn::ClientConfig::new(Arc::new(
        QuicClientConfig::try_from(client_crypto)?,
    )))
}
