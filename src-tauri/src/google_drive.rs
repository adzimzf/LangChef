//! Google Drive backup with incremental uploads and conflict handling.
//!
//! Layout on Drive (`drive.file` scope, so only files this app made are visible):
//! a folder `LangChef Backup` holding `manifest.json` and the chain files the backup engine
//! (`backup::engine`) makes: `full_XXXX_*.db.zst` and `delta_XXXX_*.bin`. Each backup uploads only
//! the pages that changed since the previous one; every `MAX_DELTA_CHAIN` deltas a new full backup
//! starts a chain, and the two newest chains are kept.
//!
//! Conflicts: the manifest is a single line of backups that any of the user's devices may extend.
//! A device only extends it when the newest backup in the cloud is the one it last uploaded or
//! restored (`SyncConfig::synced_head`). Otherwise another device (or an earlier install) backed up
//! in the meantime, and the device stops backing up and records a `Conflict` until the user picks:
//! restore the cloud copy here, or keep this device's data (a new full backup on top; the cloud's
//! previous state stays in the older chain).
//!
//! The old single-file backup (`langchef_backup.db` in the Drive root) can still be restored.

use crate::backup::engine::{self, Plan};
use crate::backup::manifest::{BackupEntry, BackupType, Manifest};
use crate::google_auth::get_access_token;
use crate::locale::app_error;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const DRIVE_UPLOAD_URL: &str = "https://www.googleapis.com/upload/drive/v3/files";
const DRIVE_FILES_URL: &str = "https://www.googleapis.com/drive/v3/files";
const FOLDER_NAME: &str = "LangChef Backup";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
const MANIFEST_NAME: &str = "manifest.json";
const LEGACY_FILE_NAME: &str = "langchef_backup.db";
/// Chains kept in Drive (each a full backup plus its deltas).
const KEEP_CHAINS: usize = 2;
/// After a failed automatic backup, wait at least this long before trying again.
const RETRY_AFTER_SECS: i64 = 15 * 60;
pub const SERVICE_LABEL: &str = "LangChef auto-backup";

/// One sync at a time (background service, "Back up now" and restore share the files).
static SYNC_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// ─── Data Structures ──────────────────────────────────────────────────────

/// Old single-file backup metadata (`google_backup_meta.json`). Still read to recognise a
/// legacy backup this device made or restored.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackupMetadata {
    pub file_id: String,
    pub modified_time: String,
    pub size_bytes: u64,
    pub backed_up_at: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Conflict {
    /// `cloud_newer`: another device backed up after this one last synced.
    /// `never_synced`: the cloud has a backup this device never restored or made.
    pub reason: String,
    /// OS of the device that made the newest cloud backup (`android`, `macos`, …), if known.
    pub device_os: String,
    pub backed_up_at: i64,
    pub size: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SyncConfig {
    pub enabled: bool,
    pub interval_minutes: u64,
    pub last_sync_at: Option<i64>,
    pub wifi_only: bool,
    /// Random id of this install, written into every backup it makes.
    #[serde(default)]
    pub device_id: String,
    /// Checksum of the newest cloud backup this device made or restored.
    #[serde(default)]
    pub synced_head: Option<String>,
    #[serde(default)]
    pub conflict: Option<Conflict>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_attempt_at: Option<i64>,
}

impl Default for SyncConfig {
    fn default() -> Self {
        SyncConfig {
            enabled: false,
            interval_minutes: 60,
            last_sync_at: None,
            wifi_only: true,
            device_id: String::new(),
            synced_head: None,
            conflict: None,
            last_error: None,
            last_attempt_at: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Extend the cloud chain; stop on a conflict.
    Normal,
    /// The user chose to keep this device's data: start a new chain on top of the cloud's.
    Overwrite,
}

// ─── Local files ──────────────────────────────────────────────────────────

fn app_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e: tauri::Error| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// `<app data>/backup`: `baseline.db` (state of the newest backup this device knows),
/// `baseline.head` (that backup's checksum), `manifest.json` (last manifest seen) and `work/`.
fn backup_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_dir(app)?.join("backup");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn sync_config_file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_dir(app)?.join("google_sync_config.json"))
}

pub fn save_sync_config(app: &AppHandle, config: &SyncConfig) -> Result<(), String> {
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(sync_config_file(app)?, json).map_err(|e| e.to_string())
}

pub fn load_sync_config(app: &AppHandle) -> Result<SyncConfig, String> {
    let path = sync_config_file(app)?;
    let mut config: SyncConfig = if path.exists() {
        let json = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&json).unwrap_or_default()
    } else {
        SyncConfig::default()
    };
    if config.device_id.is_empty() {
        config.device_id = uuid::Uuid::new_v4().to_string();
        save_sync_config(app, &config)?;
    }
    Ok(config)
}

