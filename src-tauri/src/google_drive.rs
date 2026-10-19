use crate::google_auth::{get_access_token};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const DRIVE_API_URL: &str = "https://www.googleapis.com/upload/drive/v3/files";
const DRIVE_FILES_URL: &str = "https://www.googleapis.com/drive/v3/files";
const BACKUP_FILE_NAME: &str = "langchef_backup.db";
const MIME_TYPE: &str = "application/x-sqlite3";

// ─── Data Structures ──────────────────────────────────────────────────────

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
}

impl Default for SyncConfig {
    fn default() -> Self {
        SyncConfig {
            enabled: false,
            interval_minutes: 60,
            last_sync_at: None,
            wifi_only: true,
        }
    }
}

// ─── Config Persistence ───────────────────────────────────────────────────

fn sync_config_file(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e: tauri::Error| e.to_string())?;
    Ok(dir.join("google_sync_config.json"))
}

pub fn save_sync_config(app: &AppHandle, config: &SyncConfig) -> Result<(), String> {
    let path = sync_config_file(app)?;
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

pub fn load_sync_config(app: &AppHandle) -> Result<SyncConfig, String> {
    let path = sync_config_file(app)?;
    if !path.exists() {
        return Ok(SyncConfig::default());
    }
    let json = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let config: SyncConfig = serde_json::from_str(&json).unwrap_or_default();
    Ok(config)
}

// ─── Database File Helpers ────────────────────────────────────────────────

fn db_file_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e: tauri::Error| e.to_string())?;
    Ok(dir.join("langchef.db"))
}

fn backup_metadata_file(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e: tauri::Error| e.to_string())?;
    Ok(dir.join("google_backup_meta.json"))
}

fn save_backup_metadata(app: &AppHandle, meta: &BackupMetadata) -> Result<(), String> {
    let path = backup_metadata_file(app)?;
    let json = serde_json::to_string_pretty(meta).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

fn load_backup_metadata(app: &AppHandle) -> Result<Option<BackupMetadata>, String> {
    let path = backup_metadata_file(app)?;
    if !path.exists() {
        return Ok(None);
    }
    let json = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let meta: BackupMetadata = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    Ok(Some(meta))
}

// ─── Google Drive API ─────────────────────────────────────────────────────

/// Find existing backup file on Google Drive by name
async fn find_backup_file(access_token: &str) -> Result<Option<DriveFile>, String> {
    let client = reqwest::Client::new();
    let query = format!("name='{}' and trashed=false", BACKUP_FILE_NAME);
    let url = format!(
        "{}?q={}+&fields=files(id,name,modifiedTime,size)",
        DRIVE_FILES_URL,
        urlencoding::encode(&query)
    );

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .await
        .map_err(|e| format!("Network error searching Drive: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "Failed to search Drive files ({}): {}",
            status, body
        ));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    // Use a static empty array to avoid temporary value issues
    static EMPTY: serde_json::Value = serde_json::Value::Array(vec![]);
    let files = body["files"].as_array().unwrap_or(EMPTY.as_array().unwrap());

    if files.is_empty() {
        return Ok(None);
    }

    // Return the first (most recently modified) match
    let f = &files[0];
    Ok(Some(DriveFile {
        file_id: f["id"].as_str().unwrap_or("").to_string(),
        name: f["name"].as_str().unwrap_or("").to_string(),
        modified_time: f["modifiedTime"].as_str().unwrap_or("").to_string(),
        size: f["size"].as_u64().unwrap_or(0),
    }))
}

#[derive(Debug, Deserialize)]
struct DriveFile {
    #[serde(rename = "id")]
    file_id: String,
    name: String,
    #[serde(rename = "modifiedTime")]
    modified_time: String,
    size: u64,
}

impl From<DriveFile> for BackupMetadata {
    fn from(f: DriveFile) -> Self {
        BackupMetadata {
            file_id: f.file_id,
            modified_time: f.modified_time,
            size_bytes: f.size,
            backed_up_at: 0, // will be set by caller
        }
    }
}

/// Upload database file to Google Drive.
async fn upload_to_drive(
    app: &AppHandle,
    access_token: &str,
    existing_file_id: Option<&str>,
    _db_size: usize,
) -> Result<BackupMetadata, String> {
    let db_path = db_file_path(app)?;

    if !db_path.exists() {
        return Err("Database file not found".to_string());
    }

    let db_bytes = fs::read(&db_path).map_err(|e| format!("Failed to read database: {}", e))?;

    if db_bytes.is_empty() {
        return Err("Database file is empty".to_string());
    }

    let actual_len = db_bytes.len();

    let client = reqwest::Client::new();

    // Build multipart upload request
    let file_metadata = serde_json::json!({
        "name": BACKUP_FILE_NAME,
        "description": "LangChef database backup",
        "appProperties": {
            "app": "langchef",
            "backed_up_at": chrono::Utc::now().timestamp()
        }
    });

    let mut form_part = reqwest::multipart::Form::new();
    form_part = form_part.part(
        "metadata",
        reqwest::multipart::Part::text(file_metadata.to_string())
            .mime_str("application/json")
            .map_err(|e| e.to_string())?,
    );
    form_part = form_part.part(
        "file",
        reqwest::multipart::Part::bytes(db_bytes)
            .file_name(BACKUP_FILE_NAME)
            .mime_str(MIME_TYPE)
            .map_err(|e| e.to_string())?,
    );

    let url = match existing_file_id {
        Some(id) => format!("{}/{}?uploadType=multipart", DRIVE_API_URL, id),
        None => format!("{}?uploadType=multipart", DRIVE_API_URL),
    };

    let request_builder = match existing_file_id {
        Some(_) => client.patch(&url),
        None => client.post(&url),
    };

    let resp = request_builder
        .header("Authorization", format!("Bearer {}", access_token))
        .multipart(form_part)
        .send()
        .await
        .map_err(|e| format!("Upload failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Upload failed ({}): {}", status, body));
    }

    let result: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();

    let meta = BackupMetadata {
        file_id: result["id"].as_str().unwrap_or("").to_string(),
        modified_time: result["modifiedTime"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        size_bytes: result["size"].as_u64().unwrap_or(actual_len as u64),
        backed_up_at: now,
    };

    // Save metadata locally
    save_backup_metadata(app, &meta)?;

    Ok(meta)
}

/// Download backup from Google Drive
async fn download_from_drive(
    _app: &AppHandle,
    access_token: &str,
    file_id: &str,
) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::new();
    let url = format!("{}/{}?alt=media", DRIVE_FILES_URL, file_id);

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .await
        .map_err(|e| format!("Download failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Download failed ({}): {}", status, body));
    }

    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    Ok(bytes.to_vec())
}

