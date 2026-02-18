# Audio Relay Service

env ARS_CONFIG_PATH - specify a path to config file, for container use mainly 











Ideas:

QUIC for transport

Look for audio transport and playback protocol...
If not, create a simple one  with sequence, session_id and m.type=ping/data header, c.length
Data is pure opus

For session service:
mixing: MCU vs SFU - default is sfu and if you want to, the server will use mcu if the app struggles 
audio server will look for session based on auth on first connection and init it appropriately
Session service will emit events created, deleted
Session service will have session_id that will be used when user joins a chat.

ChatRoomHistory {
    room_id, ChatMessage[] 
}

ChatMessage {
    user_id, text, timestamp
}

Room {
    room_id, max_users, name, category, allow_voice
}

RoomActiveVoiceSession {
    session_id, room_id, session_key, user_id, mixing, created_at 
} -> immidiate archive to make session_id u32 safely, index by user_id also in archive (SQL probably) 

UnnamedProtoSend:
    |   8   |   8   |   8   |   8   |
    |  Session ID   |   Session Key | 32 Bytes overhead!!!  Packet of 1200 by default
    |   SequenceNo  |M.Type |  Len  | 3% for security is very nice
    |           Data ....           |

UnnamedProtoReceive:
    |   8   |   8   |   8   |   8   |
    |           User ID             | User Id -1 if mix is mcu
    |   SequenceNo  |  Mix  |  Len  | 3% for security is very nice
    |           Data ....           |

So... to join a voice room creates a session and pings the endpoint to keep it active. 
audio-relay-service getting the session_id and authenticating with session_key will receive data.
The packet will get redirected to a sink created for a room the session is in.

Basically 
receivers = Vec<Receiver<Packet>>
every 5ms:
for user in Room {
    if mcu {
        data = mux(receivers.all_except_users(user))
        user.sender.send(UnnamedProtoReceivePacket(data)) 
    }
    if ftu {
        for other_user that isn't current user {
            if let Some(p) = other_user.read_packet() {
                user.sender.send(UnnamedProtoReceivePacket(p))
            }
        } 
    }
}
and for r in receivers {
    if 
}     