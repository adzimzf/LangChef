use super::{DEFAULT_REDIRECT_HOST, DRIVE_SCOPE, GOOGLE_AUTH_URL, GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET, GOOGLE_REVOKE_URL, GOOGLE_TOKEN_URL, LOCALHOST_ADDR, SUCCESS_HTML, TokenData};
use oauth2::basic::BasicClient;
use oauth2::{AuthUrl, CsrfToken, PkceCodeChallenge, RedirectUrl, Scope, TokenUrl};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

/// Sign in using the desktop OAuth 2.0 Authorization Code flow with PKCE.
///
/// Opens the system browser for the user to authenticate, then listens on a local
/// port for the OAuth callback to receive the authorization code.
pub fn sign_in() -> Result<TokenData, String> {
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
    let (mut authorize_url, _csrf_state) = _client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new(DRIVE_SCOPE.to_string()))
        .add_scope(Scope::new("openid".to_string()))
        .add_scope(Scope::new("email".to_string()))
        .set_pkce_challenge(pkce_code_challenge)
        .url();
    authorize_url
        .query_pairs_mut()
        .append_pair("access_type", "offline");

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

/// Refresh an expired access token using a stored refresh token.
pub fn refresh_token(refresh_token: &str) -> Result<TokenData, String> {
    exchange_refresh_token(refresh_token)
}

/// Revoke an access token (best-effort on sign-out).
pub fn revoke_token(access_token: String) -> Result<(), String> {
    let client = reqwest::blocking::Client::new();
    let _ = client
        .post(GOOGLE_REVOKE_URL)
        .form(&[("token", access_token)])
        .send();
    Ok(())
}

// ─── Internal helpers ────────────────────────────────────────────────────

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

    parse_token_response(resp)
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

fn parse_token_response(resp: reqwest::blocking::Response) -> Result<TokenData, String> {
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
