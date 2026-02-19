use lib_common_voxoxide::types::{ArsAuthError, ArsAuthRequest};

use crate::app::App;

pub async fn auth_user_for_session(
    _app: &'static App,
    connection: &mut quinn::Connection,
) -> Result<(), ArsAuthError> {
    // Accept first bidirectional stream (control)
    let (mut send, mut recv) = connection
        .accept_bi()
        .await
        .map_err(|_| ArsAuthError::NoAuthRequestReceived)?;

    let auth_request = recv
        .read_to_end(1024)
        .await
        .map_err(|_| ArsAuthError::InvalidAuthRequestReceived)?; // too long - invalid request

    tracing::debug!(
        "Auth payload from {}: {:?}",
        connection.remote_address(),
        String::from_utf8_lossy(&auth_request)
    );
    let auth_request = serde_json::from_slice::<ArsAuthRequest>(&auth_request.as_slice())
        .map_err(|_| ArsAuthError::InvalidAuthRequestReceived)?;

    tracing::info!("Auth request: {:?}", auth_request);

    send.write_all(b"OK").await.unwrap();
    send.finish().unwrap();
    Ok(())
}
