use crate::chat::{complete_explained, complete_prompt, json_part};
use crate::locale::{app_error, explain_lang, lang_name};
use crate::db::open_db;
use crate::furigana;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Serialize, Deserialize, Clone)]
pub struct ReadingText {
    pub title: String,
    pub body: String,
    pub plain_body: String,
    pub source: String,
    pub language: String,
    pub level: String,
    /// Made together with an AI-written passage; empty for a news article (ask `generate_reading_questions`).
    pub questions: Vec<ReadingQuestion>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ReadingQuestion {
    pub question: String,
    pub answer_hint: String,
}

#[derive(Serialize, Deserialize)]
pub struct AnswerEvaluation {
    pub result: String,
    pub feedback: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SavedAnswer {
    pub question_index: usize,
    pub user_answer: String,
    pub result: String,
    pub retries: u32,
    /// Feedback on the final answer, shown again on the results screen.
    #[serde(default)]
    pub feedback: String,
}

/// How the questions of one article went: correct (on any try), correct on the first try,
/// moved on with a partial answer, and wrong tries in total.
#[derive(Serialize, Deserialize, Default, Clone, Copy, PartialEq, Debug)]
pub struct AnswerStats {
    pub answered: i64,
    pub correct: i64,
    pub first_try: i64,
    pub partial: i64,
    pub retries: i64,
}

fn answer_stats(answers: &[SavedAnswer]) -> AnswerStats {
    let mut s = AnswerStats { answered: answers.len() as i64, ..Default::default() };
    for a in answers {
        s.retries += a.retries as i64;
        match a.result.as_str() {
            "correct" => {
                s.correct += 1;
                if a.retries == 0 { s.first_try += 1; }
            }
            "partial" => s.partial += 1,
            _ => {}
        }
    }
    s
}

fn answers_from_json(json: &str) -> Vec<SavedAnswer> {
    serde_json::from_str(json).unwrap_or_default()
}

#[derive(Serialize, Deserialize)]
pub struct ReadingSession {
    pub id: i64,
    pub language: String,
    pub level: String,
    pub title: String,
    pub body: String,
    pub plain_body: String,
    pub source: String,
    pub questions: Vec<ReadingQuestion>,
    pub answers: Vec<SavedAnswer>,
    pub phase: String,
    pub current_q: i64,
    pub created_at: String,
    pub elapsed_seconds: i64,
    pub stats: AnswerStats,
}

#[derive(Serialize, Deserialize)]
pub struct ReadingSessionSummary {
    pub id: i64,
    pub language: String,
    pub level: String,
    pub title: String,
    pub source: String,
    pub phase: String,
    pub current_q: i64,
    pub total_questions: i64,
    pub created_at: String,
    pub stats: AnswerStats,
}

/// Totals for the top of the reading page.
#[derive(Serialize, Deserialize)]
pub struct ReadingOverview {
    pub finished: i64,
    pub in_progress: i64,
    pub questions_answered: i64,
    /// Share of answered questions that were right on the first try, 0–100; None before any answer.
    pub first_try_pct: Option<i64>,
    /// The latest article not finished yet, offered as "continue reading".
    pub resume: Option<ReadingSessionSummary>,
}

fn level_text_char_range(level: &str) -> (usize, usize) {
    match level {
        "N5" | "A1" => (80, 300),
        "N4" | "A2" => (200, 500),
        "N3" | "B1" => (400, 800),
        "N2" | "B2" => (600, 1200),
        "N1" => (900, 1800),
        _ => (200, 500),
    }
}

fn reading_question_count(level: &str) -> usize {
    match level { "N5" | "A1" | "A2" => 3, "N4" | "N3" | "B1" => 4, _ => 5 }
}

fn truncate_at_sentence_boundary(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars { return text.to_string(); }
    let truncated: String = text.chars().take(max_chars).collect();
    for end_char in &['。', '！', '？', '.', '!', '?'] {
        if let Some(pos) = truncated.rfind(*end_char) {
            return truncated[..pos + end_char.len_utf8()].to_string();
        }
    }
    truncated
}

fn strip_html_to_plain(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => { in_tag = true; }
            '>' => { in_tag = false; result.push(' '); }
            _ if !in_tag => { result.push(ch); }
            _ => {}
        }
    }
    result.split_whitespace().collect::<Vec<_>>().join(" ")
        .replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
        .replace("&quot;", "\"").replace("&apos;", "'").replace("&nbsp;", " ")
}