fn db_file_path(app: &AppHandle) -> Result<PathBuf, String> {
    crate::db::db_file(app)
}

fn load_legacy_metadata(app: &AppHandle) -> Option<BackupMetadata> {
    let path = app_dir(app).ok()?.join("google_backup_meta.json");
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

fn save_legacy_metadata(app: &AppHandle, meta: &BackupMetadata) -> Result<(), String> {
    let json = serde_json::to_string_pretty(meta).map_err(|e| e.to_string())?;
    fs::write(app_dir(app)?.join("google_backup_meta.json"), json).map_err(|e| e.to_string())
}

/// The baseline file, if it is the state of backup `head`.
fn baseline_for(dir: &Path, head: Option<&str>) -> Option<PathBuf> {
    let head = head?;
    let stored = fs::read_to_string(dir.join("baseline.head")).ok()?;
    let path = dir.join("baseline.db");
    (stored.trim() == head && path.exists()).then_some(path)
}

fn set_baseline_head(dir: &Path, head: &str) -> Result<(), String> {
    fs::write(dir.join("baseline.head"), head).map_err(|e| e.to_string())
}

fn clear_baseline(dir: &Path) {
    let _ = fs::remove_file(dir.join("baseline.db"));
    let _ = fs::remove_file(dir.join("baseline.head"));
}

fn save_local_manifest(dir: &Path, m: &Manifest) {
    if let Ok(json) = serde_json::to_vec_pretty(m) {
        let _ = fs::write(dir.join(MANIFEST_NAME), json);
    }
}

// ─── Google Drive API ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct DriveFile {
    id: String,
    modified_time: String,
    size: u64,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .unwrap_or_default()
}

async fn check(resp: reqwest::Response, what: &str) -> Result<reqwest::Response, String> {
    if resp.status().is_success() {
        return Ok(resp);
    }
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    Err(format!("{what} failed ({status}): {body}"))
}

async fn list_files(token: &str, query: &str) -> Result<Vec<DriveFile>, String> {
    let url = format!(
        "{DRIVE_FILES_URL}?q={}&orderBy=modifiedTime%20desc&fields=files(id,modifiedTime,size)&pageSize=100",
        urlencoding::encode(query)
    );
    let resp = client()
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("Network error searching Drive: {e}"))?;
    let body: serde_json::Value = check(resp, "Drive search").await?.json().await.map_err(|e| e.to_string())?;
    Ok(body["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .map(|f| DriveFile {
                    id: f["id"].as_str().unwrap_or("").to_string(),
                    modified_time: f["modifiedTime"].as_str().unwrap_or("").to_string(),
                    size: f["size"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
                })
                .filter(|f| !f.id.is_empty())
                .collect()
        })
        .unwrap_or_default())
}

async fn find_folder(token: &str) -> Result<Option<String>, String> {
    let q = format!("name='{FOLDER_NAME}' and mimeType='{FOLDER_MIME}' and trashed=false");
    Ok(list_files(token, &q).await?.into_iter().next().map(|f| f.id))
}

async fn ensure_folder(token: &str) -> Result<String, String> {
    if let Some(id) = find_folder(token).await? {
        return Ok(id);
    }
    let resp = client()
        .post(DRIVE_FILES_URL)
        .bearer_auth(token)
        .json(&serde_json::json!({ "name": FOLDER_NAME, "mimeType": FOLDER_MIME }))
        .send()
        .await
        .map_err(|e| format!("Network error creating Drive folder: {e}"))?;
    let body: serde_json::Value = check(resp, "Creating Drive folder").await?.json().await.map_err(|e| e.to_string())?;
    body["id"].as_str().map(String::from).ok_or_else(|| "Drive folder has no id".into())
}

