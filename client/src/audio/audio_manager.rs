use std::sync::Arc;
use std::sync::Mutex;

use bytes::Bytes;
use quinn::{Connection, VarInt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::Receiver;

use crate::{
    app::App,
    app_config::AppConfig,
    audio::{self, create_audio_connection},
};

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
    signal_sender: Option<tokio::sync::mpsc::Sender<u8>>,
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
        if self.active() {
            tracing::warn!("Cannot join new room while being in another one");
            return;
        }
        if self.is_errored() {
            self.exit_room();
        }
        let config = self.app_config.clone();
        // send a request to join a room
        self.active_session = Some(RoomActiveAudioSession::default());
        let (sender, receiver) = tokio::sync::mpsc::channel::<u8>(12);
        self.signal_sender = Some(sender);
        let error = self.stream_error.clone();
        tokio::spawn(async move {
            if let Err(e) = Self::handle_audio_streaming(config, receiver).await {
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
        mut receiver: Receiver<u8>,
    ) -> anyhow::Result<()> {
        let mut connection = create_audio_connection(config).await.unwrap();
        Self::authenticate_audio_connection(&mut connection).await?;
        let mut audio_source = audio::audio_source::RTPOpusAudioSource::new().unwrap();
        loop {
            tokio::select! {
                Some(signal) = receiver.recv() => {
                if signal == 0 {
                        tracing::info!("got signal 0!");
                        connection.close(VarInt::from_u32(0), b"done");
                    break;
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
        if self.active_session.is_none() {
            return;
        }
        self.send_signal(0);
        self.stream_error.clear_poison();
        self.stream_error = Arc::new(Mutex::new(None));
        self.active_session = None;
        self.signal_sender = None;
    }
    fn send_signal(&mut self, signal: u8) {
        if let Some(ss) = self.signal_sender.clone() {
            tokio::spawn(async move { ss.send(signal).await });
        }
    }

    pub fn active(&self) -> bool {
        self.active_session.is_some() || self.signal_sender.is_some()
    }
    pub fn is_errored(&self) -> bool {
        self.stream_error.clone().lock().ok().is_some()
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
