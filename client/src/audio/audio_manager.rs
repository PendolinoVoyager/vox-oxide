use std::sync::Arc;
use std::sync::Mutex;

use lib_common_voxoxide::types::ArsAuthRequest;
use quinn::{Connection, VarInt};
use rvoip_rtp_core::RtpPacket;
use tokio::sync::mpsc::Receiver;

use crate::audio::audio_sink::AudioSink;
use crate::audio::create_audio_connection;
use crate::{
    app_config::AppConfig,
    audio::{self},
};

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum AudioManagerSignal {
    EXIT,
    MUTE,
    UNMUTE,
    SETVOLUME(f32),
}
impl std::fmt::Display for AudioManagerSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AudioManagerSignal::EXIT => "EXIT",
            AudioManagerSignal::MUTE => "MUTE",
            AudioManagerSignal::UNMUTE => "UNMUTE",
            AudioManagerSignal::SETVOLUME(_) => "SETVOLUME",
        })
    }
}

#[derive(Debug, Default)]
pub struct RoomActiveAudioSession {
    _session_id: u32,
    _session_key: u32,
    _user_id: u32,
    _mixing: u8,
    _room_id: u32,
}
#[derive(Debug, Default)]
pub struct AudioManagerInternalData {
    pub active_session: Option<RoomActiveAudioSession>,
    pub stream_error: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    pub muted: bool,
    pub signal_sender: Option<tokio::sync::mpsc::Sender<AudioManagerSignal>>,
    pub state: AudioManagerState,
}
impl AudioManagerInternalData {
    pub fn cleanup(&mut self) {
        if let Some(sender) = &self.signal_sender {
            let _ = sender.try_send(AudioManagerSignal::EXIT);
        }

        self.active_session = None;
        self.signal_sender = None;
        self.stream_error = None;
        self.state = AudioManagerState::Idle;
    }
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum AudioManagerState {
    #[default]
    Idle,
    Connecting,
    Connected,
    Errored,
}

#[derive(Debug)]
pub struct AudioManager {
    app_config: AppConfig,
    internal_data: Arc<Mutex<AudioManagerInternalData>>,
}

impl AudioManager {
    pub fn new(app_config: AppConfig) -> Self {
        Self {
            app_config,
            internal_data: Arc::new(Mutex::new(AudioManagerInternalData::default())),
        }
    }
    pub fn join_room(&mut self, room_id: u32) -> anyhow::Result<()> {
        let receiver = self.cleanup_and_init_for_connection(room_id)?;

        let config = self.app_config.clone();
        let data = self.internal_data.clone();
        tokio::spawn(async move {
            if let Err(e) = Self::handle_audio_streaming(config, receiver, data.clone()).await {
                tracing::error!("Error while connecting: {e}");
                let mut data = data.lock().unwrap();
                data.cleanup();
                data.state = AudioManagerState::Errored;
                data.stream_error = Some(e.into_boxed_dyn_error());
            }
        });
        Ok(())
    }
    #[cfg(test)]
    async fn join_room_async(&mut self, room_id: u32) -> anyhow::Result<()> {
        let receiver = self.cleanup_and_init_for_connection(room_id)?;

        let config = self.app_config.clone();
        let data = self.internal_data.clone();

        if let Err(e) = Self::handle_audio_streaming(config, receiver, data.clone()).await {
            tracing::error!("Error while connecting: {e}");
            let mut data = data.lock().unwrap();
            data.cleanup();
            data.state = AudioManagerState::Errored;
            return Err(e);
        }
        Ok(())
    }
    fn cleanup_and_init_for_connection(
        &self,
        room_id: u32,
    ) -> anyhow::Result<Receiver<AudioManagerSignal>> {
        if matches!(
            self.get_state(),
            AudioManagerState::Connected | AudioManagerState::Connecting
        ) {
            tracing::warn!("Already in a room");
            return Err(anyhow::anyhow!("Already in a room"));
        }

        let mut data = self
            .internal_data
            .lock()
            .expect("Audio manager data not avaialable!");
        data.cleanup();
        data.state = AudioManagerState::Connecting;

        tracing::info!("Joining room {}", room_id);

        let (sender, receiver) = tokio::sync::mpsc::channel(12);
        data.signal_sender = Some(sender.clone());
        Ok(receiver)
    }
    async fn handle_audio_streaming(
        config: AppConfig,
        mut receiver: Receiver<AudioManagerSignal>,
        shared_data: Arc<Mutex<AudioManagerInternalData>>,
    ) -> anyhow::Result<()> {
        let mut connection = create_audio_connection(config).await?;
        authenticate_audio_connection(&mut connection)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed authentication: {e}: close reason: {:?}",
                    connection.close_reason()
                )
            })?;
        let play = !shared_data.lock().unwrap().muted;
        let mut audio_source = audio::audio_source::RTPOpusAudioSource::new(false)?;
        // only after authenticating are we in a session
        shared_data.lock().unwrap().active_session = Some(RoomActiveAudioSession::default());
        shared_data.lock().unwrap().state = AudioManagerState::Connected;

        audio_source.set_playing(play).await;
        let mut audio_sink = AudioSink::new()?;
        loop {
            tokio::select! {

                Some(signal) = receiver.recv() => {
                    tracing::info!("Received signal: {}", signal);

                    match signal {
                        AudioManagerSignal::EXIT => {
                            connection.close(VarInt::from_u32(0), b"done");
                            shared_data.lock().unwrap().cleanup();
                            shared_data.lock().unwrap().state = AudioManagerState::Idle;
                            break;
                        }
                        AudioManagerSignal::MUTE => {
                            audio_source.set_playing(false).await;
                            let mut state = shared_data.lock().unwrap();
                            state.muted = true;
                        }
                        AudioManagerSignal::UNMUTE => {
                            audio_source.set_playing(true).await;
                            let mut state = shared_data.lock().unwrap();
                            state.muted = false;
                        }
                        AudioManagerSignal::SETVOLUME(v) => {
                            audio_sink.set_volume(v);
                        }
                    }
                }
                Ok(dgram) = connection.read_datagram() => {
                    match RtpPacket::parse(&dgram) {
                        Ok(packet) => audio_sink.write_packet(packet)?,
                        Err(e) => tracing::warn!("RTP Packet cannot be parsed: {e}")
                    }

                }
                Some(packet) = audio_source.read() => {
                    let bytes = packet.serialize().unwrap();
                    if let Err(e) = connection.send_datagram(bytes) {
                        // returning error will update state anyway
                        return Err(e.into());
                    }
                }
            }
        }
        connection.close(0u8.into(), b"end stream");
        Ok(())
    }

    /// Basically everything clean-up.
    pub fn exit_room(&mut self) {
        self.internal_data.lock().unwrap().cleanup();
        self.set_state(AudioManagerState::Idle);
    }

    pub fn set_muted(&self, muted: bool) {
        let mut state = self.internal_data.lock().unwrap();
        state.muted = muted;

        if let Some(sender) = &state.signal_sender {
            let _ = sender.try_send(if muted {
                AudioManagerSignal::MUTE
            } else {
                AudioManagerSignal::UNMUTE
            });
        }
    }
    pub fn set_volume(&self, volume: f32) {
        let state = self.internal_data.lock().unwrap();
        if let Some(signal_sender) = &state.signal_sender {
            let _ = signal_sender.try_send(AudioManagerSignal::SETVOLUME(volume));
        }
    }

    pub fn get_muted(&self) -> bool {
        return self.internal_data.lock().unwrap().muted;
    }
    pub fn get_state(&self) -> AudioManagerState {
        self.internal_data.lock().unwrap().state.clone()
    }
    fn set_state(&self, new_state: AudioManagerState) {
        self.internal_data.lock().unwrap().state = new_state;
    }

    pub fn get_error(&self) -> Option<String> {
        self.internal_data
            .lock()
            .unwrap()
            .stream_error
            .as_ref()
            .map(|e| e.to_string())
    }
}

