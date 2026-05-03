use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub mod desktop;
#[cfg(any(target_os = "android", target_os = "ios"))]
pub mod mobile;

// ─── Constants ─────────────────────────────────────────────────────────────

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
pub const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
const LOCALHOST_ADDR: &str = "127.0.0.1";
const DEFAULT_REDIRECT_HOST: &str = "localhost";
const SUCCESS_HTML: &str = "Go back to your app :)";

// ─── Credentials (compile-time env) ──────────────────────────────────────

const GOOGLE_CLIENT_ID: &str = env!("GOOGLE_CLIENT_ID");
const GOOGLE_CLIENT_SECRET: &str = env!("GOOGLE_CLIENT_SECRET");

// ─── Token Data ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TokenData {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub expires_at: u64,
}

// ─── Token File I/O ──────────────────────────────────────────────────────

fn token_file(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("google_tokens.json"))
}

pub fn save_tokens(app: &AppHandle, tokens: &TokenData) -> Result<(), String> {
    let path = token_file(app)?;
    let json = serde_json::to_string_pretty(tokens).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

pub fn load_tokens(app: &AppHandle) -> Result<Option<TokenData>, String> {
    let path = token_file(app)?;
    if !path.exists() { return Ok(None); }
    let json = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let tokens: TokenData = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    Ok(Some(tokens))
}

pub fn delete_tokens(app: &AppHandle) -> Result<(), String> {
    let path = token_file(app)?;
    if path.exists() { fs::remove_file(&path).map_err(|e| e.to_string())?; }
    Ok(())
}

// ─── Auth Status ──────────────────────────────────────────────────────────

pub fn get_auth_status(app: &AppHandle) -> Result<bool, String> {
    match load_tokens(app)? {
        Some(t) => Ok(!t.access_token.is_empty()),
        None => Ok(false),
    }
}

// ─── Public API (platform-agnostic) ────────────────────────────────────────

pub async fn get_access_token(app: &AppHandle) -> Result<String, String> {
    let mut tokens = match load_tokens(app)? {
        Some(t) => t,
        None => return Err("Not signed in to Google".to_string()),
    };
    let now = chrono::Utc::now().timestamp() as u64;
    eprintln!("[google_auth] get_access_token: now={}, expires_at={}, has_refresh={}, access_token_len={}",
        now, tokens.expires_at,
        tokens.refresh_token.as_ref().map_or(false, |r| !r.is_empty()),
        tokens.access_token.len(),
    );
    if now >= tokens.expires_at.saturating_sub(300) {
        eprintln!("[google_auth] Token expired, refreshing...");
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            if let Some(refresh) = &tokens.refresh_token {
                let refresh = refresh.clone();
                let old_id_token = tokens.id_token.clone();
                tokens = tokio::task::spawn_blocking(move || desktop::refresh_token(&refresh))
                    .await
                    .map_err(|e| format!("Refresh token task failed: {e}"))??;
                // Preserve existing id_token if refresh didn't return a new one
                if tokens.id_token.is_none() {
                    tokens.id_token = old_id_token;
                }
                save_tokens(app, &tokens)?;
            } else {
                return Err("No refresh token available. Please sign in again.".to_string());
            }
        }
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            // Mobile: use refresh token if available (same as desktop), otherwise re-authorize
            let handle = app
                .try_state::<mobile::GoogleAuthHandle<tauri::Wry>>()
                .ok_or("Google auth plugin not initialized")?;
            let old_id_token = tokens.id_token.clone();
            let refresh_str = tokens.refresh_token.as_deref().unwrap_or("").to_string();
            eprintln!("[google_auth] Mobile refresh: sending refresh_token_len={}", refresh_str.len());
            tokens = mobile::refresh_token(handle.inner(), &refresh_str)?;
            eprintln!("[google_auth] Mobile refresh result: access_token_len={}, id_token_len={}, refresh_token_len={}, expires_at={}",
                tokens.access_token.len(),
                tokens.id_token.as_ref().map_or(0, |t| t.len()),
                tokens.refresh_token.as_ref().map_or(0, |t| t.len()),
                tokens.expires_at,
            );
            // Preserve existing id_token if refresh didn't return a new one
            if tokens.id_token.as_deref().is_some_and(|s| s.is_empty()) {
                tokens.id_token = old_id_token;
            }
            save_tokens(app, &tokens)?;
        }
    } else {
        eprintln!("[google_auth] Token still valid, using cached");
    }
    eprintln!("[google_auth] Returning access_token_len={}", tokens.access_token.len());
    Ok(tokens.access_token)
}

