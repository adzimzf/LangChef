use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use rusqlite::{Connection, params};
use tauri::Manager;

mod google_auth;
mod google_drive;

#[derive(Serialize, Deserialize)]
struct Message {
    id: i64,
    role: String,
    content: String,
    created_at: i64,
}

#[derive(Serialize, Deserialize)]
struct ChatSession {
    id: String,
    title: Option<String>,
    language: Option<String>,
    level: Option<String>,
    created_at: i64,
    last_activity: i64,
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[derive(Debug, Deserialize)]
struct ModelObject {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelObject>,
}

fn build_url(base: &str, path: &str) -> String {
    let b = base.trim_end_matches('/');
    let p = path.trim_start_matches('/');
    format!("{}/{}", b, p)
}

#[tauri::command]
async fn list_models(api_host: String, api_key: String) -> Result<Vec<String>, String> {
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
async fn test_provider(api_host: String, api_key: String) -> Result<String, String> {
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

#[derive(Debug, Deserialize, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[tauri::command]
async fn llm_chat(
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

fn db_file(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("langchef.db"))
}

fn open_db(app: &tauri::AppHandle) -> Result<Connection, String> {
    let path = db_file(app)?;
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS messages(
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          role TEXT NOT NULL,
          content TEXT NOT NULL,
          created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS review_items(
          id TEXT PRIMARY KEY,
          content TEXT UNIQUE NOT NULL,
          EF REAL NOT NULL,
          I INTEGER NOT NULL,
          R INTEGER NOT NULL,
          state TEXT NOT NULL DEFAULT 'new',
          step INTEGER NOT NULL DEFAULT 0,
          lapse_count INTEGER NOT NULL DEFAULT 0,
          next_review INTEGER,
          tags TEXT,
          language TEXT,
          detail TEXT
        );
        CREATE TABLE IF NOT EXISTS known_words(
          word TEXT PRIMARY KEY
        );
        CREATE TABLE IF NOT EXISTS telemetry_days(
          date TEXT PRIMARY KEY,
          sessions INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS chat_sessions(
          id TEXT PRIMARY KEY,
          title TEXT,
          language TEXT,
          created_at INTEGER NOT NULL,
          last_activity INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS grammar_items(
          id TEXT PRIMARY KEY,
          pattern TEXT NOT NULL,
          detail TEXT NOT NULL DEFAULT '',
          language TEXT NOT NULL,
          level TEXT NOT NULL DEFAULT '',
          is_default INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS message_grammar_analysis(
            message_id INTEGER PRIMARY KEY,
            analysis TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS user_message_feedback(
            message_id INTEGER PRIMARY KEY,
            score INTEGER NOT NULL,
            feedback TEXT NOT NULL
        );
        "
    ).map_err(|e| e.to_string())?;
    // best-effort: add session_id column if missing
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN session_id TEXT", []);
    // best-effort: add language column if missing
    let _ = conn.execute("ALTER TABLE chat_sessions ADD COLUMN language TEXT", []);
    // best-effort: add selected_grammar column if missing (JSON array of grammar item IDs)
    let _ = conn.execute("ALTER TABLE chat_sessions ADD COLUMN selected_grammar TEXT", []);
    // best-effort: add level column if missing (JLPT/CEFR level for the session)
    let _ = conn.execute("ALTER TABLE chat_sessions ADD COLUMN level TEXT", []);
    // best-effort: add language and detail columns to review_items if missing
    let _ = conn.execute("ALTER TABLE review_items ADD COLUMN language TEXT", []);
    let _ = conn.execute("ALTER TABLE review_items ADD COLUMN detail TEXT", []);
    // best-effort: add new SM-2 columns to review_items if missing
    let _ = conn.execute("ALTER TABLE review_items ADD COLUMN state TEXT DEFAULT 'new'", []);
    let _ = conn.execute("ALTER TABLE review_items ADD COLUMN step INTEGER DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE review_items ADD COLUMN lapse_count INTEGER DEFAULT 0", []);
    // Migrate existing cards that have NULL state to 'review' if they have R > 0
    let _ = conn.execute("UPDATE review_items SET state = 'review' WHERE state IS NULL OR state = ''", []);
    let _ = conn.execute("UPDATE review_items SET state = 'new' WHERE state IS NULL OR state = ''", []);
    // Auto-seed bundled grammar data if grammar_items is empty
    seed_bundled_grammar_if_empty(&conn);
    Ok(conn)
}

// ─── Bundled Grammar Markdown Data ──────────────────────────────────────────
// These .md files are embedded at compile time from the grammar/ directory.
// Format: each file is split by "---" separators. Each section starts with
// "## pattern" on the first line, followed by the Markdown detail.

static BUNDLED_GRAMMAR: &[(&str, &str, &str)] = &[
    ("ja", "N5", include_str!("../../grammar/ja/N5.md")),
    ("ja", "N4", include_str!("../../grammar/ja/N4.md")),
    ("ja", "N3", include_str!("../../grammar/ja/N3.md")),
    ("ja", "N2", include_str!("../../grammar/ja/N2.md")),
    ("ja", "N1", include_str!("../../grammar/ja/N1.md")),
    ("id", "A1", include_str!("../../grammar/id/A1.md")),
    ("id", "A2", include_str!("../../grammar/id/A2.md")),
];

/// Parse a Markdown grammar file.
///
/// Each section is separated by a line containing only "---".
/// The first line of each section must be "## pattern" where pattern is the grammar name.
/// Everything after that line (until the next "---") is the detail (raw Markdown).
fn parse_grammar_md(content: &str) -> Vec<(String, String)> {
    let mut items = Vec::new();
    let sections = content.split("\n---\n");

    for section in sections {
        let section = section.trim();
        if section.is_empty() {
            continue;
        }

        // Find the first line starting with "## "
        let mut pattern = String::new();
        let mut detail_start = 0;
        for (i, line) in section.lines().enumerate() {
            if let Some(rest) = line.strip_prefix("## ") {
                pattern = rest.trim().to_string();
                detail_start = i + 1;
                break;
            }
        }

        if pattern.is_empty() {
            continue;
        }

        // Everything after the "##" line is the detail
        let detail: String = section
            .lines()
            .skip(detail_start)
            .collect::<Vec<&str>>()
            .join("\n")
            .trim()
            .to_string();

        items.push((pattern, detail));
    }
    items
}

/// Seed the grammar_items table from bundled CSV data if it's currently empty.
/// Checks per-language so that adding a new language's CSV will seed correctly
/// even if other languages already have data.
fn seed_bundled_grammar_if_empty(conn: &Connection) {
    for &(lang, level, csv) in BUNDLED_GRAMMAR {
        // Check if this specific language+level already has data
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM grammar_items WHERE language=?1 AND level=?2",
            params![lang, level],
            |row| row.get(0),
        ).unwrap_or(0);

        if count > 0 {
            continue; // This level already has data
        }

        let items = parse_grammar_md(csv);
        eprintln!("[seed] grammar/{}/{level}: parsed {} items", lang, items.len());
        for (pattern, detail) in items {
            let id = uuid::Uuid::new_v4().to_string();
            match conn.execute(
                "INSERT INTO grammar_items(id, pattern, detail, language, level, is_default) VALUES (?1, ?2, ?3, ?4, ?5, 1)",
                params![id, pattern, detail, lang, level],
            ) {
                Ok(_) => {}
                Err(e) => eprintln!("[seed] FAILED to insert '{}': {}", pattern, e),
            }
        }
    }
}

#[tauri::command]
fn db_add_message(app: tauri::AppHandle, role: String, content: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "INSERT INTO messages(role, content, created_at) VALUES (?1, ?2, ?3)",
        params![role, content, now],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn db_new_session(app: tauri::AppHandle, title: Option<String>, language: Option<String>, level: Option<String>) -> Result<String, String> {
    let conn = open_db(&app)?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "INSERT INTO chat_sessions(id, title, language, level, created_at, last_activity) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, title, language, level, now, now],
    ).map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command]
fn db_list_sessions(app: tauri::AppHandle) -> Result<Vec<ChatSession>, String> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare("SELECT id, title, language, level, created_at, last_activity FROM chat_sessions ORDER BY last_activity DESC")
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok(ChatSession {
            id: row.get(0)?,
            title: row.get(1)?,
            language: row.get(2)?,
            level: row.get(3)?,
            created_at: row.get(4)?,
            last_activity: row.get(5)?,
        })
    }).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
fn db_add_message_in_session(app: tauri::AppHandle, session_id: String, role: String, content: String) -> Result<i64, String> {
    let conn = open_db(&app)?;
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "INSERT INTO messages(role, content, created_at, session_id) VALUES (?1, ?2, ?3, ?4)",
        params![role, content, now, session_id],
    ).map_err(|e| e.to_string())?;
    conn.execute("UPDATE chat_sessions SET last_activity=?1 WHERE id=?2", params![now, session_id])
        .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

#[tauri::command]
fn db_list_session_messages(app: tauri::AppHandle, session_id: String) -> Result<Vec<Message>, String> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare("SELECT id, role, content, created_at FROM messages WHERE session_id=?1 ORDER BY created_at ASC")
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![session_id], |row| {
        Ok(Message {
            id: row.get(0)?,
            role: row.get(1)?,
            content: row.get(2)?,
            created_at: row.get(3)?,
        })
    }).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[derive(Serialize, Deserialize)]
struct ReviewItem {
    id: String,
    content: String,
    EF: f64,
    I: i64,
    R: i64,
    state: String,          // "new", "learning", "review", "relearning"
    step: i64,              // current step index in learning/relearning steps
    lapse_count: i64,       // number of times this card has lapsed
    next_review: Option<i64>,
    tags: Option<String>,
    language: Option<String>,
    detail: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct TelemetryDay {
    date: String,
    sessions: i64,
}

// ─── SRS Configuration ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone)]
struct SRSConfig {
    learning_steps: Vec<i64>,       // minutes, e.g. [1, 10]
    relearning_steps: Vec<i64>,     // minutes, e.g. [10]
    graduating_interval: i64,       // days
    easy_interval: i64,             // days
    new_interval: f64,              // fraction of previous interval on lapse (0–1)
    minimum_lapse_interval: i64,    // minimum days after a lapse
    easy_bonus: f64,                // multiplier for Easy on Review cards
    interval_modifier: f64,         // global interval modifier
    maximum_interval: i64,          // cap in days
    grace_period_multiplier: f64,   // bonus for late reviews
}

impl Default for SRSConfig {
    fn default() -> Self {
        SRSConfig {
            learning_steps: vec![1, 10],
            relearning_steps: vec![10],
            graduating_interval: 1,
            easy_interval: 4,
            new_interval: 0.0,
            minimum_lapse_interval: 1,
            easy_bonus: 1.3,
            interval_modifier: 1.0,
            maximum_interval: 36500,
            grace_period_multiplier: 0.5,
        }
    }
}

// ─── SM-2 Scheduling Result ───────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct SRSResult {
    EF: f64,
    I: i64,
    R: i64,
    state: String,
    step: i64,
    lapse_count: i64,
    next_review: i64,       // unix timestamp (seconds)
    next_minutes: i64,      // for display
    lapsed: bool,
}

/// Compute new EF from current EF and quality (0–5).
/// Formula: EF' = EF + (0.1 - (5-q)*(0.08 + (5-q)*0.02)), min 1.3
fn compute_ef(ef: f64, q: i64) -> f64 {
    let qf = q as f64;
    f64::max(1.3, ef + (0.1 - (5.0 - qf) * (0.08 + (5.0 - qf) * 0.02)))
}

/// Grace bonus: extra days for late correct reviews.
fn grace_bonus(next_review: Option<i64>, now: i64, multiplier: f64) -> f64 {
    match next_review {
        Some(nr) => {
            let now_ms = now * 1000;
            let nr_ms = nr * 1000;
            let days_late = f64::max(0.0, (now_ms - nr_ms) as f64 / (24.0 * 60.0 * 60.0 * 1000.0));
            days_late * multiplier
        }
        None => 0.0,
    }
}

/// Compute a difficulty tag from EF and R values.
/// Mirrors the frontend easeScore formula:
///   ease = 0.6 * efNorm + 0.4 * rNorm
/// Returns: "hard" (ease < 0.3), "medium" (0.3 <= ease < 0.7), "easy" (ease >= 0.7)
fn compute_tag_from_ef_r(ef: f64, r: i64) -> &'static str {
    let ef_norm = ((ef - 1.3) / (3.0 - 1.3)).clamp(0.0, 1.0);
    let r_norm = (r as f64 / 5.0).clamp(0.0, 1.0);
    let ease = 0.6 * ef_norm + 0.4 * r_norm;
    if ease < 0.3 {
        "hard"
    } else if ease < 0.7 {
        "medium"
    } else {
        "easy"
    }
}

/// Full SM-2 scheduling with Anki-style card lifecycle.
/// Grade: "again" (q=1), "hard" (q=3), "good" (q=4), "easy" (q=5)
fn schedule_card(
    ef: f64, i: i64, r: i64, state: &str, step: i64, lapse_count: i64,
    next_review: Option<i64>, grade: &str, config: &SRSConfig,
) -> SRSResult {
    let q = match grade {
        "again" => 1,
        "hard" => 3,
        "good" => 4,
        "easy" => 5,
        _ => 4,
    };
    let new_ef = compute_ef(ef, q);
    let now = chrono::Utc::now().timestamp();
    let mut lapsed = false;

    let (new_i, new_r, new_state, new_step, new_lapse_count, next, next_minutes) = match state {
        "new" | "learning" => {
            let steps = &config.learning_steps;
            match grade {
                "again" => {
                    let ns = 0;
                    let nm = steps.get(ns as usize).copied().unwrap_or(1);
                    let nr = now + nm * 60;
                    (1, 0, "learning".to_string(), ns, lapse_count, nr, nm)
                }
                "hard" => {
                    let ns = i64::max(0, step); // stay at current step
                    let nm = steps.get(ns as usize).copied().unwrap_or(1);
                    let nr = now + nm * 60;
                    (1, 0, "learning".to_string(), ns, lapse_count, nr, nm)
                }
                "good" => {
                    let next_step = step + 1;
                    if (next_step as usize) >= steps.len() {
                        // Graduate
                        let gi = config.graduating_interval;
                        let nm = gi * 24 * 60;
                        let nr = now + gi * 24 * 60 * 60;
                        (gi, 1, "review".to_string(), 0, lapse_count, nr, nm)
                    } else {
                        let nm = steps.get(next_step as usize).copied().unwrap_or(1);
                        let nr = now + nm * 60;
                        (1, 0, "learning".to_string(), next_step, lapse_count, nr, nm)
                    }
                }
                _ => {
                    // Easy: immediate graduation with easyInterval
                    let ei = config.easy_interval;
                    let nm = ei * 24 * 60;
                    let nr = now + ei * 24 * 60 * 60;
                    (ei, 1, "review".to_string(), 0, lapse_count, nr, nm)
                }
            }
        }
        "review" => {
            let bonus = grace_bonus(next_review, now, config.grace_period_multiplier);
            if grade == "again" {
                // Lapse
                lapsed = true;
                let lapse_i = i64::max(
                    config.minimum_lapse_interval,
                    (i as f64 * config.new_interval).round() as i64,
                );
                if !config.relearning_steps.is_empty() {
                    let nm = config.relearning_steps[0];
                    let nr = now + nm * 60;
                    (lapse_i, 0, "relearning".to_string(), 0, lapse_count + 1, nr, nm)
                } else {
                    let nm = lapse_i * 24 * 60;
                    let nr = now + lapse_i * 24 * 60 * 60;
                    (lapse_i, 0, "review".to_string(), 0, lapse_count + 1, nr, nm)
                }
            } else {
                let new_i_val = match grade {
                    "hard" => i64::max(i + 1, (i as f64 * 1.2 * config.interval_modifier).round() as i64),
                    "good" => {
                        let raw = ((i as f64 * new_ef) + bonus) * config.interval_modifier;
                        i64::max(i + 1, raw.round() as i64)
                    }
                    _ => {
                        // Easy
                        let raw = ((i as f64 * new_ef * config.easy_bonus) + bonus) * config.interval_modifier;
                        i64::max(i + 1, raw.round() as i64)
                    }
                };
                let capped = i64::min(config.maximum_interval, new_i_val);
                let nm = capped * 24 * 60;
                let nr = now + capped * 24 * 60 * 60;
                (capped, r + 1, "review".to_string(), 0, lapse_count, nr, nm)
            }
        }
        "relearning" => {
            let steps = &config.relearning_steps;
            match grade {
                "again" => {
                    let nm = steps.get(0).copied().unwrap_or(10);
                    let nr = now + nm * 60;
                    (i, 0, "relearning".to_string(), 0, lapse_count, nr, nm)
                }
                "hard" => {
                    let ns = i64::max(0, step);
                    let nm = steps.get(ns as usize).copied().unwrap_or(10);
                    let nr = now + nm * 60;
                    (i, 0, "relearning".to_string(), ns, lapse_count, nr, nm)
                }
                "good" => {
                    let next_step = step + 1;
                    if (next_step as usize) >= steps.len() {
                        // Re-graduate
                        let nm = i * 24 * 60;
                        let nr = now + i * 24 * 60 * 60;
                        (i, 1, "review".to_string(), 0, lapse_count, nr, nm)
                    } else {
                        let nm = steps.get(next_step as usize).copied().unwrap_or(10);
                        let nr = now + nm * 60;
                        (i, 0, "relearning".to_string(), next_step, lapse_count, nr, nm)
                    }
                }
                _ => {
                    // Easy: immediate re-graduation with boosted interval
                    let boosted = i64::max(1, (i as f64 * 1.3).round() as i64);
                    let nm = boosted * 24 * 60;
                    let nr = now + boosted * 24 * 60 * 60;
                    (boosted, 1, "review".to_string(), 0, lapse_count, nr, nm)
                }
            }
        }
        _ => {
            // Fallback: treat as new
            let nm = config.learning_steps.get(0).copied().unwrap_or(1);
            let nr = now + nm * 60;
            (1, 0, "learning".to_string(), 0, lapse_count, nr, nm)
        }
    };

    SRSResult {
        EF: new_ef,
        I: new_i,
        R: new_r,
        state: new_state,
        step: new_step,
        lapse_count: new_lapse_count,
        next_review: next,
        next_minutes,
        lapsed,
    }
}

#[tauri::command]
fn db_upsert_review_item(app: tauri::AppHandle, content: String, tag: Option<String>, language: Option<String>) -> Result<(), String> {
    let conn = open_db(&app)?;
    let id = uuid::Uuid::new_v4().to_string();
    let tags = tag.unwrap_or_else(|| compute_tag_from_ef_r(2.5, 0).to_string());
    conn.execute(
        "INSERT INTO review_items(id, content, EF, I, R, state, step, lapse_count, next_review, tags, language)
         VALUES (?1, ?2, 2.5, 1, 0, 'new', 0, 0, NULL, ?3, ?4)
         ON CONFLICT(content) DO UPDATE SET tags=excluded.tags, language=COALESCE(excluded.language, review_items.language)",
        params![id, content, tags, language],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn db_get_review_items(app: tauri::AppHandle) -> Result<Vec<ReviewItem>, String> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare(
        "SELECT
            id, content, EF, I, R, state, step, lapse_count, next_review, tags, language, detail
        FROM review_items").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok(ReviewItem {
            id: row.get(0)?,
            content: row.get(1)?,
            EF: row.get(2)?,
            I: row.get(3)?,
            R: row.get(4)?,
            state: row.get(5)?,
            step: row.get(6)?,
            lapse_count: row.get(7)?,
            next_review: row.get(8)?,
            tags: row.get(9)?,
            language: row.get(10)?,
            detail: row.get(11)?,
        })
    }).map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    for r in rows {
        items.push(r.map_err(|e| e.to_string())?);
    }
    Ok(items)
}