pub(crate) async fn authenticate_audio_connection(
    connection: &mut Connection,
) -> anyhow::Result<()> {
    let (mut rx, mut tx) = connection.open_bi().await?;
    rx.write_all(&serde_json::ser::to_vec(&ArsAuthRequest::new()).unwrap()[..])
        .await?;
    rx.finish()?;
    let response = tx.read_to_end(1024).await?;
    tracing::info!("{}", String::from_utf8_lossy(&response));
    Ok(())
}

#[cfg(test)]
mod audio_manager_tests {
    use std::{net::SocketAddr, str::FromStr};

    use rustls::crypto::CryptoProvider;

    use crate::{
        app_config::AppConfig,
        audio::audio_manager::{AudioManager, AudioManagerState},
    };
    fn create_test_config() -> AppConfig {
        let _ = CryptoProvider::install_default(rustls::crypto::aws_lc_rs::default_provider());
        AppConfig {
            audio_service_address: SocketAddr::from_str("127.0.0.1:4433").unwrap(),
            host: None,
            cert_path: Some("../dev-certs/dev-ca.pem".into()),
            bind: SocketAddr::from_str("127.0.0.1:0").unwrap(),
            log_file: "/dev/null".into(),
            log_level: "info".into(),
        }
    }
    #[tokio::test]
    async fn should_create_idle_manager() {
        let app_config = create_test_config();
        let audio_manager = AudioManager::new(app_config);
        assert_eq!(audio_manager.get_state(), AudioManagerState::Idle)
    }
    #[tokio::test]

    async fn should_error_on_no_connection() {
        let app_config = create_test_config();
        let mut audio_manager = AudioManager::new(app_config);
        let res = audio_manager.join_room_async(10).await;
        assert!(matches!(
            audio_manager.get_state(),
            AudioManagerState::Errored
        ));
        assert!(res.is_err())
    }
    #[tokio::test]
    async fn should_not_join_room_on_connecting_or_connected() {
        let app_config = create_test_config();
        let mut audio_manager = AudioManager::new(app_config);
        audio_manager.set_state(AudioManagerState::Connected);
        assert!(audio_manager.join_room(10).is_err());

        audio_manager.set_state(AudioManagerState::Connecting);
        assert!(audio_manager.join_room(10).is_err());
    }
}