async fn get_user_email(access_token: &str, id_token: Option<&str>) -> Result<String, String> {
    // Primary: extract email from ID token JWT payload (no network needed)
    if let Some(jwt) = id_token {
        if !jwt.is_empty() {
            if let Some(email) = extract_email_from_jwt(jwt) {
                return Ok(email);
            }
        }
    }

    // Fallback: call userinfo endpoint (requires userinfo scope on access_token)
    let client = reqwest::Client::new();
    let resp = client
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch user info: {}", e))?;
    if !resp.status().is_success() {
        return Err("Failed to get user info".to_string());
    }
    let info: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(info["email"].as_str().unwrap_or("Unknown").to_string())
}

/// Decode the payload of a Google ID token JWT and extract the email claim.
fn extract_email_from_jwt(jwt: &str) -> Option<String> {
    let payload = jwt.split('.').nth(1)?;
    let decoded = base64_url_decode(payload).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    json["email"].as_str().map(String::from)
}

/// Decode a base64url-encoded string (no padding) into bytes.
fn base64_url_decode(input: &str) -> Result<Vec<u8>, String> {
    let mut decoded = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits = 0u32;

    for ch in input.bytes() {
        let val = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => continue, // skip padding or invalid chars
        };
        buf = (buf << 6) | (val as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            decoded.push(((buf >> bits) & 0xFF) as u8);
        }
    }

    if decoded.is_empty() {
        return Err("Empty base64url input".to_string());
    }
    Ok(decoded)
}

// ─── Tauri Commands ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn google_sign_in(app: AppHandle) -> Result<serde_json::Value, String> {
    // Preserve existing refresh_token before sign-in overwrites it
    // (Google only returns refresh_token on the first auth code exchange)
    let old_refresh = load_tokens(&app)?
        .and_then(|t| t.refresh_token)
        .filter(|r| !r.is_empty());

    let app_clone = app.clone();
    let tokens = tokio::task::spawn_blocking(move || {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        { desktop::sign_in() }
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            let handle = app_clone
                .try_state::<mobile::GoogleAuthHandle<tauri::Wry>>()
                .ok_or("Google auth plugin not initialized".to_string())?;
            mobile::sign_in(handle.inner())
        }
    })
    .await
    .map_err(|e| format!("Sign-in task failed: {e}"))??;

    save_tokens(&app, &tokens)?;

    // Restore refresh_token if re-login didn't return a new one
    if tokens.refresh_token.as_deref().is_some_and(|s| s.is_empty()) {
        if let Some(refresh) = old_refresh {
            let mut restored = tokens.clone();
            restored.refresh_token = Some(refresh);
            save_tokens(&app, &restored)?;
        }
    }

    let email = get_user_email(&tokens.access_token, tokens.id_token.as_deref()).await.unwrap_or_else(|_| "Unknown".to_string());
    Ok(serde_json::json!({ "email": email }))
}

#[tauri::command]
pub async fn google_sign_out(app: AppHandle) -> Result<(), String> {
    if let Some(tokens) = load_tokens(&app)? {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            let access_token = tokens.access_token;
            let _ = tokio::task::spawn_blocking(move || desktop::revoke_token(access_token)).await;
        }
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            if let Some(handle) = app.try_state::<mobile::GoogleAuthHandle<tauri::Wry>>() {
                let _ = mobile::revoke_token(handle.inner(), tokens.access_token);
            }
        }
    }
    delete_tokens(&app)
}

#[tauri::command]
pub async fn google_get_auth_status(app: AppHandle) -> Result<serde_json::Value, String> {
    let signed_in = get_auth_status(&app)?;
    let email = if signed_in {
        // Load tokens to get id_token (for JWT-based email extraction)
        let id_token = load_tokens(&app)?
            .and_then(|t| t.id_token)
            .filter(|s| !s.is_empty());
        match get_access_token(&app).await {
            Ok(token) => get_user_email(&token, id_token.as_deref()).await.ok(),
            Err(_) => None,
        }
    } else {
        None
    };
    Ok(serde_json::json!({
        "signed_in": signed_in,
        "email": email,
    }))
}
