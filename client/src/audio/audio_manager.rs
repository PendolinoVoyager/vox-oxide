use std::sync::Arc;
use std::sync::Mutex;

use quinn::{Connection, VarInt};
use tokio::sync::mpsc::Receiver;

use crate::{
    app::App,
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
#[derive(Debug)]
pub struct AudioManager {
    app_config: AppConfig,
    signal_sender: Option<tokio::sync::mpsc::Sender<AudioManagerSignal>>,
    stream_error: Arc<Mutex<Option<anyhow::Error>>>,
    active_session: Option<RoomActiveAudioSession>,
    muted: bool,
    talking: bool,
}
impl AudioManager {
    pub fn new(app_config: AppConfig) -> Self {
        Self {
            app_config,
            active_session: None,
            stream_error: Arc::new(Mutex::new(None)),
            signal_sender: None,
            muted: false,
            talking: false,
        }
    }
    pub fn join_room(&mut self, room_id: u32) {
        if self.is_errored() {
            self.stream_error = Arc::new(Mutex::new(None));
        }
        if self.active() {
            tracing::warn!("Cannot join new room while being in another one");
            return;
        }
        tracing::info!("Joining room {}", room_id);
        let config = self.app_config.clone();
        // send a request to join a room
        self.active_session = Some(RoomActiveAudioSession::default());
        let (sender, receiver) = tokio::sync::mpsc::channel::<AudioManagerSignal>(12);
        self.signal_sender = Some(sender);
        let error = self.stream_error.clone();
        let mut playing = !self.get_muted();
        tokio::spawn(async move {
            if let Err(e) = Self::handle_audio_streaming(config, receiver, playing).await {
                error.lock().unwrap().replace(e);
            }
        });

        // join room
        // spawn a task to send audio (control via muted and talking to playbac)
        // if not talking, don't send
        // else send unconditionally if volume is above ceratain threshold
        // when exit_room is called, just close send and api request to terminate session
    }
    async fn handle_audio_streaming(
        config: AppConfig,
        mut receiver: Receiver<AudioManagerSignal>,
        playing: bool,
    ) -> anyhow::Result<()> {
        let mut connection = create_audio_connection(config).await?;
        Self::authenticate_audio_connection(&mut connection).await?;
        let mut audio_source = audio::audio_source::RTPOpusAudioSource::new(playing)?;
        loop {
            tokio::select! {
                Some(signal) = receiver.recv() => {
                    tracing::info!("Received signal: {}", signal);
                    match signal {
                        AudioManagerSignal::EXIT => {
                            tracing::info!("got signal 0!");
                            connection.close(VarInt::from_u32(0), b"done");

                        }
                        AudioManagerSignal::MUTE => {
                            audio_source.set_playing(false).await;
                        },
                        AudioManagerSignal::UNMUTE => {
                            audio_source.set_playing(true).await;
                        }
                    }
            }

                Some(packet) = audio_source.read() => {
                    let bytes = packet.serialize().unwrap();
                    let res = connection.send_datagram(bytes);
                    if let Err(e) = res {
                        tracing::error!("Failed task: {e}");
                        break;
                    }

                }
            }
        }

        Ok(())
    }
    pub fn exit_room(&mut self) {
        self.send_signal(AudioManagerSignal::EXIT);
        self.stream_error.clear_poison();
        self.stream_error = Arc::new(Mutex::new(None));
        self.active_session = None;
        self.signal_sender = None;
    }
    fn send_signal(&mut self, signal: AudioManagerSignal) {
        if let Some(ss) = self.signal_sender.clone() {
            tokio::spawn(async move { ss.send(signal).await });
        }
    }
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if let Some(ss) = &self.signal_sender {
            let _ = ss.try_send(if muted {
                AudioManagerSignal::MUTE
            } else {
                AudioManagerSignal::UNMUTE
            });
        }
    }
    pub fn get_muted(&self) -> bool {
        return self.muted;
    }
    pub fn active(&self) -> bool {
        self.active_session.is_some() || self.signal_sender.is_some()
    }
    pub fn is_errored(&self) -> bool {
        self.stream_error.clone().lock().ok().is_some()
    }
    pub fn get_error<'a>(&'a self) -> Arc<Mutex<Option<anyhow::Error>>> {
        self.stream_error.clone()
    }
    async fn authenticate_audio_connection(connection: &mut Connection) -> anyhow::Result<()> {
        let (mut rx, mut tx) = connection.open_bi().await?;
        rx.write_all(b"itsame :D").await?;
        rx.finish()?;
        let response = tx.read_to_end(1024).await?;
        tracing::info!("{}", String::from_utf8_lossy(&response));
        Ok(())
    }
}
