use crate::chat::{build_url, ChatMessage, ChatRequest, ChatResponse};
use crate::db::open_db;
use reqwest::Client;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct GrammarItem {
    pub id: String,
    pub pattern: String,
    pub detail: String,
    pub language: String,
    pub level: String,
    pub is_default: i64,
}

#[derive(Debug, Deserialize)]
pub struct SeedGrammarItem {
    pattern: String,
    detail: String,
    language: String,
    level: String,
}

#[derive(Serialize, Deserialize)]
pub struct GrammarAnalysis {
    message_id: i64,
    analysis: String,
}

#[tauri::command]
pub fn db_list_grammar_items(app: tauri::AppHandle, language: Option<String>, level: Option<String>) -> Result<Vec<GrammarItem>, String> {
    let conn = open_db(&app)?;
    let mut items = Vec::new();
    match (language, level) {
        (Some(lang), Some(lvl)) => {
            let mut stmt = conn.prepare(
                "SELECT id, pattern, detail, language, level, is_default FROM grammar_items WHERE language=?1 AND level=?2 ORDER BY is_default DESC, pattern ASC"
            ).map_err(|e| e.to_string())?;
            let rows = stmt.query_map(params![lang, lvl], |row| {
                Ok(GrammarItem { id: row.get(0)?, pattern: row.get(1)?, detail: row.get(2)?, language: row.get(3)?, level: row.get(4)?, is_default: row.get(5)? })
            }).map_err(|e| e.to_string())?;
            for r in rows { items.push(r.map_err(|e| e.to_string())?); }
        }
        (Some(lang), None) => {
            let mut stmt = conn.prepare(
                "SELECT id, pattern, detail, language, level, is_default FROM grammar_items WHERE language=?1 ORDER BY level ASC, is_default DESC, pattern ASC"
            ).map_err(|e| e.to_string())?;
            let rows = stmt.query_map(params![lang], |row| {
                Ok(GrammarItem { id: row.get(0)?, pattern: row.get(1)?, detail: row.get(2)?, language: row.get(3)?, level: row.get(4)?, is_default: row.get(5)? })
            }).map_err(|e| e.to_string())?;
            for r in rows { items.push(r.map_err(|e| e.to_string())?); }
        }
        (None, Some(lvl)) => {
            let mut stmt = conn.prepare(
                "SELECT id, pattern, detail, language, level, is_default FROM grammar_items WHERE level=?1 ORDER BY language ASC, is_default DESC, pattern ASC"
            ).map_err(|e| e.to_string())?;
            let rows = stmt.query_map(params![lvl], |row| {
                Ok(GrammarItem { id: row.get(0)?, pattern: row.get(1)?, detail: row.get(2)?, language: row.get(3)?, level: row.get(4)?, is_default: row.get(5)? })
            }).map_err(|e| e.to_string())?;
            for r in rows { items.push(r.map_err(|e| e.to_string())?); }
        }
        (None, None) => {
            let mut stmt = conn.prepare(
                "SELECT id, pattern, detail, language, level, is_default FROM grammar_items ORDER BY language ASC, level ASC, is_default DESC, pattern ASC"
            ).map_err(|e| e.to_string())?;
            let rows = stmt.query_map([], |row| {
                Ok(GrammarItem { id: row.get(0)?, pattern: row.get(1)?, detail: row.get(2)?, language: row.get(3)?, level: row.get(4)?, is_default: row.get(5)? })
            }).map_err(|e| e.to_string())?;
            for r in rows { items.push(r.map_err(|e| e.to_string())?); }
        }
    }
    Ok(items)
}

#[tauri::command]
pub fn db_add_grammar_item(app: tauri::AppHandle, pattern: String, detail: String, language: String, level: String) -> Result<GrammarItem, String> {
    let conn = open_db(&app)?;
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO grammar_items(id, pattern, detail, language, level, is_default) VALUES (?1, ?2, ?3, ?4, ?5, 0)",
        params![id, pattern, detail, language, level],
    ).map_err(|e| e.to_string())?;
    Ok(GrammarItem { id, pattern, detail, language, level, is_default: 0 })
}

