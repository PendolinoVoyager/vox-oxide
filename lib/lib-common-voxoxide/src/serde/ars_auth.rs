use core::fmt;
use derive_more::{Display, Error};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, Error, Display)]
#[serde(rename_all = "PascalCase")]
pub enum AuthErrorSerde {
    NoAuthRequestReceived,
    InvalidAuthRequestReceived,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArsAuthRequestSerde {
    pub room_id: u32,
}

impl ArsAuthRequestSerde {
    pub fn new() -> Self {
        Self { room_id: 10 }
    }
}
