//! Re-exports for voice-chat module handling audio parsing.

use std::u32;

use crate::app::App;
use anyhow::Result;
pub mod group_voice_session;

pub async fn handle_connection(app: &'static App, conn: quinn::Incoming) -> Result<()> {
    let mut connection = conn.await?;
    let auth_request =
        match crate::common::services::auth::auth_user_for_session(app, &mut connection).await {
            Ok(ar) => ar,
            Err(e) => {
                tracing::warn!("Unable to authenticate user: {e}");
                connection.close(0u8.into(), e.to_string().as_bytes());
                return Err(e.into());
            }
        };

    tracing::info!("Established connection with: {:?}", &auth_request);

    let mut session_member = app
        .session_store
        .register_user(rand::random_range(0..u32::MAX), connection);
    tracing::info!("User {} joined session!", session_member.user_id);
    tokio::select! {
            res = session_member.session_loop(app) => {
                if let Err(e) = res {
                    tracing::info!("Audio session ended with: {e}");
                }
                app.session_store.remove_user(session_member.user_id).await;
                tracing::info!("User {} exited session", session_member.user_id);
                return Ok(());
            }
            _ = app.cancellation_token.cancelled() => {
                tracing::debug!("Shutting down connection with {}", session_member.connection.remote_address());
                session_member.connection.close(1u32.into(), b"server shutdown");
                return Ok(());
        }
    }
}
