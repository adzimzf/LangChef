//! Google Drive sync and backup.
//!
//! Everything lives in a `LangChef Backup` folder (`drive.file` scope: only files this app made
//! are visible):
//! - **Change files** `chg_<device>_<first>_<last>.jsonl.zst`: each device's row changes
//!   (`sync::export`), written only by that device. Every device applies the others' files with
//!   the row-level merge rules in `sync` — no device ever overwrites another device's data.
//! - **Snapshots** per device: `manifest_<device>.json` plus `full_*.db.zst` / `delta_*.bin` from
//!   the incremental backup engine (`backup::engine`). They let a new device start from a recent
//!   state, let a device that was away longer than change files are kept catch up, and are what
//!   "Restore" puts back.
//!
//! One sync: record this device's first sync if needed → export local changes → upload them →
//! download and apply other devices' new changes → snapshot (only changed pages) → prune old files.
//!
//! The old single-file backup (`langchef_backup.db` in the Drive root) can still be restored.

use crate::backup::engine::{self, Plan};
use crate::backup::manifest::{BackupEntry, BackupType, Manifest};
use crate::google_auth::get_access_token;
use crate::locale::app_error;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};

const DRIVE_UPLOAD_URL: &str = "https://www.googleapis.com/upload/drive/v3/files";
const DRIVE_FILES_URL: &str = "https://www.googleapis.com/drive/v3/files";
const FOLDER_NAME: &str = "LangChef Backup";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
const LEGACY_FILE_NAME: &str = "langchef_backup.db";
/// Snapshot chains kept per device (each a full backup plus its deltas).
const KEEP_CHAINS: usize = 2;
/// A device deletes its own change files this old, once it has a newer snapshot.
const KEEP_CHANGES_DAYS: i64 = 60;
/// After a failed automatic sync, wait at least this long before trying again.
const RETRY_AFTER_SECS: i64 = 15 * 60;
/// Shortest automatic sync interval the user can pick.
pub const MIN_INTERVAL_MINUTES: u64 = 5;
/// How often the foreground timer asks whether a sync is due.
const TICK_SECS: u64 = 60;
/// Opening (or returning to) the app syncs right away, unless the last sync is this recent.
const OPEN_GRACE_SECS: i64 = 2 * 60;
/// Event telling the UI an automatic sync started (`{"state":"syncing"}`) or ended (sync result).
pub const AUTO_SYNC_EVENT: &str = "auto-sync";

/// One sync at a time (automatic sync, "Sync now" and restore share the files).
static SYNC_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// ─── Settings and local files ─────────────────────────────────────────────

/// Old single-file backup metadata (`google_backup_meta.json`).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackupMetadata {
    pub file_id: String,
    pub modified_time: String,
    pub size_bytes: u64,
    pub backed_up_at: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SyncConfig {
    pub enabled: bool,
    pub interval_minutes: u64,
    pub last_sync_at: Option<i64>,
    pub wifi_only: bool,
    /// Random id of this install; names its change files and snapshots.
    #[serde(default)]
    pub device_id: String,
    /// Next sequence number for this device's change records. Kept here, not in the database,
    /// so it never goes back when a backup is restored.
    #[serde(default = "one")]
    pub next_seq: u64,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_attempt_at: Option<i64>,
}

fn one() -> u64 {
    1
}

impl Default for SyncConfig {
    fn default() -> Self {
        SyncConfig {
            enabled: false,
            interval_minutes: 60,
            last_sync_at: None,
            wifi_only: true,
            device_id: String::new(),
            next_seq: 1,
            last_error: None,
            last_attempt_at: None,
        }
    }
}

fn app_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e: tauri::Error| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// `<app data>/backup`: `baseline.db` + `baseline.head` (state of this device's newest snapshot),
/// `outgoing/` (change files waiting for upload), `work/`.
fn backup_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_dir(app)?.join("backup");
    fs::create_dir_all(dir.join("outgoing")).map_err(|e| e.to_string())?;
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

fn save_legacy_metadata(app: &AppHandle, meta: &BackupMetadata) -> Result<(), String> {
    let json = serde_json::to_string_pretty(meta).map_err(|e| e.to_string())?;
    fs::write(app_dir(app)?.join("google_backup_meta.json"), json).map_err(|e| e.to_string())
}

