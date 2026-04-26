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
    if now >= tokens.expires_at.saturating_sub(300) {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            if let Some(refresh) = &tokens.refresh_token {
                tokens = desktop::refresh_token(refresh)?;
                save_tokens(app, &tokens)?;
            } else {
                return Err("No refresh token available. Please sign in again.".to_string());
            }
        }
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            // Mobile: re-authorize via AuthorizationClient (silent if previously granted)
            let handle = app
                .try_state::<mobile::GoogleAuthHandle<tauri::Wry>>()
                .ok_or("Google auth plugin not initialized")?;
            tokens = mobile::refresh_token(handle.inner(), "")?;
            save_tokens(app, &tokens)?;
        }
    }
    Ok(tokens.access_token)
}

async fn get_user_email(access_token: &str) -> Result<String, String> {
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

// ─── Tauri Commands ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn google_sign_in(app: AppHandle) -> Result<serde_json::Value, String> {
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
    let email = get_user_email(&tokens.access_token).await.unwrap_or_else(|_| "Unknown".to_string());
    Ok(serde_json::json!({ "email": email }))
}

#[tauri::command]
pub async fn google_sign_out(app: AppHandle) -> Result<(), String> {
    let tokens = match load_tokens(&app)? {
        Some(t) => t,
        None => return Ok(()),
    };
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    { let _ = desktop::revoke_token(tokens.access_token); }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        if let Some(handle) = app.try_state::<mobile::GoogleAuthHandle<tauri::Wry>>() {
            let _ = mobile::revoke_token(handle.inner(), tokens.access_token);
        }
    }
    delete_tokens(&app)
}

#[tauri::command]
pub async fn google_get_auth_status(app: AppHandle) -> Result<serde_json::Value, String> {
    let signed_in = get_auth_status(&app)?;
    let email = if signed_in {
        match get_access_token(&app).await {
            Ok(token) => get_user_email(&token).await.ok(),
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
