use crate::chat::{complete_explained, json_part};
use crate::db::open_db;
use crate::locale::{app_error, explain_lang, lang_name};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct GrammarItem {
    pub id: String,
    pub pattern: String,
    pub detail: String,
    pub language: String,
    pub level: String,
    pub is_default: i64,
    /// Language the explanation is written in.
    pub explain_lang: String,
    pub ai_translated: bool,
    /// Shown in English because this level has nothing in the learner's language yet.
    #[serde(default)]
    pub fallback: bool,
}

const GRAMMAR_COLS: &str = "id, pattern, detail, language, level, is_default, explain_lang, ai_translated";

fn row_to_grammar(row: &rusqlite::Row) -> rusqlite::Result<GrammarItem> {
    Ok(GrammarItem {
        id: row.get(0)?,
        pattern: row.get(1)?,
        detail: row.get(2)?,
        language: row.get(3)?,
        level: row.get(4)?,
        is_default: row.get(5)?,
        explain_lang: row.get(6)?,
        ai_translated: row.get(7)?,
        fallback: false,
    })
}

/// Keeps, per (language, level), the rows in the learner's language; a level with none falls back
/// to English rows, flagged so the UI can offer an AI translation.
fn pick_explain_lang(items: Vec<GrammarItem>, explain: &str) -> Vec<GrammarItem> {
    use std::collections::HashSet;
    let covered: HashSet<(String, String)> = items
        .iter()
        .filter(|g| g.explain_lang == explain)
        .map(|g| (g.language.clone(), g.level.clone()))
        .collect();
    items
        .into_iter()
        .filter_map(|mut g| {
            let key = (g.language.clone(), g.level.clone());
            if g.explain_lang == explain {
                Some(g)
            } else if !covered.contains(&key) && g.explain_lang == "en" {
                g.fallback = true;
                Some(g)
            } else {
                None
            }
        })
        .collect()
}

fn list_grammar(conn: &Connection, language: Option<&str>, level: Option<&str>) -> Result<Vec<GrammarItem>, String> {
    let mut sql = format!("SELECT {} FROM grammar_items WHERE 1=1", GRAMMAR_COLS);
    let mut args: Vec<String> = Vec::new();
    if let Some(l) = language {
        args.push(l.to_string());
        sql.push_str(&format!(" AND language=?{}", args.len()));
    }
    if let Some(l) = level {
        args.push(l.to_string());
        sql.push_str(&format!(" AND level=?{}", args.len()));
    }
    sql.push_str(" ORDER BY language ASC, level ASC, is_default DESC, pattern ASC");
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(args.iter()), row_to_grammar)
        .map_err(|e| e.to_string())?;
    let items: Vec<GrammarItem> = rows.collect::<Result<_, _>>().map_err(|e| e.to_string())?;
    Ok(pick_explain_lang(items, &explain_lang(conn)))
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
    list_grammar(&conn, language.as_deref(), level.as_deref())
}

#[allow(clippy::too_many_arguments)]
fn insert_grammar(conn: &Connection, pattern: &str, detail: &str, language: &str, level: &str, is_default: bool, explain: &str, ai_translated: bool) -> Result<GrammarItem, String> {
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO grammar_items(id, pattern, detail, language, level, is_default, explain_lang, ai_translated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, pattern, detail, language, level, is_default, explain, ai_translated],
    )
    .map_err(|e| e.to_string())?;
    Ok(GrammarItem {
        id,
        pattern: pattern.to_string(),
        detail: detail.to_string(),
        language: language.to_string(),
        level: level.to_string(),
        is_default: is_default as i64,
        explain_lang: explain.to_string(),
        ai_translated,
        fallback: false,
    })
}

#[tauri::command]
pub fn db_add_grammar_item(app: tauri::AppHandle, pattern: String, detail: String, language: String, level: String) -> Result<GrammarItem, String> {
    let conn = open_db(&app)?;
    let explain = explain_lang(&conn);
    insert_grammar(&conn, &pattern, &detail, &language, &level, false, &explain, false)
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
    import_grammar(&app, items, true)
}

fn import_grammar(app: &tauri::AppHandle, items: Vec<SeedGrammarItem>, is_default: bool) -> Result<String, String> {
    let conn = open_db(app)?;
    let explain = explain_lang(&conn);
    let imported = items
        .iter()
        .filter(|i| insert_grammar(&conn, &i.pattern, &i.detail, &i.language, &i.level, is_default, &explain, false).is_ok())
        .count();
    Ok(imported.to_string())
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
    import_grammar(&app, items, false)
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
        "SELECT message_id, analysis FROM message_grammar_analysis WHERE message_id=?1 AND lang=?2",
        params![message_id, explain_lang(&conn)],
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
    let explain = explain_lang(&open_db(&app)?);
    let lang_name = lang_name(&language);
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
    let analysis = complete_explained(&api_host, &api_key, &model, &prompt, &explain, |t| t.to_string()).await?;

    let conn = open_db(&app)?;
    conn.execute(
        "INSERT OR REPLACE INTO message_grammar_analysis(message_id, analysis, lang) VALUES (?1, ?2, ?3)",
        params![message_id, analysis, explain],
    ).map_err(|e| e.to_string())?;

    Ok(analysis)
}