/// True when nothing was written to the database since the baseline snapshot was taken, so a
/// snapshot would find no changed pages. Skips copying and byte-diffing the whole database.
/// ponytail: trusts the file mtime; a write that leaves the mtime alone is only picked up by the
/// next sync after a real change. Swap for a `PRAGMA data_version` check if that ever bites.
fn unchanged_since(baseline: &Path, db_path: &Path) -> bool {
    let Ok(base) = fs::metadata(baseline).and_then(|m| m.modified()) else { return false };
    ["db", "db-wal"]
        .iter()
        .filter_map(|ext| fs::metadata(db_path.with_extension(ext)).and_then(|m| m.modified()).ok())
        .all(|t| t <= base)
}

fn baseline_for(dir: &Path, head: Option<&str>) -> Option<PathBuf> {
    let head = head?;
    let stored = fs::read_to_string(dir.join("baseline.head")).ok()?;
    let path = dir.join("baseline.db");
    (stored.trim() == head && path.exists()).then_some(path)
}

/// Runs database work off the async runtime, on its own connection.
async fn with_db<T: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce(&mut Connection) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        let mut conn = crate::db::open_db(&app)?;
        f(&mut conn)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ─── Google Drive API ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct DriveFile {
    id: String,
    name: String,
    modified_time: String,
    size: u64,
}

impl DriveFile {
    fn modified(&self) -> i64 {
        chrono::DateTime::parse_from_rfc3339(&self.modified_time).map_or(0, |t| t.timestamp())
    }
}

/// One shared client for the whole process: a sync is ~15 requests, and building a client per
/// request means a new TLS handshake per request.
fn client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .build()
            .unwrap_or_default()
    })
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
    let mut out = Vec::new();
    let mut page: Option<String> = None;
    loop {
        let mut url = format!(
            "{DRIVE_FILES_URL}?q={}&orderBy=modifiedTime%20desc&fields=nextPageToken,files(id,name,modifiedTime,size)&pageSize=1000",
            urlencoding::encode(query)
        );
        if let Some(p) = &page {
            url.push_str(&format!("&pageToken={}", urlencoding::encode(p)));
        }
        let resp = client()
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("Network error searching Drive: {e}"))?;
        let body: serde_json::Value = check(resp, "Drive search").await?.json().await.map_err(|e| e.to_string())?;
        for f in body["files"].as_array().into_iter().flatten() {
            let id = f["id"].as_str().unwrap_or("").to_string();
            if id.is_empty() {
                continue;
            }
            out.push(DriveFile {
                id,
                name: f["name"].as_str().unwrap_or("").to_string(),
                modified_time: f["modifiedTime"].as_str().unwrap_or("").to_string(),
                size: f["size"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
            });
        }
        match body["nextPageToken"].as_str() {
            Some(p) => page = Some(p.to_string()),
            None => return Ok(out),
        }
    }
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

async fn list_in_folder(token: &str, folder: &str, name_part: &str) -> Result<Vec<DriveFile>, String> {
    let q = format!("'{folder}' in parents and name contains '{name_part}' and trashed=false");
    list_files(token, &q).await
}

async fn find_in_folder(token: &str, folder: &str, name: &str) -> Result<Option<DriveFile>, String> {
    let q = format!("name='{name}' and '{folder}' in parents and trashed=false");
    Ok(list_files(token, &q).await?.into_iter().next())
}

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

// ─── Snapshots ────────────────────────────────────────────────────────────

fn manifest_name(device: &str) -> String {
    format!("manifest_{device}.json")
}

/// Every device's snapshot manifest: (device, manifest file id, manifest).
async fn load_manifests(token: &str, folder: &str) -> Result<Vec<(String, String, Manifest)>, String> {
    let files: Vec<DriveFile> = list_in_folder(token, folder, "manifest_")
        .await?
        .into_iter()
        .filter(|f| f.name.starts_with("manifest_") && f.name.ends_with(".json"))
        .collect();
    let ids: Vec<String> = files.iter().map(|f| f.id.clone()).collect();
    let bodies = download_many(token, &ids).await?;
    let mut out = Vec::new();
    for (f, body) in files.iter().zip(bodies) {
        let Some(dev) = f.name.strip_prefix("manifest_").and_then(|s| s.strip_suffix(".json")) else { continue };
        let Ok(m) = serde_json::from_slice::<Manifest>(&body) else { continue };
        out.push((dev.to_string(), f.id.clone(), m));
    }
    Ok(out)
}