fn extract_xml_tag_value(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open).map(|i| i + open.len())?;
    let end = start + xml[start..].find(&close)?;
    let raw = xml[start..end].trim();
    let content = if raw.starts_with("<![CDATA[") && raw.ends_with("]]>") {
        &raw[9..raw.len() - 3]
    } else {
        raw
    };
    Some(content.trim().to_string())
}

fn parse_rss_articles(xml: &str) -> Vec<(String, String)> {
    xml.split("<item")
        .skip(1)
        .filter_map(|chunk| {
            let end = chunk.find("</item>").unwrap_or(chunk.len());
            let item = &chunk[..end];
            let title = extract_xml_tag_value(item, "title")?;
            let desc_raw = extract_xml_tag_value(item, "description")?;
            let desc = strip_html_to_plain(&desc_raw);
            if title.is_empty() || desc.len() < 30 { return None; }
            Some((title, desc))
        })
        .collect()
}

async fn try_fetch_rss(language: &str) -> Option<(String, String)> {
    let url = match language {
        "ja" => "https://www3.nhk.or.jp/rss/news/cat0.xml",
        "id" => "https://rss.detik.com/index.php/detikcom",
        _ => return None,
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build().ok()?;
    let resp = client.get(url)
        .header("User-Agent", "Mozilla/5.0 (compatible; LangChef/1.0)")
        .send().await.ok()?;
    if !resp.status().is_success() { return None; }
    let xml = resp.text().await.ok()?;
    let articles = parse_rss_articles(&xml);
    if articles.is_empty() { return None; }
    let idx = rand::random::<usize>() % articles.len();
    Some(articles[idx].clone())
}

/// The shared rules for comprehension questions, used both with a new passage and for a news article.
fn question_rules(lang_name: &str, n: usize) -> String {
    format!(
        "{n} comprehension questions: answerable from the text only; answers in {lang_name}; \
        vary types (who, what, why, how)."
    )
}

/// Writes a passage and its questions in one request. Furigana is not asked for: Lindera adds it
/// afterwards, and having the model write it roughly doubled the answer and the wait.
async fn llm_generate_reading_text(
    api_host: &str, api_key: &str, model: &str,
    language: &str, level: &str,
    target_words: &[String],
) -> Result<(String, String, Vec<ReadingQuestion>), String> {
    let lang_name = lang_name(language);
    let (min, max) = level_text_char_range(level);
    let rules = question_rules(lang_name, reading_question_count(level));
    let vocab_note = if !target_words.is_empty() {
        let words = target_words.iter().take(20).cloned().collect::<Vec<_>>().join(", ");
        format!("The learner is studying these vocabulary words: {words}.\n\
                Naturally incorporate as many as fit the passage — do not force words that don't fit.\n")
    } else { String::new() };
    let prompt = format!(
        "Write a {level}-level {lang_name} reading passage on a random everyday topic.\n\
        Length: {min}–{max} characters. Vocabulary and grammar must match {level}. Plain text, no furigana or markup.\n\
        {vocab_note}\
        Then write {rules}\n\
        Return ONLY valid JSON (no markdown fences): \
        {{\"title\": \"...\", \"body\": \"...\", \"questions\": [{{\"question\": \"...\", \"answer_hint\": \"...\"}}]}}",
    );
    let raw = complete_prompt(api_host, api_key, model, prompt).await?;
    let cleaned = json_part(&raw);
    #[derive(Deserialize)]
    struct Generated {
        #[serde(default)]
        title: String,
        #[serde(default)]
        body: String,
        #[serde(default)]
        questions: Vec<ReadingQuestion>,
    }
    let g: Generated = serde_json::from_str(cleaned)
        .map_err(|_| app_error("api_bad_response", cleaned))?;
    if g.body.is_empty() { return Err(app_error("api_bad_response", "empty passage")); }
    Ok((g.title, g.body, g.questions))
}

#[tauri::command]
pub async fn fetch_reading_text(
    state: tauri::State<'_, Mutex<Option<furigana::FuriganaAnnotator>>>,
    api_host: String, api_key: String, model: String,
    language: String, level: String,
    target_words: Vec<String>,
) -> Result<ReadingText, String> {
    let (min, max) = level_text_char_range(&level);
    let (title, raw_body, source, questions) = match try_fetch_rss(&language).await {
        Some((t, b)) if b.chars().count() >= min / 2 => {
            let src = match language.as_str() {
                "ja" => "NHK News".to_string(),
                "id" => "Detik".to_string(),
                _ => "RSS".to_string(),
            };
            (t, truncate_at_sentence_boundary(&b, max), src, Vec::new())
        }
        _ => {
            let (t, b, q) = llm_generate_reading_text(&api_host, &api_key, &model, &language, &level, &target_words).await?;
            (t, b, "ai".to_string(), q)
        }
    };
    // An AI passage is kept whole: its questions were written for all of it.
    let plain_body = furigana::strip_existing_furigana(&raw_body);
    let body = if language == "ja" {
        match state.lock() {
            Ok(mut g) => g.as_mut().map(|a| a.annotate(&plain_body)).unwrap_or_else(|| plain_body.clone()),
            Err(_) => plain_body.clone(),
        }
    } else {
        plain_body.clone()
    };
    Ok(ReadingText { title, body, plain_body, source, language, level, questions })
}

#[tauri::command]
pub async fn generate_reading_questions(
    api_host: String, api_key: String, model: String,
    plain_body: String, language: String, level: String,
) -> Result<Vec<ReadingQuestion>, String> {
    let lang_name = lang_name(&language);
    let rules = question_rules(lang_name, reading_question_count(&level));
    let prompt = format!(
        "You are a {lang_name} teacher. For this {level}-level passage, write {rules}\n\
        Return ONLY a JSON array (no markdown): [{{\"question\": \"...\", \"answer_hint\": \"...\"}}]\n\n\
        Passage:\n{plain_body}",
    );
    let raw = complete_prompt(&api_host, &api_key, &model, prompt).await?;
    let cleaned = json_part(&raw);
    serde_json::from_str::<Vec<ReadingQuestion>>(cleaned)
        .map_err(|_| app_error("api_bad_response", cleaned))
}

/// Checks a comprehension answer. Feedback is in the learner's explanation language. With
/// `answer_in_explain` (beginners), the answer itself may be in that language: only understanding is checked.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn evaluate_reading_answer(
    app: tauri::AppHandle,
    api_host: String, api_key: String, model: String,
    question: String, answer_hint: String,
    user_answer: String, plain_body: String, language: String,
    answer_in_explain: Option<bool>,
) -> Result<AnswerEvaluation, String> {
    let explain = explain_lang(&crate::db::open_db(&app)?);
    let explain_name = lang_name(&explain);
    let lang_name = lang_name(&language);
    let answer_rule = if answer_in_explain.unwrap_or(false) {
        format!("The student may answer in {explain_name} or {lang_name}. Judge only whether they understood the text, not their {lang_name}.\n")
    } else {
        format!("The student should answer in {lang_name}.\n")
    };
    let prompt = format!(
        "You are a {lang_name} teacher evaluating a reading comprehension answer.\n\n\
        Passage:\n{plain_body}\n\n\
        Question: {question}\n\
        Expected (hint only, not exact match required): {answer_hint}\n\
        Student answer: {user_answer}\n\
        {answer_rule}\n\
        Classify as:\n\
        - \"correct\": factually right and complete enough\n\
        - \"partial\": right idea but incomplete or slightly off\n\
        - \"incorrect\": wrong or misunderstands the text\n\n\
        Return ONLY JSON (no markdown): {{\"result\": \"correct\"|\"partial\"|\"incorrect\", \"feedback\": \"1-2 sentences in {explain_name}\"}}",
    );
    let feedback_of = |raw: &str| {
        serde_json::from_str::<AnswerEvaluation>(json_part(raw)).map(|e| e.feedback).unwrap_or_default()
    };
    let raw = complete_explained(&api_host, &api_key, &model, &prompt, &explain, feedback_of).await?;
    Ok(serde_json::from_str::<AnswerEvaluation>(json_part(&raw)).unwrap_or(AnswerEvaluation {
        result: "incorrect".to_string(),
        feedback: raw.trim().to_string(),
    }))
}