async fn find_in_folder(token: &str, folder: &str, name: &str) -> Result<Option<DriveFile>, String> {
    let q = format!("name='{name}' and '{folder}' in parents and trashed=false");
    Ok(list_files(token, &q).await?.into_iter().next())
}

/// Old single-file backup in the Drive root.
async fn find_legacy(token: &str) -> Result<Option<DriveFile>, String> {
    let q = format!("name='{LEGACY_FILE_NAME}' and trashed=false");
    Ok(list_files(token, &q).await?.into_iter().next())
}

/// Creates a file in `folder` with a resumable upload (works for any size).
async fn upload_new(token: &str, folder: &str, name: &str, data: Vec<u8>, mime: &str) -> Result<String, String> {
    let c = client();
    let init = c
        .post(format!("{DRIVE_UPLOAD_URL}?uploadType=resumable"))
        .bearer_auth(token)
        .header("X-Upload-Content-Type", mime)
        .header("X-Upload-Content-Length", data.len())
        .json(&serde_json::json!({ "name": name, "parents": [folder], "appProperties": { "app": "langchef" } }))
        .send()
        .await
        .map_err(|e| format!("Upload failed: {e}"))?;
    let init = check(init, "Starting upload").await?;
    let location = init
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .ok_or("Drive did not return an upload address")?
        .to_string();
    let resp = c
        .put(location)
        .header(reqwest::header::CONTENT_TYPE, mime)
        .body(data)
        .send()
        .await
        .map_err(|e| format!("Upload failed: {e}"))?;
    let body: serde_json::Value = check(resp, "Upload").await?.json().await.map_err(|e| e.to_string())?;
    body["id"].as_str().map(String::from).ok_or_else(|| "Uploaded file has no id".into())
}

async fn update_content(token: &str, file_id: &str, data: Vec<u8>, mime: &str) -> Result<(), String> {
    let resp = client()
        .patch(format!("{DRIVE_UPLOAD_URL}/{file_id}?uploadType=media"))
        .bearer_auth(token)
        .header(reqwest::header::CONTENT_TYPE, mime)
        .body(data)
        .send()
        .await
        .map_err(|e| format!("Upload failed: {e}"))?;
    check(resp, "Updating manifest").await.map(|_| ())
}

async fn download(token: &str, file_id: &str) -> Result<Vec<u8>, String> {
    let resp = client()
        .get(format!("{DRIVE_FILES_URL}/{file_id}?alt=media"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("Download failed: {e}"))?;
    let bytes = check(resp, "Download").await?.bytes().await.map_err(|e| e.to_string())?;
    Ok(bytes.to_vec())
}

async fn delete_file(token: &str, file_id: &str) {
    let _ = client()
        .delete(format!("{DRIVE_FILES_URL}/{file_id}"))
        .bearer_auth(token)
        .send()
        .await;
}

/// The manifest in Drive: (file id, manifest).
async fn load_remote_manifest(token: &str, folder: &str) -> Result<Option<(String, Manifest)>, String> {
    let Some(file) = find_in_folder(token, folder, MANIFEST_NAME).await? else {
        return Ok(None);
    };
    let data = download(token, &file.id).await?;
    let m: Manifest = serde_json::from_slice(&data).map_err(|e| format!("Unreadable backup manifest: {e}"))?;
    Ok(Some((file.id, m)))
}

async fn write_remote_manifest(token: &str, folder: &str, file_id: Option<&str>, m: &Manifest) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(m).map_err(|e| e.to_string())?;
    match file_id {
        Some(id) => update_content(token, id, data, "application/json").await,
        None => upload_new(token, folder, MANIFEST_NAME, data, "application/json").await.map(|_| ()),
    }
}

async fn download_entry(token: &str, folder: &str, entry: &BackupEntry) -> Result<Vec<u8>, String> {
    let id = match &entry.drive_file_id {
        Some(id) => id.clone(),
        None => find_in_folder(token, folder, &entry.file_name)
            .await?
            .map(|f| f.id)
            .ok_or_else(|| format!("backup file {} is missing from Drive", entry.file_name))?,
    };
    download(token, &id).await
}