/// The newest snapshot among `manifests` (optionally leaving out one device).
fn newest<'a>(manifests: &'a [(String, String, Manifest)], except: Option<&str>) -> Option<&'a Manifest> {
    manifests
        .iter()
        .filter(|(d, _, m)| Some(d.as_str()) != except && m.latest().is_some())
        .max_by_key(|(_, _, m)| m.latest().map(|e| e.timestamp))
        .map(|(_, _, m)| m)
}

/// Downloads several files with up to `LIMIT` requests in flight, keeping the given order.
/// A sync used to download every manifest and change file one after another.
async fn download_many(token: &str, ids: &[String]) -> Result<Vec<Vec<u8>>, String> {
    const LIMIT: usize = 6;
    let mut out = Vec::with_capacity(ids.len());
    for chunk in ids.chunks(LIMIT) {
        let tasks: Vec<_> = chunk
            .iter()
            .map(|id| {
                let (token, id) = (token.to_string(), id.clone());
                tauri::async_runtime::spawn(async move { download(&token, &id).await })
            })
            .collect();
        for t in tasks {
            out.push(t.await.map_err(|e| e.to_string())??);
        }
    }
    Ok(out)
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

/// Downloads a snapshot chain and rebuilds the database at `out` (checksums + integrity checked).
async fn rebuild_snapshot(token: &str, folder: &str, manifest: &Manifest, out: PathBuf) -> Result<PathBuf, String> {
    let chain = manifest.restore_chain().ok_or("snapshot has no full backup")?;
    let mut files = Vec::with_capacity(chain.len());
    for entry in chain {
        files.push((entry.clone(), download_entry(token, folder, entry).await?));
    }
    let path = out.clone();
    tokio::task::spawn_blocking(move || engine::restore_chain(&files, &path)).await.map_err(|e| e.to_string())??;
    Ok(out)
}

/// Uploads a snapshot of this device's database to its own chain (only changed pages).
async fn snapshot_upload(app: &AppHandle, token: &str, folder: &str, cfg: &SyncConfig, manifests: &[(String, String, Manifest)]) -> Result<Option<BackupEntry>, String> {
    let dir = backup_dir(app)?;
    let me = cfg.device_id.clone();
    let (manifest_id, mut manifest) = manifests
        .iter()
        .find(|(d, _, _)| *d == me)
        .map(|(_, id, m)| (Some(id.clone()), m.clone()))
        .unwrap_or((None, Manifest::new(0)));
    let head = manifest.latest().map(|e| e.blake3_checksum.clone());
    let baseline = baseline_for(&dir, head.as_deref());
    let db_path = crate::db::db_file(app)?;
    if baseline.as_deref().is_some_and(|b| unchanged_since(b, &db_path)) {
        return Ok(None);
    }
    let work = dir.join("work");
    let plan = {
        let manifest = manifest.clone();
        let me = me.clone();
        tokio::task::spawn_blocking(move || {
            let opts = engine::Options {
                max_delta_chain: engine::MAX_DELTA_CHAIN,
                compression_level: engine::COMPRESSION_LEVEL,
                force_full: false,
                device_id: &me,
                device_os: std::env::consts::OS,
            };
            engine::prepare_backup(&db_path, &work, &manifest, baseline.as_deref(), &opts)
        })
        .await
        .map_err(|e| e.to_string())??
    };
    let prepared = match plan {
        Plan::NoChanges => return Ok(None),
        Plan::Upload(p) => *p,
    };
    let data = fs::read(&prepared.file).map_err(|e| e.to_string())?;
    let mut entry = prepared.entry.clone();
    let uploaded = match upload_new(token, folder, &entry.file_name, data, "application/octet-stream").await {
        Ok(id) => id,
        Err(e) => {
            engine::discard(prepared);
            return Err(e);
        }
    };
    entry.drive_file_id = Some(uploaded.clone());
    manifest.page_size = prepared.page_size;
    manifest.backups.push(entry.clone());
    let dropped = manifest.prune(KEEP_CHAINS);
    let json = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    let written = match &manifest_id {
        Some(id) => update_content(token, id, json, "application/json").await,
        None => upload_new(token, folder, &manifest_name(&me), json, "application/json").await.map(|_| ()),
    };
    if let Err(e) = written {
        delete_file(token, &uploaded).await;
        engine::discard(prepared);
        return Err(e);
    }
    for old in &dropped {
        if let Some(id) = &old.drive_file_id {
            delete_file(token, id).await;
        }
    }
    engine::commit(prepared, &dir.join("baseline.db"))?;
    fs::write(dir.join("baseline.head"), &entry.blake3_checksum).map_err(|e| e.to_string())?;
    Ok(Some(entry))
}

// ─── Change files ─────────────────────────────────────────────────────────

struct ChangeFile {
    file: DriveFile,
    device: String,
    first: u64,
    last: u64,
}

fn change_file_name(device: &str, first: u64, last: u64) -> String {
    format!("chg_{device}_{first:010}_{last:010}.jsonl.zst")
}

fn parse_change_name(name: &str) -> Option<(String, u64, u64)> {
    let stem = name.strip_prefix("chg_")?.strip_suffix(".jsonl.zst")?;
    let mut parts = stem.rsplitn(3, '_');
    let last = parts.next()?.parse().ok()?;
    let first = parts.next()?.parse().ok()?;
    Some((parts.next()?.to_string(), first, last))
}

/// Writes records to `outgoing/` (uploaded next, kept until then).
fn queue_outgoing(dir: &Path, me: &str, records: &[crate::sync::Record]) -> Result<(), String> {
    let (Some(first), Some(last)) = (records.first(), records.last()) else {
        return Ok(());
    };
    let data = crate::sync::encode(me, records)?;
    let name = change_file_name(me, first.s, last.s);
    let tmp = dir.join("outgoing").join(format!("{name}.tmp"));
    fs::write(&tmp, data).map_err(|e| e.to_string())?;
    fs::rename(&tmp, dir.join("outgoing").join(name)).map_err(|e| e.to_string())
}

async fn upload_outgoing(dir: &Path, token: &str, folder: &str) -> Result<usize, String> {
    let mut names: Vec<String> = fs::read_dir(dir.join("outgoing"))
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("chg_") && n.ends_with(".jsonl.zst"))
        .collect();
    names.sort();
    for name in &names {
        let path = dir.join("outgoing").join(name);
        let data = fs::read(&path).map_err(|e| e.to_string())?;
        upload_new(token, folder, name, data, "application/zstd").await?;
        let _ = fs::remove_file(&path);
    }
    Ok(names.len())
}

