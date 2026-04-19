use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::AppHandle;

const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_DEVICE_CODE_URL: &str = "https://oauth2.googleapis.com/device/code";
const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive.file";

// These should be replaced with real credentials at build time or via config
// For development, use your own Google Cloud Project OAuth 2.0 Client IDs
const CLIENT_ID: &str = env!("GOOGLE_CLIENT_ID");
const CLIENT_SECRET: &str = env!("GOOGLE_CLIENT_SECRET");

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TokenData {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: u64, // Unix timestamp when token expires
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub expires_in: i64,
    pub interval: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: i64,
    pub token_type: String,
}

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
    if !path.exists() {
        return Ok(None);
    }
    let json = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let tokens: TokenData = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    Ok(Some(tokens))
}

pub fn delete_tokens(app: &AppHandle) -> Result<(), String> {
    let path = token_file(app)?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Check if we have valid stored tokens (refresh token exists)
pub fn get_auth_status(app: &AppHandle) -> Result<bool, String> {
    match load_tokens(app)? {
        Some(tokens) => Ok(tokens.refresh_token.is_some()),
        None => Ok(false),
    }
}

/// Get a valid access token, refreshing if needed
pub async fn get_access_token(app: &AppHandle) -> Result<String, String> {
    let mut tokens = match load_tokens(app)? {
        Some(t) => t,
        None => return Err("Not signed in to Google".to_string()),
    };

    // Check if access token is still valid (with 5 min buffer)
    let now = chrono::Utc::now().timestamp() as u64;
    if now >= tokens.expires_at.saturating_sub(300) {
        // Token needs refresh
        if let Some(refresh) = &tokens.refresh_token {
            tokens = refresh_access_token(refresh).await?;
            save_tokens(app, &tokens)?;
        } else {
            return Err("No refresh token available. Please sign in again.".to_string());
        }
    }

    Ok(tokens.access_token)
}

async fn refresh_access_token(refresh_token: &str) -> Result<TokenData, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(GOOGLE_TOKEN_URL)
        .form(&[
            ("refresh_token", refresh_token),
            ("client_id", CLIENT_ID),
            ("client_secret", CLIENT_SECRET),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Token refresh failed ({}): {}", status, body));
    }

    let token_resp: TokenResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse token response: {}", e))?;

    let expires_at = chrono::Utc::now().timestamp() as u64 + token_resp.expires_in as u64;

    Ok(TokenData {
        access_token: token_resp.access_token,
        refresh_token: token_resp.refresh_token.or_else(|| Some(refresh_token.to_string())),
        expires_at,
    })
}

/// Step 1 of Device Auth: Request device code
pub async fn request_device_code() -> Result<DeviceCodeResponse, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(GOOGLE_DEVICE_CODE_URL)
        .form(&[
            ("client_id", CLIENT_ID),
            ("scope", DRIVE_SCOPE),
        ])
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Device code request failed ({}): {}", status, body));
    }

    let code_resp: DeviceCodeResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse device code response: {}", e))?;

    Ok(code_resp)
}

/// Step 2 of Device Auth: Poll for token until user completes sign-in
pub async fn poll_for_token(
    device_code: &str,
    interval_secs: i64,
) -> Result<TokenData, String> {
    let client = reqwest::Client::new();
    let max_attempts = 300; // ~15 minutes at 3s intervals

    for _ in 0..max_attempts {
        tokio::time::sleep(tokio::time::Duration::from_secs(interval_secs.max(3) as u64)).await;

        let resp = client
            .post(GOOGLE_TOKEN_URL)
            .form(&[
                ("client_id", CLIENT_ID),
                ("client_secret", CLIENT_SECRET),
                ("device_code", device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;

        match resp.status().as_u16() {
            200 => {
                let token_resp: TokenResponse = resp
                    .json()
                    .await
                    .map_err(|e| format!("Failed to parse token response: {}", e))?;

                let expires_at =
                    chrono::Utc::now().timestamp() as u64 + token_resp.expires_in as u64;

                return Ok(TokenData {
                    access_token: token_resp.access_token,
                    refresh_token: token_resp.refresh_token,
                    expires_at,
                });
            }
            400 => {
                // "authorization_pending" - user hasn't completed yet, continue polling
                let body: serde_json::Value = resp.json().await.unwrap_or_default();
                let error = body["error"].as_str().unwrap_or("");
                if error == "authorization_pending" || error == "slow_down" {
                    continue;
                }
                return Err(format!("Authorization error: {}", error));
            }
            429 => {
                // Rate limited - slow down
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                continue;
            }
            other => {
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("Token poll failed ({}): {}", other, body));
            }
        }
    }

    Err("Sign-in timed out. Please try again.".to_string())
}

/// Revoke the refresh token and delete stored credentials
pub async fn revoke_and_sign_out(app: &AppHandle) -> Result<(), String> {
    let tokens = match load_tokens(app)? {
        Some(t) => t,
        None => return Ok(()), // Already signed out
    };

    if let Some(refresh_token) = tokens.refresh_token {
        let client = reqwest::Client::new();
        let _ = client
            .post("https://oauth2.googleapis.com/revoke")
            .form(&[("token", refresh_token)])
            .send()
            .await;
    }

    delete_tokens(app)
}

// --- Tauri Commands ---

#[tauri::command]
pub async fn google_sign_in(app: AppHandle) -> Result<serde_json::Value, String> {
    // Step 1: Get device code
    let code_resp = request_device_code().await?;

    // Return device code info to frontend so user can be shown the code
    // Frontend will then call google_complete_sign_in with the device_code
    Ok(serde_json::json!({
        "user_code": code_resp.user_code,
        "verification_url": code_resp.verification_url,
        "device_code": code_resp.device_code,
        "interval": code_resp.interval,
    }))
}

#[tauri::command]
pub async fn google_complete_sign_in(
    app: AppHandle,
    device_code: String,
    interval: i64,
) -> Result<serde_json::Value, String> {
    // Step 2: Poll for token
    let tokens = poll_for_token(&device_code, interval).await?;

    save_tokens(app, &tokens)?;

    // Fetch user info to get email
    let email = get_user_email(&tokens.access_token).await.unwrap_or_else(|_| "Unknown".to_string());

    Ok(serde_json::json!({
        "email": email,
    }))
}

#[tauri::command]
pub async fn google_sign_out(app: AppHandle) -> Result<(), String> {
    revoke_and_sign_out(&app).await
}

#[tauri::command]
pub async fn google_get_auth_status(app: AppHandle) -> Result<serde_json::Value, String> {
    let signed_in = get_auth_status(&app)?;

    let email = if signed_in {
        match load_tokens(&app)? {
            Some(tokens) => get_user_email(&tokens.access_token).await.ok(),
            None => None,
        }
    } else {
        None
    };

    Ok(serde_json::json!({
        "signed_in": signed_in,
        "email": email,
    }))
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
    Ok(info["email"]
        .as_str()
        .unwrap_or("Unknown")
        .to_string())
}
