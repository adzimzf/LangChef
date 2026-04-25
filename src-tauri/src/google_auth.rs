use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
const LOCALHOST_ADDR: &str = "127.0.0.1";
const DEFAULT_REDIRECT_HOST: &str = "localhost";
const SUCCESS_HTML: &str = "Go back to your app :)";

// ─── Credentials (compile-time env) ──────────────────────────────────

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
        Some(t) => Ok(t.refresh_token.is_some()),
        None => Ok(false),
    }
}

// ─── Desktop Implementation (Auth Code + PKCE) ────────────────────────────

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod desktop_impl {
    use super::{
        AppHandle, DEFAULT_REDIRECT_HOST, DRIVE_SCOPE, GOOGLE_AUTH_URL,
        GOOGLE_REVOKE_URL, GOOGLE_TOKEN_URL, LOCALHOST_ADDR, SUCCESS_HTML, TokenData,
        GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET,
    };
    use oauth2::basic::BasicClient;
    use oauth2::{AuthUrl, CsrfToken, PkceCodeChallenge, RedirectUrl, Scope, TokenUrl};
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    pub fn sign_in(_app: &AppHandle) -> Result<TokenData, String> {
        // Bind TCP listener first to get actual port
        let listener =
            TcpListener::bind(format!("{LOCALHOST_ADDR}:0")).map_err(|e| format!("Failed to bind to local port: {}", e))?;
        let actual_port = listener
            .local_addr()
            .map_err(|e| format!("Failed to get local address: {}", e))?
            .port();
        let redirect_url = format!("http://{DEFAULT_REDIRECT_HOST}:{actual_port}");

        // Generate PKCE challenge
        let (pkce_code_challenge, pkce_code_verifier) = PkceCodeChallenge::new_random_sha256();

        // Build OAuth2 client (builder pattern)
        let auth_url = AuthUrl::new(GOOGLE_AUTH_URL.to_string())
            .map_err(|_| "Invalid auth URL".to_string())?;
        let token_url = TokenUrl::new(GOOGLE_TOKEN_URL.to_string())
            .map_err(|_| "Invalid token URL".to_string())?;
        let redirect_uri = RedirectUrl::new(redirect_url.clone())
            .map_err(|_| "Invalid redirect URL".to_string())?;

        let _client = BasicClient::new(oauth2::ClientId::new(GOOGLE_CLIENT_ID.to_string()))
            .set_client_secret(oauth2::ClientSecret::new(GOOGLE_CLIENT_SECRET.to_string()))
            .set_auth_uri(auth_url)
            .set_token_uri(token_url)
            .set_redirect_uri(redirect_uri);

        // Build authorization URL
        let (authorize_url, _csrf_state) = _client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new(DRIVE_SCOPE.to_string()))
            .set_pkce_challenge(pkce_code_challenge)
            .url();

        // Open system browser
        open::that_detached(authorize_url.to_string())
            .map_err(|e| format!("Failed to open browser: {}", e))?;

        // Wait for redirect callback on localhost
        let code = {
            let mut stream = listener
                .incoming()
                .flatten()
                .next()
                .ok_or("Listener terminated without receiving a callback".to_string())?;
            let mut reader = BufReader::new(&stream);
            let mut request_line = String::new();
            reader.read_line(&mut request_line).map_err(|e| e.to_string())?;
            let request_path = request_line
                .split_whitespace()
                .nth(1)
                .ok_or("Invalid HTTP request format".to_string())?;
            let url = url::Url::parse(&format!(
                "http://{DEFAULT_REDIRECT_HOST}{request_path}"
            ))
            .map_err(|e| format!("Failed to parse callback URL: {e}"))?;
            let auth_code = url
                .query_pairs()
                .find(|(key, _)| key == "code")
                .map(|(_, v)| v.into_owned())
                .ok_or("Authorization code not found in callback".to_string())?;

            // Send success response
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{}",
                SUCCESS_HTML.len(),
                SUCCESS_HTML,
            );
            stream.write_all(response.as_bytes()).map_err(|e| e.to_string())?;
            auth_code
        };

        exchange_code(
            code,
            &redirect_url,
            &pkce_code_verifier.secret(),
        )
    }

    pub fn refresh_token(_app: &AppHandle, refresh_token: &str) -> Result<TokenData, String> {
        exchange_refresh_token(refresh_token)
    }

    pub fn revoke_token(access_token: String) -> Result<(), String> {
        let client = reqwest::blocking::Client::new();
        let _ = client
            .post(GOOGLE_REVOKE_URL)
            .form(&[("token", access_token)])
            .send();
        Ok(())
    }

    fn exchange_code(
        code: String,
        redirect_uri: &str,
        pkce_verifier: &str,
    ) -> Result<TokenData, String> {
        let http = reqwest::blocking::Client::new();
        let params = [
            ("code", code),
            ("client_id", GOOGLE_CLIENT_ID.to_string()),
            ("client_secret", GOOGLE_CLIENT_SECRET.to_string()),
            ("redirect_uri", redirect_uri.to_string()),
            ("grant_type", "authorization_code".to_string()),
            ("code_verifier", pkce_verifier.to_string()),
        ];
        let resp = http
            .post(GOOGLE_TOKEN_URL)
            .form(&params)
            .send()
            .map_err(|e| format!("Token exchange failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!(
                "Token exchange failed ({}): {}",
                resp.status(),
                resp.text().unwrap_or_default(),
            ));
        }

        let json: serde_json::Value = resp.json().map_err(|e: reqwest::Error| e.to_string())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(TokenData {
            access_token: json["access_token"].as_str().unwrap_or("").to_string(),
            refresh_token: json["refresh_token"].as_str().map(String::from),
            id_token: json["id_token"].as_str().map(String::from),
            expires_at: now + json["expires_in"].as_u64().unwrap_or(0),
        })
    }

    fn exchange_refresh_token(refresh_token: &str) -> Result<TokenData, String> {
        let http = reqwest::blocking::Client::new();
        let params = [
            ("refresh_token", refresh_token.to_string()),
            ("client_id", GOOGLE_CLIENT_ID.to_string()),
            ("client_secret", GOOGLE_CLIENT_SECRET.to_string()),
            ("grant_type", "refresh_token".to_string()),
        ];
        let resp = http
            .post(GOOGLE_TOKEN_URL)
            .form(&params)
            .send()
            .map_err(|e| format!("Token refresh failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!(
                "Token refresh failed ({}): {}",
                resp.status(),
                resp.text().unwrap_or_default(),
            ));
        }

        let json: serde_json::Value = resp.json().map_err(|e: reqwest::Error| e.to_string())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(TokenData {
            access_token: json["access_token"].as_str().unwrap_or("").to_string(),
            refresh_token: json["refresh_token"]
                .as_str()
                .map(|s: &str| s.to_string())
                .or_else(|| Some(refresh_token.to_string())),
            id_token: json["id_token"].as_str().map(String::from),
            expires_at: now + json["expires_in"].as_u64().unwrap_or(0),
        })
    }
}

