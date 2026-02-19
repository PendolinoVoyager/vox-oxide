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
    session_id: u32,
    session_key: u32,
    user_id: u32,
    mixing: u8,
    room_id: u32,
}
#[derive(Debug, Default)]
pub struct AudioManagerState {
    pub active_session: Option<RoomActiveAudioSession>,
    pub stream_error: Option<anyhow::Error>,
    pub muted: bool,
    pub signal_sender: Option<tokio::sync::mpsc::Sender<AudioManagerSignal>>,
}

#[derive(Debug)]
pub struct AudioManager {
    app_config: AppConfig,
    state: Arc<Mutex<AudioManagerState>>,
}

impl AudioManager {
    pub fn new(app_config: AppConfig) -> Self {
        Self {
            app_config,
            state: Arc::new(Mutex::new(AudioManagerState::default())),
        }
    }
    pub fn join_room(&self, room_id: u32) {
        let mut state = self.state.lock().unwrap();

        if state.active_session.is_some() {
            tracing::warn!("Already in a room");
            return;
        }

        tracing::info!("Joining room {}", room_id);

        state.stream_error = None;

        let (sender, receiver) = tokio::sync::mpsc::channel(12);
        state.signal_sender = Some(sender.clone());

        let config = self.app_config.clone();
        let shared_state = self.state.clone();

        drop(state); // IMPORTANT: release lock before spawning

        tokio::spawn(async move {
            if let Err(e) =
                Self::handle_audio_streaming(config, receiver, shared_state.clone()).await
            {
                tracing::error!("ARS Connection error: {e}");

                let mut state = shared_state.lock().unwrap();
                state.stream_error = Some(e);
                state.active_session = None;
                state.signal_sender = None;
            }
        });
    }

    async fn handle_audio_streaming(
        config: AppConfig,
        mut receiver: Receiver<AudioManagerSignal>,
        shared_state: Arc<Mutex<AudioManagerState>>,
    ) -> anyhow::Result<()> {
        let mut connection = create_audio_connection(config).await?;
        let play = !shared_state.lock().unwrap().muted;
        Self::authenticate_audio_connection(&mut connection)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed authentication: {e}: close reason: {:?}",
                    connection.close_reason()
                )
            })?;
        // only after authenticating are we in a session
        shared_state.lock().unwrap().active_session = Some(RoomActiveAudioSession::default());

        let mut audio_source = audio::audio_source::RTPOpusAudioSource::new(play)?;

        loop {
            tokio::select! {

                Some(signal) = receiver.recv() => {
                    tracing::info!("Received signal: {}", signal);

                    match signal {
                        AudioManagerSignal::EXIT => {
                            connection.close(VarInt::from_u32(0), b"done");
                            break;
                        }
                        AudioManagerSignal::MUTE => {
                            audio_source.set_playing(false).await;
                            let mut state = shared_state.lock().unwrap();
                            state.muted = true;
                        }
                        AudioManagerSignal::UNMUTE => {
                            audio_source.set_playing(true).await;
                            let mut state = shared_state.lock().unwrap();
                            state.muted = false;
                        }
                    }
                }

                Some(packet) = audio_source.read() => {
                    let bytes = packet.serialize().unwrap();
                    if let Err(e) = connection.send_datagram(bytes) {
                        return Err(e.into());
                    }
                }
            }
        }

        Ok(())
    }

    pub fn exit_room(&self) {
        let mut state = self.state.lock().unwrap();

        if let Some(sender) = &state.signal_sender {
            let _ = sender.try_send(AudioManagerSignal::EXIT);
        }

        state.active_session = None;
        state.signal_sender = None;
        state.stream_error = None;
    }

    pub fn set_muted(&self, muted: bool) {
        let mut state = self.state.lock().unwrap();
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
        return self.state.lock().unwrap().muted;
    }
    pub fn get_active(&self) -> bool {
        self.state.lock().unwrap().active_session.is_some()
    }
    pub fn is_errored(&self) -> bool {
        self.state.lock().unwrap().stream_error.is_some()
    }

    pub fn get_error(&self) -> Option<String> {
        self.state
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