// ─── Public Operations ────────────────────────────────────────────────────

/// Back up the database to Google Drive
pub async fn backup_database(app: &AppHandle) -> Result<BackupMetadata, String> {
    let access_token = get_access_token(app).await?;

    // Check if there's an existing backup file
    let existing = find_backup_file(&access_token).await?;

    let file_id = existing.as_ref().map(|f| f.file_id.as_str());

    // Read DB size before upload (needed as fallback for response size)
    let db_path = db_file_path(app)?;
    let db_size = if db_path.exists() {
        fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0) as usize
    } else {
        0
    };

    let meta = upload_to_drive(app, &access_token, file_id, db_size).await?;

    // Update sync config with last sync time
    let mut config = load_sync_config(app)?;
    config.last_sync_at = Some(chrono::Utc::now().timestamp());
    save_sync_config(app, &config)?;

    Ok(meta)
}

/// Restore database from Google Drive backup
pub async fn restore_database(app: &AppHandle) -> Result<BackupMetadata, String> {
    let access_token = get_access_token(app).await?;

    // Find the backup file
    let existing = find_backup_file(&access_token).await?;

    let file = existing.ok_or_else(|| crate::locale::app_error("no_backup", ""))?;

    // Download the backup
    let data = download_from_drive(app, &access_token, &file.file_id).await?;

    if data.is_empty() {
        return Err("Downloaded backup file is empty".to_string());
    }

    // Write to a temp location first, then replace
    let db_path = db_file_path(app)?;
    let tmp_path = db_path.with_extension("db.restoring");

    fs::write(&tmp_path, &data).map_err(|e| format!("Failed to write restored database: {}", e))?;

    // Replace the old database
    if db_path.exists() {
        // Create a backup of current DB before replacing (safety net)
        let corrupt_backup = db_path.with_extension("db.pre_restore");
        let _ = fs::copy(&db_path, &corrupt_backup);
        fs::remove_file(&db_path)
            .map_err(|e| format!("Failed to remove old database: {}", e))?;
    }

    fs::rename(&tmp_path, &db_path)
        .map_err(|e| format!("Failed to rename restored database: {}", e))?;

    // Update metadata
    let now = chrono::Utc::now().timestamp();
    let meta = BackupMetadata {
        file_id: file.file_id.clone(),
        modified_time: file.modified_time.clone(),
        size_bytes: file.size.max(data.len() as u64),
        backed_up_at: now,
    };
    save_backup_metadata(app, &meta)?;

    Ok(meta)
}

