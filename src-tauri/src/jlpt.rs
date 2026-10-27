//! JLPT practice: the real test cut into short timed sessions, one section per session.
//!
//! Each level has a plan per section (`plan`): which 問題 types a session asks, how many questions of
//! each and the time limit, chosen so a session keeps roughly the real test's pace. Sessions follow a
//! rotation per level (`rotation`), so practising regularly covers the whole test.
//!
//! The AI writes each 問題 type in its own request (`part_prompt`); `clean_part` drops anything a
//! question can't safely use and shuffles the options. Answers and explanations stay in
//! `jlpt_sessions.data` until the learner hands the session in (`db_jlpt_submit`).
//! Spec: `PRDs/JLPT Practice.md`.

use crate::chat::{complete_explained, json_part};
use crate::db::open_db;
use crate::grammar_review::{set_setting, setting};
use crate::locale::{app_error, explain_lang, lang_name};
use rand::seq::SliceRandom;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const LEVELS: &[&str] = &["N5", "N4", "N3", "N2", "N1"];
pub const SECTIONS: &[&str] = &["vocab", "grammar", "reading", "listening"];

/// Parts built around one passage: every question of the part is about the same text.
const PASSAGE_PARTS: &[&str] = &["text_grammar", "short_text", "mid_text", "long_text", "integrated", "thematic", "info_search"];
const LISTENING_PARTS: &[&str] = &["task", "point", "summary", "utterance", "quick_response", "integrated_listening"];
/// Parts whose choices are only spoken in the real test, three of them.
const SPOKEN_CHOICE_PARTS: &[&str] = &["utterance", "quick_response"];
/// A late hand-in within this many seconds of the limit still counts as on time.
const GRACE_SECONDS: i64 = 10;

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
pub struct PartPlan {
    pub part: &'static str,
    pub count: usize,
}

#[derive(Serialize, Debug)]
pub struct SectionPlan {
    pub section: &'static str,
    pub parts: Vec<PartPlan>,
    pub questions: usize,
    /// Seconds for the whole session.
    pub time_limit: i64,
    /// Length of the real test's section, in minutes.
    pub real_minutes: i64,
    /// "grammar_reading" / "language_reading" when the real test times this section together with others, else "".
    pub real_block: &'static str,
}