// ─── Conflict detection ───────────────────────────────────────────────────

fn conflict_with(entry: &BackupEntry, manifest: &Manifest, reason: &str) -> Conflict {
    Conflict {
        reason: reason.to_string(),
        device_os: entry.device_os.clone(),
        backed_up_at: entry.timestamp.timestamp(),
        size: manifest.restore_chain().map_or(0, |c| c.iter().map(|e| e.file_size_bytes).sum()),
    }
}

/// Whether this device may extend the cloud chain. `None` = go ahead.
fn detect_conflict(synced_head: Option<&str>, remote: &Manifest) -> Option<Conflict> {
    let head = remote.latest()?;
    match synced_head {
        Some(h) if h == head.blake3_checksum => None,
        Some(_) => Some(conflict_with(head, remote, "cloud_newer")),
        None => Some(conflict_with(head, remote, "never_synced")),
    }
}

fn legacy_conflict(file: &DriveFile) -> Conflict {
    Conflict {
        reason: "never_synced".into(),
        device_os: String::new(),
        backed_up_at: chrono::DateTime::parse_from_rfc3339(&file.modified_time).map_or(0, |t| t.timestamp()),
        size: file.size,
    }
}

// ─── Backup / restore ─────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct BackupOutcome {
    /// `full`, `delta` or `none` (nothing changed since the last backup).
    pub kind: String,
    /// Bytes uploaded.
    pub size: u64,
    pub pages_changed: Option<u32>,
    pub backed_up_at: i64,
}

fn record_conflict(app: &AppHandle, conflict: Conflict) -> String {
    if let Ok(mut cfg) = load_sync_config(app) {
        cfg.conflict = Some(conflict);
        let _ = save_sync_config(app, &cfg);
    }
    app_error("backup_conflict", "")
}

