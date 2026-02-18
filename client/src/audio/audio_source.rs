use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use opus::{Application, Channels, Encoder};
use rvoip_rtp_core::{RtpHeader, RtpPacket, RtpSequenceNumber};
use std::{
    sync::{Arc, Mutex, atomic::AtomicBool},
    time::Duration,
};
use tokio::sync::mpsc::Receiver;
const SAMPLE_RATE: u32 = 48000;
const CHANNELS: Channels = Channels::Mono;
const FRAME_SIZE: usize = 960; // 20ms at 48kHz
const BUF_SIZE: usize = 10; // 0.2s jitter max

pub struct RTPOpusAudioSource {
    receiver: Receiver<RtpPacket>,
    _stream: cpal::Stream,
    playing: Arc<AtomicBool>,
}

impl RTPOpusAudioSource {
    pub fn new(play_on_start: bool) -> Result<Self> {
        let host = cpal::default_host();

        let device = host
            .default_input_device()
            .expect("No input device available");
        tracing::info!("Selected default audio device {:?}", device.description());

        let config = cpal::StreamConfig {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            buffer_size: cpal::BufferSize::Default,
        };
        let playing = Arc::new(AtomicBool::new(play_on_start));
        let encoder = Arc::new(Mutex::new(Encoder::new(
            SAMPLE_RATE,
            CHANNELS,
            Application::Voip,
        )?));

        let (sender, receiver) = tokio::sync::mpsc::channel::<RtpPacket>(BUF_SIZE);

        let mut pcm_buffer = Vec::<f32>::new();
        let mut sequence_no = 0;
        let mut start_time = 1200;
        let ssrc = rand::random_range(0..u32::MAX / 2);
        let stream = device.build_input_stream(
            &config,
            {
                let playing = Arc::clone(&playing);
                let encoder = encoder.clone();

                move |data: &[f32], _| {
                    // it's ok reaaaallyyyy...
                    // The data will be produced in the background, but so what?
                    if !playing.load(std::sync::atomic::Ordering::Relaxed) {
                        pcm_buffer.clear();
                        return;
                    }
                    pcm_buffer.extend_from_slice(data);

                    while pcm_buffer.len() >= FRAME_SIZE {
                        let frame: Vec<f32> = pcm_buffer.drain(..FRAME_SIZE).collect();

                        let mut output = vec![0u8; 4000];
                        let mut encoder = encoder.lock().unwrap();

                        if let Ok(len) = encoder.encode_float(&frame, &mut output) {
                            output.truncate(len);
                            let output = bytes::Bytes::from_iter(output.into_iter());
                            let packet = create_rtp_packet(sequence_no, start_time, ssrc, output);
                            sequence_no += 1;
                            start_time += 160;
                            // non-blocking send (drop if channel full)
                            match sender.try_send(packet) {
                                Err(tokio::sync::mpsc::error::TrySendError::Closed { .. }) => {
                                    tracing::error!("e");
                                    break;
                                }
                                _ => (),
                            };
                        }
                    }
                }
            },
            move |err| {
                tracing::error!("Audio stream error: {:?}", err);
            },
            Some(Duration::from_secs(2)),
        )?;
        stream.play()?;

        Ok(Self {
            receiver,
            _stream: stream,
            playing,
        })
    }

    /// Async read of next Opus packet
    pub async fn read(&mut self) -> Option<RtpPacket> {
        self.receiver.recv().await
    }
    pub async fn set_playing(&mut self, playing: bool) {
        self.playing
            .store(playing, std::sync::atomic::Ordering::Relaxed);
    }
}

fn create_rtp_packet(
    sq_no: RtpSequenceNumber,
    timestamp: u32,
    ssrc: u32,
    payload: bytes::Bytes,
) -> RtpPacket {
    let rtp_header = RtpHeader::new(111, sq_no, timestamp, ssrc);
    rvoip_rtp_core::RtpPacket::new(rtp_header, payload)
}
