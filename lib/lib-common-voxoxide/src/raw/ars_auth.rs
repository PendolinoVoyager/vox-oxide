use core::fmt;

use derive_more::Error;

#[derive(Debug, Clone, Error)]
pub enum AuthErrorRaw {
    NoAuthRequestReceived,
    InvalidAuthRequestReceived,
}
impl fmt::Display for AuthErrorRaw {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid first item to double")
    }
}

#[derive(Debug, Clone)]
pub struct ArsAuthRequestRaw {
    placeholder_id: u32,
}
