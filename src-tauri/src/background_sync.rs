use async_trait::async_trait;
use std::time::Duration;
use tauri::Wry;
use tauri_plugin_background_service::{BackgroundService, ServiceContext, ServiceError};

const TICK_SECS: u64 = 60;

pub struct AutoSyncService;

impl AutoSyncService {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl BackgroundService<Wry> for AutoSyncService {
    async fn init(&mut self, _ctx: &ServiceContext<Wry>) -> Result<(), ServiceError> {
        Ok(())
    }

    async fn run(&mut self, ctx: &ServiceContext<Wry>) -> Result<(), ServiceError> {
        let mut interval = tokio::time::interval(Duration::from_secs(TICK_SECS));
        interval.tick().await;
        loop {
            tokio::select! {
                _ = ctx.shutdown.cancelled() => break,
                _ = interval.tick() => {
                    match crate::google_drive::trigger_auto_sync(&ctx.app).await {
                        Ok(result) => {
                            if result.get("synced").and_then(|v| v.as_bool()).unwrap_or(false) {
                                eprintln!("[background_sync] backup completed");
                            }
                        }
                        Err(e) => {
                            eprintln!("[background_sync] trigger failed: {}", e);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
