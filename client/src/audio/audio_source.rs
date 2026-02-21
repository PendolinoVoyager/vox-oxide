use anyhow::Result;
use clap::builder::Str;
use cpal::{
    Device, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use opus::{Application, Channels, Encoder};
use rvoip_rtp_core::{RtpHeader, RtpPacket, RtpSequenceNumber};
use std::{
    sync::{Arc, Mutex, atomic::AtomicBool},
    time::Duration,
};
use tokio::sync::mpsc::Receiver;

use crate::audio::audio_filter::{AudioFilter, DefaultAudioFilter};
const SAMPLE_RATE: u32 = 48000;
const CHANNELS: Channels = Channels::Mono;
const FRAME_SIZE: usize = 960; // 20ms at 48kHz
const BUF_SIZE: usize = 10; // 0.2s jitter max
const NOISE_THRESHOLD: f32 = 0.05;
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
        let config = Self::get_config_for_device(&device)?;
        // config needs to be 48000, that shouldn't be a problem. The problem is channels.
        // If preferred config cannot be used due to channels, convert it to mono
        // If sample rate is different... idk then
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
                let mut filter = DefaultAudioFilter::new(NOISE_THRESHOLD);
                move |data: &[f32], _| {
                    // it's ok reaaaallyyyy...
                    // The data will be produced in the background, but so what?
                    // filter.transform(data);
                    if !playing.load(std::sync::atomic::Ordering::Relaxed) {
                        pcm_buffer.clear();
                        return;
                    }
                    Self::reduce_to_mono(config.channels, data, &mut pcm_buffer);

                    while pcm_buffer.len() >= FRAME_SIZE {
                        let mut frame: Vec<f32> = pcm_buffer.drain(..FRAME_SIZE).collect();
                        frame.iter_mut().for_each(|s| {
                            filter.transform_sample(s);
                        });

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
    fn reduce_to_mono(channels: u16, data: &[f32], pcm_buf: &mut Vec<f32>) {
        if channels > 1 {
            let mono: Vec<f32> = data
                .chunks_exact(channels as usize)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                .collect();
            pcm_buf.extend_from_slice(&mono);
        } else {
            pcm_buf.extend_from_slice(data);
        }
    }
    fn get_config_for_device(device: &Device) -> Result<StreamConfig> {
        static PREFERRED_CONFIG: StreamConfig = StreamConfig {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            buffer_size: cpal::BufferSize::Default,
        };
        if device.supported_input_configs()?.any(|r| {
            r.channels() == 1
                && r.min_sample_rate() <= SAMPLE_RATE
                && r.max_sample_rate() >= SAMPLE_RATE
        }) {
            Ok(cpal::StreamConfig {
                channels: 1,
                sample_rate: SAMPLE_RATE,
                buffer_size: cpal::BufferSize::Default,
            })
        } else {
            let ch = PREFERRED_CONFIG.channels;
            tracing::warn!(
                "Device does not support mono 48kHz; falling back to {} channel(s) with mixdown",
                ch
            );
            Ok(cpal::StreamConfig {
                channels: ch,
                sample_rate: SAMPLE_RATE,
                buffer_size: cpal::BufferSize::Default,
            })
        }
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
