//! This module contains the GroupVoiceSession struct.
//! A Group Voice Session is created, when at least one user joins a room and creates a session.
//! Other users joining the room will be assigned to this GroupVoiceSession, bringing their own session with them.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU16, AtomicU32},
    },
};

use quinn::Connection;
use ringbuf::{
    StaticRb,
    traits::{Consumer, RingBuffer},
};
use rvoip_rtp_core::{RtpHeader, RtpPacket, RtpSsrc};
use tokio::sync::{
    Mutex, broadcast,
    mpsc::{self},
};

use crate::app::App;
const RTP_PACKET_RB_SIZE: usize = 64;
const BROADCAST_CAPACITY: usize = 200;
const MIX_INTERVAL_MS: u64 = 20; // typical 20ms RTP ptime

pub struct GroupVoiceSessionMember {
    pub connection: Connection,
    pub user_id: u32,
    audio_sender: mpsc::Sender<(u32, RtpPacket)>,
    /// Receives pre-mixed audio from the session task
    mixed_receiver: broadcast::Receiver<RtpPacket>,
}

impl GroupVoiceSessionMember {
    pub async fn session_loop(&mut self, _app: &'static App) -> anyhow::Result<()> {
        loop {
            tokio::select! {
               res = Self::receive_audio_fut(&mut self.connection) => {
                    let packet = res?;
                    tracing::debug!("Client {} mixed packet: {:?}", self.user_id, packet);
                    // If send fails the session task is gone - exit cleanly
                    if self.audio_sender.send((self.user_id, packet)).await.is_err() {
                        tracing::info!("Session closed, disconnecting user {}", self.user_id);
                        break;
                    }
                }

                recv_result = self.mixed_receiver.recv() => {
                    match recv_result {
                        Ok(mixed) => {
                            // Send to client - if their connection is bad,
                            // only THEY are affected
                            self.connection.send_datagram(mixed.serialize()?)?;
                        }
                        // We lagged behind - broadcast dropped some packets for us.
                        // Log and continue; other members are unaffected.
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("User {} lagged, dropped {n} mixed packets", self.user_id);
                        }
                        // Session task exited
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
        Ok(())
    }
    async fn receive_audio_fut(connection: &mut Connection) -> anyhow::Result<RtpPacket> {
        let read_res = connection.read_datagram().await;
        let bytes = match read_res {
            Err(quinn::ConnectionError::ApplicationClosed(frame)) => {
                tracing::info!("connection closed: {}", frame);
                return Err(anyhow::anyhow!(frame.to_string()));
            }
            Err(e) => return Err(e.into()),
            Ok(dgram) => dgram,
        };
        Ok(rvoip_rtp_core::RtpPacket::parse(&bytes)?)
    }
}

pub struct GroupVoiceSession {
    /// Inbound audio from all members, tagged by user_id via ssrc
    sender: mpsc::Sender<(u32, RtpPacket)>,

    /// Mixed audio sent OUT to all members
    mixed_sender: broadcast::Sender<RtpPacket>,

    // Audio receiver owned only by this struct. In mutex so the main struct doesn't force &mut refs.
    shared_audio_receiver: Mutex<mpsc::Receiver<(u32, RtpPacket)>>,
    /// Per-member jitter buffers keyed by ssrc/user_id
    members: Arc<Mutex<HashMap<u32, StaticRb<RtpPacket, RTP_PACKET_RB_SIZE>>>>,
    encoder: Mutex<opus::Encoder>,
    decoder: Mutex<opus::Decoder>,
}

impl GroupVoiceSession {
    pub fn new() -> Self {
        let (sender, shared_audio_receiver) = mpsc::channel(200);
        // broadcast capacity - lagging receivers are simply dropped/skipped
        let (mixed_sender, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            sender,
            shared_audio_receiver: Mutex::new(shared_audio_receiver),
            mixed_sender,
            members: Arc::new(Mutex::new(HashMap::with_capacity(10))),
            encoder: Mutex::new(
                opus::Encoder::new(48000, opus::Channels::Mono, opus::Application::Voip).unwrap(),
            ),
            decoder: Mutex::new(opus::Decoder::new(48000, opus::Channels::Mono).unwrap()),
        }
    }

    /// Returns the inbound sender + a receiver for mixed outbound audio
    pub fn register_user(&self, user_id: u32, connection: Connection) -> GroupVoiceSessionMember {
        GroupVoiceSessionMember {
            connection,
            user_id,
            audio_sender: self.sender.clone(),
            mixed_receiver: self.mixed_sender.subscribe(),
        }
    }
    pub async fn remove_user(&self, user_id: u32) {
        self.members.lock().await.remove(&user_id);
    }
    /// Runs the mix loop - spawn this as its own task
    pub async fn run(&self, app: &'static App) {
        let mut mix_ticker =
            tokio::time::interval(tokio::time::Duration::from_millis(MIX_INTERVAL_MS));
        let mut shared_audio_receiver = self.shared_audio_receiver.lock().await;

        loop {
            tokio::select! {
                // Drain inbound packets into per-member jitter buffers
                Some(packet) = shared_audio_receiver.recv() => {
                    self.members.lock().await
                        .entry(packet.0)
                        .or_insert_with(|| StaticRb::default())
                        .push_overwrite(packet.1); // overwrite on overflow = drop oldest
                }

                // Every 20ms, mix and broadcast
                _ = mix_ticker.tick() => {
                    self.mix_and_broadcast().await;
                }
                _ = app.cancellation_token.cancelled() => {
                    tracing::info!("GroupVoiceSession shutting down");
                    break;
                }
            }
        }
    }

    async fn mix_and_broadcast(&self) {
        if self.mixed_sender.receiver_count() == 0 {
            return;
        }

        // 960 samples = 20ms at 48kHz - standard RTP ptime, required by Opus
        const FRAME_SIZE: usize = 960;

        let mut pcm_buf: Vec<i16> = vec![0i16; FRAME_SIZE];
        let mut combined_pcm: Vec<i16> = vec![0i16; FRAME_SIZE];
        let mut opus_buf: Vec<u8> = vec![0u8; 4000];
        let mut has_audio = false;
        let mut decoder = self.decoder.lock().await;

        // First collect ALL packets into combined_pcm, THEN mix
        for (&_user_id, buffer) in self.members.lock().await.iter_mut() {
            if let Some(packet) = buffer.try_pop() {
                match decoder.decode(&packet.payload, &mut pcm_buf, false) {
                    Ok(decoded_len) => {
                        // decoded_len should equal FRAME_SIZE for CBR Opus
                        for (i, sample) in combined_pcm[..decoded_len].iter_mut().enumerate() {
                            // Saturating add prevents i16 overflow clipping artifacts
                            *sample = sample.saturating_add(pcm_buf[i]);
                        }
                        has_audio = true;
                    }
                    Err(e) => {
                        tracing::warn!("Opus decode failed for user {_user_id}: {e}");
                    }
                }
                // Reset pcm_buf for next member
                pcm_buf.fill(0);
            }
        }

        if !has_audio {
            return;
        }
        let mut encoder = self.encoder.lock().await;
        match encoder.encode(&combined_pcm, &mut opus_buf) {
            Ok(n) => {
                opus_buf.truncate(n);
                static SEQUENCE: AtomicU16 = AtomicU16::new(0);
                static TIMESTAMP: AtomicU32 = AtomicU32::new(1000);
                const MIXED_SSRC: RtpSsrc = 0;

                let seq = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let ts =
                    TIMESTAMP.fetch_add(FRAME_SIZE as u32, std::sync::atomic::Ordering::Relaxed);

                let _ = self.mixed_sender.send(RtpPacket::new(
                    RtpHeader::new(111u8, seq, ts, MIXED_SSRC),
                    opus_buf.into(),
                ));
            }
            Err(e) => {
                tracing::error!("Opus encode failed: {e}");
            }
        }
    }
}