async fn list_changes(token: &str, folder: &str) -> Result<Vec<ChangeFile>, String> {
    let mut out: Vec<ChangeFile> = list_in_folder(token, folder, "chg_")
        .await?
        .into_iter()
        .filter_map(|file| {
            let (device, first, last) = parse_change_name(&file.name)?;
            Some(ChangeFile { file, device, first, last })
        })
        .collect();
    out.sort_by(|a, b| (a.device.as_str(), a.first).cmp(&(b.device.as_str(), b.first)));
    Ok(out)
}

// ─── Sync ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Default)]
pub struct SyncOutcome {
    /// Change records sent / received.
    pub sent: usize,
    pub received: usize,
    /// Snapshot uploaded this time: `full`, `delta` or `none`.
    pub snapshot: String,
    pub snapshot_size: u64,
    pub synced_at: i64,
}

/// Puts a database file in place of the current one (the old one is kept as `.pre_restore`).
fn replace_database(app: &AppHandle, restored: &Path) -> Result<u64, String> {
    let db_path = crate::db::db_file(app)?;
    let tmp = db_path.with_extension("db.restoring");
    fs::copy(restored, &tmp).map_err(|e| format!("Failed to write restored database: {e}"))?;
    if db_path.exists() {
        let _ = fs::copy(&db_path, db_path.with_extension("db.pre_restore"));
        fs::remove_file(&db_path).map_err(|e| format!("Failed to remove old database: {e}"))?;
    }
    for ext in ["db-journal", "db-wal", "db-shm"] {
        let _ = fs::remove_file(db_path.with_extension(ext));
    }
    fs::rename(&tmp, &db_path).map_err(|e| format!("Failed to rename restored database: {e}"))?;
    Ok(fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0))
}