#[tauri::command]
pub fn db_save_reading_session(
    app: tauri::AppHandle,
    language: String, level: String, title: String,
    body: String, plain_body: String, source: String,
    questions: String,
) -> Result<i64, String> {
    let conn = open_db(&app)?;
    conn.execute(
        "INSERT INTO reading_sessions (language, level, title, body, plain_body, source, questions, answers, phase, current_q)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '[]', 'reading', 0)",
        params![language, level, title, body, plain_body, source, questions],
    ).map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

fn list_sessions(conn: &rusqlite::Connection, filter: &str, limit: i64) -> Result<Vec<ReadingSessionSummary>, String> {
    let sql = format!(
        "SELECT id, language, level, title, source, phase, current_q,
                json_array_length(questions) as total_questions, created_at, answers
         FROM reading_sessions {filter}
         ORDER BY id DESC
         LIMIT ?1"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![limit], |row| {
        Ok(ReadingSessionSummary {
            id: row.get(0)?,
            language: row.get(1)?,
            level: row.get(2)?,
            title: row.get(3)?,
            source: row.get(4)?,
            phase: row.get(5)?,
            current_q: row.get(6)?,
            total_questions: row.get(7)?,
            created_at: row.get(8)?,
            stats: answer_stats(&answers_from_json(&row.get::<_, String>(9)?)),
        })
    }).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn db_list_reading_sessions(app: tauri::AppHandle) -> Result<Vec<ReadingSessionSummary>, String> {
    list_sessions(&open_db(&app)?, "", 100)
}

const SESSION_COLS: &str =
    "id, language, level, title, body, plain_body, source, questions, answers, phase, current_q, created_at, elapsed_seconds";

fn session_from_row(row: &rusqlite::Row) -> rusqlite::Result<ReadingSession> {
    let questions: Vec<ReadingQuestion> = serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default();
    let answers = answers_from_json(&row.get::<_, String>(8)?);
    Ok(ReadingSession {
        id: row.get(0)?,
        language: row.get(1)?,
        level: row.get(2)?,
        title: row.get(3)?,
        body: row.get(4)?,
        plain_body: row.get(5)?,
        source: row.get(6)?,
        questions,
        stats: answer_stats(&answers),
        answers,
        phase: row.get(9)?,
        current_q: row.get(10)?,
        created_at: row.get(11)?,
        elapsed_seconds: row.get(12)?,
    })
}

#[tauri::command]
pub fn db_get_reading_session(app: tauri::AppHandle, id: i64) -> Result<Option<ReadingSession>, String> {
    let conn = open_db(&app)?;
    let sql = format!("SELECT {SESSION_COLS} FROM reading_sessions WHERE id = ?1");
    match conn.query_row(&sql, params![id], session_from_row) {
        Ok(s) => Ok(Some(s)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub fn db_get_active_reading_session(
    app: tauri::AppHandle,
    language: String,
) -> Result<Option<ReadingSession>, String> {
    let conn = open_db(&app)?;
    let sql = format!(
        "SELECT {SESSION_COLS} FROM reading_sessions WHERE language = ?1 AND phase != 'complete'
         ORDER BY id DESC LIMIT 1"
    );
    match conn.query_row(&sql, params![language], session_from_row) {
        Ok(s) => Ok(Some(s)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub fn db_reading_overview(app: tauri::AppHandle) -> Result<ReadingOverview, String> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare("SELECT phase, answers FROM reading_sessions").map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?;
    let (mut finished, mut in_progress, mut answered, mut first_try) = (0, 0, 0, 0);
    for row in rows {
        let (phase, answers) = row.map_err(|e| e.to_string())?;
        if phase == "complete" { finished += 1 } else { in_progress += 1 }
        let s = answer_stats(&answers_from_json(&answers));
        answered += s.answered;
        first_try += s.first_try;
    }
    Ok(ReadingOverview {
        finished,
        in_progress,
        questions_answered: answered,
        first_try_pct: (answered > 0).then(|| (first_try * 100 + answered / 2) / answered),
        resume: list_sessions(&conn, "WHERE phase != 'complete'", 1)?.into_iter().next(),
    })
}

#[tauri::command]
pub fn db_delete_reading_session(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute("DELETE FROM reading_sessions WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    // The cached translation of the passage goes with it.
    let _ = conn.execute("DELETE FROM translations WHERE kind = 'reading' AND ref_id = ?1", params![id]);
    Ok(())
}

#[tauri::command]
pub fn db_update_reading_session(
    app: tauri::AppHandle,
    id: i64, phase: String, current_q: i64, answers: String,
    elapsed_seconds: Option<i64>,
) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute(
        "UPDATE reading_sessions SET phase = ?1, current_q = ?2, answers = ?3,
                elapsed_seconds = COALESCE(?5, elapsed_seconds) WHERE id = ?4",
        params![phase, current_q, answers, id, elapsed_seconds],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ans(result: &str, retries: u32) -> SavedAnswer {
        SavedAnswer { question_index: 0, user_answer: String::new(), result: result.into(), retries, feedback: String::new() }
    }

    #[test]
    fn stats_count_first_try_partial_and_retries() {
        let s = answer_stats(&[ans("correct", 0), ans("correct", 2), ans("partial", 1)]);
        assert_eq!(s, AnswerStats { answered: 3, correct: 2, first_try: 1, partial: 1, retries: 3 });
    }

    #[test]
    fn old_answers_without_feedback_still_parse() {
        let a = answers_from_json(r#"[{"question_index":0,"user_answer":"x","result":"correct","retries":0}]"#);
        assert_eq!(a.len(), 1);
    }
}
