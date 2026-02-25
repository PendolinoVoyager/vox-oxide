use anyhow::Result;
use cpal::{
    BuildStreamError, Device, InputCallbackInfo, SampleFormat, StreamConfig, SupportedStreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use num_traits::AsPrimitive;
use opus::{Application, Channels, Encoder};
use rvoip_rtp_core::{RtpHeader, RtpPacket, RtpSequenceNumber};
use std::{
    iter::Sum,
    sync::{Arc, Mutex, atomic::AtomicBool},
    time::Duration,
};
use tokio::sync::mpsc::{Receiver, Sender};

const SAMPLE_RATE: u32 = 48000;
const CHANNELS: Channels = Channels::Mono;
const FRAME_SIZE: usize = 960; // 20ms at 48kHz
const BUF_SIZE: usize = 10; // 0.2s jitter max

const AUDIO_STREAM_TIMEOUT: Duration = Duration::from_secs(2);
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

        let stream = device.build_input_stream(
            &config.config(),
            Self::_data_callback_f32(
                encoder.clone(),
                playing.clone(),
                config.config().clone(),
                sender.clone(),
            ),
            move |err| {
                tracing::error!("Audio stream error: {:?}", err);
            },
            Some(AUDIO_STREAM_TIMEOUT),
        );
        let stream = match stream {
            Ok(s) => s,
            Err(BuildStreamError::StreamConfigNotSupported) => {
                tracing::warn!(
                    "Stream config not supported: {:?}\n Trying i16 fallback last time...",
                    &config
                );
                device.build_input_stream(
                    &config.config(),
                    Self::_data_callback_i16(
                        encoder,
                        playing.clone(),
                        config.config().clone(),
                        sender,
                    ),
                    move |err| {
                        tracing::error!("Audio stream error: {:?}", err);
                    },
                    Some(AUDIO_STREAM_TIMEOUT),
                )?
            }
            Err(_) => todo!(),
        };
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
    fn reduce_to_mono<'a, T>(channels: u16, data: &'a [T], pcm_buf: &'a mut Vec<T>)
    where
        T: Sum<&'a T> + Copy + num_traits::NumOps + num_traits::AsPrimitive<u16>,
        u16: AsPrimitive<T>,
    {
        if channels > 1 {
            let mono: Vec<T> = data
                .chunks_exact(channels as usize)
                .map(|frame| frame.iter().sum::<T>() / channels.as_())
                .collect();
            pcm_buf.extend_from_slice(&mono);
        } else {
            pcm_buf.extend_from_slice(data);
        }
    }
    fn get_config_for_device(device: &Device) -> Result<SupportedStreamConfig> {
        let mut supported_configs_range = device.supported_input_configs()?;
        tracing::debug!(
            "Found {:?} supported configs",
            supported_configs_range.clone().count()
        );
        for (i, c) in supported_configs_range.clone().enumerate() {
            tracing::debug!("{i}. supported config :\n{:?}", &c);
        }
        let supported_config_mono = supported_configs_range.clone().find(|c| {
            c.channels() == 1
                && c.min_sample_rate() <= SAMPLE_RATE
                && c.max_sample_rate() >= SAMPLE_RATE
                && matches!(c.sample_format(), SampleFormat::F32 | SampleFormat::I16)
        });
        if let Some(supported_config_mono) = supported_config_mono {
            return Ok(supported_config_mono.with_sample_rate(SAMPLE_RATE));
        }
        tracing::warn!("No mono input found, looking for more channels with downsampling");
        Ok(supported_configs_range
            .find(|c| {
                c.min_sample_rate() <= SAMPLE_RATE
                    && c.max_sample_rate() >= SAMPLE_RATE
                    && matches!(c.sample_format(), SampleFormat::F32 | SampleFormat::I16)
            })
            .ok_or_else(|| anyhow::anyhow!("Cannot find supported audio input config"))?
            .with_sample_rate(SAMPLE_RATE))
    }
    fn _data_callback_f32(
        encoder: Arc<Mutex<opus::Encoder>>,
        playing: Arc<AtomicBool>,
        config: StreamConfig,
        sender: Sender<RtpPacket>,
    ) -> impl FnMut(&[f32], &cpal::InputCallbackInfo) + Send + 'static {
        let cb = {
            let mut pcm_buffer = Vec::<f32>::with_capacity(1000);
            let playing: Arc<AtomicBool> = Arc::clone(&playing);
            let encoder = encoder.clone();
            // let mut filter = DefaultAudioFilter::new(NOISE_THRESHOLD);
            let ssrc = rand::random_range(0..u32::MAX / 2);
            let mut sequence_no = 0;
            let mut start_time = 1200;

            move |data: &[f32], _: &InputCallbackInfo| {
                // it's ok reaaaallyyyy...
                // The data will be produced in the background, but so what?
                // filter.transform(data);
                if !playing.load(std::sync::atomic::Ordering::Relaxed) {
                    pcm_buffer.clear();
                    return;
                }
                Self::reduce_to_mono(config.channels, data, &mut pcm_buffer);

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
        };
        cb
    }
    fn _data_callback_i16(
        encoder: Arc<Mutex<opus::Encoder>>,
        playing: Arc<AtomicBool>,
        config: StreamConfig,
        sender: Sender<RtpPacket>,
    ) -> impl FnMut(&[i16], &cpal::InputCallbackInfo) + Send + 'static {
        let cb = {
            let mut pcm_buffer = Vec::<i16>::with_capacity(1000);
            let playing: Arc<AtomicBool> = Arc::clone(&playing);
            let encoder = encoder.clone();
            let ssrc = rand::random_range(0..u32::MAX / 2);
            let mut sequence_no = 0;
            let mut start_time = 1200;

            move |data: &[i16], _: &InputCallbackInfo| {
                // it's ok reaaaallyyyy...
                // The data will be produced in the background, but so what?
                if !playing.load(std::sync::atomic::Ordering::Relaxed) {
                    pcm_buffer.clear();
                    return;
                }
                Self::reduce_to_mono(config.channels, data, &mut pcm_buffer);

                while pcm_buffer.len() >= FRAME_SIZE {
                    let frame: Vec<i16> = pcm_buffer.drain(..FRAME_SIZE).collect();

                    let mut output = vec![0u8; 4000];
                    let mut encoder = encoder.lock().unwrap();
                    if let Ok(len) = encoder.encode(&frame, &mut output) {
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
        };
        cb
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── reduce_to_mono ────────────────────────────────────────────────────────

    #[test]
    fn reduce_to_mono_passthrough_for_single_channel() {
        let data: Vec<f32> = vec![0.1, 0.2, 0.3];
        let mut buf = Vec::new();
        RTPOpusAudioSource::reduce_to_mono(1, &data, &mut buf);
        assert_eq!(buf, data);
    }

    #[test]
    fn reduce_to_mono_averages_stereo_f32() {
        // Stereo frames: [0.0, 1.0] and [0.5, 0.5]
        let data: Vec<f32> = vec![0.0, 1.0, 0.5, 0.5];
        let mut buf = Vec::new();
        RTPOpusAudioSource::reduce_to_mono(2, &data, &mut buf);
        assert_eq!(buf.len(), 2);
        assert!((buf[0] - 0.5).abs() < 1e-6, "expected 0.5, got {}", buf[0]);
        assert!((buf[1] - 0.5).abs() < 1e-6, "expected 0.5, got {}", buf[1]);
    }

    #[test]
    fn reduce_to_mono_averages_stereo_i16() {
        // Stereo: [100, 200] → 150,  [0, 0] → 0
        let data: Vec<i16> = vec![100, 200, 0, 0];
        let mut buf = Vec::new();
        RTPOpusAudioSource::reduce_to_mono(2, &data, &mut buf);
        assert_eq!(buf, vec![150i16, 0i16]);
    }

    #[test]
    fn reduce_to_mono_quad_channel_i16() {
        // Four channels: 100+200+300+400 = 1000 / 4 = 250
        let data: Vec<i16> = vec![100, 200, 300, 400];
        let mut buf = Vec::new();
        RTPOpusAudioSource::reduce_to_mono(4, &data, &mut buf);
        assert_eq!(buf, vec![250i16]);
    }

    #[test]
    fn reduce_to_mono_appends_to_existing_buffer() {
        let mut buf = vec![0.9f32];
        RTPOpusAudioSource::reduce_to_mono(1, &[0.1f32, 0.2], &mut buf);
        assert_eq!(buf, vec![0.9, 0.1, 0.2]);
    }

    #[test]
    fn reduce_to_mono_empty_input() {
        let mut buf: Vec<f32> = Vec::new();
        RTPOpusAudioSource::reduce_to_mono(2, &[], &mut buf);
        assert!(buf.is_empty());
    }

    // ── create_rtp_packet ─────────────────────────────────────────────────────

    #[test]
    fn rtp_packet_fields_round_trip() {
        let payload = bytes::Bytes::from_static(b"\x01\x02\x03");
        let packet = create_rtp_packet(42, 1200, 0xDEAD_BEEF, payload.clone());
        let hdr = packet.header;
        assert_eq!(hdr.sequence_number, 42);
        assert_eq!(hdr.timestamp, 1200);
        assert_eq!(hdr.ssrc, 0xDEAD_BEEF);
        assert_eq!(packet.payload, &payload);
    }

    #[test]
    fn rtp_packet_sequence_wraps_at_u16_max() {
        // Callers increment sequence_no freely; ensure u16 wrapping doesn't panic
        let packet = create_rtp_packet(u16::MAX, u32::MAX, 1, bytes::Bytes::new());
        assert_eq!(packet.header.sequence_number, u16::MAX);
    }

    #[cfg(feature = "audio-integration-tests")]
    mod integration {
        use super::super::*;
        use cpal::traits::{DeviceTrait, HostTrait};

        /// Regression test: get_config_for_device must call supported_INPUT_configs,
        /// not output. On Windows with a stereo 48 kHz device this used to fail
        /// because the wrong config list was queried.
        #[test]
        fn get_config_selects_input_config_at_48khz() {
            let host = cpal::default_host();
            let device = host
                .default_input_device()
                .expect("No input device available for integration test");

            let config = RTPOpusAudioSource::get_config_for_device(&device)
                .expect("get_config_for_device failed");

            assert_eq!(config.sample_rate(), 48000, "Sample rate must be 48000 Hz");
        }

        /// Verify that a source can be constructed and immediately produces
        /// packets when playing is enabled.
        #[tokio::test]
        async fn source_produces_packets_when_playing() {
            let mut source =
                RTPOpusAudioSource::new(true).expect("Failed to create RTPOpusAudioSource");

            // Give the audio thread up to 2 s to produce at least one packet.
            let packet = tokio::time::timeout(std::time::Duration::from_secs(2), source.read())
                .await
                .expect("Timed out waiting for first RTP packet")
                .expect("Channel closed unexpectedly");

            let hdr = packet.header;
            assert_eq!(
                hdr.sequence_number, 0,
                "First packet should have sequence 0"
            );
            assert_eq!(
                hdr.timestamp, 1200,
                "First packet should have timestamp 1200"
            );
            assert!(!packet.payload.is_empty(), "Payload must not be empty");
        }

        /// Verify that set_playing(false) stops packet production.
        #[tokio::test]
        async fn source_stops_when_paused() {
            let mut source =
                RTPOpusAudioSource::new(true).expect("Failed to create RTPOpusAudioSource");

            // Wait for at least one packet to confirm it was running.
            tokio::time::timeout(std::time::Duration::from_secs(2), source.read())
                .await
                .expect("Timed out waiting for initial packet");

            source.set_playing(false).await;

            // Drain any in-flight packets (up to 50 ms worth).
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            while source.receiver.try_recv().is_ok() {}

            // After draining, no new packets should arrive within 500 ms.
            let result =
                tokio::time::timeout(std::time::Duration::from_millis(500), source.read()).await;

            assert!(
                result.is_err(),
                "Expected timeout (no packets) while paused"
            );
        }

        /// Confirm the i16 fallback path can be exercised without panicking.
        /// This test only makes sense if the device actually supports i16 —
        /// on most Windows machines this is the native format.
        #[test]
        fn i16_fallback_does_not_panic() {
            // We can't force cpal to reject f32 in a unit test, but we can at
            // least verify _data_callback_i16 compiles and runs without panicking
            // by invoking it directly with synthetic data.
            use cpal::StreamConfig;
            use opus::{Application, Encoder};
            use std::sync::atomic::AtomicBool;
            use std::sync::{Arc, Mutex};
            use tokio::sync::mpsc;

            let encoder = Arc::new(Mutex::new(
                Encoder::new(SAMPLE_RATE, CHANNELS, Application::Voip).unwrap(),
            ));
            let playing = Arc::new(AtomicBool::new(true));
            let (tx, mut rx) = mpsc::channel(32);
            let config = StreamConfig {
                channels: 2,
                sample_rate: 48000,
                buffer_size: cpal::BufferSize::Default,
            };

            let mut cb = RTPOpusAudioSource::_data_callback_i16(encoder, playing, config, tx);

            // Feed two full stereo frames of silence (FRAME_SIZE mono samples
            // = FRAME_SIZE * 2 stereo samples).
            let stereo_silence = vec![0i16; FRAME_SIZE * 2 * 2];
            let info = unsafe {
                // InputCallbackInfo has no public constructor; zero-init is safe
                // because we never read from it in the callback.
                std::mem::zeroed::<cpal::InputCallbackInfo>()
            };
            cb(&stereo_silence, &info);

            // At least one packet should have been sent.
            assert!(
                rx.try_recv().is_ok(),
                "i16 callback produced no packets from silence"
            );
        }
    }
}