/// Merges the newest snapshot of another device into this database, row by row.
async fn merge_newest_snapshot(app: &AppHandle, token: &str, folder: &str, manifests: &[(String, String, Manifest)], me: &str) -> Result<bool, String> {
    let Some(m) = newest(manifests, Some(me)) else { return Ok(false) };
    let out = backup_dir(app)?.join("work").join("merge.db");
    let path = rebuild_snapshot(token, folder, m, out).await?;
    let me = me.to_string();
    let p = path.clone();
    let stats = with_db(app, move |conn| {
        let snap = Connection::open(&p).map_err(|e| e.to_string())?;
        crate::sync::merge_snapshot(conn, &snap, &me)
    })
    .await?;
    eprintln!("[sync] merged snapshot: {stats:?}");
    let _ = fs::remove_file(&path);
    Ok(true)
}

/// First sync of this database as this device.
async fn join(app: &AppHandle, token: &str, folder: &str, cfg: &mut SyncConfig, manifests: &[(String, String, Manifest)]) -> Result<(), String> {
    let me = cfg.device_id.clone();
    let (current, has_data) = with_db(app, |c| Ok((crate::sync::device(c), crate::sync::has_user_data(c)))).await?;
    if current.as_deref() == Some(me.as_str()) {
        return Ok(());
    }
    if current.is_some() {
        // Restored from another device's snapshot: its rows are already known everywhere.
        let me2 = me.clone();
        return with_db(app, move |c| crate::sync::adopt(c, &me2)).await;
    }
    if !has_data {
        if let Some(m) = newest(manifests, Some(&me)) {
            // A fresh install: start from the newest snapshot instead of an empty database.
            let out = backup_dir(app)?.join("work").join("join.db");
            let path = rebuild_snapshot(token, folder, m, out).await?;
            replace_database(app, &path)?;
            let _ = fs::remove_file(&path);
            let me2 = me.clone();
            return with_db(app, move |c| crate::sync::adopt(c, &me2)).await;
        }
    }
    // Data of its own: send all of it, then merge what the cloud has.
    let dir = backup_dir(app)?;
    let (me2, seq) = (me.clone(), cfg.next_seq);
    let next = with_db(app, move |c| {
        let mut seq = seq;
        let records = crate::sync::initialize(c, &me2, &mut seq)?;
        queue_outgoing(&dir, &me2, &records)?;
        Ok(seq)
    })
    .await?;
    cfg.next_seq = next;
    save_sync_config(app, cfg)?;
    merge_newest_snapshot(app, token, folder, manifests, &me).await?;
    Ok(())
}

