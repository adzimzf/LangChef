//! The learner's explanation language ("Bahasa Anda"): the language the app, the AI's
//! explanations and card meanings are in. Separate from the target language being learned.

use crate::db::open_db;
use rusqlite::{params, Connection, OptionalExtension};

pub const EXPLAIN_LANGS: &[&str] = &["en", "id"];
const DEFAULT_EXPLAIN_LANG: &str = "en";

/// SQL for the stored explanation language, for use inside other queries.
pub const EXPLAIN_LANG_SQL: &str =
    "COALESCE((SELECT value FROM app_settings WHERE key = 'explain_lang'), 'en')";

/// `meaning_lang` of a back that reads the same in every language (e.g. kana → romaji).
pub const ANY_LANG: &str = "*";

/// `{p}detail` when the card's meaning is in the current explanation language (or language-neutral),
/// else NULL: a meaning in another language counts as no meaning at all.
pub fn effective_detail_sql(prefix: &str) -> String {
    format!(
        "CASE WHEN COALESCE({p}meaning_lang, 'en') IN ({lang}, '*') THEN {p}detail END",
        p = prefix,
        lang = EXPLAIN_LANG_SQL
    )
}

pub fn explain_lang(conn: &Connection) -> String {
    conn.query_row("SELECT value FROM app_settings WHERE key = 'explain_lang'", [], |r| r.get::<_, String>(0))
        .optional()
        .ok()
        .flatten()
        .filter(|l| EXPLAIN_LANGS.contains(&l.as_str()))
        .unwrap_or_else(|| DEFAULT_EXPLAIN_LANG.to_string())
}

pub fn lang_name(code: &str) -> &str {
    match code {
        "ja" => "Japanese",
        "id" => "Indonesian",
        "en" => "English",
        other => other,
    }
}

/// Appended to every prompt whose output is read by the learner as an explanation.
pub fn explain_instruction(explain: &str) -> String {
    match explain {
        "id" => "LANGUAGE RULE: Write every explanation, meaning, note, summary and feedback for the learner \
                 in natural, simple Bahasa Indonesia (Indonesian). Do NOT use English. Translate directly into \
                 Indonesian (not via English). Keep words and example sentences of the language being studied as they are."
            .to_string(),
        _ => "LANGUAGE RULE: Write every explanation, meaning, note, summary and feedback for the learner in English. \
              Keep words and example sentences of the language being studied as they are."
            .to_string(),
    }
}

const ENGLISH_MARKERS: &[&str] = &[
    "the", "is", "are", "and", "of", "to", "you", "this", "that", "with", "it", "for", "was", "not", "your",
];
const INDONESIAN_MARKERS: &[&str] = &[
    "yang", "dan", "ini", "itu", "dengan", "untuk", "tidak", "adalah", "di", "ke", "dari", "anda", "kata",
    "kalimat", "bisa", "sudah", "artinya", "atau", "juga", "karena", "lebih", "sangat",
];

/// Cheap check that text meant to be Indonesian came back in English instead.
pub fn looks_english(text: &str) -> bool {
    let words: Vec<String> = text
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect();
    if words.len() < 4 {
        return false;
    }
    let en = words.iter().filter(|w| ENGLISH_MARKERS.contains(&w.as_str())).count();
    let id = words.iter().filter(|w| INDONESIAN_MARKERS.contains(&w.as_str())).count();
    en >= 2 && en > id * 2
}

/// Errors the frontend shows in the learner's language. The `detail` stays raw, behind "Detail".
pub fn app_error(code: &str, detail: impl Into<String>) -> String {
    serde_json::json!({ "code": code, "detail": detail.into() }).to_string()
}

#[tauri::command]
pub fn db_get_explain_lang(app: tauri::AppHandle) -> Result<String, String> {
    let conn = open_db(&app)?;
    Ok(explain_lang(&conn))
}

/// Switches the explanation language for the whole app. Card meanings in the old language then
/// count as empty (see `effective_detail_sql`); drafts and open questions built from them are dropped.
#[tauri::command]
pub fn db_set_explain_lang(app: tauri::AppHandle, lang: String) -> Result<(), String> {
    if !EXPLAIN_LANGS.contains(&lang.as_str()) {
        return Err(app_error("unsupported_language", lang));
    }
    let conn = open_db(&app)?;
    if explain_lang(&conn) == lang {
        // Make sure a first run still stores the value.
        conn.execute(
            "INSERT OR IGNORE INTO app_settings(key, value) VALUES ('explain_lang', ?1)",
            params![lang],
        )
        .map_err(|e| e.to_string())?;
        return Ok(());
    }
    conn.execute(
        "INSERT INTO app_settings(key, value) VALUES ('explain_lang', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![lang],
    )
    .map_err(|e| e.to_string())?;
    conn.execute("UPDATE review_items SET draft_detail = NULL", []).map_err(|e| e.to_string())?;
    conn.execute("UPDATE study_card SET task = NULL", []).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_english_in_indonesian_slot() {
        assert!(looks_english("This is the word you use when you want to say that it is good."));
        assert!(!looks_english("Ini adalah kata yang dipakai untuk menyatakan sesuatu yang baik."));
        assert!(!looks_english("makan"));
    }

    #[test]
    fn effective_detail_hides_other_language() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE app_settings(key TEXT PRIMARY KEY, value TEXT);
             CREATE TABLE review_items(id TEXT, detail TEXT, meaning_lang TEXT);
             INSERT INTO review_items VALUES ('a', 'eat', 'en'), ('b', 'makan', 'id'), ('c', 'ka', '*');",
        )
        .unwrap();
        let get = |id: &str| -> Option<String> {
            conn.query_row(
                &format!("SELECT {} FROM review_items WHERE id = ?1", effective_detail_sql("")),
                params![id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(get("a").as_deref(), Some("eat"));
        assert_eq!(get("b"), None);
        assert_eq!(get("c").as_deref(), Some("ka"));
        conn.execute("INSERT INTO app_settings VALUES ('explain_lang', 'id')", []).unwrap();
        assert_eq!(get("a"), None);
        assert_eq!(get("b").as_deref(), Some("makan"));
        assert_eq!(get("c").as_deref(), Some("ka"));
    }
}