#[tauri::command]
pub fn db_update_grammar_item(app: tauri::AppHandle, id: String, pattern: Option<String>, detail: Option<String>, level: Option<String>) -> Result<(), String> {
    let conn = open_db(&app)?;
    if let Some(p) = pattern {
        conn.execute("UPDATE grammar_items SET pattern=?1 WHERE id=?2", params![p, id]).map_err(|e| e.to_string())?;
    }
    if let Some(d) = detail {
        conn.execute("UPDATE grammar_items SET detail=?1 WHERE id=?2", params![d, id]).map_err(|e| e.to_string())?;
    }
    if let Some(l) = level {
        conn.execute("UPDATE grammar_items SET level=?1 WHERE id=?2", params![l, id]).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn db_delete_grammar_item(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute("DELETE FROM grammar_items WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn db_seed_grammar_items(app: tauri::AppHandle, items: Vec<SeedGrammarItem>) -> Result<String, String> {
    let conn = open_db(&app)?;
    let mut seeded = 0u32;
    for item in items {
        let id = uuid::Uuid::new_v4().to_string();
        let result = conn.execute(
            "INSERT INTO grammar_items(id, pattern, detail, language, level, is_default) VALUES (?1, ?2, ?3, ?4, ?5, 1)",
            params![id, item.pattern, item.detail, item.language, item.level],
        );
        if let Ok(n) = result {
            if n > 0 { seeded += 1; }
        }
    }
    Ok(format!("Seeded {} grammar items", seeded))
}

#[tauri::command]
pub fn db_get_grammar_levels(app: tauri::AppHandle, language: String) -> Result<Vec<String>, String> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT level FROM grammar_items WHERE language=?1 ORDER BY level ASC"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![language], |row| row.get::<_, String>(0)).map_err(|e| e.to_string())?;
    let mut levels = Vec::new();
    for r in rows {
        levels.push(r.map_err(|e| e.to_string())?);
    }
    Ok(levels)
}

#[tauri::command]
pub fn db_import_grammar_csv(app: tauri::AppHandle, items: Vec<SeedGrammarItem>) -> Result<String, String> {
    let conn = open_db(&app)?;
    let mut imported = 0u32;
    for item in items {
        let id = uuid::Uuid::new_v4().to_string();
        let result = conn.execute(
            "INSERT INTO grammar_items(id, pattern, detail, language, level, is_default) VALUES (?1, ?2, ?3, ?4, ?5, 0)",
            params![id, item.pattern, item.detail, item.language, item.level],
        );
        if let Ok(n) = result {
            if n > 0 { imported += 1; }
        }
    }
    Ok(format!("Imported {} grammar items", imported))
}

#[tauri::command]
pub fn db_save_selected_grammar(app: tauri::AppHandle, session_id: String, grammar_item_ids: Vec<String>) -> Result<(), String> {
    let conn = open_db(&app)?;
    let json = serde_json::to_string(&grammar_item_ids).map_err(|e| e.to_string())?;
    conn.execute("UPDATE chat_sessions SET selected_grammar=?1 WHERE id=?2", params![json, session_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn db_get_selected_grammar(app: tauri::AppHandle, session_id: String) -> Result<Vec<String>, String> {
    let conn = open_db(&app)?;
    let raw: Option<String> = conn.query_row(
        "SELECT selected_grammar FROM chat_sessions WHERE id=?1",
        params![session_id],
        |row| row.get(0),
    ).ok().flatten();
    match raw {
        Some(json) => {
            let ids: Vec<String> = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            Ok(ids)
        }
        None => Ok(vec![]),
    }
}

#[tauri::command]
pub fn db_get_grammar_analysis(app: tauri::AppHandle, message_id: i64) -> Result<Option<GrammarAnalysis>, String> {
    let conn = open_db(&app)?;
    let result = conn.query_row(
        "SELECT message_id, analysis FROM message_grammar_analysis WHERE message_id=?1",
        params![message_id],
        |row| Ok(GrammarAnalysis {
            message_id: row.get(0)?,
            analysis: row.get(1)?,
        }),
    );
    match result {
        Ok(analysis) => Ok(Some(analysis)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn db_analyze_grammar(
    app: tauri::AppHandle,
    api_host: String,
    api_key: String,
    model: String,
    message_id: i64,
    message_content: String,
    language: String,
    grammar_patterns: Vec<String>,
) -> Result<String, String> {
    let lang_name = match language.as_str() {
        "ja" => "Japanese",
        "id" => "Indonesian",
        _ => &language,
    };
    let focus = if grammar_patterns.is_empty() {
        "all notable grammar patterns".to_string()
    } else {
        format!("these grammar patterns (focus your analysis on them): {}", grammar_patterns.join(", "))
    };
    let prompt = format!(
        "You are a language learning grammar analyst. Analyze the following {} text and identify {} used in it. For each pattern:\n\
        - State the pattern name\n\
        - Show where it appears in the text\n\
        - Explain its meaning/usage briefly\n\n\
        Format your response in Markdown. Use headers (##) for each grammar pattern. If the text uses a specific JLPT-level grammar pattern, mention the level.\n\n\
        Text to analyze:\n\"{}\"",
        lang_name, focus, message_content
    );
    let url = build_url(&api_host, "chat/completions");
    let client = Client::new();
    let req = ChatRequest {
        model: &model,
        messages: &vec![ChatMessage { role: "user".to_string(), content: prompt }],
    };
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&req)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("API error: {} — {}", status, body.trim()));
    }
    let parsed: ChatResponse = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
    let analysis = parsed.choices.get(0).map(|c| c.message.content.clone()).unwrap_or_default();

    let conn = open_db(&app)?;
    conn.execute(
        "INSERT OR REPLACE INTO message_grammar_analysis(message_id, analysis) VALUES (?1, ?2)",
        params![message_id, analysis],
    ).map_err(|e| e.to_string())?;

    Ok(analysis)
}