async fn sync_locked(app: &AppHandle) -> Result<SyncOutcome, String> {
    let token = get_access_token(app).await?;
    let folder = ensure_folder(&token).await?;
    let mut cfg = load_sync_config(app)?;
    let me = cfg.device_id.clone();
    let dir = backup_dir(app)?;
    let manifests = load_manifests(&token, &folder).await?;

    join(app, &token, &folder, &mut cfg, &manifests).await?;

    // Local changes → outgoing file → Drive.
    let (me2, seq, d2) = (me.clone(), cfg.next_seq, dir.clone());
    let (sent, next) = with_db(app, move |c| {
        let mut seq = seq;
        let records = crate::sync::export(c, &me2, &mut seq)?;
        queue_outgoing(&d2, &me2, &records)?;
        Ok((records.len(), seq))
    })
    .await?;
    cfg.next_seq = next;
    save_sync_config(app, &cfg)?;
    upload_outgoing(&dir, &token, &folder).await?;

    // Other devices' changes (and this device's own, after a restore).
    let changes = list_changes(&token, &folder).await?;
    let mut cursors = with_db(app, |c| crate::sync::cursors(c)).await?;
    let mut merged = false;
    let mut by_device: HashMap<&str, Vec<&ChangeFile>> = HashMap::new();
    for f in &changes {
        by_device.entry(f.device.as_str()).or_default().push(f);
    }
    for (dev, files) in &by_device {
        let cur = cursors.get(*dev).copied().unwrap_or(0);
        let gap = files.iter().find(|f| f.last > cur).is_some_and(|f| f.first > cur + 1);
        if gap && !merged && *dev != me {
            // Older changes were already removed from Drive: catch up from a snapshot first.
            merged = merge_newest_snapshot(app, &token, &folder, &manifests, &me).await?;
            cursors = with_db(app, |c| crate::sync::cursors(c)).await?;
        }
    }
    // All new files at once (`list_changes` sorted them per device), then one database pass.
    let wanted: Vec<(String, String)> = by_device
        .iter()
        .flat_map(|(dev, files)| {
            let cur = cursors.get(*dev).copied().unwrap_or(0);
            files.iter().filter(move |f| f.last > cur).map(|f| (dev.to_string(), f.file.id.clone()))
        })
        .collect();
    let ids: Vec<String> = wanted.iter().map(|(_, id)| id.clone()).collect();
    let bodies = download_many(&token, &ids).await?;
    let devices: Vec<String> = wanted.into_iter().map(|(dev, _)| dev).collect();
    let received = with_db(app, move |c| {
        let mut applied = 0;
        for (dev, data) in devices.iter().zip(&bodies) {
            let (_, records) = crate::sync::decode(data)?;
            applied += crate::sync::apply(c, dev, &records)?.applied;
        }
        Ok(applied)
    })
    .await?;

    // Snapshot after merging, then tidy up this device's old files.
    let snapshot = snapshot_upload(app, &token, &folder, &cfg, &manifests).await?;
    let snapshot_time = snapshot
        .as_ref()
        .map(|e| e.timestamp.timestamp())
        .or_else(|| manifests.iter().find(|(d, _, _)| *d == me).and_then(|(_, _, m)| m.latest()).map(|e| e.timestamp.timestamp()));
    if let Some(t) = snapshot_time {
        let cutoff = t.min(chrono::Utc::now().timestamp()) - KEEP_CHANGES_DAYS * 86400;
        for f in changes.iter().filter(|f| f.device == me && f.file.modified() < cutoff) {
            delete_file(&token, &f.file.id).await;
        }
    }

    let now = chrono::Utc::now().timestamp();
    let mut cfg = load_sync_config(app)?;
    cfg.last_sync_at = Some(now);
    save_sync_config(app, &cfg)?;
    Ok(SyncOutcome {
        sent,
        received,
        snapshot: snapshot.as_ref().map_or("none".into(), |e| if e.backup_type == BackupType::Full { "full".into() } else { "delta".into() }),
        snapshot_size: snapshot.as_ref().map_or(0, |e| e.file_size_bytes),
        synced_at: now,
    })
}

async fn sync_now(app: &AppHandle) -> Result<SyncOutcome, String> {
    let _guard = SYNC_LOCK.lock().await;
    let result = sync_locked(app).await;
    let mut cfg = load_sync_config(app)?;
    cfg.last_attempt_at = Some(chrono::Utc::now().timestamp());
    cfg.last_error = result.as_ref().err().cloned();
    save_sync_config(app, &cfg)?;
    result
}

/// Replaces this device's data with the newest snapshot (any device), keeping nothing local
/// that wasn't synced: local changes are sent first and come back with the other changes.
async fn restore_locked(app: &AppHandle) -> Result<u64, String> {
    let token = get_access_token(app).await?;
    let dir = backup_dir(app)?;
    let work = dir.join("work");
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let me = load_sync_config(app)?.device_id;

    if let Some(folder) = find_folder(&token).await? {
        let manifests = load_manifests(&token, &folder).await?;
        if let Some(m) = newest(&manifests, None) {
            // Send unsynced local changes first, so nothing is lost.
            let _ = sync_locked(app).await;
            let path = rebuild_snapshot(&token, &folder, m, work.join("restored.db")).await?;
            let size = replace_database(app, &path)?;
            let _ = fs::remove_file(&path);
            let me2 = me.clone();
            with_db(app, move |c| crate::sync::adopt(c, &me2)).await?;
            // Changes made after that snapshot (this device's included).
            let _ = sync_locked(app).await;
            return Ok(size);
        }
    }

    // Old single-file backup.
    let file = find_legacy(&token).await?.ok_or_else(|| app_error("no_backup", ""))?;
    let data = download(&token, &file.id).await?;
    if data.is_empty() {
        return Err("Downloaded backup file is empty".into());
    }
    let restored = work.join("restored.db");
    fs::write(&restored, &data).map_err(|e| e.to_string())?;
    let size = replace_database(app, &restored)?;
    let _ = fs::remove_file(&restored);
    save_legacy_metadata(
        app,
        &BackupMetadata {
            file_id: file.id.clone(),
            modified_time: file.modified_time.clone(),
            size_bytes: size,
            backed_up_at: chrono::Utc::now().timestamp(),
        },
    )?;
    Ok(size)
}

