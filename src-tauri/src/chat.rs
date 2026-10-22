use crate::db::open_db;
use crate::locale::{app_error, explain_instruction, explain_lang, lang_name, looks_english};
use reqwest::Client;
use rusqlite::{params, OptionalExtension};
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
    pub stream: bool,
    /// OpenRouter only: turns off (or keeps short) the silent "thinking" before a model writes anything.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<serde_json::Value>,
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

/// Maps a failed HTTP call to an error code the frontend can explain in the learner's language.
fn http_error(status: reqwest::StatusCode, body: &str, url: &str) -> String {
    let code = match status.as_u16() {
        401 | 403 => "api_unauthorized",
        402 => "api_no_credit",
        404 => "api_not_found",
        429 => "api_rate_limited",
        500..=599 => "api_server_error",
        _ => "api_error",
    };
    app_error(code, format!("{} — {} (url: {})", status, body.trim(), url))
}

fn send_error(e: reqwest::Error, url: &str) -> String {
    app_error("api_unreachable", format!("{} (url: {})", e, url))
}

async fn get_models(api_host: &str, api_key: &str) -> Result<ModelsResponse, String> {
    let url = build_url(api_host, "models");
    let resp = Client::new()
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await
        .map_err(|e| send_error(e, &url))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(http_error(status, &body, &url));
    }
    resp.json().await.map_err(|e| app_error("api_bad_response", format!("{} (url: {})", e, url)))
}

/// Longest silence allowed while waiting for the next piece of a streamed answer.
const STREAM_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// One chat completion against the user's OpenAI-compatible provider. Every LLM call goes through here.
/// The answer is streamed so long generations keep the connection busy instead of sitting silent
/// until the whole text is ready (which mobile networks and proxies cut off as a timeout).
pub async fn complete(api_host: &str, api_key: &str, model: &str, messages: &[ChatMessage]) -> Result<String, String> {
    let url = build_url(api_host, "chat/completions");
    let client = Client::builder()
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| send_error(e, &url))?;
    let send = |reasoning: Option<serde_json::Value>| {
        let req = ChatRequest { model, messages, stream: true, reasoning };
        client.post(&url).header("Authorization", format!("Bearer {}", api_key)).json(&req).send()
    };
    // None of our prompts need reasoning, and a reasoning model writes nothing while it thinks:
    // on N1 passages it was ~75% of the answer and most of the wait. Some models can't turn it
    // off; those get the shortest reasoning instead.
    let openrouter = api_host.contains("openrouter.ai");
    let mut resp = send(openrouter.then(|| serde_json::json!({ "enabled": false })))
        .await
        .map_err(|e| send_error(e, &url))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !(openrouter && status.as_u16() == 400 && body.contains("easoning")) {
            return Err(http_error(status, &body, &url));
        }
        resp = send(Some(serde_json::json!({ "effort": "low" })))
            .await
            .map_err(|e| send_error(e, &url))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(http_error(status, &body, &url));
        }
    }
    let is_stream = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/event-stream"));

    let mut stream = SseReader::default();
    let mut raw: Vec<u8> = Vec::new();
    let mut received = 0usize;
    loop {
        let chunk = tokio::time::timeout(STREAM_IDLE_TIMEOUT, resp.chunk())
            .await
            .map_err(|_| {
                let model = stream.model.as_deref().unwrap_or("?");
                app_error(
                    "api_timeout",
                    format!("no data for {}s after {} bytes (model: {}, url: {})", STREAM_IDLE_TIMEOUT.as_secs(), received, model, url),
                )
            })?
            .map_err(|e| send_error(e, &url))?;
        let Some(chunk) = chunk else { break };
        received += chunk.len();
        if !is_stream {
            raw.extend_from_slice(&chunk);
            continue;
        }
        if stream.push(&chunk)? {
            break;
        }
    }
    if is_stream {
        return Ok(stream.text);
    }
    // The provider ignored `stream` and sent the whole answer at once.
    let parsed: ChatResponse = serde_json::from_slice(&raw)
        .map_err(|e| app_error("api_bad_response", format!("{} (url: {})", e, url)))?;
    Ok(parsed.choices.first().map(|c| c.message.content.clone()).unwrap_or_default())
}

/// Collects the text of a server-sent-events chat completion stream.
#[derive(Default)]
struct SseReader {
    pending: Vec<u8>,
    text: String,
    /// The model the provider actually picked (e.g. behind `openrouter/auto`), for error details.
    model: Option<String>,
}