/// Get the current backup status (from cached metadata + remote check)
pub async fn get_backup_status(app: &AppHandle) -> Result<serde_json::Value, String> {
    // Check auth first
    let signed_in = crate::google_auth::get_auth_status(app)?;

    if !signed_in {
        return Ok(serde_json::json!({
            "signed_in": false,
            "has_backup": false,
            "last_backup": serde_json::Value::Null,
            "backup_size": serde_json::Value::Null,
        }));
    }

    // Load cached metadata
    let cached_meta = load_backup_metadata(app)?;

    // Try to check remote status (best-effort)
    let remote_meta: Option<BackupMetadata> = match get_access_token(app).await {
        Ok(token) => find_backup_file(&token).await.ok().map(|f| f.map(BackupMetadata::from)).flatten(),
        Err(_) => None,
    };

    let has_backup = remote_meta.is_some() || cached_meta.is_some();

    // Prefer remote metadata, fall back to cached
    let final_meta: &BackupMetadata = match (&remote_meta, &cached_meta) {
        (Some(r), _) => r,
        (_, Some(c)) => c,
        _ => {
            return Ok(serde_json::json!({
                "signed_in": true,
                "has_backup": false,
                "last_backup": serde_json::Value::Null,
                "backup_size": serde_json::Value::Null,
            }));
        }
    };

    Ok(serde_json::json!({
        "signed_in": true,
        "has_backup": has_backup,
        "last_backup": final_meta.backed_up_at,
        "backup_size": final_meta.size_bytes,
        "remote_modified": final_meta.modified_time,
    }))
}

/// Update auto-sync configuration
pub fn update_sync_config(
    app: &AppHandle,
    enabled: Option<bool>,
    interval_minutes: Option<u64>,
    wifi_only: Option<bool>,
) -> Result<SyncConfig, String> {
    let mut config = load_sync_config(app)?;

    if let Some(e) = enabled {
        config.enabled = e;
    }
    if let Some(i) = interval_minutes {
        config.interval_minutes = i;
    }
    if let Some(w) = wifi_only {
        config.wifi_only = w;
    }

    save_sync_config(app, &config)?;
    Ok(config)
}

/// Check if auto-sync should run and execute it if needed
pub async fn trigger_auto_sync(app: &AppHandle) -> Result<serde_json::Value, String> {
    let config = load_sync_config(app)?;

    if !config.enabled {
        return Ok(serde_json::json!({
            "synced": false,
            "reason": "auto_sync_disabled"
        }));
    }

    // Check if enough time has passed since last sync
    if let Some(last_sync) = config.last_sync_at {
        let now = chrono::Utc::now().timestamp();
        let elapsed_secs = now.saturating_sub(last_sync);
        let interval_secs = config.interval_minutes as i64 * 60;

        if elapsed_secs < interval_secs {
            return Ok(serde_json::json!({
                "synced": false,
                "reason": "not_due_yet",
                "next_sync_in_seconds": interval_secs - elapsed_secs
            }));
        }
    }

    // Perform the backup
    match backup_database(app).await {
        Ok(meta) => Ok(serde_json::json!({
            "synced": true,
            "reason": "backup_completed",
            "backed_up_at": meta.backed_up_at,
            "size": meta.size_bytes,
        })),
        Err(e) => Ok(serde_json::json!({
            "synced": false,
            "reason": "error",
            "error": e
        })),
    }
}

// ─── Tauri Commands ───────────────────────────────────────────────────────

#[tauri::command]
pub async fn google_backup_db(app: AppHandle) -> Result<serde_json::Value, String> {
    let meta = backup_database(&app).await?;
    Ok(serde_json::json!({
        "success": true,
        "file_id": meta.file_id,
        "backed_up_at": meta.backed_up_at,
        "size": meta.size_bytes,
    }))
}

#[tauri::command]
pub async fn google_restore_db(app: AppHandle) -> Result<serde_json::Value, String> {
    let meta = restore_database(&app).await?;
    Ok(serde_json::json!({
        "success": true,
        "file_id": meta.file_id,
        "restored_at": meta.backed_up_at,
        "size": meta.size_bytes,
    }))
}

#[tauri::command]
pub async fn google_get_backup_status(app: AppHandle) -> Result<serde_json::Value, String> {
    get_backup_status(&app).await
}

#[tauri::command]
pub fn google_set_auto_sync(
    app: AppHandle,
    enabled: bool,
    interval_minutes: u64,
    wifi_only: bool,
) -> Result<serde_json::Value, String> {
    let config = update_sync_config(&app, Some(enabled), Some(interval_minutes), Some(wifi_only))?;
    Ok(serde_json::json!({
        "enabled": config.enabled,
        "interval_minutes": config.interval_minutes,
        "wifi_only": config.wifi_only,
        "last_sync_at": config.last_sync_at,
    }))
}

#[tauri::command]
pub fn google_get_auto_sync(app: AppHandle) -> Result<serde_json::Value, String> {
    let config = load_sync_config(&app)?;
    Ok(serde_json::json!({
        "enabled": config.enabled,
        "interval_minutes": config.interval_minutes,
        "wifi_only": config.wifi_only,
        "last_sync_at": config.last_sync_at,
    }))
}

#[tauri::command]
pub async fn google_trigger_sync(app: AppHandle) -> Result<serde_json::Value, String> {
    trigger_auto_sync(&app).await
}