/// What one session of a section asks at a level, or None for an unknown level or section.
pub fn plan(level: &str, section: &str) -> Option<SectionPlan> {
    if !LEVELS.contains(&level) {
        return None;
    }
    let (parts, time_limit): (&[(&'static str, usize)], i64) = match (section, level) {
        ("vocab", "N5") => (&[("kanji_reading", 3), ("orthography", 3), ("context", 2), ("paraphrase", 2)], 360),
        ("vocab", "N4" | "N3") => (&[("kanji_reading", 2), ("orthography", 2), ("context", 2), ("paraphrase", 2), ("usage", 2)], 480),
        ("vocab", "N2") => (
            &[("kanji_reading", 2), ("orthography", 1), ("word_formation", 1), ("context", 2), ("paraphrase", 2), ("usage", 2)],
            420,
        ),
        ("vocab", _) => (&[("kanji_reading", 2), ("context", 3), ("paraphrase", 2), ("usage", 3)], 420),
        ("grammar", _) => (
            &[("sentence_grammar", 5), ("sentence_order", 2), ("text_grammar", 3)],
            match level { "N5" => 420, "N1" => 540, _ => 480 },
        ),
        ("reading", "N5" | "N4") => (&[("short_text", 1), ("mid_text", 2), ("info_search", 1)], if level == "N5" { 600 } else { 720 }),
        ("reading", "N3") => (&[("short_text", 1), ("mid_text", 2), ("long_text", 2), ("info_search", 1)], 960),
        ("reading", "N2") => (&[("short_text", 1), ("mid_text", 2), ("integrated", 1), ("thematic", 1), ("info_search", 1)], 960),
        ("reading", _) => (
            &[("short_text", 1), ("mid_text", 1), ("long_text", 1), ("integrated", 1), ("thematic", 1), ("info_search", 1)],
            1080,
        ),
        ("listening", "N5" | "N4") => (&[("task", 1), ("point", 1), ("utterance", 1), ("quick_response", 2)], 360),
        ("listening", "N3") => (&[("task", 1), ("point", 1), ("summary", 1), ("utterance", 1), ("quick_response", 2)], 480),
        ("listening", _) => (&[("task", 1), ("point", 1), ("summary", 1), ("quick_response", 2), ("integrated_listening", 1)], 540),
        _ => return None,
    };
    let (real_minutes, real_block) = match (level, section) {
        ("N5", "vocab") => (20, ""),
        ("N4", "vocab") => (25, ""),
        ("N3", "vocab") => (30, ""),
        ("N5", "grammar" | "reading") => (40, "grammar_reading"),
        ("N4", "grammar" | "reading") => (55, "grammar_reading"),
        ("N3", "grammar" | "reading") => (70, "grammar_reading"),
        ("N2", "listening") => (50, ""),
        ("N1", "listening") => (55, ""),
        ("N2", _) => (105, "language_reading"),
        ("N1", _) => (110, "language_reading"),
        ("N5", _) => (30, ""),
        ("N4", _) => (35, ""),
        _ => (40, ""),
    };
    let parts: Vec<PartPlan> = parts.iter().map(|&(part, count)| PartPlan { part, count }).collect();
    Some(SectionPlan {
        section: SECTIONS.iter().find(|s| **s == section).copied().unwrap_or("vocab"),
        questions: parts.iter().map(|p| p.count).sum(),
        parts,
        time_limit,
        real_minutes,
        real_block,
    })
}

/// The order sessions come in. From N3 up, reading is a third of the score on its own, so it comes twice.
pub fn rotation(level: &str) -> &'static [&'static str] {
    if matches!(level, "N5" | "N4") {
        &["vocab", "grammar", "reading", "listening"]
    } else {
        &["vocab", "reading", "listening", "grammar", "reading"]
    }
}

// ---------------------------------------------------------------------------
// Generating questions
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StoredQuestion {
    pub stem: String,
    /// Listening: the lines of the recording, in Japanese.
    #[serde(default)]
    pub script: Vec<String>,
    pub options: Vec<String>,
    pub answer: usize,
    #[serde(default)]
    pub explanation: String,
    /// Vocabulary parts: the tested word, so a missed one can become a card.
    #[serde(default)]
    pub word: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StoredPart {
    pub part: String,
    #[serde(default)]
    pub passage: String,
    pub questions: Vec<StoredQuestion>,
}

#[derive(Deserialize)]
struct RawPart {
    #[serde(default)]
    passage: String,
    #[serde(default)]
    questions: Vec<RawQuestion>,
}

#[derive(Deserialize)]
struct RawQuestion {
    #[serde(default)]
    stem: String,
    #[serde(default)]
    script: Vec<String>,
    #[serde(default)]
    options: Vec<String>,
    #[serde(default)]
    answer: Option<usize>,
    #[serde(default)]
    explanation: String,
    #[serde(default)]
    word: String,
}

fn option_count(part: &str) -> usize {
    if SPOKEN_CHOICE_PARTS.contains(&part) { 3 } else { 4 }
}

/// Topics to spread questions over, one picked per request so sessions don't all sound alike.
const THEMES: &[&str] = &[
    "school and studying", "shopping", "travel and trains", "work and part-time jobs", "weather and seasons",
    "family and friends", "health and the doctor", "hobbies and sports", "food and restaurants", "the town and directions",
    "housing and neighbours", "events and festivals", "the post office and the bank", "the internet and phones",
];

fn passage_length(level: &str, part: &str) -> &'static str {
    match (part, level) {
        ("short_text", "N5") => "about 80 characters",
        ("short_text", "N4") => "about 120 characters",
        ("short_text", _) => "about 200 characters",
        ("mid_text", "N5") => "about 250 characters",
        ("mid_text", "N4" | "N3") => "about 350 characters",
        ("mid_text", _) => "about 500 characters",
        ("long_text", "N3") => "about 550 characters",
        ("long_text", _) => "about 800 characters",
        ("integrated", _) => "two texts of about 300 characters each",
        ("thematic", _) => "about 700 characters",
        ("text_grammar", "N5" | "N4") => "about 200 characters",
        ("text_grammar", _) => "about 350 characters",
        _ => "short",
    }
}

/// How the real test asks this 問題 type.
fn part_instruction(level: &str, part: &str, count: usize) -> String {
    let len = passage_length(level, part);
    let paraphrase_options = if matches!(level, "N5" | "N4") { "4 full sentences" } else { "4 words or short phrases" };
    match part {
        "kanji_reading" => format!(
            "漢字読み (kanji reading). {count} questions. stem: a short sentence with one word written in kanji, wrapped \
             in <u>…</u>. options: 4 hiragana readings; the wrong ones are the confusions learners make (long vowels, \
             voiced/unvoiced sounds, small っ, the other on/kun reading)."
        ),
        "orthography" => format!(
            "表記 (writing). {count} questions. stem: a short sentence with one word written in hiragana, wrapped in <u>…</u> \
             (also katakana words, written in hiragana). options: 4 ways to write it in kanji or katakana; the wrong ones \
             use look-alike kanji or a wrong katakana sound."
        ),
        "word_formation" => format!(
            "語形成 (word formation). {count} questions. stem: a sentence where a prefix, suffix or part of a compound is \
             missing, shown as （　　）. options: 4 such word parts (e.g. 不・無・未・非, or 〜的・〜性・〜化・〜者)."
        ),
        "context" => format!(
            "文脈規定 (word in context). {count} questions. stem: a sentence with （　　） where one word belongs. options: \
             4 words of the same kind (all verbs, all nouns, …); only one fits the context."
        ),
        "paraphrase" => format!(
            "言い換え類義 (paraphrase). {count} questions. stem: a sentence with a word or phrase wrapped in <u>…</u>. \
             options: {paraphrase_options}; only one has about the same meaning as the marked part."
        ),
        "usage" => format!(
            "用法 (usage). {count} questions. stem: just the tested word. options: 4 sentences that use the word; only one \
             uses it correctly, the others put it where a similar word belongs."
        ),
        "sentence_grammar" => format!(
            "文の文法1 (grammar form). {count} questions. stem: a sentence, or a short dialogue written 「A「…」<br>B「…」」, \
             with （　　）. options: 4 particles, grammar patterns or verb forms; only one is right."
        ),
        "sentence_order" => format!(
            "文の文法2 (sentence order). {count} questions. stem: a sentence with four blanks written ＿＿＿, one of them \
             written ★ instead, e.g. 「つくえの ＿＿＿ ＿＿＿ ★ ＿＿＿ あります。」. options: the 4 pieces that fill the \
             blanks, in mixed order. answer: the piece that goes in the ★ blank. The explanation gives the whole sentence."
        ),
        "text_grammar" => format!(
            "文章の文法 (text grammar). passage: a short essay or letter written by a learner, {len}, with {count} gaps \
             written 【1】, 【2】, … in order. {count} questions, one per gap in order; stem: just the gap, e.g. 【1】. \
             options: 4 words or phrases (conjunctions, grammar forms, words that refer back)."
        ),
        "short_text" => format!(
            "内容理解・短文 (short text). passage: a memo, notice, e-mail or short essay, {len}. {count} question(s) about \
             what it says or what someone must do."
        ),
        "mid_text" => format!(
            "内容理解・中文 (mid-length text). passage: an essay or explanation, {len}. {count} questions about reasons, \
             what a phrase refers to, and the writer's view."
        ),
        "long_text" => format!(
            "内容理解・長文 (long text). passage: an essay, {len}. {count} questions about its details and its main point."
        ),
        "integrated" => format!(
            "統合理解 (comparing texts). passage: {len} on the same question with different views; start them with \
             「A」 and 「B」 on their own lines. {count} question(s) comparing what A and B say."
        ),
        "thematic" => format!(
            "主張理解 (the writer's claim). passage: an opinion essay, {len}. {count} question(s) about what the writer \
             most wants to say."
        ),
        "info_search" => format!(
            "情報検索 (finding information). passage: information material such as a notice, timetable, price list or \
             guide, {len}, written as plain lines (line breaks and ・ bullets, no tables, no markdown). {count} question(s) \
             asking what someone with given conditions should choose, pay or do."
        ),
        "task" => format!(
            "課題理解 (task). {count} question(s). script: a narrator line with the situation and the question, then a \
             dialogue of 2 speakers (lines start with 男： or 女：), then the question again. The question asks what someone \
             will do, buy or bring next; the dialogue changes plans once. stem: the question. options: 4 short answers."
        ),
        "point" => format!(
            "ポイント理解 (key point). {count} question(s). script: a narrator line with the situation and the question, \
             then a dialogue or monologue (lines start with 男： or 女：), then the question again. The question asks for a \
             reason or one detail; other details are mentioned to distract. stem: the question. options: 4 short answers."
        ),
        "summary" => format!(
            "概要理解 (summary). {count} question(s). script: a narrator line with the situation only, a monologue or \
             dialogue, then the question (about the speaker's main point or what the talk is about). stem: the question. \
             options: 4 short answers."
        ),
        "integrated_listening" => format!(
            "統合理解 (integrated listening). {count} question(s). script: a longer conversation of 2-3 people comparing \
             several choices (e.g. which course or plan), then the question of which one they choose. stem: the question. \
             options: 4 short answers."
        ),
        "utterance" => format!(
            "発話表現 (utterance). {count} question(s). stem: the situation in Japanese, ending with 何と言いますか。 \
             script: the same situation line. options: 3 short things the person could say; only one fits."
        ),
        _ => format!(
            "即時応答 (quick response). {count} question(s). script: one short line someone says to the learner. stem: \"\". \
             options: 3 short replies; only one is a natural answer."
        ),
    }
}

fn part_prompt(level: &str, part: &str, count: usize, explain: &str, avoid: &[String], theme: &str) -> String {
    let native = lang_name(explain);
    let spacing = if matches!(level, "N5" | "N4") {
        " Like the real test at this level, put a space between phrases (分かち書き) in Japanese sentences."
    } else {
        ""
    };
    let script = if LISTENING_PARTS.contains(&part) { "\"script\": [Japanese lines of the recording], " } else { "" };
    let passage = if PASSAGE_PARTS.contains(&part) { "\"passage\": the Japanese text, " } else { "" };
    let avoid = if avoid.is_empty() { String::new() } else { format!("\nDo not test these words again: {}.", avoid.join("、")) };
    format!(
        "You write practice questions for the Japanese-Language Proficiency Test (JLPT) {level}. Match the real test: \
         its format, its difficulty, and only vocabulary, kanji and grammar of {level} and below.{spacing}\n\n\
         Part: {}\n\nUse everyday situations around: {theme}. Make the wrong options plausible: the mistakes {level} \
         learners really make. Exactly one option is right. Never refer to options by number in explanations.{avoid}\n\n\
         Return {{{passage}\"questions\": [{{\"stem\": ..., {script}\"options\": [{} options], \"answer\": index of the \
         right option (0-based), \"explanation\": one or two sentences in {native} on why it is right and why the most \
         tempting wrong option is wrong, \"word\": the tested word in its dictionary form for vocabulary questions, \
         else \"\"}}]}}.\nReturn ONLY that JSON, no markdown.",
        part_instruction(level, part, count),
        option_count(part),
    )
}

/// Parses one part's answer, drops questions a learner can't answer as asked and shuffles the options.
/// Fails when no usable question is left.
pub fn clean_part(part: &str, count: usize, raw: &str) -> Result<StoredPart, String> {
    let bad = || app_error("api_bad_response", raw.to_string());
    let parsed: RawPart = serde_json::from_str(json_part(raw)).map_err(|e| app_error("api_bad_response", format!("{}: {}", e, raw)))?;
    let passage = parsed.passage.trim().to_string();
    if PASSAGE_PARTS.contains(&part) && passage.is_empty() {
        return Err(bad());
    }
    let n = option_count(part);
    let mut rng = rand::thread_rng();
    let mut questions = Vec::new();
    for q in parsed.questions {
        let options: Vec<String> = q.options.iter().map(|o| o.trim().to_string()).collect();
        let Some(answer) = q.answer else { continue };
        let distinct = options.iter().enumerate().all(|(i, o)| !options[..i].contains(o));
        let stem = q.stem.trim().to_string();
        let script: Vec<String> = q.script.iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
        let usable = options.len() == n
            && answer < n
            && distinct
            && options.iter().all(|o| !o.is_empty())
            && match part {
                "kanji_reading" | "orthography" | "paraphrase" => stem.contains("<u>") && stem.contains("</u>"),
                "sentence_order" => stem.contains('★'),
                "quick_response" => true,
                _ => !stem.is_empty(),
            }
            && (!LISTENING_PARTS.contains(&part) || !script.is_empty());
        if !usable {
            continue;
        }
        let right = options[answer].clone();
        let mut options = options;
        options.shuffle(&mut rng);
        let answer = options.iter().position(|o| *o == right).unwrap_or(0);
        questions.push(StoredQuestion {
            stem,
            script,
            options,
            answer,
            explanation: q.explanation.trim().to_string(),
            word: q.word.trim().to_string(),
        });
        if questions.len() == count {
            break;
        }
    }
    if questions.is_empty() {
        return Err(bad());
    }
    Ok(StoredPart { part: part.to_string(), passage, questions })
}

/// Explanations, for the "came back in English" check.
fn learner_text(raw: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(json_part(raw)).unwrap_or(serde_json::Value::Null);
    v["questions"]
        .as_array()
        .map(|qs| qs.iter().filter_map(|q| q["explanation"].as_str()).collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Scores
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
pub struct Score {
    pub correct: i64,
    pub total: i64,
}

#[derive(Serialize, Debug)]
pub struct EstimatePart {
    /// "language_reading" (N5/N4), "language", "reading" (N3+), "listening".
    pub code: &'static str,
    pub score: i64,
    pub max: i64,
    /// The sectional minimum: below it the real test fails whatever the total.
    pub min: i64,
}

#[derive(Serialize, Debug)]
pub struct Estimate {
    pub parts: Vec<EstimatePart>,
    pub total: i64,
    pub max: i64,
    pub pass_mark: i64,
    pub passed: bool,
}

/// A rough score on the real test's scale from the latest result of each section, once every section has one.
pub fn estimate(level: &str, last: &HashMap<String, Score>) -> Option<Estimate> {
    let pct = |s: &str| last.get(s).filter(|x| x.total > 0).map(|x| x.correct as f64 / x.total as f64);
    let (vocab, grammar, reading, listening) = (pct("vocab")?, pct("grammar")?, pct("reading")?, pct("listening")?);
    let scale = |p: f64, max: i64| (p * max as f64).round() as i64;
    let parts = if matches!(level, "N5" | "N4") {
        vec![
            EstimatePart { code: "language_reading", score: scale((vocab + grammar + reading) / 3.0, 120), max: 120, min: 38 },
            EstimatePart { code: "listening", score: scale(listening, 60), max: 60, min: 19 },
        ]
    } else {
        vec![
            EstimatePart { code: "language", score: scale((vocab + grammar) / 2.0, 60), max: 60, min: 19 },
            EstimatePart { code: "reading", score: scale(reading, 60), max: 60, min: 19 },
            EstimatePart { code: "listening", score: scale(listening, 60), max: 60, min: 19 },
        ]
    };
    let pass_mark = match level { "N5" => 80, "N3" => 95, "N1" => 100, _ => 90 };
    let total = parts.iter().map(|p| p.score).sum();
    let passed = total >= pass_mark && parts.iter().all(|p| p.score >= p.min);
    Some(Estimate { parts, total, max: 180, pass_mark, passed })
}

#[derive(Serialize, Debug, PartialEq)]
pub struct PartScore {
    pub part: String,
    pub correct: i64,
    pub total: i64,
}

/// Right answers per part; a missing or blank answer counts as wrong.
pub fn grade(parts: &[StoredPart], answers: &[Option<usize>]) -> Vec<PartScore> {
    let mut i = 0;
    parts
        .iter()
        .map(|p| {
            let correct = p
                .questions
                .iter()
                .filter(|q| {
                    let right = answers.get(i).copied().flatten() == Some(q.answer);
                    i += 1;
                    right
                })
                .count() as i64;
            PartScore { part: p.part.clone(), correct, total: p.questions.len() as i64 }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn today() -> String {
    chrono::Local::now().date_naive().to_string()
}

fn rotation_key(level: &str) -> String {
    format!("jlpt_rotation:{}", level)
}

fn cycle_key(level: &str) -> String {
    format!("jlpt_cycle:{}", level)
}

fn check_level(level: &str) -> Result<(), String> {
    if LEVELS.contains(&level) { Ok(()) } else { Err(app_error("jlpt_bad_level", level)) }
}

/// Position in the rotation and how many rotations were completed before this one (starting at 1).
fn rotation_state(conn: &Connection, level: &str) -> (usize, i64) {
    let len = rotation(level).len();
    let next = setting(conn, &rotation_key(level)).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0) % len;
    let cycle = setting(conn, &cycle_key(level)).and_then(|v| v.parse::<i64>().ok()).unwrap_or(1).max(1);
    (next, cycle)
}

/// Latest finished result per section at a level.
fn last_scores(conn: &Connection, level: &str) -> Result<HashMap<String, Score>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT section, correct, total FROM jlpt_sessions s
             WHERE level = ?1 AND finished_at IS NOT NULL
               AND finished_at = (SELECT MAX(finished_at) FROM jlpt_sessions t WHERE t.level = s.level AND t.section = s.section)",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![level], |r| Ok((r.get::<_, String>(0)?, Score { correct: r.get(1)?, total: r.get(2)? })))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<HashMap<_, _>, _>>().map_err(|e| e.to_string())
}

/// A stored session, answers included.
struct Loaded {
    level: String,
    section: String,
    time_limit: i64,
    parts: Vec<StoredPart>,
    answers: Vec<Option<usize>>,
    started_at: Option<i64>,
    finished_at: Option<i64>,
}

fn load_session(conn: &Connection, id: i64) -> Result<Loaded, String> {
    let row = conn
        .query_row(
            "SELECT level, section, time_limit, data, answers, started_at, finished_at FROM jlpt_sessions WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| app_error("jlpt_session_gone", id.to_string()))?;
    let (level, section, time_limit, data, answers, started_at, finished_at) = row;
    Ok(Loaded {
        level,
        section,
        time_limit,
        parts: serde_json::from_str(&data).map_err(|e| e.to_string())?,
        answers: serde_json::from_str(&answers).unwrap_or_default(),
        started_at,
        finished_at,
    })
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct RotationStep {
    pub section: &'static str,
    /// "done" (earlier in this rotation), "next" or "upcoming".
    pub state: &'static str,
    pub last: Option<Score>,
}

#[derive(Serialize)]
pub struct JlptOverview {
    pub level: String,
    pub rotation: Vec<RotationStep>,
    pub next_section: &'static str,
    pub cycle: i64,
    pub plans: Vec<SectionPlan>,
    pub done_today: bool,
    /// The last 7 days, oldest first: whether a session was finished that day.
    pub week: Vec<bool>,
    pub estimate: Option<Estimate>,
    /// A started session that wasn't handed in (the app was closed), to resume or hand in.
    pub open_session: Option<i64>,
}

#[tauri::command]
pub fn db_jlpt_overview(app: tauri::AppHandle, level: String) -> Result<JlptOverview, String> {
    check_level(&level)?;
    let conn = open_db(&app)?;
    let (next, cycle) = rotation_state(&conn, &level);
    let last = last_scores(&conn, &level)?;
    let steps = rotation(&level)
        .iter()
        .enumerate()
        .map(|(i, &section)| RotationStep {
            section,
            state: if i == next { "next" } else if i < next { "done" } else { "upcoming" },
            last: last.get(section).copied(),
        })
        .collect();
    let days: Vec<String> = conn
        .prepare("SELECT DISTINCT finished_on FROM jlpt_sessions WHERE finished_on >= date('now', 'localtime', '-6 days')")
        .and_then(|mut s| s.query_map([], |r| r.get::<_, String>(0))?.collect())
        .map_err(|e| e.to_string())?;
    let local_today = chrono::Local::now().date_naive();
    let week = (0..7)
        .rev()
        .map(|back| days.contains(&(local_today - chrono::Duration::days(back)).to_string()))
        .collect::<Vec<_>>();
    let open_session = conn
        .query_row(
            "SELECT id FROM jlpt_sessions WHERE level = ?1 AND started_at IS NOT NULL AND finished_at IS NULL ORDER BY started_at DESC LIMIT 1",
            params![level],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(JlptOverview {
        rotation: steps,
        next_section: rotation(&level)[next],
        cycle,
        plans: SECTIONS.iter().filter_map(|s| plan(&level, s)).collect(),
        done_today: *week.last().unwrap_or(&false),
        week,
        estimate: estimate(&level, &last),
        open_session,
        level,
    })
}

#[derive(Serialize)]
pub struct QuestionView {
    pub stem: String,
    pub script: Vec<String>,
    pub options: Vec<String>,
}

#[derive(Serialize)]
pub struct PartView {
    pub part: String,
    pub passage: String,
    pub questions: Vec<QuestionView>,
}

/// A session as the page gets it: no answers, no explanations.
#[derive(Serialize)]
pub struct SessionView {
    pub id: i64,
    pub level: String,
    pub section: String,
    pub time_limit: i64,
    /// Milliseconds since the epoch, once the clock runs.
    pub started_at: Option<i64>,
    pub answers: Vec<Option<usize>>,
    pub parts: Vec<PartView>,
}

fn session_view(conn: &Connection, id: i64) -> Result<SessionView, String> {
    let Loaded { level, section, time_limit, parts, answers, started_at, .. } = load_session(conn, id)?;
    let parts = parts
        .into_iter()
        .map(|p| PartView {
            part: p.part,
            passage: p.passage,
            questions: p.questions.into_iter().map(|q| QuestionView { stem: q.stem, script: q.script, options: q.options }).collect(),
        })
        .collect();
    Ok(SessionView { id, level, section, time_limit, started_at, answers, parts })
}

/// Words tested in the latest sessions of a level, so new questions pick other ones.
fn recent_words(conn: &Connection, level: &str) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare("SELECT data FROM jlpt_sessions WHERE level = ?1 ORDER BY id DESC LIMIT 6") else { return vec![] };
    let Ok(rows) = stmt.query_map(params![level], |r| r.get::<_, String>(0)) else { return vec![] };
    let mut words: Vec<String> = rows
        .flatten()
        .filter_map(|d| serde_json::from_str::<Vec<StoredPart>>(&d).ok())
        .flatten()
        .flat_map(|p| p.questions.into_iter().map(|q| q.word))
        .filter(|w| !w.is_empty())
        .collect();
    words.dedup();
    words.truncate(40);
    words
}

/// Writes a session of a section: one AI request per 問題 type, all at once. A session written
/// earlier and never started is handed back instead, so leaving the page before starting costs nothing.
#[tauri::command]
pub async fn llm_jlpt_prepare(
    app: tauri::AppHandle,
    api_host: String,
    api_key: String,
    model: String,
    level: String,
    section: String,
) -> Result<SessionView, String> {
    check_level(&level)?;
    let plan = plan(&level, &section).ok_or_else(|| app_error("jlpt_bad_section", section.clone()))?;
    let (explain, avoid) = {
        let conn = open_db(&app)?;
        let explain = explain_lang(&conn);
        let waiting: Option<i64> = conn
            .query_row(
                "SELECT id FROM jlpt_sessions WHERE level = ?1 AND section = ?2 AND explain_lang = ?3 AND started_at IS NULL
                 ORDER BY id DESC LIMIT 1",
                params![level, section, explain],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(id) = waiting {
            return session_view(&conn, id);
        }
        (explain, recent_words(&conn, &level))
    };
    let mut tasks = Vec::new();
    for p in &plan.parts {
        let theme = THEMES.choose(&mut rand::thread_rng()).copied().unwrap_or("daily life");
        let prompt = part_prompt(&level, p.part, p.count, &explain, &avoid, theme);
        let (host, key, model, explain, part, count) = (api_host.clone(), api_key.clone(), model.clone(), explain.clone(), p.part, p.count);
        tasks.push(tauri::async_runtime::spawn(async move {
            let raw = complete_explained(&host, &key, &model, &prompt, &explain, learner_text).await?;
            clean_part(part, count, &raw)
        }));
    }
    let mut parts = Vec::new();
    for task in tasks {
        parts.push(task.await.map_err(|e| e.to_string())??);
    }
    let conn = open_db(&app)?;
    let data = serde_json::to_string(&parts).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO jlpt_sessions(level, section, explain_lang, data, time_limit) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![level, section, explain, data, plan.time_limit],
    )
    .map_err(|e| e.to_string())?;
    session_view(&conn, conn.last_insert_rowid())
}

/// Starts the clock (once). Returns the session with its start time.
#[tauri::command]
pub fn db_jlpt_begin(app: tauri::AppHandle, id: i64) -> Result<SessionView, String> {
    let conn = open_db(&app)?;
    conn.execute("UPDATE jlpt_sessions SET started_at = ?2 WHERE id = ?1 AND started_at IS NULL", params![id, now_ms()])
        .map_err(|e| e.to_string())?;
    session_view(&conn, id)
}

#[tauri::command]
pub fn db_jlpt_get_session(app: tauri::AppHandle, id: i64) -> Result<SessionView, String> {
    session_view(&open_db(&app)?, id)
}

/// Keeps the answers so far, so a session survives the app being closed.
#[tauri::command]
pub fn db_jlpt_save_answers(app: tauri::AppHandle, id: i64, answers: Vec<Option<usize>>) -> Result<(), String> {
    let conn = open_db(&app)?;
    let json = serde_json::to_string(&answers).map_err(|e| e.to_string())?;
    conn.execute("UPDATE jlpt_sessions SET answers = ?2 WHERE id = ?1 AND finished_at IS NULL", params![id, json])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Serialize)]
pub struct ReviewedQuestion {
    pub stem: String,
    pub script: Vec<String>,
    pub options: Vec<String>,
    pub answer: usize,
    pub chosen: Option<usize>,
    pub explanation: String,
    pub word: String,
}

#[derive(Serialize)]
pub struct ReviewedPart {
    pub part: String,
    pub passage: String,
    pub correct: i64,
    pub total: i64,
    pub questions: Vec<ReviewedQuestion>,
}

#[derive(Serialize)]
pub struct JlptResult {
    pub id: i64,
    pub level: String,
    pub section: String,
    pub correct: i64,
    pub total: i64,
    pub answered: i64,
    /// Seconds.
    pub time_used: i64,
    pub time_limit: i64,
    pub timed_out: bool,
    /// Milliseconds since the epoch.
    pub finished_at: i64,
    /// The section's previous result at this level.
    pub prev: Option<Score>,
    pub parts: Vec<ReviewedPart>,
    /// Missed vocabulary, to add as cards.
    pub missed_words: Vec<String>,
    pub next_section: &'static str,
    pub cycle: i64,
}

/// Hands a session in: grades it, moves the rotation on when this was the section due, and returns
/// every question with its answer and explanation. Handing in again returns the same result.
#[tauri::command]
pub fn db_jlpt_submit(app: tauri::AppHandle, id: i64, answers: Vec<Option<usize>>, timed_out: bool) -> Result<JlptResult, String> {
    let conn = open_db(&app)?;
    let Loaded { level, section, time_limit, parts, started_at, finished_at, .. } = load_session(&conn, id)?;
    if finished_at.is_none() {
        let scores = grade(&parts, &answers);
        let now = now_ms();
        let elapsed = ((now - started_at.unwrap_or(now)) / 1000).max(0);
        let late = elapsed > time_limit + GRACE_SECONDS;
        let json = serde_json::to_string(&answers).map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE jlpt_sessions SET answers = ?2, finished_at = ?3, finished_on = ?4, correct = ?5, total = ?6, time_used = ?7, timed_out = ?8
             WHERE id = ?1",
            params![
                id,
                json,
                now,
                today(),
                scores.iter().map(|s| s.correct).sum::<i64>(),
                scores.iter().map(|s| s.total).sum::<i64>(),
                elapsed.min(time_limit),
                timed_out || late
            ],
        )
        .map_err(|e| e.to_string())?;
        let rot = rotation(&level);
        let (next, cycle) = rotation_state(&conn, &level);
        if rot[next] == section {
            let moved = (next + 1) % rot.len();
            set_setting(&conn, &rotation_key(&level), &moved.to_string())?;
            if moved == 0 {
                set_setting(&conn, &cycle_key(&level), &(cycle + 1).to_string())?;
            }
        }
        conn.execute(
            "INSERT INTO user_activity(date, activity_type, count) VALUES (?1, 'jlpt', 1)
             ON CONFLICT(date, activity_type) DO UPDATE SET count = count + 1",
            params![today()],
        )
        .map_err(|e| e.to_string())?;
    }
    result_of(&conn, id)
}

/// A finished session as the results screen shows it: every question with its answer and explanation.
fn result_of(conn: &Connection, id: i64) -> Result<JlptResult, String> {
    let Loaded { level, section, time_limit, parts, answers, finished_at, .. } = load_session(conn, id)?;
    let finished_at = finished_at.ok_or_else(|| app_error("jlpt_not_finished", id.to_string()))?;
    let (time_used, timed_out): (i64, bool) = conn
        .query_row("SELECT time_used, timed_out FROM jlpt_sessions WHERE id = ?1", params![id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| e.to_string())?;
    // The same section's result before this one.
    let prev = conn
        .query_row(
            "SELECT correct, total FROM jlpt_sessions
             WHERE level = ?1 AND section = ?2 AND finished_at < ?3 ORDER BY finished_at DESC LIMIT 1",
            params![level, section, finished_at],
            |r| Ok(Score { correct: r.get(0)?, total: r.get(1)? }),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let scores = grade(&parts, &answers);
    let correct: i64 = scores.iter().map(|s| s.correct).sum();
    let total: i64 = scores.iter().map(|s| s.total).sum();
    let answered = answers.iter().filter(|a| a.is_some()).count() as i64;
    let mut i = 0;
    let mut missed_words = Vec::new();
    let reviewed = parts
        .into_iter()
        .zip(scores)
        .map(|(p, s)| ReviewedPart {
            part: p.part,
            passage: p.passage,
            correct: s.correct,
            total: s.total,
            questions: p
                .questions
                .into_iter()
                .map(|q| {
                    let chosen = answers.get(i).copied().flatten();
                    i += 1;
                    if chosen != Some(q.answer) && !q.word.is_empty() && !missed_words.contains(&q.word) {
                        missed_words.push(q.word.clone());
                    }
                    ReviewedQuestion { stem: q.stem, script: q.script, options: q.options, answer: q.answer, chosen, explanation: q.explanation, word: q.word }
                })
                .collect(),
        })
        .collect();
    let (next, cycle) = rotation_state(conn, &level);
    Ok(JlptResult {
        id,
        next_section: rotation(&level)[next],
        cycle,
        section,
        level,
        correct,
        total,
        answered,
        time_used,
        time_limit,
        timed_out,
        finished_at,
        prev,
        parts: reviewed,
        missed_words,
    })
}

/// A past session's full review, for the history list.
#[tauri::command]
pub fn db_jlpt_result(app: tauri::AppHandle, id: i64) -> Result<JlptResult, String> {
    result_of(&open_db(&app)?, id)
}

// ---------------------------------------------------------------------------
// History and stats
// ---------------------------------------------------------------------------

#[derive(Serialize, Debug, PartialEq)]
pub struct HistoryItem {
    pub id: i64,
    pub level: String,
    pub section: String,
    pub correct: i64,
    pub total: i64,
    pub time_used: i64,
    pub time_limit: i64,
    pub timed_out: bool,
    /// Milliseconds since the epoch.
    pub finished_at: i64,
}

fn history(conn: &Connection, level: Option<&str>, limit: i64) -> Result<Vec<HistoryItem>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, level, section, correct, total, time_used, time_limit, timed_out, finished_at FROM jlpt_sessions
             WHERE finished_at IS NOT NULL AND (?1 IS NULL OR level = ?1)
             ORDER BY finished_at DESC, id DESC LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![level, limit], |r| {
            Ok(HistoryItem {
                id: r.get(0)?,
                level: r.get(1)?,
                section: r.get(2)?,
                correct: r.get(3)?,
                total: r.get(4)?,
                time_used: r.get(5)?,
                time_limit: r.get(6)?,
                timed_out: r.get(7)?,
                finished_at: r.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Finished sessions, newest first; all levels when `level` is None.
#[tauri::command]
pub fn db_jlpt_history(app: tauri::AppHandle, level: Option<String>, limit: Option<i64>) -> Result<Vec<HistoryItem>, String> {
    history(&open_db(&app)?, level.as_deref(), limit.unwrap_or(200).clamp(1, 500))
}

#[derive(Serialize, Debug, PartialEq)]
pub struct SectionStats {
    pub section: &'static str,
    pub sessions: i64,
    /// Share of right answers over the period, 0-100; None without sessions.
    pub accuracy: Option<i64>,
}

/// Accuracy per section on one day (0-100), for the trend chart; None where that section wasn't practised.
#[derive(Serialize, Debug, PartialEq)]
pub struct DayStats {
    pub date: String,
    pub vocab: Option<i64>,
    pub grammar: Option<i64>,
    pub reading: Option<i64>,
    pub listening: Option<i64>,
}

#[derive(Serialize, Debug)]
pub struct LevelEstimate {
    pub level: String,
    pub estimate: Estimate,
}

#[derive(Serialize, Debug)]
pub struct JlptStats {
    pub sessions: i64,
    /// Share of sessions handed in before the time ran out, 0-100.
    pub in_time: Option<i64>,
    pub sections: Vec<SectionStats>,
    pub days: Vec<DayStats>,
    /// Estimated score per level, for levels with a result in every section (all time, not just the period).
    pub estimates: Vec<LevelEstimate>,
}

fn pct(correct: i64, total: i64) -> Option<i64> {
    (total > 0).then(|| ((correct as f64 / total as f64) * 100.0).round() as i64)
}

fn stats(conn: &Connection, date_from: &str, date_to: &str) -> Result<JlptStats, String> {
    let range = "finished_at IS NOT NULL AND finished_on >= ?1 AND finished_on <= ?2";
    let (sessions, timed_out): (i64, i64) = conn
        .query_row(
            &format!("SELECT COUNT(*), COALESCE(SUM(timed_out), 0) FROM jlpt_sessions WHERE {range}"),
            params![date_from, date_to],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let mut per_section: HashMap<String, (i64, i64, i64)> = HashMap::new();
    let mut per_day: Vec<(String, String, i64, i64)> = Vec::new();
    {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT finished_on, section, COUNT(*), SUM(correct), SUM(total) FROM jlpt_sessions WHERE {range}
                 GROUP BY finished_on, section ORDER BY finished_on"
            ))
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![date_from, date_to], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?))
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (day, section, n, correct, total) = row.map_err(|e| e.to_string())?;
            let e = per_section.entry(section.clone()).or_default();
            e.0 += n;
            e.1 += correct;
            e.2 += total;
            per_day.push((day, section, correct, total));
        }
    }
    let sections = SECTIONS
        .iter()
        .map(|&section| {
            let (n, correct, total) = per_section.get(section).copied().unwrap_or_default();
            SectionStats { section, sessions: n, accuracy: pct(correct, total) }
        })
        .collect();
    let mut days: Vec<DayStats> = Vec::new();
    for (day, section, correct, total) in per_day {
        if days.last().map(|d| d.date != day).unwrap_or(true) {
            days.push(DayStats { date: day.clone(), vocab: None, grammar: None, reading: None, listening: None });
        }
        let d = days.last_mut().expect("pushed above");
        let value = pct(correct, total);
        match section.as_str() {
            "vocab" => d.vocab = value,
            "grammar" => d.grammar = value,
            "reading" => d.reading = value,
            "listening" => d.listening = value,
            _ => {}
        }
    }
    let mut estimates = Vec::new();
    for level in LEVELS {
        if let Some(estimate) = estimate(level, &last_scores(conn, level)?) {
            estimates.push(LevelEstimate { level: level.to_string(), estimate });
        }
    }
    Ok(JlptStats {
        sessions,
        in_time: pct(sessions - timed_out, sessions),
        sections,
        days,
        estimates,
    })
}

/// JLPT practice for the stats dashboard: sessions, accuracy per section and per day in the period.
#[tauri::command]
pub fn db_jlpt_stats(app: tauri::AppHandle, date_from: String, date_to: String) -> Result<JlptStats, String> {
    stats(&open_db(&app)?, &date_from, &date_to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_level_has_a_plan_for_every_section() {
        for level in LEVELS {
            for section in SECTIONS {
                let p = plan(level, section).unwrap();
                assert_eq!(p.questions, p.parts.iter().map(|x| x.count).sum::<usize>());
                assert!(p.questions >= 4 && p.questions <= 10, "{level} {section}");
                assert!(p.time_limit > 0 && p.time_limit < p.real_minutes * 60);
            }
        }
        assert!(plan("N6", "vocab").is_none());
        assert!(plan("N5", "kanji").is_none());
    }

    #[test]
    fn rotation_covers_every_section() {
        for level in LEVELS {
            for section in SECTIONS {
                assert!(rotation(level).contains(section));
            }
        }
        assert_eq!(rotation("N3").iter().filter(|s| **s == "reading").count(), 2);
    }

    #[test]
    fn every_part_has_a_prompt() {
        for level in LEVELS {
            for section in SECTIONS {
                for p in plan(level, section).unwrap().parts {
                    let prompt = part_prompt(level, p.part, p.count, "id", &[], "shopping");
                    assert!(prompt.contains(level) && prompt.contains("Indonesian"));
                }
            }
        }
    }

    #[test]
    fn clean_part_keeps_the_answer_after_shuffling() {
        let raw = r#"{"questions":[{"stem":"この <u>川</u>は きれいです。","options":["かわ","やま","いけ","うみ"],"answer":0,"explanation":"x","word":"川"}]}"#;
        for _ in 0..20 {
            let p = clean_part("kanji_reading", 3, raw).unwrap();
            let q = &p.questions[0];
            assert_eq!(q.options[q.answer], "かわ");
            assert_eq!(q.word, "川");
        }
    }

    #[test]
    fn clean_part_drops_unusable_questions() {
        let raw = r#"```json
        {"questions":[
          {"stem":"no underline","options":["a","b","c","d"],"answer":0},
          {"stem":"<u>x</u>","options":["a","a","c","d"],"answer":0},
          {"stem":"<u>x</u>","options":["a","b","c"],"answer":0},
          {"stem":"<u>x</u>","options":["a","b","c","d"],"answer":4},
          {"stem":"<u>x</u>","options":["a","b","c","d"]},
          {"stem":"<u>ok</u>","options":["a","b","c","d"],"answer":2}
        ]}```"#;
        let p = clean_part("kanji_reading", 3, raw).unwrap();
        assert_eq!(p.questions.len(), 1);
        assert_eq!(p.questions[0].stem, "<u>ok</u>");
        assert!(clean_part("kanji_reading", 3, r#"{"questions":[]}"#).is_err());
    }

    #[test]
    fn clean_part_checks_part_shapes() {
        let order = r#"{"questions":[{"stem":"A ＿＿＿ ★ ＿＿＿ ＿＿＿ B","options":["a","b","c","d"],"answer":1}]}"#;
        assert!(clean_part("sentence_order", 2, order).is_ok());
        assert!(clean_part("sentence_order", 2, &order.replace('★', "＿＿＿")).is_err());
        let listening = r#"{"questions":[{"stem":"","script":["お国は どちらですか。"],"options":["a","b","c"],"answer":0}]}"#;
        assert!(clean_part("quick_response", 2, listening).is_ok());
        assert!(clean_part("quick_response", 2, &listening.replace("\"お国は どちらですか。\"", "")).is_err());
        let no_passage = r#"{"passage":"","questions":[{"stem":"?","options":["a","b","c","d"],"answer":0}]}"#;
        assert!(clean_part("short_text", 1, no_passage).is_err());
        assert!(clean_part("short_text", 1, &no_passage.replace("\"passage\":\"\"", "\"passage\":\"本文\"")).is_ok());
    }

    #[test]
    fn clean_part_stops_at_the_planned_count() {
        let q = r#"{"stem":"（　　）","options":["a","b","c","d"],"answer":0}"#;
        let raw = format!(r#"{{"questions":[{q},{q},{q}]}}"#);
        assert_eq!(clean_part("context", 2, &raw).unwrap().questions.len(), 2);
    }

    fn part(answers: &[usize]) -> StoredPart {
        StoredPart {
            part: "context".into(),
            passage: String::new(),
            questions: answers
                .iter()
                .map(|&a| StoredQuestion { stem: "s".into(), script: vec![], options: vec!["a".into(); 4], answer: a, explanation: String::new(), word: String::new() })
                .collect(),
        }
    }

    #[test]
    fn grades_per_part_with_blanks_wrong() {
        let parts = [part(&[0, 1]), part(&[2, 3, 0])];
        let scores = grade(&parts, &[Some(0), None, Some(2), Some(1)]);
        assert_eq!(scores[0], PartScore { part: "context".into(), correct: 1, total: 2 });
        assert_eq!(scores[1], PartScore { part: "context".into(), correct: 1, total: 3 });
    }

    #[test]
    fn estimate_needs_every_section_and_uses_the_level_scale() {
        let mut last = HashMap::new();
        last.insert("vocab".to_string(), Score { correct: 7, total: 10 });
        last.insert("grammar".to_string(), Score { correct: 6, total: 10 });
        last.insert("reading".to_string(), Score { correct: 3, total: 4 });
        assert!(estimate("N5", &last).is_none());
        last.insert("listening".to_string(), Score { correct: 3, total: 5 });
        let n5 = estimate("N5", &last).unwrap();
        assert_eq!(n5.parts.len(), 2);
        assert_eq!(n5.parts[0].score, 82); // (0.7 + 0.6 + 0.75) / 3 × 120
        assert_eq!(n5.total, 82 + 36);
        assert!(n5.passed);
        let n3 = estimate("N3", &last).unwrap();
        assert_eq!(n3.parts.len(), 3);
        assert_eq!(n3.pass_mark, 95);
        last.insert("listening".to_string(), Score { correct: 1, total: 5 });
        assert!(!estimate("N5", &last).unwrap().passed, "listening below its minimum fails");
    }

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::prepare_db(&conn).unwrap();
        conn
    }

    /// A finished session with one 4-option question per answer, `right` of them answered right.
    fn finished(conn: &Connection, level: &str, section: &str, day: &str, at: i64, right: usize, of: usize, timed_out: bool) -> i64 {
        let parts = vec![part(&vec![0; of])];
        let answers: Vec<Option<usize>> = (0..of).map(|i| Some(if i < right { 0 } else { 1 })).collect();
        conn.execute(
            "INSERT INTO jlpt_sessions(level, section, explain_lang, data, time_limit, answers, started_at, finished_at, finished_on, correct, total, time_used, timed_out)
             VALUES (?1, ?2, 'en', ?3, 360, ?4, ?5, ?5, ?6, ?7, ?8, 100, ?9)",
            params![level, section, serde_json::to_string(&parts).unwrap(), serde_json::to_string(&answers).unwrap(), at, day, right as i64, of as i64, timed_out],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn history_is_newest_first_and_filters_by_level() {
        let conn = db();
        finished(&conn, "N5", "vocab", "2026-10-20", 1_000, 7, 10, false);
        finished(&conn, "N4", "grammar", "2026-10-21", 2_000, 5, 10, false);
        let newest = finished(&conn, "N5", "reading", "2026-10-22", 3_000, 3, 4, true);
        conn.execute("INSERT INTO jlpt_sessions(level, section, explain_lang, data, time_limit) VALUES ('N5', 'vocab', 'en', '[]', 360)", []).unwrap();
        let all = history(&conn, None, 50).unwrap();
        assert_eq!(all.len(), 3, "unfinished sessions are left out");
        assert_eq!(all[0].id, newest);
        assert!(all[0].timed_out);
        assert_eq!(history(&conn, Some("N5"), 50).unwrap().len(), 2);
        assert_eq!(history(&conn, Some("N5"), 1).unwrap().len(), 1);
    }

    #[test]
    fn result_of_compares_with_the_same_sections_previous_result() {
        let conn = db();
        finished(&conn, "N5", "vocab", "2026-10-20", 1_000, 6, 10, false);
        finished(&conn, "N5", "grammar", "2026-10-21", 2_000, 9, 10, false);
        let second = finished(&conn, "N5", "vocab", "2026-10-22", 3_000, 8, 10, false);
        finished(&conn, "N5", "vocab", "2026-10-23", 4_000, 2, 10, false);
        let r = result_of(&conn, second).unwrap();
        assert_eq!((r.correct, r.total, r.answered), (8, 10, 10));
        assert_eq!(r.prev, Some(Score { correct: 6, total: 10 }), "the earlier vocab result, not the later one or grammar");
        assert_eq!(r.finished_at, 3_000);
        let unfinished = { conn.execute("INSERT INTO jlpt_sessions(level, section, explain_lang, data, time_limit) VALUES ('N5', 'vocab', 'en', '[]', 360)", []).unwrap(); conn.last_insert_rowid() };
        assert!(result_of(&conn, unfinished).is_err());
    }

    #[test]
    fn stats_cover_the_period_per_section_and_day() {
        let conn = db();
        finished(&conn, "N5", "vocab", "2026-10-01", 500, 10, 10, false); // before the period
        finished(&conn, "N5", "vocab", "2026-10-20", 1_000, 6, 10, false);
        finished(&conn, "N5", "vocab", "2026-10-20", 1_100, 8, 10, true);
        finished(&conn, "N5", "listening", "2026-10-21", 2_000, 3, 5, false);
        let s = stats(&conn, "2026-10-15", "2026-10-27").unwrap();
        assert_eq!(s.sessions, 3);
        assert_eq!(s.in_time, Some(67));
        let vocab = s.sections.iter().find(|x| x.section == "vocab").unwrap();
        assert_eq!((vocab.sessions, vocab.accuracy), (2, Some(70)));
        let grammar = s.sections.iter().find(|x| x.section == "grammar").unwrap();
        assert_eq!((grammar.sessions, grammar.accuracy), (0, None));
        assert_eq!(s.days.len(), 2);
        assert_eq!(s.days[0], DayStats { date: "2026-10-20".into(), vocab: Some(70), grammar: None, reading: None, listening: None });
        assert_eq!(s.days[1].listening, Some(60));
        assert!(s.estimates.is_empty(), "no level has a result in every section yet");
        finished(&conn, "N5", "grammar", "2026-10-22", 3_000, 6, 10, false);
        finished(&conn, "N5", "reading", "2026-10-22", 3_100, 3, 4, false);
        let s = stats(&conn, "2026-10-15", "2026-10-27").unwrap();
        assert_eq!(s.estimates.len(), 1);
        assert_eq!(s.estimates[0].level, "N5");
        let empty = stats(&conn, "2027-01-01", "2027-01-31").unwrap();
        assert_eq!((empty.sessions, empty.in_time, empty.days.len()), (0, None, 0));
    }
}