impl SseReader {
    /// Feeds raw bytes; returns true once the stream says it is done.
    fn push(&mut self, bytes: &[u8]) -> Result<bool, String> {
        self.pending.extend_from_slice(bytes);
        while let Some(nl) = self.pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=nl).collect();
            let line = String::from_utf8_lossy(&line);
            // Lines starting with ':' are keep-alive comments; blank lines end an event.
            let Some(data) = line.trim().strip_prefix("data:") else { continue };
            let data = data.trim();
            if data == "[DONE]" {
                return Ok(true);
            }
            let Ok(event) = serde_json::from_str::<serde_json::Value>(data) else { continue };
            if self.model.is_none() {
                self.model = event["model"].as_str().map(str::to_string);
            }
            if let Some(err) = event.get("error") {
                return Err(app_error("api_error", err.to_string()));
            }
            if let Some(piece) = event["choices"][0]["delta"]["content"].as_str() {
                self.text.push_str(piece);
            }
        }
        Ok(false)
    }
}

pub async fn complete_prompt(api_host: &str, api_key: &str, model: &str, prompt: String) -> Result<String, String> {
    complete(api_host, api_key, model, &[ChatMessage { role: "user".to_string(), content: prompt }]).await
}

/// Like `complete_prompt`, with the explanation-language rule appended. When the learner reads
/// Indonesian and the part of the answer they read (`learner_text`) came back in English, asks once more.
pub async fn complete_explained(
    api_host: &str,
    api_key: &str,
    model: &str,
    prompt: &str,
    explain: &str,
    learner_text: impl Fn(&str) -> String,
) -> Result<String, String> {
    let first = complete_prompt(api_host, api_key, model, format!("{}\n\n{}", prompt, explain_instruction(explain))).await?;
    if explain != "id" || !looks_english(&learner_text(&first)) {
        return Ok(first);
    }
    let retry = format!(
        "{}\n\n{}\nYour previous answer was in English. Answer again with all learner-facing text in Bahasa Indonesia.",
        prompt,
        explain_instruction(explain)
    );
    complete_prompt(api_host, api_key, model, retry).await
}

/// Strips a ```json fence and anything around the outermost JSON object or array.
pub fn json_part(raw: &str) -> &str {
    let t = raw.trim();
    let start = t.find(['{', '[']);
    let end = t.rfind(['}', ']']);
    match (start, end) {
        (Some(s), Some(e)) if e > s => &t[s..=e],
        _ => t,
    }
}

#[tauri::command]
pub async fn list_models(api_host: String, api_key: String) -> Result<Vec<String>, String> {
    Ok(get_models(&api_host, &api_key).await?.data.into_iter().map(|m| m.id).collect())
}

#[tauri::command]
pub async fn test_provider(api_host: String, api_key: String) -> Result<String, String> {
    get_models(&api_host, &api_key).await.map(|_| "ok".to_string())
}

#[tauri::command]
pub async fn llm_chat(
    api_host: String,
    api_key: String,
    model: String,
    messages: Vec<ChatMessage>,
) -> Result<String, String> {
    complete(&api_host, &api_key, &model, &messages).await
}

