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

/// The rules for comprehension questions, for an AI passage and a news article alike.
fn question_rules(lang_name: &str, n: usize) -> String {
    format!(
        "{n} comprehension questions: answerable from the text only; answers in {lang_name}; \
        vary types (who, what, why, how)."
    )
}

/// Topics for an AI passage. The model is asked for one of these instead of "a random topic":
/// left to choose, it picks the same one every time.
const READING_TOPICS: &[&str] = &[
    "a morning routine", "a weekend trip", "shopping at a market", "a birthday", "the weather this week",
    "a new hobby", "a day at school", "a part-time job", "visiting a friend's home", "a local festival",
    "riding the train", "a lost item", "cleaning the room", "a pet", "a visit to the doctor",
    "sports day", "a letter to family", "moving to a new town", "a rainy day", "a library visit",
    "a museum", "cooking dinner at home", "a picnic in the park", "a concert", "learning to swim",
    "a bicycle ride", "a phone call with a friend", "a summer holiday", "a winter evening", "a garden",
    "a job interview", "a hotel stay", "a restaurant menu", "a neighbor", "a movie night",
    "a camping trip", "a hospital visit", "a train delay", "a gift for someone", "a mountain hike",
    "the first day at work", "a beach day", "a broken phone", "an online video", "a cafe",
    "planning a party", "a long walk", "a school club", "a souvenir", "a city map",
];

/// How many of the learner's words a passage is asked to use. A few, so they don't decide the topic.
const READING_TARGET_WORDS: usize = 12;

fn pick_reading_topic(rng: &mut impl rand::Rng) -> &'static str {
    use rand::seq::SliceRandom;
    READING_TOPICS.choose(rng).copied().unwrap_or("daily life")
}

/// A fresh random sample of the learner's word cards in `language` for each passage, so the
/// same oldest cards don't steer every passage to the same subject.
fn pick_reading_target_words(conn: &rusqlite::Connection, language: &str) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT content FROM review_items
         WHERE item_type = 'word' AND (language = ?1 OR language IS NULL OR language = '')
         ORDER BY RANDOM() LIMIT ?2",
    ) else { return Vec::new() };
    stmt.query_map(params![language, READING_TARGET_WORDS as i64], |r| r.get::<_, String>(0))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

const PARAGRAPH_BREAK: &str = "\n\n";
/// Sentences per paragraph when a passage arrives as one block.
const SENTENCES_PER_PARAGRAPH: usize = 3;

/// Splits a passage into short paragraphs, which is easier to read on a phone than one block.
/// Keeps the paragraphs the text already has; a text in one block (a news article, or a model
/// that ignored the instruction) is broken every few sentences, never leaving one sentence alone at the end.
fn paragraphize(text: &str, language: &str) -> String {
    let existing: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if existing.len() > 1 {
        return existing.join(PARAGRAPH_BREAK);
    }
    let sentences = crate::breakdown::split_sentences(text);
    if sentences.len() <= SENTENCES_PER_PARAGRAPH + 1 {
        return text.trim().to_string();
    }
    let joiner = if language == "ja" { "" } else { " " };
    let mut groups: Vec<Vec<String>> = sentences.chunks(SENTENCES_PER_PARAGRAPH).map(<[String]>::to_vec).collect();
    if groups.len() > 1 && groups.last().is_some_and(|g| g.len() == 1) {
        let last = groups.pop().unwrap_or_default();
        if let Some(prev) = groups.last_mut() { prev.extend(last); }
    }
    groups.iter().map(|g| g.join(joiner)).collect::<Vec<_>>().join(PARAGRAPH_BREAK)
}

/// How many of the level's grammar patterns a passage must use.
const READING_LEVEL_PATTERNS: usize = 4;

/// What a passage at `level` reads like. Told only "match N3", models write N5-style ます
/// sentences, so this sets the floor as well as the ceiling.
fn level_style(level: &str) -> &'static str {
    match level {
        "N5" => "Polite です/ます form, short simple sentences, basic everyday words; kanji only for the most common N5 words.",
        "N4" => "Polite form, sentences joined with て-form, から, けど, たら and simple relative clauses; common N4 kanji words.",
        "N3" => "Written like a short blog post or column, in plain form (だ/である) or a mix with polite form. \
                 Most sentences join two or three clauses with relative clauses, ので, のに, ために, ように. \
                 Use N3 vocabulary written in kanji (e.g. 経験, 将来, 関係, 習慣, 確認). \
                 It must NOT read like a beginner text made of short です/ます sentences.",
        "N2" => "Written like a newspaper feature or opinion column, in plain form (だ/である). \
                 Long sentences with embedded clauses, abstract vocabulary in kanji (e.g. 影響, 状況, 対策, 傾向, 維持), \
                 and connectives such as 一方で, にもかかわらず, ことから. It must NOT read like a beginner or N3 text.",
        "N1" => "Written like an editorial or essay, in plain form (である), with a clear line of argument, nuanced and abstract \
                 vocabulary, formal written connectives and advanced grammar. Native-level, no simplification.",
        "A1" => "Very short simple sentences in the present, the most common everyday words.",
        "A2" => "Short sentences with common connectives (dan, tetapi, karena), past and future with sudah/akan.",
        "B1" => "Connected paragraphs with subordinate clauses, the me-/di- verb forms and everyday vocabulary beyond basics.",
        "B2" => "Written like a magazine or news feature: complex sentences, abstract and formal vocabulary.",
        _ => "",
    }
}