async fn backup_locked(app: &AppHandle, mode: Mode) -> Result<BackupOutcome, String> {
    let token = get_access_token(app).await?;
    let folder = ensure_folder(&token).await?;
    let cfg = load_sync_config(app)?;
    let dir = backup_dir(app)?;

    let remote = load_remote_manifest(&token, &folder).await?;
    let (manifest_id, mut manifest) = match remote {
        Some((id, m)) => (Some(id), m),
        None => (None, Manifest::new(0)),
    };

    if mode == Mode::Normal {
        if manifest.is_empty() {
            // Only an old single-file backup: fine if this device made or restored it.
            if let Some(legacy) = find_legacy(&token).await? {
                let ours = load_legacy_metadata(app).is_some_and(|m| m.file_id == legacy.id);
                if !ours && cfg.synced_head.is_none() {
                    return Err(record_conflict(app, legacy_conflict(&legacy)));
                }
            }
        } else if let Some(c) = detect_conflict(cfg.synced_head.as_deref(), &manifest) {
            return Err(record_conflict(app, c));
        }
    }

    let head = manifest.latest().map(|e| e.blake3_checksum.clone());
    let baseline = baseline_for(&dir, head.as_deref());
    let db_path = db_file_path(app)?;
    let work = dir.join("work");
    let plan = {
        let (manifest, device_id, force_full) = (manifest.clone(), cfg.device_id.clone(), mode == Mode::Overwrite);
        tokio::task::spawn_blocking(move || {
            let opts = engine::Options {
                max_delta_chain: engine::MAX_DELTA_CHAIN,
                compression_level: engine::COMPRESSION_LEVEL,
                force_full,
                device_id: &device_id,
                device_os: std::env::consts::OS,
            };
            engine::prepare_backup(&db_path, &work, &manifest, baseline.as_deref(), &opts)
        })
        .await
        .map_err(|e| e.to_string())??
    };

    let now = chrono::Utc::now().timestamp();
    let prepared = match plan {
        Plan::NoChanges => {
            let mut cfg = load_sync_config(app)?;
            cfg.last_sync_at = Some(now);
            cfg.conflict = None;
            save_sync_config(app, &cfg)?;
            return Ok(BackupOutcome { kind: "none".into(), size: 0, pages_changed: Some(0), backed_up_at: now });
        }
        Plan::Upload(p) => *p,
    };

    let data = fs::read(&prepared.file).map_err(|e| e.to_string())?;
    let mut entry = prepared.entry.clone();
    let uploaded = match upload_new(&token, &folder, &entry.file_name, data, "application/octet-stream").await {
        Ok(id) => id,
        Err(e) => {
            engine::discard(prepared);
            return Err(e);
        }
    };
    entry.drive_file_id = Some(uploaded.clone());

    // Another device may have written while we uploaded: re-read just before replacing the
    // manifest and give up (keeping the cloud as it is) if its newest backup changed.
    let latest_remote = load_remote_manifest(&token, &folder).await?;
    let remote_head = latest_remote.as_ref().and_then(|(_, m)| m.latest()).map(|e| e.blake3_checksum.clone());
    if remote_head != head {
        delete_file(&token, &uploaded).await;
        engine::discard(prepared);
        let c = latest_remote
            .as_ref()
            .and_then(|(_, m)| m.latest().map(|e| conflict_with(e, m, "cloud_newer")))
            .unwrap_or(Conflict { reason: "cloud_newer".into(), device_os: String::new(), backed_up_at: now, size: 0 });
        return Err(record_conflict(app, c));
    }

    // The chain's page size follows its newest full backup.
    manifest.page_size = prepared.page_size;
    manifest.backups.push(entry.clone());
    let dropped = manifest.prune(KEEP_CHAINS);
    if let Err(e) = write_remote_manifest(&token, &folder, manifest_id.as_deref(), &manifest).await {
        delete_file(&token, &uploaded).await;
        engine::discard(prepared);
        return Err(e);
    }
    for old in &dropped {
        if let Some(id) = &old.drive_file_id {
            delete_file(&token, id).await;
        }
    }

    engine::commit(prepared, &dir.join("baseline.db"))?;
    set_baseline_head(&dir, &entry.blake3_checksum)?;
    save_local_manifest(&dir, &manifest);

    let mut cfg = load_sync_config(app)?;
    cfg.synced_head = Some(entry.blake3_checksum.clone());
    cfg.last_sync_at = Some(now);
    cfg.conflict = None;
    cfg.last_error = None;
    save_sync_config(app, &cfg)?;

    Ok(BackupOutcome {
        kind: if entry.backup_type == BackupType::Full { "full".into() } else { "delta".into() },
        size: entry.file_size_bytes,
        pages_changed: entry.pages_changed,
        backed_up_at: now,
    })
}

async fn backup_database(app: &AppHandle, mode: Mode) -> Result<BackupOutcome, String> {
    let _guard = SYNC_LOCK.lock().await;
    let result = backup_locked(app, mode).await;
    let mut cfg = load_sync_config(app)?;
    cfg.last_attempt_at = Some(chrono::Utc::now().timestamp());
    match &result {
        Err(e) if !e.contains("backup_conflict") => cfg.last_error = Some(e.clone()),
        _ => cfg.last_error = None,
    }
    save_sync_config(app, &cfg)?;
    result
}

/// Puts a restored database in place of the current one (the old one is kept as `.pre_restore`).
fn replace_database(app: &AppHandle, restored: &Path) -> Result<u64, String> {
    let db_path = db_file_path(app)?;
    let tmp = db_path.with_extension("db.restoring");
    fs::copy(restored, &tmp).map_err(|e| format!("Failed to write restored database: {e}"))?;
    if db_path.exists() {
        let _ = fs::copy(&db_path, db_path.with_extension("db.pre_restore"));
        fs::remove_file(&db_path).map_err(|e| format!("Failed to remove old database: {e}"))?;
    }
    let _ = fs::remove_file(db_path.with_extension("db-journal"));
    fs::rename(&tmp, &db_path).map_err(|e| format!("Failed to rename restored database: {e}"))?;
    Ok(fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0))
}

