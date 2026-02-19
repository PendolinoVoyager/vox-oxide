//! This module handles loading certificates for use in TLS.

use std::fs;

use anyhow::Context;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, pem::PemObject};

use crate::common::app_config::AppConfig;
pub fn load_certs<'a>(
    config: &AppConfig,
) -> anyhow::Result<(Vec<CertificateDer<'a>>, PrivateKeyDer<'a>)> {
    let options = config.clone();
    tracing::debug!(
        "Loading certificates from {:?} and {:?}",
        &options.cert.to_str(),
        &options.key.to_str()
    );
    let key = if options.key.extension().is_some_and(|x| x == "der") {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            fs::read(options.key).context("failed to read private key file")?,
        ))
    } else {
        PrivateKeyDer::from_pem_file(options.key)
            .context("failed to read PEM from private key file")?
    };

    let cert_chain = if options.cert.extension().is_some_and(|x| x == "der") {
        vec![CertificateDer::from(
            fs::read(options.cert).context("failed to read certificate chain file")?,
        )]
    } else {
        CertificateDer::pem_file_iter(options.cert)
            .context("failed to read PEM from certificate chain file")?
            .collect::<Result<_, _>>()
            .context("invalid PEM-encoded certificate")?
    };
    tracing::info!(
        "Created certificate chain with {} certificate",
        cert_chain.len()
    );
    Ok((cert_chain, key))
}