/// A random handful of grammar patterns at `level` (bundled originals and the learner's own),
/// for the passage to use, so it is written at the level and not below it.
fn pick_level_patterns(conn: &rusqlite::Connection, language: &str, level: &str) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT pattern FROM grammar_items
         WHERE language = ?1 AND level = ?2 AND source_id IS NULL
         GROUP BY pattern ORDER BY RANDOM() LIMIT ?3",
    ) else { return Vec::new() };
    stmt.query_map(params![language, level, READING_LEVEL_PATTERNS as i64], |r| r.get::<_, String>(0))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

/// Writes a passage only, so the learner can start reading sooner; its questions come from
/// `generate_reading_questions` while they read. Furigana is not asked for: Lindera adds it
/// afterwards, and having the model write it roughly doubled the answer and the wait.
#[allow(clippy::too_many_arguments)]
async fn llm_generate_reading_text(
    api_host: &str, api_key: &str, model: &str,
    language: &str, level: &str,
    topic: &str,
    target_words: &[String],
    level_patterns: &[String],
) -> Result<(String, String), String> {
    let lang_name = lang_name(language);
    let (min, max) = level_text_char_range(level);
    let style = level_style(level);
    let style_note = if style.is_empty() { String::new() } else { format!("Style for {level}: {style}\n") };
    let grammar_note = if !level_patterns.is_empty() {
        format!("Use each of these {level} grammar patterns at least once: {}.\n", level_patterns.join(", "))
    } else { String::new() };
    let vocab_note = if !target_words.is_empty() {
        let words = target_words.join(", ");
        format!("The learner is studying these vocabulary words: {words}.\n\
                Use the ones that fit the topic naturally — do not force words that don't fit, \
                and do not change the topic to fit them.\n")
    } else { String::new() };
    let prompt = format!(
        "Write a {level}-level {lang_name} reading passage about {topic}.\n\
        The learner is at {level}: the passage should stretch them at that level, not be easier. \
        Vocabulary and grammar should be typical of {level}, without going above it.\n\
        {style_note}\
        {grammar_note}\
        Length: {min}–{max} characters. Plain text, no furigana or markup.\n\
        Split the body into short paragraphs of 2–4 sentences, one idea each, separated by a blank line (\\n\\n).\n\
        {vocab_note}\
        Return ONLY valid JSON (no markdown fences): {{\"title\": \"...\", \"body\": \"...\"}}",
    );
    let raw = complete_prompt(api_host, api_key, model, prompt).await?;
    let cleaned = json_part(&raw);
    #[derive(Deserialize)]
    struct Generated {
        #[serde(default)]
        title: String,
        #[serde(default)]
        body: String,
    }
    let g: Generated = serde_json::from_str(cleaned)
        .map_err(|_| app_error("api_bad_response", cleaned))?;
    if g.body.is_empty() { return Err(app_error("api_bad_response", "empty passage")); }
    Ok((g.title, g.body))
}

#[tauri::command]
pub async fn fetch_reading_text(
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<Option<furigana::FuriganaAnnotator>>>,
    api_host: String, api_key: String, model: String,
    language: String, level: String,
) -> Result<ReadingText, String> {
    let (min, max) = level_text_char_range(&level);
    let (title, raw_body, source) = match try_fetch_rss(&language).await {
        Some((t, b)) if b.chars().count() >= min / 2 => {
            let src = match language.as_str() {
                "ja" => "NHK News".to_string(),
                "id" => "Detik".to_string(),
                _ => "RSS".to_string(),
            };
            (t, truncate_at_sentence_boundary(&b, max), src)
        }
        _ => {
            let (target_words, level_patterns) = {
                let conn = open_db(&app)?;
                (pick_reading_target_words(&conn, &language), pick_level_patterns(&conn, &language, &level))
            };
            let topic = pick_reading_topic(&mut rand::thread_rng());
            let (t, b) = llm_generate_reading_text(
                &api_host, &api_key, &model, &language, &level, topic, &target_words, &level_patterns,
            ).await?;
            (t, b, "ai".to_string())
        }
    };
    let plain_body = paragraphize(&furigana::strip_existing_furigana(&raw_body), &language);
    let body = if language == "ja" {
        match state.lock() {
            Ok(mut g) => match g.as_mut() {
                // Per paragraph, so the tokenizer can't lose the breaks between them.
                Some(a) => plain_body.split(PARAGRAPH_BREAK).map(|p| a.annotate(p)).collect::<Vec<_>>().join(PARAGRAPH_BREAK),
                None => plain_body.clone(),
            },
            Err(_) => plain_body.clone(),
        }
    } else {
        plain_body.clone()
    };
    Ok(ReadingText { title, body, plain_body, source, language, level })
}