async fn restore_locked(app: &AppHandle) -> Result<u64, String> {
    let token = get_access_token(app).await?;
    let dir = backup_dir(app)?;
    let work = dir.join("work");
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let restored = work.join("restored.db");

    let folder = find_folder(&token).await?;
    let remote = match &folder {
        Some(f) => load_remote_manifest(&token, f).await?,
        None => None,
    };

    if let (Some(folder), Some((_, manifest))) = (&folder, &remote) {
        if let Some(chain) = manifest.restore_chain() {
            let mut files = Vec::with_capacity(chain.len());
            for entry in chain {
                files.push((entry.clone(), download_entry(&token, folder, entry).await?));
            }
            let out = restored.clone();
            tokio::task::spawn_blocking(move || engine::restore_chain(&files, &out))
                .await
                .map_err(|e| e.to_string())??;
            let size = replace_database(app, &restored)?;

            let head = chain.last().expect("non-empty chain").blake3_checksum.clone();
            fs::rename(&restored, dir.join("baseline.db")).map_err(|e| e.to_string())?;
            set_baseline_head(&dir, &head)?;
            save_local_manifest(&dir, manifest);
            let mut cfg = load_sync_config(app)?;
            cfg.synced_head = Some(head);
            cfg.conflict = None;
            cfg.last_error = None;
            cfg.last_sync_at = Some(chrono::Utc::now().timestamp());
            save_sync_config(app, &cfg)?;
            return Ok(size);
        }
    }

    // Old single-file backup.
    let file = find_legacy(&token).await?.ok_or_else(|| app_error("no_backup", ""))?;
    let data = download(&token, &file.id).await?;
    if data.is_empty() {
        return Err("Downloaded backup file is empty".into());
    }
    fs::write(&restored, &data).map_err(|e| e.to_string())?;
    let size = replace_database(app, &restored)?;
    let _ = fs::remove_file(&restored);
    clear_baseline(&dir);
    save_legacy_metadata(
        app,
        &BackupMetadata {
            file_id: file.id.clone(),
            modified_time: file.modified_time.clone(),
            size_bytes: size,
            backed_up_at: chrono::Utc::now().timestamp(),
        },
    )?;
    let mut cfg = load_sync_config(app)?;
    cfg.synced_head = None;
    cfg.conflict = None;
    cfg.last_error = None;
    save_sync_config(app, &cfg)?;
    Ok(size)
}

pub async fn restore_database(app: &AppHandle) -> Result<u64, String> {
    let _guard = SYNC_LOCK.lock().await;
    restore_locked(app).await
}

/// Backup status for Settings: newest backup in the cloud, conflict, last error.
pub async fn get_backup_status(app: &AppHandle) -> Result<serde_json::Value, String> {
    let cfg = load_sync_config(app)?;
    if !crate::google_auth::get_auth_status(app)? {
        return Ok(serde_json::json!({ "signed_in": false, "has_backup": false, "last_backup": null, "backup_size": null }));
    }
    let base = serde_json::json!({
        "signed_in": true,
        "conflict": cfg.conflict,
        "last_error": cfg.last_error,
    });
    let with = |mut v: serde_json::Value, extra: serde_json::Value| {
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        v
    };

    let remote = async {
        let token = get_access_token(app).await?;
        let manifest = match find_folder(&token).await? {
            Some(f) => load_remote_manifest(&token, &f).await?.map(|(_, m)| m),
            None => None,
        };
        let legacy = if manifest.as_ref().is_none_or(|m| m.is_empty()) { find_legacy(&token).await? } else { None };
        Ok::<_, String>((manifest, legacy))
    }
    .await;

    // Offline: fall back to what this device last saw.
    let (manifest, legacy) = match remote {
        Ok(r) => r,
        Err(_) => {
            let local = fs::read(backup_dir(app)?.join(MANIFEST_NAME)).ok().and_then(|d| serde_json::from_slice::<Manifest>(&d).ok());
            (local, None)
        }
    };

    if let Some(m) = manifest.filter(|m| !m.is_empty()) {
        let head = m.latest().expect("non-empty");
        let chain = m.restore_chain().unwrap_or(&[]);
        return Ok(with(
            base,
            serde_json::json!({
                "has_backup": true,
                "last_backup": head.timestamp.timestamp(),
                "backup_size": chain.iter().map(|e| e.file_size_bytes).sum::<u64>(),
                "last_kind": if head.backup_type == BackupType::Full { "full" } else { "delta" },
                "last_device_os": head.device_os,
                "last_from_this_device": head.device_id == cfg.device_id,
                "chain_length": chain.len(),
            }),
        ));
    }
    if let Some(f) = legacy {
        return Ok(with(
            base,
            serde_json::json!({
                "has_backup": true,
                "last_backup": chrono::DateTime::parse_from_rfc3339(&f.modified_time).map_or(0, |t| t.timestamp()),
                "backup_size": f.size,
                "last_kind": "legacy",
            }),
        ));
    }
    Ok(with(base, serde_json::json!({ "has_backup": false, "last_backup": null, "backup_size": null })))
}