pub async fn restore_database(app: &AppHandle) -> Result<u64, String> {
    let _guard = SYNC_LOCK.lock().await;
    restore_locked(app).await
}

/// Status for Settings: newest snapshot, devices that sync, last error.
pub async fn get_backup_status(app: &AppHandle) -> Result<serde_json::Value, String> {
    let cfg = load_sync_config(app)?;
    if !crate::google_auth::get_auth_status(app)? {
        return Ok(serde_json::json!({ "signed_in": false, "has_backup": false, "last_backup": null, "backup_size": null }));
    }
    let remote = async {
        let token = get_access_token(app).await?;
        let Some(folder) = find_folder(&token).await? else {
            return Ok::<_, String>((Vec::new(), find_legacy(&token).await?));
        };
        let manifests = load_manifests(&token, &folder).await?;
        let legacy = if manifests.is_empty() { find_legacy(&token).await? } else { None };
        Ok((manifests, legacy))
    }
    .await;
    let (manifests, legacy) = match remote {
        Ok(r) => r,
        Err(e) => {
            return Ok(serde_json::json!({
                "signed_in": true, "has_backup": false, "last_backup": null, "backup_size": null,
                "offline": true, "last_error": cfg.last_error, "detail": e,
            }))
        }
    };
    let devices: Vec<serde_json::Value> = manifests
        .iter()
        .filter_map(|(d, _, m)| {
            let head = m.latest()?;
            Some(serde_json::json!({ "os": head.device_os, "last_backup": head.timestamp.timestamp(), "this_device": *d == cfg.device_id }))
        })
        .collect();
    if let Some(m) = newest(&manifests, None) {
        let head = m.latest().expect("has a backup");
        return Ok(serde_json::json!({
            "signed_in": true,
            "has_backup": true,
            "last_backup": head.timestamp.timestamp(),
            "backup_size": m.restore_chain().map_or(0, |c| c.iter().map(|e| e.file_size_bytes).sum::<u64>()),
            "last_device_os": head.device_os,
            "last_from_this_device": head.device_id == cfg.device_id,
            "devices": devices,
            "last_error": cfg.last_error,
        }));
    }
    if let Some(f) = legacy {
        return Ok(serde_json::json!({
            "signed_in": true, "has_backup": true, "last_backup": f.modified(), "backup_size": f.size,
            "last_kind": "legacy", "devices": devices, "last_error": cfg.last_error,
        }));
    }
    Ok(serde_json::json!({ "signed_in": true, "has_backup": false, "last_backup": null, "backup_size": null, "devices": devices, "last_error": cfg.last_error }))
}

// ─── Automatic sync ───────────────────────────────────────────────────────

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
        config.interval_minutes = i.max(MIN_INTERVAL_MINUTES);
    }
    if let Some(w) = wifi_only {
        config.wifi_only = w;
    }
    save_sync_config(app, &config)?;
    Ok(config)
}

fn sync_result(r: Result<SyncOutcome, String>) -> serde_json::Value {
    match r {
        Ok(o) => serde_json::json!({
            "synced": true,
            "reason": "synced",
            "sent": o.sent,
            "received": o.received,
            "snapshot": o.snapshot,
            "size": o.snapshot_size,
            "synced_at": o.synced_at,
        }),
        Err(e) => serde_json::json!({ "synced": false, "reason": "error", "error": e }),
    }
}

/// Automatic sync only runs while the app is in use: on Android between `onResume` and
/// `onPause`, on desktop while the app is open.
static FOREGROUND: AtomicBool = AtomicBool::new(true);

