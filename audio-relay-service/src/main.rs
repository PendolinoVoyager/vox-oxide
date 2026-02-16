//! This example demonstrates an HTTP server that serves files from a directory.
//!
//! Checkout the `README.md` for guidance.

use std::{
    fs,
    io::{self, Read},
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use hound::WavSpec;
use quinn_proto::crypto::rustls::QuicServerConfig;
use rustls::{
    crypto::{self},
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, pem::PemObject},
};
use tokio::io::AsyncWriteExt;
use tracing::{error, info};
use tracing_subscriber::fmt::format;

mod common;

#[derive(Parser, Debug)]
#[clap(name = "server")]
struct Opt {
    /// file to log TLS keys to for debugging
    #[clap(long = "keylog")]
    keylog: bool,
    /// TLS private key in PEM format
    #[clap(
        short = 'k',
        long = "key",
        requires = "cert",
        default_value = "../dev-certs/dev-server.pem"
    )]
    key: PathBuf,
    /// TLS certificate in PEM format
    #[clap(
        short = 'c',
        long = "cert",
        requires = "key",
        default_value = "../dev-certs/dev-server.key"
    )]
    cert: PathBuf,

    /// Address to listen on
    #[clap(long = "listen", default_value = "[::1]:4433")]
    listen: SocketAddr,
    /// Maximum number of concurrent connections to allow
    #[clap(long = "connection-limit", default_value = "50")]
    connection_limit: usize,
}

fn main() {
    rustls::crypto::CryptoProvider::install_default(crypto::aws_lc_rs::default_provider()).unwrap();
    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            // .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .finish(),
    )
    .unwrap();
    let opt = Opt::parse();
    let code = {
        if let Err(e) = run(opt) {
            eprintln!("ERROR: {e}");
            1
        } else {
            0
        }
    };
    ::std::process::exit(code);
}

#[tokio::main]
async fn run(options: Opt) -> Result<()> {
    let (certs, key) = {
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

        (cert_chain, key)
    };

    let mut server_crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    server_crypto.alpn_protocols = vec![b"hq-29".to_vec()];

    let mut server_config =
        quinn::ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(server_crypto)?));
    let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
    transport_config.max_concurrent_uni_streams(0_u8.into());
    // streams for auth...
    transport_config.max_concurrent_bidi_streams(5_u8.into());
    transport_config.datagram_receive_buffer_size(Some(1024 * 50));
    transport_config.stream_receive_window(1024_u32.into());
    let endpoint = quinn::Endpoint::server(server_config, options.listen)?;
    eprintln!("listening on {}", endpoint.local_addr()?);

    while let Some(conn) = endpoint.accept().await {
        if endpoint.open_connections() >= options.connection_limit {
            info!("refusing due to open connection limit");
            conn.refuse();
        } else if !conn.remote_address_validated() {
            info!("requiring connection to validate its address");
            conn.retry().unwrap();
        } else {
            info!("accepting connection");
            let fut = handle_connection(conn);
            tokio::spawn(async move {
                if let Err(e) = fut.await {
                    error!("connection failed: {reason}", reason = e.to_string())
                }
            });
        }
    }

    Ok(())
}

async fn handle_connection(conn: quinn::Incoming) -> Result<()> {
    let connection = conn.await?;
    // Accept first bidirectional stream (control)
    let (mut send, mut recv) = connection.accept_bi().await?;

    let request = recv.read_to_end(4096).await?;
    println!("Auth payload: {:?}", request);

    // Validate token / session
    let valid = true; // your logic here

    if !valid {
        connection.close(0u32.into(), b"auth failed");
        return Err(anyhow!("auth failed"));
    }

    // Send OK
    send.write_all(b"OK").await.unwrap();
    send.finish().unwrap();
    info!("established");

    playback_loop(connection).await
}

async fn playback_loop(connection: quinn::Connection) -> anyhow::Result<()> {
    let mut decoder = opus::Decoder::new(48000, opus::Channels::Mono)?;
    let mut pcm_buf = vec![0i16; 960]; // 20ms @ 48kHz

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav_writer =
        hound::WavWriter::create(format!("test{}.wav", connection.stable_id()), spec)?;
    // Main receive loop - write Opus packets to FFmpeg stdin
    loop {
        let read_res = connection.read_datagram().await;
        let bytes = match read_res {
            Err(quinn::ConnectionError::ApplicationClosed(frame)) => {
                tracing::info!("connection closed: {}", frame);
                return Ok(());
            }
            Err(e) => return Err(e.into()),
            Ok(dgram) => dgram,
        };
        let rtp_packet = rvoip_rtp_core::RtpPacket::parse(&bytes)?;
        tracing::info!(
            "Packet {} from {}",
            rtp_packet.header.sequence_number,
            rtp_packet.header.ssrc
        );
        let len = decoder.decode(&rtp_packet.payload, &mut pcm_buf, false)?;
        for sample in &pcm_buf[0..len] {
            wav_writer.write_sample(*sample)?;
        }
    }
}