// ─── Automatic backup ─────────────────────────────────────────────────────

/// Network state: (connected, unmetered). Desktop connections count as unmetered.
async fn network_state(app: &AppHandle) -> (bool, bool) {
    #[cfg(target_os = "android")]
    {
        if let Some(h) = app.try_state::<crate::google_auth::mobile::GoogleAuthHandle<tauri::Wry>>() {
            if let Ok(s) = crate::google_auth::mobile::network_status(h.inner()) {
                return (s.connected, s.unmetered);
            }
        }
        (true, true)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        (true, true)
    }
}

pub fn update_sync_config(app: &AppHandle, enabled: Option<bool>, interval_minutes: Option<u64>, wifi_only: Option<bool>) -> Result<SyncConfig, String> {
    let mut config = load_sync_config(app)?;
    if let Some(e) = enabled {
        config.enabled = e;
    }
    if let Some(i) = interval_minutes {
        config.interval_minutes = i.max(5);
    }
    if let Some(w) = wifi_only {
        config.wifi_only = w;
    }
    save_sync_config(app, &config)?;
    Ok(config)
}

/// Called every minute by the background service: backs up when due, allowed and safe.
pub async fn trigger_auto_sync(app: &AppHandle) -> Result<serde_json::Value, String> {
    let config = load_sync_config(app)?;
    let skip = |reason: &str| Ok(serde_json::json!({ "synced": false, "reason": reason }));
    if !config.enabled {
        return skip("auto_sync_disabled");
    }
    if !crate::google_auth::get_auth_status(app)? {
        return skip("not_signed_in");
    }
    if config.conflict.is_some() {
        return skip("conflict");
    }
    let now = chrono::Utc::now().timestamp();
    let interval = config.interval_minutes as i64 * 60;
    if let Some(last) = config.last_sync_at {
        if now - last < interval {
            return Ok(serde_json::json!({ "synced": false, "reason": "not_due_yet", "next_sync_in_seconds": interval - (now - last) }));
        }
    }
    if config.last_error.is_some() && config.last_attempt_at.is_some_and(|t| now - t < RETRY_AFTER_SECS.min(interval)) {
        return skip("backing_off");
    }
    let (connected, unmetered) = network_state(app).await;
    if !connected {
        return skip("offline");
    }
    if config.wifi_only && !unmetered {
        return skip("waiting_for_wifi");
    }
    Ok(sync_result(backup_database(app, Mode::Normal).await))
}

fn sync_result(r: Result<BackupOutcome, String>) -> serde_json::Value {
    match r {
        Ok(o) => serde_json::json!({
            "synced": o.kind != "none",
            "reason": if o.kind == "none" { "no_changes" } else { "backup_completed" },
            "kind": o.kind,
            "size": o.size,
            "backed_up_at": o.backed_up_at,
        }),
        Err(e) if e.contains("backup_conflict") => serde_json::json!({ "synced": false, "reason": "conflict" }),
        Err(e) => serde_json::json!({ "synced": false, "reason": "error", "error": e }),
    }
}

