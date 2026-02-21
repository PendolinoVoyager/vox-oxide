//! This module contains Send + Static Froup GroupVoiceSessionStore for main App struct.
//! It manages active sessions by roomId and handles creating / removing them when users switch.
//! It does not handle any audio processing unlike GroupVoiceSession

use std::{collections::BTreeMap, sync::Arc};

use tokio::sync::Mutex;

use crate::{common::app_config::AppConfig, vc::group_voice_session::GroupVoiceSession};

// pub enum SessionStoreError {
//     RoomSessionAlreadyExists,
// }
// type RoomId = u32;
// /// Struct containing rooms with active voice chats.
// pub struct SessionStore {
//     sessions: Arc<Mutex<BTreeMap<RoomId, GroupVoiceSession>>>,
// }

// impl SessionStore {
//     pub fn new(_config: &AppConfig) -> Self {
//         let ret = Self {
//             sessions: Arc::new(Mutex::new(BTreeMap::new())),
//         };
//         tokio::spawn(async {});
//         ret
//     }
//     async fn get_stale_sessions(&self) -> Vec<RoomId> {
//         let mut stale_sessions: Vec<RoomId> = Vec::with_capacity(10);
//         self.sessions
//             .lock()
//             .await
//             .iter()
//             .filter(|(_, s)| s.members.len() == 0)
//             .for_each(|(id, _)| stale_sessions.push(*id));
//         stale_sessions
//     }
//     pub async fn cleanup_stale_sessions(&self) -> usize {
//         let stale_sessions = self.get_stale_sessions().await;

//         for id in &stale_sessions {
//             // unfortunately, due to Mutex lock, they need to be removed sequentially
//             // no biggie probably since there shouldn't be many stale sessions
//             self.remove_session(*id).await;
//         }

//         stale_sessions.len()
//     }
//     pub async fn get_room_session(&self, room_id: RoomId) -> Option<&GroupVoiceSession> {
//         let session_lock = self.sessions.lock().await;

//         match session_lock.get(&room_id) {
//             Some(s) => Some(s),
//             None => {
//                 self.add_room_session(room_id);
//                 session_lock
//                     .get(&room_id)
//                     .expect("Session didn't create correctly!");
//             }
//         }
//     }
//     pub async fn add_room_session(&self, room_id: RoomId) -> Result<(), SessionStoreError> {
//         let mut session_lock = self.sessions.lock().await;
//         if session_lock.get(&room_id).is_some() {
//             return Err(SessionStoreError::RoomSessionAlreadyExists);
//         }
//         let new_session = GroupVoiceSession::new();
//         session_lock.insert(room_id, new_session);
//         Ok(())
//     }
//     pub async fn remove_session(&self, room_id: RoomId) {
//         let mut session_lock = self.sessions.lock().await;

//         if let Some(session) = session_lock.get(&room_id)
//             && session.members.len() != 0
//         {
//             //disconnect all
//             todo!()
//         }
//         session_lock.remove(&room_id);
//     }
// }
