//! Backup manifest, ported from `adzimzf/sqlite-incremental-backup` (`src/manifest.rs`).
//! Same JSON shape, plus the fields LangChef needs to keep the chain in Google Drive and to tell
//! devices apart (`device_id`, `device_os`, `drive_file_id`, `page_count`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub version: u32,
    pub source_database: String,
    pub page_size: u32,
    pub backups: Vec<BackupEntry>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum BackupType {
    Full,
    Delta,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackupEntry {
    pub id: u64,
    #[serde(rename = "type")]
    pub backup_type: BackupType,
    pub timestamp: DateTime<Utc>,
    pub file_name: String,
    pub file_size_bytes: u64,
    /// Blake3 hex checksum of the stored file. Unique per backup, so it also names the chain head.
    pub blake3_checksum: String,
    pub pages_changed: Option<u32>,
    pub parent_id: Option<u64>,
    #[serde(default)]
    pub page_count: u32,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub device_os: String,
    #[serde(default)]
    pub drive_file_id: Option<String>,
}

impl Manifest {
    pub fn new(page_size: u32) -> Self {
        Manifest {
            version: MANIFEST_VERSION,
            source_database: "langchef.db".into(),
            page_size,
            backups: Vec::new(),
        }
    }

    pub fn next_id(&self) -> u64 {
        self.backups.last().map_or(0, |e| e.id) + 1
    }

    pub fn latest(&self) -> Option<&BackupEntry> {
        self.backups.last()
    }

    pub fn is_empty(&self) -> bool {
        self.backups.is_empty()
    }

    /// Consecutive deltas at the end of the chain.
    pub fn delta_chain_length(&self) -> u32 {
        self.backups
            .iter()
            .rev()
            .take_while(|e| e.backup_type == BackupType::Delta)
            .count() as u32
    }

    pub fn should_create_full_backup(&self, max_delta_chain: u32) -> bool {
        self.is_empty() || self.delta_chain_length() >= max_delta_chain
    }

    /// The entries needed to rebuild the latest state: the last full backup and every delta after it.
    pub fn restore_chain(&self) -> Option<&[BackupEntry]> {
        let start = self.backups.iter().rposition(|e| e.backup_type == BackupType::Full)?;
        Some(&self.backups[start..])
    }

    /// Drops every chain older than the newest `keep_fulls` full backups and returns the dropped
    /// entries, so their files can be deleted.
    pub fn prune(&mut self, keep_fulls: usize) -> Vec<BackupEntry> {
        let fulls: Vec<usize> = self
            .backups
            .iter()
            .enumerate()
            .filter(|(_, e)| e.backup_type == BackupType::Full)
            .map(|(i, _)| i)
            .collect();
        if keep_fulls == 0 || fulls.len() <= keep_fulls {
            return Vec::new();
        }
        let cut = fulls[fulls.len() - keep_fulls];
        self.backups.drain(..cut).collect()
    }
}