/// Starts the background backup service when automatic backup is on (at app launch on every
/// platform, and when the user turns it on), or stops it.
pub fn apply_auto_sync_service(app: &AppHandle) {
    use tauri_plugin_background_service::{ServiceError, ServiceManagerHandle, StartConfig};
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let enabled = load_sync_config(&app).map(|c| c.enabled).unwrap_or(false)
            && crate::google_auth::get_auth_status(&app).unwrap_or(false);
        let Some(manager) = app.try_state::<ServiceManagerHandle<tauri::Wry>>() else {
            return;
        };
        if enabled {
            let config = StartConfig {
                service_label: SERVICE_LABEL.to_string(),
                foreground_service_type: "dataSync".to_string(),
            };
            match manager.start(app.clone(), config).await {
                Ok(()) | Err(ServiceError::AlreadyRunning) => {}
                Err(e) => eprintln!("[google_drive] couldn't start auto-backup service: {e}"),
            }
        } else if manager.is_running().await {
            let _ = manager.stop().await;
        }
    });
}

// ─── Tauri Commands ───────────────────────────────────────────────────────

#[tauri::command]
pub async fn google_backup_db(app: AppHandle) -> Result<serde_json::Value, String> {
    let o = backup_database(&app, Mode::Normal).await?;
    Ok(serde_json::json!({ "success": true, "kind": o.kind, "backed_up_at": o.backed_up_at, "size": o.size }))
}

/// Conflict resolved by keeping this device's data: back up over the cloud copy.
#[tauri::command]
pub async fn google_backup_overwrite(app: AppHandle) -> Result<serde_json::Value, String> {
    let o = backup_database(&app, Mode::Overwrite).await?;
    Ok(serde_json::json!({ "success": true, "kind": o.kind, "backed_up_at": o.backed_up_at, "size": o.size }))
}

#[tauri::command]
pub async fn google_restore_db(app: AppHandle) -> Result<serde_json::Value, String> {
    let size = restore_database(&app).await?;
    Ok(serde_json::json!({ "success": true, "restored_at": chrono::Utc::now().timestamp(), "size": size }))
}

#[tauri::command]
pub async fn google_get_backup_status(app: AppHandle) -> Result<serde_json::Value, String> {
    get_backup_status(&app).await
}

fn sync_config_json(config: &SyncConfig) -> serde_json::Value {
    serde_json::json!({
        "enabled": config.enabled,
        "interval_minutes": config.interval_minutes,
        "wifi_only": config.wifi_only,
        "last_sync_at": config.last_sync_at,
        "conflict": config.conflict,
        "last_error": config.last_error,
        // Only Android can tell Wi-Fi from mobile data.
        "wifi_only_supported": cfg!(target_os = "android"),
    })
}

#[tauri::command]
pub fn google_set_auto_sync(app: AppHandle, enabled: bool, interval_minutes: u64, wifi_only: bool) -> Result<serde_json::Value, String> {
    let config = update_sync_config(&app, Some(enabled), Some(interval_minutes), Some(wifi_only))?;
    apply_auto_sync_service(&app);
    Ok(sync_config_json(&config))
}

#[tauri::command]
pub fn google_get_auto_sync(app: AppHandle) -> Result<serde_json::Value, String> {
    Ok(sync_config_json(&load_sync_config(&app)?))
}

/// "Sync now": backs up right away, whatever the schedule or network.
#[tauri::command]
pub async fn google_trigger_sync(app: AppHandle) -> Result<serde_json::Value, String> {
    Ok(sync_result(backup_database(&app, Mode::Normal).await))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(sum: &str) -> BackupEntry {
        BackupEntry {
            id: 1,
            backup_type: BackupType::Full,
            timestamp: chrono::Utc::now(),
            file_name: "f".into(),
            file_size_bytes: 10,
            blake3_checksum: sum.into(),
            pages_changed: None,
            parent_id: None,
            page_count: 1,
            device_id: "other".into(),
            device_os: "android".into(),
            drive_file_id: None,
        }
    }

    #[test]
    fn conflicts() {
        let mut m = Manifest::new(4096);
        assert_eq!(detect_conflict(None, &m), None, "empty cloud");
        m.backups.push(entry("abc"));
        assert_eq!(detect_conflict(Some("abc"), &m), None, "cloud is what we last synced");
        assert_eq!(detect_conflict(Some("old"), &m).unwrap().reason, "cloud_newer");
        let c = detect_conflict(None, &m).unwrap();
        assert_eq!((c.reason.as_str(), c.device_os.as_str(), c.size), ("never_synced", "android", 10));
    }
}
