use super::{TokenData, DRIVE_SCOPE, GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tauri::Runtime;
use tauri::plugin::{PluginApi, PluginHandle};
#[cfg(target_os = "android")]
use tauri::plugin::mobile::PluginInvokeError;

// ─── Request / Response models ───────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct SignInRequest {
    client_id: String,
    client_secret: Option<String>,
    scopes: Option<Vec<String>>,
    flow_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct SignOutRequest {
    access_token: Option<String>,
    flow_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct RefreshTokenRequest {
    refresh_token: Option<String>,
    client_id: String,
    client_secret: Option<String>,
    scopes: Option<Vec<String>>,
    flow_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenResponse {
    id_token: Option<String>,
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<i64>,
    scopes: Option<Vec<String>>,
}

// ─── Plugin Handle ──────────────────────────────────────────────────────

/// Wrapper around Tauri's `PluginHandle` that exposes a simpler `run` API.
pub struct GoogleAuthHandle<R: Runtime> {
    inner: PluginHandle<R>,
}

impl<R: Runtime> GoogleAuthHandle<R> {
    /// Execute a method on the native mobile plugin and deserialize the response.
    fn run(&self, method: &str, payload: impl Serialize) -> Result<TokenResponse, String> {
        self.inner
            .run_mobile_plugin(method, payload)
            .map_err(|e| format!("Mobile plugin error: {}", e))
    }
}

/// Initialize the native mobile plugin by registering it with Tauri's plugin system.
///
/// Must be called from within a `.setup()` hook where `PluginApi` is available.
pub fn init<R: Runtime, C: DeserializeOwned>(
    api: &PluginApi<R, C>,
) -> Result<GoogleAuthHandle<R>, String> {
    #[cfg(target_os = "android")]
    let inner = api
        .register_android_plugin("com.langchef", "GoogleSignInPlugin")
        .map_err(|e| format!("Failed to register Android plugin: {}", e))?;

    #[cfg(target_os = "ios")]
    let inner = api
        .register_ios_plugin(init_ios_plugin)
        .map_err(|e| format!("Failed to register iOS plugin: {}", e))?;

    Ok(GoogleAuthHandle { inner })
}

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_ios_plugin);

// ─── Public API ─────────────────────────────────────────────────────────

/// Sign in via the native mobile flow (CredentialManager → AuthorizationClient).
pub fn sign_in<R: Runtime>(handle: &GoogleAuthHandle<R>) -> Result<TokenData, String> {
    let response = handle.run(
        "signIn",
        SignInRequest {
            client_id: GOOGLE_CLIENT_ID.to_string(),
            client_secret: Some(GOOGLE_CLIENT_SECRET.to_string()),
            scopes: Some(vec![DRIVE_SCOPE.to_string()]),
            flow_type: Some("native".to_string()),
        },
    )?;

    Ok(token_response_to_data(response))
}

/// Refresh tokens via the native mobile plugin.
pub fn refresh_token<R: Runtime>(
    handle: &GoogleAuthHandle<R>,
    refresh_token: &str,
) -> Result<TokenData, String> {
    let response = handle.run(
        "refreshToken",
        RefreshTokenRequest {
            refresh_token: Some(refresh_token.to_string()),
            client_id: GOOGLE_CLIENT_ID.to_string(),
            client_secret: Some(GOOGLE_CLIENT_SECRET.to_string()),
            scopes: Some(vec![DRIVE_SCOPE.to_string()]),
            flow_type: Some("native".to_string()),
        },
    )?;

    Ok(token_response_to_data(response))
}

/// Revoke an access token and sign out via the native mobile plugin.
pub fn revoke_token<R: Runtime>(handle: &GoogleAuthHandle<R>, access_token: String) -> Result<(), String> {
    let _ = handle
        .run(
            "signOut",
            SignOutRequest {
                access_token: Some(access_token),
                flow_type: Some("native".to_string()),
            },
        )
        .map_err(|_| "Sign-out failed".to_string())?;
    Ok(())
}

// ─── Internal helpers ────────────────────────────────────────────────────

fn token_response_to_data(resp: TokenResponse) -> TokenData {
    TokenData {
        access_token: resp.access_token,
        refresh_token: if resp.refresh_token.as_deref().is_some_and(|s| !s.is_empty()) {
            resp.refresh_token
        } else {
            None
        },
        id_token: resp.id_token,
        expires_at: resp
            .expires_at
            .map(|ts| (ts / 1000) as u64)
            .unwrap_or(0),
    }
}