#[derive(Deserialize)]
struct TranslatedGrammar {
    pattern: String,
    detail: String,
}

/// AI-translates one grammar row into the learner's language and saves it as a new row marked
/// as AI-translated, next to the original.
async fn translate_grammar_row(app: &tauri::AppHandle, api_host: &str, api_key: &str, model: &str, item: &GrammarItem, explain: &str) -> Result<GrammarItem, String> {
    let prompt = format!(
        "Translate this {studied} grammar explanation into natural, simple {to} for a learner whose language is {to}. \
         Translate directly, not via another language. Keep the pattern's {studied} text and every {studied} example sentence as they are; \
         translate only the explanation and the meanings of examples. Where it helps, compare with {to} grammar instead of English. \
         Keep the 'Usage:' / 'Context:' structure, translating those labels too.\n\n\
         Pattern: {pattern}\nExplanation:\n{detail}\n\n\
         Return ONLY JSON (no markdown): {{\"pattern\": \"the pattern, with any English words translated\", \"detail\": \"the translated explanation\"}}",
        studied = lang_name(&item.language),
        to = lang_name(explain),
        pattern = item.pattern,
        detail = item.detail,
    );
    let raw = complete_explained(api_host, api_key, model, &prompt, explain, |raw| {
        serde_json::from_str::<TranslatedGrammar>(json_part(raw)).map(|t| t.detail).unwrap_or_default()
    })
    .await?;
    let t: TranslatedGrammar = serde_json::from_str(json_part(&raw)).map_err(|e| app_error("api_bad_response", format!("{}: {}", e, raw)))?;
    let conn = open_db(app)?;
    insert_grammar(&conn, &t.pattern, &t.detail, &item.language, &item.level, false, explain, true)
}

fn grammar_by_id(conn: &Connection, id: &str) -> Result<GrammarItem, String> {
    conn.query_row(&format!("SELECT {} FROM grammar_items WHERE id=?1", GRAMMAR_COLS), params![id], row_to_grammar)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn db_translate_grammar_item(app: tauri::AppHandle, api_host: String, api_key: String, model: String, id: String) -> Result<GrammarItem, String> {
    let (item, explain) = {
        let conn = open_db(&app)?;
        (grammar_by_id(&conn, &id)?, explain_lang(&conn))
    };
    translate_grammar_row(&app, &api_host, &api_key, &model, &item, &explain).await
}

#[derive(Serialize)]
pub struct TranslateLevelResult {
    pub translated: usize,
    pub failed: Vec<String>,
}

/// Translates every English row of a level that the learner's language doesn't have yet.
#[tauri::command]
pub async fn db_translate_grammar_level(app: tauri::AppHandle, api_host: String, api_key: String, model: String, language: String, level: String) -> Result<TranslateLevelResult, String> {
    let (rows, explain) = {
        let conn = open_db(&app)?;
        let rows: Vec<GrammarItem> = list_grammar(&conn, Some(&language), Some(&level))?.into_iter().filter(|g| g.fallback).collect();
        (rows, explain_lang(&conn))
    };
    let mut translated = 0;
    let mut failed = Vec::new();
    for item in rows {
        match translate_grammar_row(&app, &api_host, &api_key, &model, &item, &explain).await {
            Ok(_) => translated += 1,
            Err(_) => failed.push(item.pattern),
        }
    }
    Ok(TranslateLevelResult { translated, failed })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(level: &str, explain: &str) -> GrammarItem {
        GrammarItem {
            id: format!("{level}-{explain}"),
            pattern: "p".into(),
            detail: "d".into(),
            language: "ja".into(),
            level: level.into(),
            is_default: 1,
            explain_lang: explain.into(),
            ai_translated: explain != "en",
            fallback: false,
        }
    }

    #[test]
    fn picks_learner_language_and_falls_back_per_level() {
        let items = vec![g("N5", "en"), g("N5", "id"), g("N3", "en")];
        let picked = pick_explain_lang(items, "id");
        let ids: Vec<(&str, bool)> = picked.iter().map(|x| (x.id.as_str(), x.fallback)).collect();
        assert_eq!(ids, vec![("N5-id", false), ("N3-en", true)]);
        let en = pick_explain_lang(vec![g("N5", "en"), g("N5", "id")], "en");
        assert_eq!(en.len(), 1);
        assert!(!en[0].fallback);
    }
}
