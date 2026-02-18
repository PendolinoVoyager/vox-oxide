use crate::app_config::AppConfig;
use std::{fs, sync::Arc};

use anyhow::{Context, Result};
use quinn::Endpoint;
use quinn_proto::crypto::rustls::QuicServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, pem::PemObject};
use tokio::signal::{self};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
pub struct App {
    ///Readonly config
    pub config: AppConfig,
    /// Token notifying of app shutdown
    pub cancellation_token: CancellationToken,
    /// Task tracker. Instead of using tokio::spawn use tracker.spawn
    task_tracker: TaskTracker,
}

impl App {
    pub fn new(config: AppConfig) -> &'static mut Self {
        let cancellation_token = CancellationToken::new();
        let task_tracker = TaskTracker::new();
        let app = Box::new(Self {
            config,
            cancellation_token,
            task_tracker,
        });
        Box::leak(app)
    }
    pub async fn run(&'static mut self) -> anyhow::Result<()> {
        let endpoint = self.create_endpoint()?;
        tracing::info!("listening on {}", endpoint.local_addr()?);
        tokio::spawn(self.main_loop(endpoint));
        self.handle_signal().await;
        self.task_tracker.close();
        self.task_tracker.wait().await;
        Ok(())
    }
    async fn main_loop(&'static self, endpoint: Endpoint) {
        let connection_limit = self.config.connection_limit;
        loop {
            tokio::select! {
                            Some(conn) = endpoint.accept() => {
                                if endpoint.open_connections() >= connection_limit {
                                    tracing::debug!("refusing due to open connection limit");
                                    conn.refuse();
                                } else if !conn.remote_address_validated() {
                                    tracing::debug!("requiring connection to validate its address");
                                    conn.retry().unwrap();
                                } else {
                                    tracing::info!("Accepted connection");
                                    let fut = handle_connection(self, conn);
                                    self.task_tracker.spawn(async move {
                                        if let Err(e) = fut.await {
                                            tracing::error!("connection failed: {reason}", reason = e.to_string())
                                        }
                                    });
                                }
                            },
                            _ = self.cancellation_token.cancelled()
            => {
                                tracing::info!("Stopping receiving new connections.");
                                break;
                            }


                        }
        }
    }
    fn create_endpoint(&'static self) -> anyhow::Result<Endpoint> {
        let options = self.config.clone();
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
        Ok(quinn::Endpoint::server(server_config, options.listen)?)
    }

    async fn handle_signal(&'static self) {
        match signal::ctrl_c().await {
            Ok(_) => {
                tracing::info!("Interrupt detected!");
                self.cancellation_token.cancel();
                tracing::info!("Sent exit signal. Waiting for jobs to finish...");
            }
            Err(e) => {
                tracing::error!("Cannot listen for interrupt, app closing: {e}");
            }
        }
    }
}

async fn handle_connection(app: &'static App, conn: quinn::Incoming) -> Result<()> {
    let mut connection = conn.await?;
    // Accept first bidirectional stream (control)
    let (mut send, mut recv) = connection.accept_bi().await?;

    let request = recv.read_to_end(4096).await?;
    tracing::debug!(
        "Auth payload from {}: {:?}",
        connection.remote_address(),
        String::from_utf8_lossy(&request)
    );

    let valid = true; // logic here

    if !valid {
        connection.close(0u32.into(), b"auth failed");
        return Err(anyhow::anyhow!("auth failed"));
    }

    // Send OK
    send.write_all(b"OK").await.unwrap();
    send.finish().unwrap();
    tracing::info!("established");

    tokio::select! {
        _ = playback_loop(&mut connection) => {
            Ok(())
        }
        _ = app.cancellation_token.cancelled() => {
            tracing::debug!("Shutting down connection with {}", connection.remote_address());
            connection.close(1u32.into(), b"server shutdown");
            Ok(())
        }
    }
}

async fn playback_loop(connection: &mut quinn::Connection) -> anyhow::Result<()> {
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
        tracing::debug!(
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
