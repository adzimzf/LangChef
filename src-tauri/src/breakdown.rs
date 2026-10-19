//! Sentence breakdown ("Urai kalimat"): the learner pastes a sentence they understand word by word
//! but not as a whole, and gets its structure (chunks and their roles, nesting, word order compared
//! with their own language, particles and endings, a word-for-word gloss) plus one question to check
//! they now read it right. Results are cached per sentence and explanation language and form the
//! page's history.

use crate::chat::{complete_explained, json_part};
use crate::db::open_db;
use crate::locale::{app_error, explain_lang, lang_name};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Chunk roles the frontend colors and names (`breakdown.role.<role>`). Anything else becomes `other`.
const ROLES: &[&str] = &[
    "topic", "subject", "object", "modifier", "purpose", "reason", "condition", "time", "place", "connector",
    "predicate", "other",
];

const SENTENCE_ENDS: &[char] = &['。', '！', '？', '!', '?', '.'];
/// Closing quotes and brackets that belong to the sentence they end.
const CLOSERS: &[char] = &['」', '』', '）', ')', '"', '”', '’'];
/// A sentence end inside these quotes is part of the quoted speech, not the end of the sentence.
const QUOTE_OPEN: &[char] = &['「', '『'];
const QUOTE_CLOSE: &[char] = &['」', '』'];