/// Translates text into the learner's explanation language. `kind` + `ref_id` (a message or a
/// reading session) cache the result so tapping "Translate" again costs nothing.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn llm_translate(
    app: tauri::AppHandle,
    api_host: String,
    api_key: String,
    model: String,
    text: String,
    from_lang: String,
    kind: Option<String>,
    ref_id: Option<i64>,
) -> Result<String, String> {
    let to = explain_lang(&open_db(&app)?);
    let cache_key = kind.zip(ref_id);
    if let Some((k, id)) = &cache_key {
        let conn = open_db(&app)?;
        let cached: Option<String> = conn
            .query_row(
                "SELECT text FROM translations WHERE kind = ?1 AND ref_id = ?2 AND lang = ?3",
                params![k, id, to],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(t) = cached {
            return Ok(t);
        }
    }
    let plain = strip_markup(&text);
    let prompt = format!(
        "Translate this {from} text into natural, simple {to}. Translate directly from {from}, not via another language. \
         Keep line breaks. Return ONLY the translation, no notes, no quotes.\n\nText:\n{plain}",
        from = lang_name(&from_lang),
        to = lang_name(&to),
    );
    let translated = complete_prompt(&api_host, &api_key, &model, prompt).await?.trim().to_string();
    if let Some((k, id)) = cache_key {
        let conn = open_db(&app)?;
        conn.execute(
            "INSERT OR REPLACE INTO translations(kind, ref_id, lang, text) VALUES (?1, ?2, ?3, ?4)",
            params![k, id, to, translated],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(translated)
}

#[derive(Serialize, Deserialize)]
pub struct HelpReply {
    pub reply: String,
    pub gloss: String,
}

/// "Help me reply": the learner says in their own language what they want to say; this returns a
/// reply in the target language at their level, plus what it means, for them to edit and send.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn llm_help_reply(
    app: tauri::AppHandle,
    api_host: String,
    api_key: String,
    model: String,
    intent: String,
    context_messages: Vec<ChatMessage>,
    target_lang: String,
    level_rule: Option<String>,
) -> Result<HelpReply, String> {
    let explain = explain_lang(&open_db(&app)?);
    let context: Vec<String> = context_messages
        .iter()
        .rev()
        .take(6)
        .rev()
        .map(|m| format!("{}: {}", m.role, strip_markup(&m.content)))
        .collect();
    let prompt = format!(
        "A learner of {target} is chatting with a conversation partner. They wrote in {explain_name} what they want to say next.\n\
         Write that reply in natural {target} that fits the conversation.\n{rule}\n\
         Conversation so far:\n{context}\n\n\
         What the learner wants to say ({explain_name}): \"{intent}\"\n\n\
         Return ONLY JSON (no markdown): {{\"reply\": \"the reply in {target}, plain text, no furigana markup\", \"gloss\": \"what the reply says, in {explain_name}\"}}",
        target = lang_name(&target_lang),
        explain_name = lang_name(&explain),
        rule = level_rule.map(|r| format!("Level rule: {}", r)).unwrap_or_default(),
        context = context.join("\n"),
        intent = intent.trim(),
    );
    let gloss_of = |raw: &str| {
        serde_json::from_str::<HelpReply>(json_part(raw)).map(|h| h.gloss).unwrap_or_default()
    };
    let raw = complete_explained(&api_host, &api_key, &model, &prompt, &explain, gloss_of).await?;
    serde_json::from_str::<HelpReply>(json_part(&raw)).map_err(|e| app_error("api_bad_response", format!("{}: {}", e, raw)))
}

/// Drops `{kanji|reading}` furigana and `[G:…]` grammar markers so the LLM sees plain text.
pub fn strip_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                let mut inner = String::new();
                let mut closed = false;
                for n in chars.by_ref() {
                    if n == '}' {
                        closed = true;
                        break;
                    }
                    inner.push(n);
                }
                match (closed, inner.split_once('|')) {
                    (true, Some((base, _))) => out.push_str(base),
                    _ => {
                        out.push('{');
                        out.push_str(&inner);
                        if closed {
                            out.push('}');
                        }
                    }
                }
            }
            '[' if chars.peek() == Some(&'G') || chars.peek() == Some(&'/') => {
                let mut tag = String::new();
                let mut closed = false;
                for n in chars.by_ref() {
                    if n == ']' {
                        closed = true;
                        break;
                    }
                    tag.push(n);
                }
                if !(closed && (tag.starts_with("G:") || tag == "/G")) {
                    out.push('[');
                    out.push_str(&tag);
                    if closed {
                        out.push(']');
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_furigana_and_grammar_markers() {
        assert_eq!(strip_markup("{食|た}べる[G:ている]見ている[/G]"), "食べる見ている");
        assert_eq!(strip_markup("[note] {x}"), "[note] {x}");
    }

    #[test]
    fn finds_json_inside_fences() {
        assert_eq!(json_part("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(json_part("[1,2]"), "[1,2]");
    }

    #[test]
    fn sse_reader_joins_deltas_across_split_chunks() {
        let mut r = SseReader::default();
        assert!(!r.push(b": OPENROUTER PROCESSING\n\ndata: {\"model\":\"m\",\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\nda").unwrap());
        assert!(!r.push("ta: {\"choices\":[{\"delta\":{\"content\":\"lo 世界\"}}]}\n\n".as_bytes()).unwrap());
        assert!(r.push(b"data: [DONE]\n\n").unwrap());
        assert_eq!(r.text, "Hello 世界");
        assert_eq!(r.model.as_deref(), Some("m"));
    }

    #[test]
    fn sse_reader_reports_a_mid_stream_error() {
        let mut r = SseReader::default();
        assert!(r.push(b"data: {\"error\":{\"message\":\"overloaded\"}}\n\n").is_err());
    }
}
