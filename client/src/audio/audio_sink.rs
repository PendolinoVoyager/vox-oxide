use std::sync::{Arc, atomic::AtomicI32, mpsc::SendError};

use cpal::{
    OutputCallbackInfo, SupportedStreamConfigRange,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use rvoip_rtp_core::RtpPacket;
use std::sync::mpsc::{self};

// static AUDIO_QUEUE_SIZE: usize = 128;

static RECEIVED_SAMPLE_RATE: u32 = 48_000;

pub struct AudioSink {
    // Atomic f32 in be bytes actually
    volume: Arc<AtomicI32>,
    sender: mpsc::Sender<RtpPacket>,
    _stream: cpal::Stream,
}

impl AudioSink {
    pub fn new() -> anyhow::Result<Self> {
        let volume = Arc::new(AtomicI32::new(i32::from_be_bytes(1.0f32.to_be_bytes())));
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("No output device found"))?;

        let mut supported_configs_range = device.supported_output_configs()?;
        let supported_config = supported_configs_range
            .find(|c| Self::is_config_valid(c))
            .ok_or_else(|| anyhow::anyhow!("Cannot find supported config"))?
            .with_sample_rate(RECEIVED_SAMPLE_RATE);

        let (sender, receiver) = mpsc::channel::<RtpPacket>();
        let callback = Self::data_callback(volume.clone(), receiver)?;
        let stream = device.build_output_stream(
            &supported_config.config(),
            callback,
            move |e| {
                tracing::error!("Failed audio playback {e}");
            },
            None,
        )?;
        stream.play()?;
        Ok(Self {
            volume: volume.clone(),
            sender,
            _stream: stream,
        })
    }
    pub fn set_volume(&self, volume: f32) {
        self.volume.store(
            i32::from_be_bytes(volume.to_be_bytes()),
            std::sync::atomic::Ordering::Relaxed,
        );
    }
    fn data_callback(
        volume: Arc<AtomicI32>,
        receiver: mpsc::Receiver<RtpPacket>,
    ) -> anyhow::Result<impl FnMut(&mut [f32], &cpal::OutputCallbackInfo) + Send + 'static> {
        let cb = {
            let mut decoder = opus::Decoder::new(RECEIVED_SAMPLE_RATE, opus::Channels::Mono)?;
            move |data: &mut [f32], _: &OutputCallbackInfo| {
                let packet = match receiver.try_recv() {
                    Ok(packet) => packet,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        tracing::warn!("Audio Sink stopped existing, shutting down playback");
                        return;
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        tracing::debug!("Audio sink buf empty, try again next time");
                        return;
                    }
                };

                decoder.decode_float(&packet.payload, data, false).unwrap();
                let v_i32 = volume.load(std::sync::atomic::Ordering::Relaxed);
                let v_f32 = f32::from_be_bytes(v_i32.to_be_bytes()).clamp(0.0, 1.0);
                for sample in data {
                    *sample *= v_f32;
                    *sample = sample.clamp(-1.0, 1.0);
                }
            }
        };
        Ok(cb)
    }
    fn is_config_valid(supported: &SupportedStreamConfigRange) -> bool {
        48_000 >= supported.min_sample_rate()
            && 48_000 <= supported.max_sample_rate()
            && supported.channels() == 1
    }
    pub fn write_packet(&mut self, packet: RtpPacket) -> Result<(), SendError<RtpPacket>> {
        self.sender.send(packet)
    }
}
