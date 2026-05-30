use reqwest::Client;
use serde::{Deserialize, Serialize};

pub fn build_url(base: &str, path: &str) -> String {
    let b = base.trim_end_matches('/');
    let p = path.trim_start_matches('/');
    format!("{}/{}", b, p)
}

#[derive(Debug, Deserialize)]
struct ModelObject {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelObject>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct ChatRequest<'a> {
    pub model: &'a str,
    pub messages: &'a [ChatMessage],
}

#[derive(Debug, Deserialize)]
pub struct ChoiceMessage {
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct Choice {
    pub message: ChoiceMessage,
}

#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<Choice>,
}

#[tauri::command]
pub async fn list_models(api_host: String, api_key: String) -> Result<Vec<String>, String> {
    let url = build_url(&api_host, "models");
    let client = Client::new();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await
        .map_err(|e| format!("Request failed: {} (url: {})", e, url))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("API error: {} — {} (url: {})", status, body.trim(), url));
    }
    let parsed: ModelsResponse = resp.json().await.map_err(|e| format!("Parse error: {} (url: {})", e, url))?;
    Ok(parsed.data.iter().map(|m| m.id.clone()).collect())
}

#[tauri::command]
pub async fn test_provider(api_host: String, api_key: String) -> Result<String, String> {
    let url = build_url(&api_host, "models");
    let client = Client::new();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await
        .map_err(|e| format!("Request failed: {} (url: {})", e, url))?;
    if resp.status().is_success() {
        Ok("ok".to_string())
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(format!("API error: {} — {} (url: {})", status, body.trim(), url))
    }
}

#[tauri::command]
pub async fn llm_chat(
    api_host: String,
    api_key: String,
    model: String,
    messages: Vec<ChatMessage>,
) -> Result<String, String> {
    let url = build_url(&api_host, "chat/completions");
    let client = Client::new();
    let req = ChatRequest {
        model: &model,
        messages: &messages,
    };
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&req)
        .send()
        .await
        .map_err(|e| format!("Request failed: {} (url: {})", e, url))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("API error: {} — {} (url: {})", status, body.trim(), url));
    }
    let parsed: ChatResponse = resp.json().await.map_err(|e| format!("Parse error: {} (url: {})", e, url))?;
    let text = parsed
        .choices
        .get(0)
        .map(|c| c.message.content.clone())
        .unwrap_or_default();
    Ok(text)
}
