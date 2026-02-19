//! This module contains the GroupVoiceSession struct.
//! A Group Voice Session is created, when at least one user joins a room and creates a session.
//! Other users joining the room will be assigned to this GroupVoiceSession, bringing their own session with them.

use std::collections::HashMap;

use rvoip_rtp_core::RtpPacket;

pub struct GroupVoiceSessionMember {
    pub packet_buffer: Vec<RtpPacket>,
}
pub struct GroupVoiceSession {
    /// Members grouped by ssrc
    _members: HashMap<u32, GroupVoiceSessionMember>,
}