#[derive(Serialize, Deserialize, Clone)]
pub struct Chunk {
    pub text: String,
    #[serde(default)]
    pub role: String,
    /// Short role label in the explanation language, e.g. "Topik · は".
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub meaning: String,
    /// Not said in the sentence but understood, e.g. an omitted subject (私は).
    #[serde(default)]
    pub implied: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Layer {
    pub text: String,
    #[serde(default)]
    pub meaning: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct OrderPart {
    pub text: String,
    #[serde(default)]
    pub role: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct WordOrder {
    #[serde(default)]
    pub target: Vec<OrderPart>,
    #[serde(default)]
    pub native: Vec<OrderPart>,
    #[serde(default)]
    pub note: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Note {
    pub text: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Gloss {
    pub text: String,
    #[serde(default)]
    pub reading: String,
    #[serde(default)]
    pub meaning: String,
}

/// The check question as stored; `answer` never leaves the backend.
#[derive(Serialize, Deserialize, Clone)]
struct Check {
    question: String,
    options: Vec<String>,
    answer: usize,
    #[serde(default)]
    explanation: String,
}

#[derive(Serialize, Clone)]
pub struct CheckView {
    pub question: String,
    pub options: Vec<String>,
}

/// What the LLM returns and what is stored in `sentence_breakdowns.data`.
#[derive(Serialize, Deserialize, Clone)]
struct BreakdownData {
    #[serde(default)]
    translation: String,
    #[serde(default)]
    core: String,
    #[serde(default)]
    core_note: String,
    #[serde(default)]
    tip: String,
    #[serde(default)]
    chunks: Vec<Chunk>,
    #[serde(default)]
    layers: Vec<Layer>,
    #[serde(default)]
    order: WordOrder,
    #[serde(default)]
    notes: Vec<Note>,
    #[serde(default)]
    gloss: Vec<Gloss>,
    #[serde(default)]
    structures: Vec<String>,
    #[serde(default)]
    check: Option<Check>,
}

#[derive(Serialize)]
pub struct SentenceBreakdown {
    pub id: i64,
    pub sentence: String,
    pub language: String,
    pub translation: String,
    pub core: String,
    pub core_note: String,
    pub tip: String,
    pub chunks: Vec<Chunk>,
    /// The said chunks spell out the sentence, so it can be shown colored by chunk.
    pub chunks_match: bool,
    pub layers: Vec<Layer>,
    pub order: WordOrder,
    pub notes: Vec<Note>,
    pub gloss: Vec<Gloss>,
    pub structures: Vec<String>,
    pub check: Option<CheckView>,
    /// Result of the check question, once answered.
    pub checked: Option<bool>,
}

#[derive(Serialize)]
pub struct BreakdownSummary {
    pub id: i64,
    pub sentence: String,
    pub structures: Vec<String>,
    pub checked: Option<bool>,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct CheckResult {
    pub correct: bool,
    pub answer: usize,
    pub explanation: String,
}

/// Splits pasted text into sentences, keeping each sentence's end mark and closing quotes.
/// Line breaks end a sentence too; end marks inside 「」 do not.
pub fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut quote_depth = 0usize;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' || c == '\r' {
            push_sentence(&mut out, &mut cur);
            quote_depth = 0;
            i += 1;
            continue;
        }
        cur.push(c);
        if QUOTE_OPEN.contains(&c) {
            quote_depth += 1;
        } else if QUOTE_CLOSE.contains(&c) {
            quote_depth = quote_depth.saturating_sub(1);
        }
        if quote_depth == 0 && SENTENCE_ENDS.contains(&c) {
            // "3.5" and "e.g." are not sentence ends: an ASCII period must be followed by a space.
            let ascii_mid = c == '.' && chars.get(i + 1).is_some_and(|n| !n.is_whitespace());
            if !ascii_mid {
                while let Some(&n) = chars.get(i + 1) {
                    if SENTENCE_ENDS.contains(&n) || CLOSERS.contains(&n) {
                        cur.push(n);
                        i += 1;
                    } else {
                        break;
                    }
                }
                push_sentence(&mut out, &mut cur);
            }
        }
        i += 1;
    }
    push_sentence(&mut out, &mut cur);
    out
}

fn push_sentence(out: &mut Vec<String>, cur: &mut String) {
    let s = cur.trim();
    if s.chars().any(|c| c.is_alphanumeric()) {
        out.push(s.to_string());
    }
    cur.clear();
}

/// Letters and digits only, so chunks can be compared with the sentence regardless of spacing and punctuation.
fn letters(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

fn chunks_spell(chunks: &[Chunk], sentence: &str) -> bool {
    let said: String = chunks.iter().filter(|c| !c.implied).map(|c| c.text.as_str()).collect();
    !chunks.is_empty() && letters(&said) == letters(sentence)
}

/// Keeps the parts of an LLM answer the page can show: known roles, a check question with a valid answer.
fn clean(mut data: BreakdownData) -> BreakdownData {
    let known = |r: &str| if ROLES.contains(&r) { r.to_string() } else { "other".to_string() };
    for c in &mut data.chunks {
        c.role = known(&c.role);
    }
    for p in data.order.target.iter_mut().chain(data.order.native.iter_mut()) {
        p.role = known(&p.role);
    }
    data.chunks.retain(|c| !c.text.trim().is_empty());
    data.structures.retain(|s| !s.trim().is_empty());
    data.structures.truncate(4);
    if let Some(check) = &data.check {
        let valid = (2..=4).contains(&check.options.len())
            && check.answer < check.options.len()
            && check.options.iter().all(|o| !o.trim().is_empty());
        if !valid {
            data.check = None;
        }
    }
    data
}

fn view(id: i64, sentence: String, language: String, data: BreakdownData, checked: Option<bool>) -> SentenceBreakdown {
    let chunks_match = chunks_spell(&data.chunks, &sentence);
    SentenceBreakdown {
        id,
        language,
        translation: data.translation,
        core: data.core,
        core_note: data.core_note,
        tip: data.tip,
        chunks_match,
        chunks: data.chunks,
        layers: data.layers,
        order: data.order,
        notes: data.notes,
        gloss: data.gloss,
        structures: data.structures,
        check: data.check.map(|c| CheckView { question: c.question, options: c.options }),
        checked,
        sentence,
    }
}

fn prompt(sentence: &str, language: &str, explain: &str) -> String {
    let studied = lang_name(language);
    let native = lang_name(explain);
    let reading_rule = if language == "ja" {
        "\"reading\": the hiragana reading of the gloss item when it has kanji, else \"\""
    } else {
        "\"reading\": \"\""
    };
    format!(
        "A learner whose language is {native} knows every word of this {studied} sentence but cannot follow \
         the sentence as a whole: who does what, what describes what, how the parts nest. Explain its STRUCTURE, \
         comparing with {native} grammar and word order. Use plain words, not linguistic jargon.\n\n\
         Sentence: {sentence}\n\n\
         Return ONLY JSON (no markdown) with these keys:\n\
         - \"translation\": natural {native} translation.\n\
         - \"core\": the bare main clause in {native} (main subject/topic + main verb), one short sentence.\n\
         - \"core_note\": one sentence in {native} on what the rest of the sentence adds (e.g. why / for what / which one).\n\
         - \"tip\": one sentence in {native} on how to read this kind of sentence (e.g. read the topic, then jump to the final verb).\n\
         - \"chunks\": the sentence cut into 3-10 meaningful chunks IN ORDER. Concatenating the \"text\" of all chunks \
         that are not implied must give back the exact sentence (keep particles and punctuation inside chunks). \
         Each chunk: {{\"text\": exact {studied} text, \"role\": one of {roles}, \"label\": short role name in {native} \
         plus the particle or ending that marks it (e.g. \"Topik · は\"), \"meaning\": {native} meaning of the chunk, \
         \"implied\": false}}. Add a chunk with \"implied\": true and \"text\" in brackets (e.g. \"(私は)\") where an \
         omitted subject or object is understood. Role \"modifier\" = a clause or word describing a noun; \
         \"predicate\" = the main verb or final predicate.\n\
         - \"layers\": the nesting from outermost (main clause) to innermost (2-4 items). Each: {{\"text\": the {studied} \
         part with \"…\" where the inner layer goes, \"meaning\": {native}}}.\n\
         - \"order\": {{\"target\": the chunks in {studied} order, written in {native}, \"native\": the same parts in \
         natural {native} order, \"note\": one sentence in {native} on the difference}}. Each part: \
         {{\"text\": {native} words, \"role\": same role codes}}.\n\
         - \"notes\": 4-10 particles, endings and words that steer the structure. Each: {{\"text\": the {studied} \
         form (split endings with · like 使わ·れる), \"note\": one or two sentences in {native} on what it does HERE}}.\n\
         - \"gloss\": word-for-word, in order, 6-16 items. Each: {{\"text\": {studied}, {reading_rule}, \"meaning\": {native}}}.\n\
         - \"structures\": 1-3 short names in {native} of the structures that make this sentence hard (e.g. \
         \"Keterangan bertingkat\", \"Bentuk pasif\").\n\
         - \"check\": one multiple-choice question in {native} that can only be answered by reading the structure \
         right (who did what, what is prevented, what describes what): {{\"question\": ..., \"options\": 3 short \
         {native} options, \"answer\": index of the right option (0-2), \"explanation\": one sentence in {native} \
         pointing at the part of the sentence that gives the answer}}. Make the wrong options the usual misreadings.",
        roles = ROLES.join(", "),
    )
}

/// Breaks a sentence down (or returns the cached breakdown for it in the current explanation language).
#[tauri::command]
pub async fn llm_sentence_breakdown(
    app: tauri::AppHandle,
    api_host: String,
    api_key: String,
    model: String,
    sentence: String,
    language: String,
) -> Result<SentenceBreakdown, String> {
    let sentence = sentence.trim().to_string();
    if sentence.is_empty() {
        return Err(app_error("breakdown_empty", ""));
    }
    let explain = explain_lang(&open_db(&app)?);
    let cached = {
        let conn = open_db(&app)?;
        conn.query_row(
            "SELECT id, data, checked FROM sentence_breakdowns WHERE language = ?1 AND explain_lang = ?2 AND sentence = ?3",
            params![language, explain, sentence],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<bool>>(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
    };
    if let Some((id, data, checked)) = cached {
        if let Ok(data) = serde_json::from_str::<BreakdownData>(&data) {
            let conn = open_db(&app)?;
            conn.execute("UPDATE sentence_breakdowns SET created_at = datetime('now') WHERE id = ?1", params![id])
                .map_err(|e| e.to_string())?;
            return Ok(view(id, sentence, language, data, checked));
        }
    }

    let learner_text = |raw: &str| {
        serde_json::from_str::<BreakdownData>(json_part(raw)).map(|d| format!("{} {}", d.core, d.core_note)).unwrap_or_default()
    };
    let raw = complete_explained(&api_host, &api_key, &model, &prompt(&sentence, &language, &explain), &explain, learner_text).await?;
    let data: BreakdownData =
        serde_json::from_str(json_part(&raw)).map_err(|e| app_error("api_bad_response", format!("{}: {}", e, raw)))?;
    let data = clean(data);
    if data.chunks.is_empty() {
        return Err(app_error("api_bad_response", raw));
    }

    let conn = open_db(&app)?;
    let json = serde_json::to_string(&data).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO sentence_breakdowns(language, explain_lang, sentence, data) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(language, explain_lang, sentence) DO UPDATE SET data = excluded.data, checked = NULL,
           created_at = datetime('now')",
        params![language, explain, sentence, json],
    )
    .map_err(|e| e.to_string())?;
    let id: i64 = conn
        .query_row(
            "SELECT id FROM sentence_breakdowns WHERE language = ?1 AND explain_lang = ?2 AND sentence = ?3",
            params![language, explain, sentence],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(view(id, sentence, language, data, None))
}

/// Pasted text cut into sentences, so the learner can pick the one to break down.
#[tauri::command]
pub fn breakdown_split_sentences(text: String) -> Vec<String> {
    split_sentences(&text)
}

/// Recent breakdowns for a target language, in the current explanation language, newest first.
#[tauri::command]
pub fn db_list_sentence_breakdowns(app: tauri::AppHandle, language: String) -> Result<Vec<BreakdownSummary>, String> {
    let conn = open_db(&app)?;
    let explain = explain_lang(&conn);
    let mut stmt = conn
        .prepare(
            "SELECT id, sentence, data, checked, created_at FROM sentence_breakdowns
             WHERE language = ?1 AND explain_lang = ?2 ORDER BY created_at DESC, id DESC LIMIT 30",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![language, explain], |r| {
            let data: String = r.get(2)?;
            let structures = serde_json::from_str::<BreakdownData>(&data).map(|d| d.structures).unwrap_or_default();
            Ok(BreakdownSummary {
                id: r.get(0)?,
                sentence: r.get(1)?,
                structures,
                checked: r.get(3)?,
                created_at: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn db_get_sentence_breakdown(app: tauri::AppHandle, id: i64) -> Result<Option<SentenceBreakdown>, String> {
    let conn = open_db(&app)?;
    let row = conn
        .query_row(
            "SELECT sentence, language, data, checked FROM sentence_breakdowns WHERE id = ?1",
            params![id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<bool>>(3)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(row.and_then(|(sentence, language, data, checked)| {
        serde_json::from_str::<BreakdownData>(&data).ok().map(|d| view(id, sentence, language, d, checked))
    }))
}

/// Checks the answer to a breakdown's question and records the result.
#[tauri::command]
pub fn db_check_breakdown_answer(app: tauri::AppHandle, id: i64, choice: usize) -> Result<CheckResult, String> {
    let conn = open_db(&app)?;
    let data: String = conn
        .query_row("SELECT data FROM sentence_breakdowns WHERE id = ?1", params![id], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let check = serde_json::from_str::<BreakdownData>(&data)
        .ok()
        .and_then(|d| d.check)
        .ok_or_else(|| app_error("breakdown_no_check", ""))?;
    let correct = choice == check.answer;
    conn.execute("UPDATE sentence_breakdowns SET checked = ?2 WHERE id = ?1", params![id, correct])
        .map_err(|e| e.to_string())?;
    Ok(CheckResult { correct, answer: check.answer, explanation: check.explanation })
}

#[tauri::command]
pub fn db_delete_sentence_breakdown(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute("DELETE FROM sentence_breakdowns WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_japanese_sentences() {
        let text = "体が動かないように、ベルトで固定する方法があります。この方法は、酔った人が手足を動かして、周りの人や自分を傷つけることを防ぐために使われる場合があります。ただし、長い時間続けることはできません。";
        let s = split_sentences(text);
        assert_eq!(s.len(), 3);
        assert!(s[1].starts_with("この方法は"));
        assert!(s[1].ends_with("あります。"));
    }

    #[test]
    fn keeps_quoted_speech_and_repeated_marks() {
        assert_eq!(split_sentences("「本当？」と聞いた。えっ！？そうなの"), vec!["「本当？」と聞いた。", "えっ！？", "そうなの"]);
    }

    #[test]
    fn splits_on_line_breaks_and_skips_empty_parts() {
        assert_eq!(split_sentences("一行目\n\n二行目。\n。"), vec!["一行目", "二行目。"]);
    }

    #[test]
    fn ascii_period_needs_a_space() {
        assert_eq!(split_sentences("Harganya 3.5 juta. Mahal sekali!"), vec!["Harganya 3.5 juta.", "Mahal sekali!"]);
    }

    fn chunk(text: &str, implied: bool) -> Chunk {
        Chunk { text: text.into(), role: "other".into(), label: String::new(), meaning: String::new(), implied }
    }

    #[test]
    fn chunks_must_spell_the_sentence() {
        let sentence = "駅前のラーメン屋に行ったけど、もう閉まっていた。";
        let ok = [chunk("駅前のラーメン屋に", false), chunk("(私は)", true), chunk("行ったけど", false), chunk("もう閉まっていた", false)];
        assert!(chunks_spell(&ok, sentence));
        let missing = [chunk("駅前のラーメン屋に", false), chunk("もう閉まっていた", false)];
        assert!(!chunks_spell(&missing, sentence));
        assert!(!chunks_spell(&[], sentence));
    }

    #[test]
    fn clean_drops_invalid_check_and_unknown_roles() {
        let raw = r#"{"chunks":[{"text":"この方法は","role":"theme"}],"check":{"question":"?","options":["a","b"],"answer":5}}"#;
        let data = clean(serde_json::from_str(raw).unwrap());
        assert_eq!(data.chunks[0].role, "other");
        assert!(data.check.is_none());
    }
}