// ─── Mobile Stubs ─────────────────────────────────────────────────────────

#[cfg(any(target_os = "android", target_os = "ios"))]
mod mobile_impl {
    use super::{AppHandle, TokenData};

    pub fn sign_in(_app: &AppHandle) -> Result<TokenData, String> {
        Err("Google Sign-In is not yet supported on mobile platforms".to_string())
    }
    pub fn refresh_token(_app: &AppHandle, _refresh_token: &str) -> Result<TokenData, String> {
        Err("Token refresh is not yet supported on mobile platforms".to_string())
    }
    pub fn revoke_token(_access_token: String) -> Result<(), String> { Ok(()) }
}

// ─── Public API (platform-agnostic) ────────────────────────────────────────

pub async fn get_access_token(app: &AppHandle) -> Result<String, String> {
    let mut tokens = match load_tokens(app)? {
        Some(t) => t,
        None => return Err("Not signed in to Google".to_string()),
    };
    let now = chrono::Utc::now().timestamp() as u64;
    if now >= tokens.expires_at.saturating_sub(300) {
        if let Some(refresh) = &tokens.refresh_token {
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            { tokens = desktop_impl::refresh_token(app, refresh)?; }
            #[cfg(any(target_os = "android", target_os = "ios"))]
            { tokens = mobile_impl::refresh_token(app, refresh)?; }
            save_tokens(app, &tokens)?;
        } else {
            return Err("No refresh token available. Please sign in again.".to_string());
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
        { desktop_impl::sign_in(&app_clone) }
        #[cfg(any(target_os = "android", target_os = "ios"))]
        { mobile_impl::sign_in(&app_clone) }
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
    { let _ = desktop_impl::revoke_token(tokens.access_token); }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    { let _ = mobile_impl::revoke_token(tokens.access_token); }
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