/// Writes the questions for a passage. With `session_id`, also saves them to that reading session,
/// so they are kept even if the learner leaves the page before they arrive.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn generate_reading_questions(
    app: tauri::AppHandle,
    api_host: String, api_key: String, model: String,
    plain_body: String, language: String, level: String,
    session_id: Option<i64>,
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
    let questions = serde_json::from_str::<Vec<ReadingQuestion>>(cleaned)
        .map_err(|_| app_error("api_bad_response", cleaned))?;
    if questions.is_empty() {
        return Err(app_error("api_bad_response", "no questions"));
    }
    if let Some(id) = session_id {
        let json = serde_json::to_string(&questions).map_err(|e| e.to_string())?;
        open_db(&app)?
            .execute("UPDATE reading_sessions SET questions = ?1 WHERE id = ?2", params![json, id])
            .map_err(|e| e.to_string())?;
    }
    Ok(questions)
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
    fn topics_vary_between_passages() {
        let mut rng = rand::thread_rng();
        let picked: std::collections::HashSet<_> = (0..50).map(|_| pick_reading_topic(&mut rng)).collect();
        assert!(picked.len() > 10, "only {} distinct topics in 50 picks", picked.len());
    }

    #[test]
    fn target_words_are_a_random_sample_of_word_cards_in_the_language() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::prepare_db(&conn).unwrap();
        let add = |content: &str, language: &str, item_type: &str| {
            conn.execute(
                "INSERT INTO review_items(id, content, EF, I, R, state, step, lapse_count, language, item_type, created_at)
                 VALUES (?1, ?1, -1.0, 0, 0, 'new', 0, 0, ?2, ?3, 0)",
                params![content, language, item_type],
            ).unwrap();
        };
        for i in 0..40 {
            add(&format!("ja{i}"), "ja", "word");
        }
        add("kata", "id", "word");
        add("〜ている", "ja", "grammar");

        let first = pick_reading_target_words(&conn, "ja");
        assert_eq!(first.len(), READING_TARGET_WORDS);
        assert!(first.iter().all(|w| w.starts_with("ja")), "{first:?}");
        let differs = (0..10).any(|_| pick_reading_target_words(&conn, "ja") != first);
        assert!(differs, "the same words came back every time");
    }

    #[test]
    fn one_block_is_split_every_few_sentences() {
        let text = "一。二。三。四。五。六。七。";
        assert_eq!(paragraphize(text, "ja"), "一。二。三。\n\n四。五。六。七。");
        assert_eq!(paragraphize("A satu. B dua. C tiga. D empat. E lima. F enam.", "id"),
                   "A satu. B dua. C tiga.\n\nD empat. E lima. F enam.");
    }

    #[test]
    fn existing_paragraphs_and_short_texts_are_kept() {
        assert_eq!(paragraphize("一。二。\n三。\n\n\n四。", "ja"), "一。二。\n\n三。\n\n四。");
        assert_eq!(paragraphize(" 一。二。三。四。 ", "ja"), "一。二。三。四。");
        assert_eq!(paragraphize("「行く。」と言った。二。三。四。五。", "ja"), "「行く。」と言った。二。三。\n\n四。五。");
    }

    #[test]
    fn level_patterns_come_from_the_level_without_repeats() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::prepare_db(&conn).unwrap();
        let n3: std::collections::HashSet<String> = conn
            .prepare("SELECT pattern FROM grammar_items WHERE language = 'ja' AND level = 'N3'").unwrap()
            .query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();

        let picked = pick_level_patterns(&conn, "ja", "N3");
        assert_eq!(picked.len(), READING_LEVEL_PATTERNS);
        assert!(picked.iter().all(|p| n3.contains(p)), "{picked:?}");
        assert_eq!(picked.iter().collect::<std::collections::HashSet<_>>().len(), picked.len());
        assert!(pick_level_patterns(&conn, "ja", "N9").is_empty());
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