/// Syncs when allowed and due. `opened`: the app was just opened or brought back, which syncs
/// right away (downloading the other devices' changes) instead of waiting for the interval.
pub async fn trigger_auto_sync(app: &AppHandle, opened: bool) -> Result<serde_json::Value, String> {
    let config = load_sync_config(app)?;
    let skip = |reason: &str| Ok(serde_json::json!({ "synced": false, "reason": reason }));
    if !config.enabled {
        return skip("auto_sync_disabled");
    }
    if !FOREGROUND.load(Ordering::Relaxed) {
        return skip("in_background");
    }
    if !crate::google_auth::get_auth_status(app)? {
        return skip("not_signed_in");
    }
    let now = chrono::Utc::now().timestamp();
    let since_last = config.last_sync_at.map(|last| now - last);
    if opened {
        if since_last.is_some_and(|d| d < OPEN_GRACE_SECS) {
            return skip("recently_synced");
        }
    } else {
        let interval = config.interval_minutes.max(MIN_INTERVAL_MINUTES) as i64 * 60;
        if let Some(d) = since_last.filter(|d| *d < interval) {
            return Ok(serde_json::json!({ "synced": false, "reason": "not_due_yet", "next_sync_in_seconds": interval - d }));
        }
        if config.last_error.is_some() && config.last_attempt_at.is_some_and(|t| now - t < RETRY_AFTER_SECS.min(interval)) {
            return skip("backing_off");
        }
    }
    if SYNC_LOCK.try_lock().is_err() {
        return skip("busy");
    }
    let (connected, unmetered) = network_state(app).await;
    if !connected {
        return skip("offline");
    }
    if config.wifi_only && !unmetered {
        return skip("waiting_for_wifi");
    }
    let _ = app.emit(AUTO_SYNC_EVENT, serde_json::json!({ "state": "syncing" }));
    let mut result = sync_result(sync_now(app).await);
    result["state"] = serde_json::json!(if result["synced"] == true { "done" } else { "error" });
    let _ = app.emit(AUTO_SYNC_EVENT, &result);
    Ok(result)
}

fn spawn_auto_sync(app: &AppHandle, opened: bool) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = trigger_auto_sync(&app, opened).await {
            eprintln!("[auto_sync] {e}");
        }
    });
}

/// Started once at launch: syncs shortly after the app opens (once the UI can show it), then
/// checks every minute whether a sync is due. Does nothing while the app is in the background.
pub fn start_auto_sync(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        let _ = trigger_auto_sync(&app, true).await;
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(TICK_SECS));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tick.tick().await;
        loop {
            tick.tick().await;
            if FOREGROUND.load(Ordering::Relaxed) {
                let _ = trigger_auto_sync(&app, false).await;
            }
        }
    });
}

/// The app went to the background (`false`) or came back (`true`, which syncs right away).
#[cfg(mobile)]
pub fn set_foreground(app: &AppHandle, active: bool) {
    let was = FOREGROUND.swap(active, Ordering::Relaxed);
    if active && !was {
        spawn_auto_sync(app, true);
    }
}

/// Signing in or turning automatic sync on syncs right away.
pub fn auto_sync_now(app: &AppHandle) {
    spawn_auto_sync(app, true);
}

// ─── Tauri Commands ───────────────────────────────────────────────────────

/// "Sync now": send and receive changes, then snapshot.
#[tauri::command]
pub async fn google_backup_db(app: AppHandle) -> Result<serde_json::Value, String> {
    let o = sync_now(&app).await?;
    Ok(serde_json::json!({ "success": true, "sent": o.sent, "received": o.received, "snapshot": o.snapshot, "size": o.snapshot_size, "synced_at": o.synced_at }))
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
        "last_error": config.last_error,
        // Only Android can tell Wi-Fi from mobile data.
        "wifi_only_supported": cfg!(target_os = "android"),
    })
}

#[tauri::command]
pub fn google_set_auto_sync(app: AppHandle, enabled: bool, interval_minutes: u64, wifi_only: bool) -> Result<serde_json::Value, String> {
    let was_enabled = load_sync_config(&app)?.enabled;
    let config = update_sync_config(&app, Some(enabled), Some(interval_minutes), Some(wifi_only))?;
    if enabled && !was_enabled {
        auto_sync_now(&app);
    }
    Ok(sync_config_json(&config))
}

#[tauri::command]
pub fn google_get_auto_sync(app: AppHandle) -> Result<serde_json::Value, String> {
    Ok(sync_config_json(&load_sync_config(&app)?))
}

/// "Sync now" from the automatic sync card: right away, whatever the schedule or network.
#[tauri::command]
pub async fn google_trigger_sync(app: AppHandle) -> Result<serde_json::Value, String> {
    Ok(sync_result(sync_now(&app).await))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_file_names_round_trip() {
        let dev = "3f2a-41c9-b0d1";
        let name = change_file_name(dev, 12, 340);
        assert_eq!(parse_change_name(&name), Some((dev.to_string(), 12, 340)));
        assert_eq!(parse_change_name("manifest_x.json"), None);
    }
}
