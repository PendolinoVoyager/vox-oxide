//! Re-exports for voice-chat module handling audio parsing.

use std::time::Duration;

use crate::app::App;
use anyhow::Result;
use tokio::time::Instant;
pub mod group_voice_session;

pub async fn handle_connection(app: &'static App, conn: quinn::Incoming) -> Result<()> {
    let mut connection = conn.await?;
    if let Err(auth_error) =
        crate::common::services::auth::auth_user_for_session(app, &mut connection).await
    {
        tracing::warn!("Unable to authenticate user: {auth_error}");
        connection.close(0u8.into(), auth_error.to_string().as_bytes());
        return Err(auth_error.into());
    }

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
    const SAMPLE_RATE: f32 = 48_000.0;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE as u32,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav_writer =
        hound::WavWriter::create(format!("test{}.wav", connection.stable_id()), spec)?;

    let mut interval = tokio::time::interval(Duration::from_millis(20));
    let mut last_write_time = Instant::now();
    loop {
        tokio::select! {
        read_res = connection.read_datagram() => {
            let bytes = match read_res {
                Err(quinn::ConnectionError::ApplicationClosed(frame)) => {
                    tracing::info!("connection closed: {}", frame);
                    return Ok(());
                }
                Err(e) => return Err(e.into()),
                Ok(dgram) => dgram,
            };
            let rtp_packet = rvoip_rtp_core::RtpPacket::parse(&bytes)?;
            tracing::trace!(
                "Packet {} from {}",
                rtp_packet.header.sequence_number,
                rtp_packet.header.ssrc
            );
            last_write_time = Instant::now();

            let len = decoder.decode(&rtp_packet.payload, &mut pcm_buf, false)?;
            for sample in pcm_buf[0..len].iter_mut() {
                wav_writer.write_sample(*sample)?;
            }

        }
        _ = interval.tick() => {
            let silence_duration = last_write_time.elapsed();
            for _ in 0..(silence_duration.as_millis() * (SAMPLE_RATE as u128 / 1000)) {
                wav_writer.write_sample(0)?
            }
            last_write_time = Instant::now();
        }
        }
    }
}
