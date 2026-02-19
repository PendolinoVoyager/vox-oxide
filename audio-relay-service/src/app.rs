use crate::common::app_config::AppConfig;

use quinn::Endpoint;
use tokio::signal::{self};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
pub struct App {
    ///Readonly config
    pub config: AppConfig,
    /// Token notifying of app shutdown
    pub cancellation_token: CancellationToken,
    /// Task tracker. Instead of using tokio::spawn use tracker.spawn
    task_tracker: TaskTracker,
}

impl App {
    pub fn new(config: AppConfig) -> &'static mut Self {
        let cancellation_token = CancellationToken::new();
        let task_tracker = TaskTracker::new();
        let app = Box::new(Self {
            config,
            cancellation_token,
            task_tracker,
        });
        Box::leak(app)
    }
    pub async fn run(&'static mut self) -> anyhow::Result<()> {
        let endpoint = self.create_endpoint()?;
        tracing::info!("listening on {}", endpoint.local_addr()?);
        tokio::spawn(self.main_loop(endpoint));
        self.handle_signal().await;
        self.task_tracker.close();
        self.task_tracker.wait().await;
        Ok(())
    }
    async fn main_loop(&'static self, endpoint: Endpoint) {
        let connection_limit = self.config.connection_limit;

        loop {
            tokio::select! {
                            Some(conn) = endpoint.accept() => {
                                if endpoint.open_connections() >= connection_limit {
                                    tracing::debug!("refusing due to open connection limit");
                                    conn.refuse();
                                } else if !conn.remote_address_validated() {
                                    tracing::debug!("requiring connection to validate its address");
                                    conn.retry().unwrap();
                                } else {
                                    tracing::info!("Accepted connection");
                                    let fut = crate::vc::handle_connection(self, conn);
                                    self.task_tracker.spawn(async move {
                                        if let Err(e) = fut.await {
                                            tracing::error!("connection failed: {reason}", reason = e.to_string())
                                        }
                                    });
                                }
                            },
                            _ = self.cancellation_token.cancelled()
            => {
                                tracing::info!("Stopping receiving new connections.");
                                break;
                            }


                        }
        }
    }
    fn create_endpoint(&'static self) -> anyhow::Result<Endpoint> {
        let options = self.config.clone();
        let (certs, key) = crate::common::security::certs::load_certs(&self.config)?;
        let server_config = crate::common::security::endpoint_config::create_server_config(
            &self.config,
            certs,
            key,
        )?;

        Ok(quinn::Endpoint::server(server_config, options.listen)?)
    }

    async fn handle_signal(&'static self) {
        match signal::ctrl_c().await {
            Ok(_) => {
                tracing::info!("Interrupt detected!");
                self.cancellation_token.cancel();
                tracing::info!("Sent exit signal. Waiting for jobs to finish...");
            }
            Err(e) => {
                tracing::error!("Cannot listen for interrupt, app closing: {e}");
            }
        }
    }
}
