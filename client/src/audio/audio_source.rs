use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use opus::{Application, Channels, Encoder};
use rvoip_rtp_core::{RtpHeader, RtpPacket, RtpSequenceNumber};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;

const SAMPLE_RATE: u32 = 48000;
const CHANNELS: Channels = Channels::Mono;
const FRAME_SIZE: usize = 960; // 20ms at 48kHz

pub struct RTPOpusAudioSource {
    receiver: mpsc::Receiver<RtpPacket>,
    _stream: cpal::Stream, // keep alive
}

impl RTPOpusAudioSource {
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();
        let input_devices = host.input_devices().unwrap();
        for d in input_devices {
            tracing::info!("{:?}", d.description());
        }
        tracing::info!("DEFAULT DEVICE *****************\n");
        let device = host
            .default_input_device()
            .expect("No input device available");
        tracing::info!("{:?}", device.description());
        tracing::info!("DEFAULT DEVICE *****************\n");

        let config = cpal::StreamConfig {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            buffer_size: cpal::BufferSize::Default,
        };

        let encoder = Arc::new(Mutex::new(Encoder::new(
            SAMPLE_RATE,
            CHANNELS,
            Application::Voip,
        )?));
        let (tx, rx) = mpsc::channel::<RtpPacket>(32);

        let pcm_buffer = Arc::new(Mutex::new(Vec::<f32>::new()));

        let mut sequence_no = 0;
        let mut start_time = 1200;
        let ssrc = rand::random_range(0..u32::MAX / 2);
        let stream = device.build_input_stream(
            &config,
            {
                let encoder = encoder.clone();
                let pcm_buffer = pcm_buffer.clone();
                let tx = tx.clone();

                move |data: &[f32], _| {
                    let mut pcm = pcm_buffer.lock().unwrap();
                    pcm.extend_from_slice(data);

                    while pcm.len() >= FRAME_SIZE {
                        let frame: Vec<f32> = pcm.drain(..FRAME_SIZE).collect();

                        let mut output = vec![0u8; 4000];
                        let mut encoder = encoder.lock().unwrap();

                        if let Ok(len) = encoder.encode_float(&frame, &mut output) {
                            output.truncate(len);
                            let output = bytes::Bytes::from_iter(output.into_iter());
                            let packet = create_rtp_packet(sequence_no, start_time, ssrc, output);
                            sequence_no += 1;
                            start_time += 160;
                            // non-blocking send (drop if channel full)
                            let _ = tx.try_send(packet);
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
            receiver: rx,
            _stream: stream,
        })
    }

    /// Async read of next Opus packet
    pub async fn read(&mut self) -> Option<RtpPacket> {
        self.receiver.recv().await
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