#[tauri::command]
fn db_update_srs(app: tauri::AppHandle, id: String, grade: String) -> Result<SRSResult, String> {
    let conn = open_db(&app)?;
    let (ef, i, r, state, step, lapse_count, next_review): (f64, i64, i64, String, i64, i64, Option<i64>) = conn.query_row(
        "SELECT EF, I, R, state, step, lapse_count, next_review FROM review_items WHERE id=?1",
        params![id.clone()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
    ).map_err(|e| e.to_string())?;

    let config = SRSConfig::default();
    let result = schedule_card(ef, i, r, &state, step, lapse_count, next_review, &grade, &config);

    // Compute dynamic tag from newly scheduled EF/R values
    let new_tag = compute_tag_from_ef_r(result.EF, result.R);

    conn.execute(
        "UPDATE review_items SET EF=?1, I=?2, R=?3, state=?4, step=?5, lapse_count=?6, next_review=?7, tags=?8 WHERE id=?9",
        params![result.EF, result.I, result.R, result.state, result.step, result.lapse_count, result.next_review, new_tag, id],
    ).map_err(|e| e.to_string())?;

    Ok(result)
}

#[tauri::command]
fn db_increment_session_today(app: tauri::AppHandle) -> Result<(), String> {
    let conn = open_db(&app)?;
    let today = chrono::Local::now().date_naive().to_string();
    conn.execute(
        "INSERT INTO telemetry_days(date, sessions) VALUES (?1, 1)
         ON CONFLICT(date) DO UPDATE SET sessions=sessions+1",
        params![today],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn db_get_telemetry_days(app: tauri::AppHandle) -> Result<Vec<TelemetryDay>, String> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare("SELECT date, sessions FROM telemetry_days ORDER BY date ASC")
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok(TelemetryDay {
            date: row.get(0)?,
            sessions: row.get(1)?,
        })
    }).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
fn db_list_known_words(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare("SELECT word FROM known_words").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| Ok(row.get::<_, String>(0)?)).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
fn db_add_known_word(app: tauri::AppHandle, word: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute("INSERT OR IGNORE INTO known_words(word) VALUES (?1)", params![word.to_lowercase()])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn db_rename_session(app: tauri::AppHandle, session_id: String, title: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute("UPDATE chat_sessions SET title=?1 WHERE id=?2", params![title, session_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn db_delete_session(app: tauri::AppHandle, session_id: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute("DELETE FROM messages WHERE session_id=?1", params![session_id])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM chat_sessions WHERE id=?1", params![session_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ImportItem {
    content: String,
    tags: Option<String>,
    language: Option<String>,
    detail: Option<String>,
}

#[tauri::command]
fn db_update_review_detail(app: tauri::AppHandle, id: String, detail: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute("UPDATE review_items SET detail=?1 WHERE id=?2", params![detail, id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn db_get_review_items_by_language(app: tauri::AppHandle, language: String) -> Result<Vec<ReviewItem>, String> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare("SELECT id, content, EF, I, R, state, step, lapse_count, next_review, tags, language, detail FROM review_items WHERE language=?1").map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![language], |row| {
        Ok(ReviewItem {
            id: row.get(0)?,
            content: row.get(1)?,
            EF: row.get(2)?,
            I: row.get(3)?,
            R: row.get(4)?,
            state: row.get(5)?,
            step: row.get(6)?,
            lapse_count: row.get(7)?,
            next_review: row.get(8)?,
            tags: row.get(9)?,
            language: row.get(10)?,
            detail: row.get(11)?,
        })
    }).map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    for r in rows {
        items.push(r.map_err(|e| e.to_string())?);
    }
    Ok(items)
}

#[tauri::command]
fn db_get_due_review_items(
    app: tauri::AppHandle,
    language: Option<String>,
    limit: Option<i64>,
    new_cards_remaining: Option<i64>,
    max_next_review: Option<i64>,
) -> Result<Vec<ReviewItem>, String> {
    let conn = open_db(&app)?;
    // Use provided cutoff, or default to end of today (23:59:59 local)
    let end_of_day = match max_next_review {
        Some(ts) => ts,
        None => {
            let now_local = chrono::Local::now();
            now_local
                .date_naive()
                .and_hms_opt(23, 59, 59)
                .unwrap()
                .and_local_timezone(chrono::Local)
                .unwrap()
                .with_timezone(&chrono::Utc)
                .timestamp()
        }
    };
    let limit_val = limit.unwrap_or(200);

    let mut sql = String::from(
        "SELECT id, content, EF, I, R, state, step, lapse_count, next_review, tags, language, detail
         FROM review_items
         WHERE (next_review IS NULL OR next_review <= ?1)"
    );

    if language.is_some() {
        sql.push_str(" AND language = ?2");
    }

    // Order: non-new cards first (review/learning/relearning), then new cards
    sql.push_str(" ORDER BY CASE WHEN state='new' OR state='' OR state IS NULL THEN 1 ELSE 0 END, next_review ASC NULLS FIRST LIMIT ?");

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut items = Vec::new();

    fn map_row(row: &rusqlite::Row) -> rusqlite::Result<ReviewItem> {
        Ok(ReviewItem {
            id: row.get(0)?,
            content: row.get(1)?,
            EF: row.get(2)?,
            I: row.get(3)?,
            R: row.get(4)?,
            state: row.get(5)?,
            step: row.get(6)?,
            lapse_count: row.get(7)?,
            next_review: row.get(8)?,
            tags: row.get(9)?,
            language: row.get(10)?,
            detail: row.get(11)?,
        })
    }

    let rows = if let Some(ref lang) = language {
        stmt.query_map(params![end_of_day, lang, limit_val], map_row).map_err(|e| e.to_string())?
    } else {
        stmt.query_map(params![end_of_day, limit_val], map_row).map_err(|e| e.to_string())?
    };

    for r in rows {
        items.push(r.map_err(|e| e.to_string())?);
    }

    // Apply new card cap on the Rust side
    if let Some(new_cap) = new_cards_remaining {
        if new_cap <= 0 {
            items.retain(|item| item.state != "new" && item.state != "");
        } else {
            let mut new_count = 0i64;
            items.retain(|item| {
                if item.state == "new" || item.state.is_empty() {
                    if new_count >= new_cap {
                        return false;
                    }
                    new_count += 1;
                }
                true
            });
        }
    }

    Ok(items)
}

#[tauri::command]
fn db_delete_review_item(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute("DELETE FROM review_items WHERE id=?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct UpdateReviewItem {
    id: String,
    content: Option<String>,
    tags: Option<String>,
    language: Option<String>,
}

#[tauri::command]
fn db_update_review_item(app: tauri::AppHandle, item: UpdateReviewItem) -> Result<(), String> {
    let conn = open_db(&app)?;
    if let Some(content) = item.content {
        // Check for UNIQUE constraint conflict: another row already has this content
        let existing_id: Option<String> = conn.query_row(
            "SELECT id FROM review_items WHERE content=?1 AND id!=?2",
            params![content, item.id],
            |row| row.get(0),
        ).ok();
        if existing_id.is_some() {
            return Err(format!("A word with content \"{}\" already exists.", content));
        }
        conn.execute("UPDATE review_items SET content=?1 WHERE id=?2", params![content, item.id])
            .map_err(|e| e.to_string())?;
    }
    if let Some(tags) = item.tags {
        conn.execute("UPDATE review_items SET tags=?1 WHERE id=?2", params![tags, item.id])
            .map_err(|e| e.to_string())?;
    }
    if let Some(language) = item.language {
        conn.execute("UPDATE review_items SET language=?1 WHERE id=?2", params![language, item.id])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn db_import_review_items(app: tauri::AppHandle, items: Vec<ImportItem>) -> Result<String, String> {
    let conn = open_db(&app)?;
    let mut imported = 0u32;
    for item in items {
        let id = uuid::Uuid::new_v4().to_string();
        let tags = item.tags.unwrap_or_default();
        let result = conn.execute(
            "INSERT INTO review_items(id, content, EF, I, R, state, step, lapse_count, next_review, tags, language, detail)
             VALUES (?1, ?2, 2.5, 1, 0, 'new', 0, 0, NULL, ?3, ?4, ?5)
             ON CONFLICT(content) DO NOTHING",
            params![id, item.content, tags, item.language, item.detail],
        );
        if let Ok(n) = result {
            if n > 0 { imported += 1; }
        }
    }
    Ok(format!("Imported {} items", imported))
}

#[tauri::command]
async fn db_generate_detail(app: tauri::AppHandle, api_host: String, api_key: String, model: String, content: String, language: String) -> Result<String, String> {
    let lang_name = match language.as_str() {
        "ja" => "Japanese",
        "id" => "Indonesian",
        _ => &language,
    };
    let prompt = format!(
        "You are a language learning assistant. For the {} word/phrase \"{}\", provide a detailed vocabulary card in this exact JSON format:\n\
        {{\"reading\": \"pronunciation in IPA or romaji\", \"meaning\": \"English translation\", \"part_of_speech\": \"noun/verb/adj/etc\", \"example\": \"a natural example sentence in {}\", \"example_meaning\": \"English translation of the example\"}}\n\
        Return ONLY the JSON object, nothing else.",
        lang_name, content, lang_name
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
    let text = parsed.choices.get(0).map(|c| c.message.content.clone()).unwrap_or_default();

    // Save the generated detail to DB
    let conn = open_db(&app)?;
    conn.execute("UPDATE review_items SET detail=?1 WHERE content=?2", params![text, content])
        .map_err(|e| e.to_string())?;

    Ok(text)
}

// ─── Grammar Items ──────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct GrammarItem {
    id: String,
    pattern: String,
    detail: String,
    language: String,
    level: String,
    is_default: i64,
}

#[tauri::command]
fn db_list_grammar_items(app: tauri::AppHandle, language: Option<String>, level: Option<String>) -> Result<Vec<GrammarItem>, String> {
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
fn db_add_grammar_item(app: tauri::AppHandle, pattern: String, detail: String, language: String, level: String) -> Result<GrammarItem, String> {
    let conn = open_db(&app)?;
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO grammar_items(id, pattern, detail, language, level, is_default) VALUES (?1, ?2, ?3, ?4, ?5, 0)",
        params![id, pattern, detail, language, level],
    ).map_err(|e| e.to_string())?;
    Ok(GrammarItem { id, pattern, detail, language, level, is_default: 0 })
}

#[tauri::command]
fn db_update_grammar_item(app: tauri::AppHandle, id: String, pattern: Option<String>, detail: Option<String>, level: Option<String>) -> Result<(), String> {
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
fn db_delete_grammar_item(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute("DELETE FROM grammar_items WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct SeedGrammarItem {
    pattern: String,
    detail: String,
    language: String,
    level: String,
}

#[tauri::command]
fn db_seed_grammar_items(app: tauri::AppHandle, items: Vec<SeedGrammarItem>) -> Result<String, String> {
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
fn db_get_grammar_levels(app: tauri::AppHandle, language: String) -> Result<Vec<String>, String> {
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
fn db_import_grammar_csv(app: tauri::AppHandle, items: Vec<SeedGrammarItem>) -> Result<String, String> {
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

// ─── Selected Grammar (persisted user selection) ───────────────────────────

#[tauri::command]
fn db_save_selected_grammar(app: tauri::AppHandle, session_id: String, grammar_item_ids: Vec<String>) -> Result<(), String> {
    let conn = open_db(&app)?;
    let json = serde_json::to_string(&grammar_item_ids).map_err(|e| e.to_string())?;
    conn.execute("UPDATE chat_sessions SET selected_grammar=?1 WHERE id=?2", params![json, session_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn db_get_selected_grammar(app: tauri::AppHandle, session_id: String) -> Result<Vec<String>, String> {
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

// ─── Grammar Analysis ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct GrammarAnalysis {
    message_id: i64,
    analysis: String,
}

#[tauri::command]
fn db_get_grammar_analysis(app: tauri::AppHandle, message_id: i64) -> Result<Option<GrammarAnalysis>, String> {
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
async fn db_analyze_grammar(
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

    // Save to DB
    let conn = open_db(&app)?;
    conn.execute(
        "INSERT OR REPLACE INTO message_grammar_analysis(message_id, analysis) VALUES (?1, ?2)",
        params![message_id, analysis],
    ).map_err(|e| e.to_string())?;

    Ok(analysis)
}

// ─── User Message Feedback ───────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct UserMessageFeedback {
    message_id: i64,
    score: i64,
    feedback: String,
    created_at: i64,
}

#[tauri::command]
fn db_get_user_feedback(app: tauri::AppHandle, message_id: i64) -> Result<Option<UserMessageFeedback>, String> {
    let conn = open_db(&app)?;
    let result = conn.query_row(
        "SELECT f.message_id, f.score, f.feedback, COALESCE(m.created_at, 0)
         FROM user_message_feedback f
         LEFT JOIN messages m ON m.id = f.message_id
         WHERE f.message_id=?1",
        params![message_id],
        |row| Ok(UserMessageFeedback {
            message_id: row.get(0)?,
            score: row.get(1)?,
            feedback: row.get(2)?,
            created_at: row.get(3)?,
        }),
    );
    match result {
        Ok(fb) => Ok(Some(fb)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn db_list_all_feedback(app: tauri::AppHandle) -> Result<Vec<UserMessageFeedback>, String> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare(
        "SELECT f.message_id, f.score, f.feedback, m.created_at
         FROM user_message_feedback f
         INNER JOIN messages m ON m.id = f.message_id
         ORDER BY m.created_at ASC"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok(UserMessageFeedback {
            message_id: row.get(0)?,
            score: row.get(1)?,
            feedback: row.get(2)?,
            created_at: row.get(3)?,
        })
    }).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
async fn db_evaluate_user_message(
    app: tauri::AppHandle,
    api_host: String,
    api_key: String,
    model: String,
    message_id: i64,
    message_content: String,
    language: String,
    level: String,
    context_messages: Vec<ChatMessage>,
) -> Result<UserMessageFeedback, String> {
    // Check cache first
    let conn = open_db(&app)?;
    if let Ok(cached) = conn.query_row(
        "SELECT f.message_id, f.score, f.feedback, COALESCE(m.created_at, 0)
         FROM user_message_feedback f
         LEFT JOIN messages m ON m.id = f.message_id
         WHERE f.message_id=?1",
        params![message_id],
        |row| Ok(UserMessageFeedback {
            message_id: row.get(0)?,
            score: row.get(1)?,
            feedback: row.get(2)?,
            created_at: row.get(3)?,
        }),
    ) {
        return Ok(cached);
    }

    let lang_name = match language.as_str() {
        "ja" => "Japanese",
        "id" => "Indonesian",
        _ => &language,
    };
    let level_context = if level.is_empty() {
        String::new()
    } else {
        format!("\nJLPT/Level context: {}", level)
    };

    let mut context_block = String::new();
    if !context_messages.is_empty() {
        context_block = "\n\n## Conversation Context\nHere is the recent conversation for reference:\n".to_string();
        for cm in &context_messages {
            let role_label = if cm.role == "user" { "Learner" } else { "Partner" };
            context_block.push_str(&format!("- **{}**: {}\n", role_label, cm.content));
        }
        context_block.push_str("\nEvaluate the learner's latest message above in the context of this conversation.\n");
    }

    let prompt = format!(
        "You are a language learning evaluator. Evaluate the following {} learner's message.{}\n\
         {}\n\
         Rate the message on a scale of 1-5:\n\
         - 1 = Many errors, hard to understand\n\
         - 2 = Frequent errors but meaning is clear\n\
         - 3 = Some errors, generally understandable\n\
         - 4 = Minor issues, mostly correct\n\
         - 5 = Natural, no errors\n\n\
         Provide your evaluation in Markdown with these sections:\n\
         ## Score: X/5\n\
         ## Vocabulary\n\
         - List words that are appropriate/good ✓\n\
         - List words that should be replaced or removed ✗ (with better alternatives)\n\
         ## Grammar\n\
         - List any grammar mistakes with corrections\n\
         - Note good grammar usage\n\n\
         Be concise and encouraging. If the message is perfect, say so!\n\n\
         Message to evaluate:\n\"{}\"\n\n\n
         BELOW is the EXPECTED RESPONSE:

         **Overall Score**
         [ `★☆☆☆☆` ] **1 / 5**

         > ⚠️ Response is incomplete and does not appropriately address the partner's comment about health.

         ---

         ### 🔤 Vocabulary Analysis

         | Status | Word | Reading / Meaning | Diagnosis & Suggestion |
         |:---:|:---|:---|:---|
         | ❌ | **何** | *nani* / \"what\" | Fragment used incorrectly. A single question word does not constitute a proper response to a statement of concern. |

         ---

         ### 📝 Grammar Correction

         **Issue:** Incomplete Sentence / Fragment
         The message `何` is not a grammatically complete response to the previous statement.

         **Recommended Alternatives:**

         > ✅ **はい、気をつけます。**
         > *(Hai, ki o tsukemasu.)* — \"Yes, I will be careful.\"

         > ✅ **わかりました。**
         > *(Wakarimashita.)* — \"I understand.\"

         > ✅ **ありがとうございます。**
         > *(Arigatou gozaimasu.)* — \"Thank you.\"

         ---

         ### 🛠️ Styling Guide for Markdown
         To achieve clean visual hierarchy, apply the following rules when rendering this feedback:

         - **Headings:** Use `#` and `##` with emoji prefixes for immediate context recognition.
         - **Tables:** Use tables for error logging (Status, Word, Meaning, Fix) for quick scanning.
         - **Callouts:** Use blockquotes (`>`) for the score banner and warnings.
         - **Positive Examples:** Use green check emoji (`✅`) in a distinct block to clearly separate the **Error** from the **Fix**.",
        lang_name, level_context, context_block, message_content
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
    let feedback = parsed.choices.get(0).map(|c| c.message.content.clone()).unwrap_or_default();

    // Extract score from the response (look for "Score: X/5" pattern)
    let score = extract_score_from_feedback(&feedback);

    // Save to DB
    conn.execute(
        "INSERT OR REPLACE INTO user_message_feedback(message_id, score, feedback) VALUES (?1, ?2, ?3)",
        params![message_id, score, feedback],
    ).map_err(|e| e.to_string())?;

    let msg_created_at: i64 = conn.query_row(
        "SELECT COALESCE(created_at, 0) FROM messages WHERE id=?1",
        params![message_id],
        |row| row.get(0),
    ).unwrap_or(0);

    Ok(UserMessageFeedback { message_id, score, feedback, created_at: msg_created_at })
}

/// Extract a score (1-5) from the LLM's markdown feedback by looking for patterns like "Score: X/5"
fn extract_score_from_feedback(feedback: &str) -> i64 {
    // Try common patterns
    for pattern in &["Score: ", "score: ", "**Score:** ", "# Score: "] {
        if let Some(idx) = feedback.find(pattern) {
            let after = &feedback[idx + pattern.len()..];
            // Take up to 3 chars and parse as number before "/"
            let num_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = num_str.parse::<i64>() {
                if (1..=5).contains(&n) {
                    return n;
                }
            }
        }
    }
    3 // Default fallback
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            greet,
            test_provider,
            list_models,
            llm_chat,
            db_add_message,
            db_new_session,
            db_list_sessions,
            db_add_message_in_session,
            db_list_session_messages,
            db_upsert_review_item,
            db_get_review_items,
            db_update_srs,
            db_increment_session_today,
            db_get_telemetry_days,
            db_list_known_words,
            db_add_known_word,
            db_rename_session,
            db_delete_session,
            db_update_review_detail,
            db_get_review_items_by_language,
            db_get_due_review_items,
            db_import_review_items,
            db_generate_detail,
            db_delete_review_item,
            db_update_review_item,
            db_list_grammar_items,
            db_add_grammar_item,
            db_update_grammar_item,
            db_delete_grammar_item,
            db_seed_grammar_items,
            db_get_grammar_levels,
            db_import_grammar_csv,
            db_get_grammar_analysis,
            db_analyze_grammar,
            db_save_selected_grammar,
            db_get_selected_grammar,
            db_get_user_feedback,
            db_list_all_feedback,
            db_evaluate_user_message,
            // Google Auth
            google_auth::google_sign_in,
            google_auth::google_sign_out,
            google_auth::google_get_auth_status,
            // Google Drive Backup & Sync
            google_drive::google_backup_db,
            google_drive::google_restore_db,
            google_drive::google_get_backup_status,
            google_drive::google_set_auto_sync,
            google_drive::google_get_auto_sync,
            google_drive::google_trigger_sync,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
