use bytes::Bytes;
use quinn::VarInt;
use tokio::{io::AsyncWriteExt, sync::mpsc::Receiver};

use crate::{
    app::App,
    app_config::AppConfig,
    audio::{self, create_audio_connection},
};

struct AudioPacket();
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
    active_session: Option<RoomActiveAudioSession>,
    muted: bool,
    talking: bool,
}
impl AudioManager {
    pub fn new(app_config: AppConfig) -> Self {
        Self {
            app_config,
            active_session: None,
            signal_sender: None,
            muted: false,
            talking: false,
        }
    }
    pub fn join_room(&mut self, room_id: u32) {
        let config = self.app_config.clone();
        // send a request to join a room
        self.active_session = Some(RoomActiveAudioSession::default());
        let (sender, receiver) = tokio::sync::mpsc::channel::<u8>(12);
        self.signal_sender = Some(sender);

        tokio::spawn(async move { Self::handle_audio_streaming(config, receiver).await });

        // join room
        // spawn a task to send audio (control via muted and talking to playbac)
        // if not talking, don't send
        // else send unconditionally if volume is above ceratain threshold
        // when exit_room is called, just close send and api request to terminate session
    }
    async fn handle_audio_streaming(config: AppConfig, mut receiver: Receiver<u8>) {
        let connection = create_audio_connection(config).await.unwrap();
        let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await.unwrap();

        let mut audio_source = audio::audio_source::RTPOpusAudioSource::new().unwrap();
        loop {
            tokio::select! {
                signal = receiver.recv() => {
                    if signal == Some(0) {
                        tracing::info!("got signal 0!");
                        connection.close(VarInt::from_u32(0), b"done");
                    drop(audio_source);
                    break;
                }
            }

                Some(packet) = audio_source.read() => {
                    let bytes = packet.serialize().unwrap();
                    let _ = socket.send_to(&bytes, "127.0.0.1:10000").await;
                    let res = connection.send_datagram(bytes);
                    if let Err(e) = res {
                        tracing::error!("Failed task: {e}");
                        break;
                    }
                }
            }
        }
    }
    pub fn exit_room(&mut self) {
        if self.active_session.is_none() {
            return;
        }
        self.send_signal(0);
        self.active_session = None;
        self.signal_sender = None;
    }
    fn send_signal(&mut self, signal: u8) {
        if let Some(ss) = self.signal_sender.clone() {
            tokio::spawn(async move { ss.send(signal).await });
        }
    }
    pub fn set_muted(&mut self, muted: bool) {}
    pub fn set_talking(&mut self, talking: bool) {}
    pub fn active(&self) -> bool {
        self.active_session.is_some()
    }
}
