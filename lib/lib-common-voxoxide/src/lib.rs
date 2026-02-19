#![allow(unused)]

mod raw;
mod serde;

#[cfg(feature = "serde")]
pub mod types {
    pub use crate::serde::ars_auth::ArsAuthRequestSerde as ArsAuthRequest;
    pub use crate::serde::ars_auth::AuthErrorSerde as ArsAuthError;
}

#[cfg(not(feature = "serde"))]
pub mod types {
    pub use crate::raw::ars_auth::ArsAuthRequestRaw as ArsAuthRequest;
    pub use crate::raw::ars_auth::AuthErrorRaw as ArsAuthError;
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_to_string_serde() {
        let error = crate::serde::ars_auth::AuthErrorSerde::InvalidAuthRequestReceived;
        assert_eq!(error.to_string(), "InvalidAuthRequestReceived");
    }
}
