use std::sync::Arc;
use std::sync::Mutex;

use lib_common_voxoxide::types::ArsAuthRequest;
use quinn::{Connection, VarInt};
use tokio::sync::mpsc::Receiver;

use crate::{
    app_config::AppConfig,
    audio::{self, create_audio_connection},
};

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum AudioManagerSignal {
    EXIT,
    MUTE,
    UNMUTE,
}
impl std::fmt::Display for AudioManagerSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AudioManagerSignal::EXIT => "EXIT",
            AudioManagerSignal::MUTE => "MUTE",
            AudioManagerSignal::UNMUTE => "UNMUTE",
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
    pub stream_error: Option<anyhow::Error>,
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
    Finished,
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
    pub fn join_room(&mut self, room_id: u32) {
        tracing::info!("{:?}", self.get_state());
        if matches!(
            self.get_state(),
            AudioManagerState::Connected | AudioManagerState::Connecting
        ) {
            tracing::warn!("Already in a room");
            return;
        }

        let mut data = self.internal_data.lock().unwrap();
        data.cleanup();
        data.state = AudioManagerState::Connecting;

        tracing::info!("Joining room {}", room_id);

        let (sender, receiver) = tokio::sync::mpsc::channel(12);
        data.signal_sender = Some(sender.clone());

        let shared_data = self.internal_data.clone();
        drop(data); // IMPORTANT: release lock before spawning

        let config = self.app_config.clone();
        tokio::spawn(async move {
            if let Err(e) =
                Self::handle_audio_streaming(config, receiver, shared_data.clone()).await
            {
                tracing::error!("ARS Connection error: {e}");
                let mut data = shared_data.lock().unwrap();
                data.cleanup();
                data.state = AudioManagerState::Errored;
                data.stream_error = Some(e);
            }
        });
    }

    async fn handle_audio_streaming(
        config: AppConfig,
        mut receiver: Receiver<AudioManagerSignal>,
        shared_data: Arc<Mutex<AudioManagerInternalData>>,
    ) -> anyhow::Result<()> {
        let mut connection = create_audio_connection(config).await?;
        let play = !shared_data.lock().unwrap().muted;
        Self::authenticate_audio_connection(&mut connection)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed authentication: {e}: close reason: {:?}",
                    connection.close_reason()
                )
            })?;
        // only after authenticating are we in a session
        shared_data.lock().unwrap().active_session = Some(RoomActiveAudioSession::default());
        shared_data.lock().unwrap().state = AudioManagerState::Connected;

        let mut audio_source = audio::audio_source::RTPOpusAudioSource::new(play)?;
        loop {
            tokio::select! {

                Some(signal) = receiver.recv() => {
                    tracing::info!("Received signal: {}", signal);

                    match signal {
                        AudioManagerSignal::EXIT => {
                            connection.close(VarInt::from_u32(0), b"done");
                            shared_data.lock().unwrap().cleanup();
                            shared_data.lock().unwrap().state = AudioManagerState::Finished;
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

    async fn authenticate_audio_connection(connection: &mut Connection) -> anyhow::Result<()> {
        let (mut rx, mut tx) = connection.open_bi().await?;
        rx.write_all(&serde_json::ser::to_vec(&ArsAuthRequest::new()).unwrap()[..])
            .await?;
        rx.finish()?;
        let response = tx.read_to_end(1024).await?;
        tracing::info!("{}", String::from_utf8_lossy(&response));
        Ok(())
    }
}
