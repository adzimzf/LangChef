use async_trait::async_trait;
use std::time::Duration;
use tauri::Wry;
use tauri_plugin_background_service::{BackgroundService, ServiceContext, ServiceError};

const TICK_SECS: u64 = 60;
const FIRST_CHECK_SECS: u64 = 20;

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
        // First check shortly after start (app launch), then every minute. `trigger_auto_sync`
        // decides whether a backup is due, allowed on this network and free of conflicts.
        let start = tokio::time::Instant::now() + Duration::from_secs(FIRST_CHECK_SECS);
        let mut interval = tokio::time::interval_at(start, Duration::from_secs(TICK_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = ctx.shutdown.cancelled() => break,
                _ = interval.tick() => {
                    match crate::google_drive::trigger_auto_sync(&ctx.app).await {
                        Ok(result) => {
                            match result.get("reason").and_then(|v| v.as_str()) {
                                Some("backup_completed") => eprintln!("[background_sync] backup completed"),
                                Some("error") => eprintln!("[background_sync] backup failed: {}", result["error"]),
                                Some("conflict") => eprintln!("[background_sync] paused: cloud backup conflict"),
                                _ => {}
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
