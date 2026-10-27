//! Grammar review: grammar patterns as review cards in the same daily queue as words.
//!
//! The learner picks a level per target language. Patterns of that level enter the deck as new
//! cards (intro, then questions); patterns of lower levels enter as level checks (one fill-the-gap
//! question: right → known, wrong → learned like a new card). Higher levels wait until the learner
//! moves up or adds a pattern themselves. Each grammar card's back is AI-made practice material
//! (meaning, form, example sentences with wrong fillers, a sentence with a mistake), and the
//! questions in `quiz.rs` are built from it.

use crate::chat::{complete_explained, json_part};
use crate::db::open_db;
use crate::locale::{app_error, lang_name};
use crate::words::generate_detail_text;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// `review_items.item_type` of a grammar card (words are `word`).
pub const ITEM_GRAMMAR: &str = "grammar";

const DEFAULT_NEW_PER_DAY: i64 = 3;
const DEFAULT_CHECKS_PER_DAY: i64 = 5;
const MAX_NEW_PER_DAY: i64 = 20;
const MAX_CHECKS_PER_DAY: i64 = 50;
/// Due grammar reviews kept in today's queue ahead of words, so word reviews can't crowd them out.
/// They come out of the review limit; more due grammar waits for a later day.
pub const GRAMMAR_REVIEWS_PER_DAY: i64 = 3;
/// Share of the learner's level that must be known before moving up is suggested.
const MOVE_UP_PCT: i64 = 80;
/// With "speed up", checks go twice as fast while at least this many of the last `SPEED_UP_SAMPLE` passed.
const SPEED_UP_SAMPLE: usize = 10;
const SPEED_UP_PASSED: usize = 9;

/// Grammar rows that are patterns of their own; translations point at one of these via `source_id`.
const ORIGINAL_ROWS: &str = "g.ai_translated = 0 AND g.level != ''";

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

pub(crate) fn setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM app_settings WHERE key = ?1", params![key], |r| r.get(0))
        .optional()
        .ok()
        .flatten()
}

pub(crate) fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO app_settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn level_key(language: &str) -> String {
    format!("grammar_level:{}", language)
}

struct Limits {
    new_per_day: i64,
    checks_per_day: i64,
    speed_up: bool,
}

fn limits(conn: &Connection) -> Limits {
    let int = |key: &str, default: i64| setting(conn, key).and_then(|v| v.parse().ok()).unwrap_or(default);
    Limits {
        new_per_day: int("grammar_new_per_day", DEFAULT_NEW_PER_DAY),
        checks_per_day: int("grammar_checks_per_day", DEFAULT_CHECKS_PER_DAY),
        speed_up: setting(conn, "grammar_speed_up").as_deref() != Some("0"),
    }
}

// ---------------------------------------------------------------------------
// Levels
// ---------------------------------------------------------------------------

/// JLPT counts down from N5 (easiest) to N1; other scales (CEFR A1, A2, …) sort as written.
fn level_rank(level: &str) -> (u8, i64, String) {
    match level.strip_prefix('N').and_then(|n| n.parse::<i64>().ok()) {
        Some(n) => (0, -n, String::new()),
        None => (1, 0, level.to_string()),
    }
}

/// The grammar levels of a language, easiest first.
pub fn level_order(conn: &Connection, language: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT DISTINCT level FROM grammar_items WHERE language = ?1 AND level != ''")
        .map_err(|e| e.to_string())?;
    let mut levels: Vec<String> = stmt
        .query_map(params![language], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    levels.sort_by_key(|l| level_rank(l));
    Ok(levels)
}

/// The learner's level for a language; the easiest level until they pick one.
fn current_level(conn: &Connection, language: &str, levels: &[String]) -> Option<String> {
    setting(conn, &level_key(language))
        .filter(|l| levels.contains(l))
        .or_else(|| levels.first().cloned())
}

fn role(levels: &[String], current: Option<&str>, level: &str) -> &'static str {
    let at = |l: &str| levels.iter().position(|x| x == l);
    match (at(level), current.and_then(at)) {
        (Some(i), Some(c)) if i < c => "below",
        (Some(i), Some(c)) if i == c => "current",
        (Some(_), Some(_)) => "above",
        _ => "other",
    }
}

// ---------------------------------------------------------------------------
// Progress per level
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct LevelProgress {
    pub level: String,
    /// `below` (checked), `current` (taught as new) or `above` (later).
    pub role: &'static str,
    pub total: i64,
    pub known: i64,
    pub learning: i64,
    pub pct: i64,
}

#[derive(Serialize)]
pub struct GrammarPlan {
    pub language: String,
    pub level: Option<String>,
    pub levels: Vec<LevelProgress>,
    pub new_per_day: i64,
    pub checks_per_day: i64,
    pub speed_up: bool,
    /// Level checks per day right now: doubled with "speed up" while the learner keeps passing.
    pub checks_today: i64,
    /// Patterns of the learner's level not yet in review, and of lower levels not yet checked.
    pub to_introduce: i64,
    pub to_check: i64,
    pub days_to_introduce: Option<i64>,
    pub days_to_check: Option<i64>,
    /// Next level, once most of the current one is known.
    pub move_up_to: Option<String>,
}

fn days(count: i64, per_day: i64) -> Option<i64> {
    (count > 0 && per_day > 0).then(|| (count + per_day - 1) / per_day)
}

/// Per level: original patterns, how many are known (graduated to review) and how many are being learned.
fn level_counts(conn: &Connection, language: &str) -> Result<HashMap<String, (i64, i64, i64)>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT g.level, COUNT(*),
                    COALESCE(SUM(r.state = 'review'), 0),
                    COALESCE(SUM(r.id IS NOT NULL AND r.state != 'review'), 0)
             FROM grammar_items g
             LEFT JOIN review_items r ON r.grammar_id = g.id AND r.item_type = 'grammar'
             WHERE g.language = ?1 AND {}
             GROUP BY g.level",
            ORIGINAL_ROWS
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![language], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))
        .map_err(|e| e.to_string())?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Level checks per day: doubled with "speed up" while the learner passes nearly all of them.
fn checks_today(conn: &Connection, language: &str, lim: &Limits) -> i64 {
    if !lim.speed_up {
        return lim.checks_per_day;
    }
    let recent: Vec<bool> = conn
        .prepare(
            "SELECT correct FROM review_answers WHERE COALESCE(language, '') = ?1 AND placement = 1
             ORDER BY answered_at DESC LIMIT ?2",
        )
        .and_then(|mut stmt| {
            stmt.query_map(params![language, SPEED_UP_SAMPLE as i64], |r| r.get(0))
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default();
    let passed = recent.iter().filter(|c| **c).count();
    if recent.len() == SPEED_UP_SAMPLE && passed >= SPEED_UP_PASSED {
        lim.checks_per_day * 2
    } else {
        lim.checks_per_day
    }
}

pub(crate) fn grammar_plan(conn: &Connection, language: &str) -> Result<GrammarPlan, String> {
    let levels = level_order(conn, language)?;
    let current = current_level(conn, language, &levels);
    let counts = level_counts(conn, language)?;
    let lim = limits(conn);
    let checks = checks_today(conn, language, &lim);

    let progress: Vec<LevelProgress> = levels
        .iter()
        .map(|l| {
            let (total, known, learning) = counts.get(l).copied().unwrap_or((0, 0, 0));
            LevelProgress {
                level: l.clone(),
                role: role(&levels, current.as_deref(), l),
                total,
                known,
                learning,
                pct: if total > 0 { known * 100 / total } else { 0 },
            }
        })
        .collect();
    let untouched = |p: &LevelProgress| p.total - p.known - p.learning;
    let to_introduce: i64 = progress.iter().filter(|p| p.role == "current").map(untouched).sum();
    let to_check: i64 = progress.iter().filter(|p| p.role == "below").map(untouched).sum();
    let move_up_to = progress
        .iter()
        .position(|p| p.role == "current" && p.total > 0 && p.pct >= MOVE_UP_PCT)
        .and_then(|i| levels.get(i + 1).cloned());

    Ok(GrammarPlan {
        language: language.to_string(),
        level: current,
        levels: progress,
        new_per_day: lim.new_per_day,
        checks_per_day: lim.checks_per_day,
        speed_up: lim.speed_up,
        checks_today: checks,
        to_introduce,
        to_check,
        days_to_introduce: days(to_introduce, lim.new_per_day),
        days_to_check: days(to_check, checks),
        move_up_to,
    })
}

/// Where a grammar row stands in review, for the Grammar page.
#[derive(Serialize, Deserialize, Clone)]
pub struct GrammarReview {
    /// `known`, `learning`, `new` (in review, not seen yet), `check` (level check waiting),
    /// `coming` / `coming_check` (enters review by itself), `not_planned` (above the learner's level).
    pub status: String,
    pub can_add: bool,
}

/// Review status of every original grammar row of a language, keyed by grammar row id.
pub(crate) fn review_statuses(conn: &Connection, language: &str) -> Result<HashMap<String, GrammarReview>, String> {
    let levels = level_order(conn, language)?;
    let current = current_level(conn, language, &levels);
    let mut stmt = conn
        .prepare(
            "SELECT g.id, g.level, r.state, r.placement FROM grammar_items g
             LEFT JOIN review_items r ON r.grammar_id = g.id AND r.item_type = 'grammar'
             WHERE g.language = ?1 AND g.ai_translated = 0",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![language], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<bool>>(3)?))
        })
        .map_err(|e| e.to_string())?;
    let mut out = HashMap::new();
    for (id, level, state, placement) in rows.filter_map(|r| r.ok()) {
        let status = match (state.as_deref(), placement.unwrap_or(false)) {
            (Some("review"), _) => "known",
            (Some("new"), true) => "check",
            (Some("new"), false) => "new",
            (Some(_), _) => "learning",
            (None, _) => match role(&levels, current.as_deref(), &level) {
                "current" => "coming",
                "below" => "coming_check",
                _ => "not_planned",
            },
        };
        out.insert(id, GrammarReview { status: status.to_string(), can_add: status == "not_planned" });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Enrolling patterns into the deck
// ---------------------------------------------------------------------------

fn card_for_grammar(conn: &Connection, grammar_id: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT id FROM review_items WHERE grammar_id = ?1 AND item_type = 'grammar'",
        params![grammar_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// Creates the review card of a grammar row. `None` when another card already uses the pattern's text.
fn enroll(conn: &Connection, grammar_id: &str, placement: bool) -> Result<Option<String>, String> {
    if let Some(id) = card_for_grammar(conn, grammar_id)? {
        return Ok(Some(id));
    }
    let (pattern, language, level): (String, String, String) = conn
        .query_row(
            "SELECT pattern, language, level FROM grammar_items WHERE id = ?1",
            params![grammar_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let inserted = conn
        .execute(
            "INSERT INTO review_items(id, content, EF, I, R, state, step, lapse_count, next_review, language,
                fsrs_stability, fsrs_difficulty, created_at, item_type, grammar_id, level, placement)
             VALUES (?1, ?2, -1.0, 0, 0, 'new', 0, 0, NULL, ?3, NULL, NULL,
                CAST(strftime('%s', 'now') * 1000 AS INTEGER), ?4, ?5, ?6, ?7)
             ON CONFLICT(content) DO NOTHING",
            params![id, pattern, language, ITEM_GRAMMAR, grammar_id, level, placement],
        )
        .map_err(|e| e.to_string())?;
    Ok((inserted > 0).then_some(id))
}

/// Enrolls up to `n` patterns not in the deck yet, from `levels` in order (easiest first).
fn enroll_next(conn: &Connection, language: &str, levels: &[String], placement: bool, mut n: i64) -> Result<(), String> {
    for level in levels {
        if n <= 0 {
            break;
        }
        let ids: Vec<String> = conn
            .prepare(&format!(
                "SELECT g.id FROM grammar_items g
                 WHERE g.language = ?1 AND g.level = ?2 AND {}
                   AND NOT EXISTS (SELECT 1 FROM review_items r WHERE r.grammar_id = g.id)
                 ORDER BY g.rowid",
                ORIGINAL_ROWS
            ))
            .and_then(|mut stmt| {
                stmt.query_map(params![language, level], |r| r.get(0)).map(|rows| rows.filter_map(|r| r.ok()).collect())
            })
            .map_err(|e| e.to_string())?;
        for id in ids {
            if n <= 0 {
                break;
            }
            if enroll(conn, &id, placement)?.is_some() {
                n -= 1;
            }
        }
    }
    Ok(())
}

/// `study_card.kind` of a new grammar card: they have their own daily limits, apart from new words.
pub(crate) fn new_grammar_kind(placement: bool) -> &'static str {
    if placement { "grammar_check" } else { "grammar_new" }
}

/// Free grammar slots in today's queue: (new patterns, level checks).
pub(crate) fn grammar_slots(conn: &Connection, study_id: i64, language: &str) -> Result<(i64, i64), String> {
    let queued = |kind: &str| -> Result<i64, String> {
        conn.query_row(
            "SELECT COUNT(*) FROM study_card WHERE study_id = ?1 AND kind = ?2",
            params![study_id, kind],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
    };
    let lim = limits(conn);
    Ok((
        (lim.new_per_day - queued(new_grammar_kind(false))?).max(0),
        (checks_today(conn, language, &lim) - queued(new_grammar_kind(true))?).max(0),
    ))
}

/// Adds grammar cards to the deck so today's grammar slots can be filled: patterns of the learner's
/// level as new cards, lower levels (easiest first) as level checks.
pub(crate) fn enroll_for_today(conn: &Connection, study_id: i64, language: &str) -> Result<(), String> {
    let levels = level_order(conn, language)?;
    let Some(level) = current_level(conn, language, &levels) else { return Ok(()) };
    let (new_slots, check_slots) = grammar_slots(conn, study_id, language)?;
    let waiting = |placement: bool| -> Result<i64, String> {
        conn.query_row(
            "SELECT COUNT(*) FROM review_items
             WHERE language = ?1 AND item_type = 'grammar' AND state = 'new' AND placement = ?2
               AND id NOT IN (SELECT review_item_id FROM study_card WHERE study_id = ?3)",
            params![language, placement, study_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
    };
    let at = levels.iter().position(|l| *l == level).unwrap_or(0);
    enroll_next(conn, language, &levels[at..=at], false, new_slots - waiting(false)?)?;
    enroll_next(conn, language, &levels[..at], true, check_slots - waiting(true)?)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Practice material (the back of a grammar card)
// ---------------------------------------------------------------------------

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct GrammarExample {
    #[serde(default)]
    pub sentence: String,
    /// The part of `sentence` that is the pattern, character for character.
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub translation: String,
    /// Wrong fillers for the target's spot.
    #[serde(default)]
    pub distractors: Vec<String>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct GrammarMistake {
    /// A sentence in chunks; `parts[wrong]` holds a typical mistake.
    #[serde(default)]
    pub parts: Vec<String>,
    #[serde(default)]
    pub wrong: usize,
    #[serde(default)]
    pub fix: String,
    #[serde(default)]
    pub translation: String,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Confusable {
    #[serde(default)]
    pub pattern: String,
    #[serde(default)]
    pub meaning: String,
}

/// `review_items.detail` of a grammar card.
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct GrammarDetail {
    /// The pattern as shown to the learner (English words translated).
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub meaning: String,
    #[serde(default)]
    pub form: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub examples: Vec<GrammarExample>,
    #[serde(default)]
    pub mistake: Option<GrammarMistake>,
    /// Patterns learners mix this one up with, as extra wrong meanings.
    #[serde(default)]
    pub confusables: Vec<Confusable>,
}

impl GrammarDetail {
    /// Enough material to ask questions.
    pub fn is_usable(&self) -> bool {
        !self.meaning.is_empty() && !self.examples.is_empty()
    }
}

/// Parses and tidies a grammar card's back. Drops anything a question can't safely be built from:
/// examples whose target isn't in the sentence, fillers equal to the answer, an invalid mistake.
pub fn parse_grammar_detail(raw: Option<&str>) -> GrammarDetail {
    let Some(text) = raw.map(str::trim).filter(|t| !t.is_empty()) else {
        return GrammarDetail::default();
    };
    let Ok(mut d) = serde_json::from_str::<GrammarDetail>(json_part(text)) else {
        return GrammarDetail::default();
    };
    let trim = |s: &mut String| *s = s.trim().to_string();
    trim(&mut d.title);
    trim(&mut d.meaning);
    trim(&mut d.form);
    trim(&mut d.note);
    d.examples.retain_mut(|e| {
        trim(&mut e.sentence);
        trim(&mut e.target);
        trim(&mut e.translation);
        let mut seen = HashSet::from([e.target.clone()]);
        e.distractors = e
            .distractors
            .iter()
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty() && seen.insert(x.clone()))
            .collect();
        !e.target.is_empty() && e.sentence.contains(&e.target)
    });
    if let Some(m) = &mut d.mistake {
        m.parts = m.parts.iter().map(|p| p.trim().to_string()).collect();
        trim(&mut m.fix);
        trim(&mut m.translation);
        let distinct: HashSet<&String> = m.parts.iter().collect();
        let valid = m.parts.len() >= 2
            && distinct.len() == m.parts.len()
            && m.parts.iter().all(|p| !p.is_empty())
            && m.parts.get(m.wrong).is_some_and(|w| !m.fix.is_empty() && *w != m.fix);
        if !valid {
            d.mistake = None;
        }
    }
    d.confusables.retain_mut(|c| {
        trim(&mut c.pattern);
        trim(&mut c.meaning);
        !c.meaning.is_empty()
    });
    d
}

/// Asks the LLM for a grammar card's practice material, in the learner's explanation language.
#[allow(clippy::too_many_arguments)]
async fn generate_grammar_text(
    api_host: &str,
    api_key: &str,
    model: &str,
    pattern: &str,
    level: &str,
    reference: &str,
    language: &str,
    explain: &str,
) -> Result<String, String> {
    let lang = lang_name(language);
    let to = lang_name(explain);
    let reference = if reference.trim().is_empty() {
        String::new()
    } else {
        format!("Reference explanation:\n{}\n\n", reference.trim())
    };
    let prompt = format!(
        "You are a {lang} teacher. Make review material for the {lang} grammar pattern \"{pattern}\" (level {level}).\n\n\
        {reference}Return ONLY this JSON object:\n\
        {{\"title\": \"the pattern as learners write it (e.g. ～てもいい); write any English words in it in {to}\",\n\
        \"meaning\": \"short {to} meaning, 2 to 8 words\",\n\
        \"form\": \"how it is formed, in {to} (e.g. verb te-form + もいい)\",\n\
        \"note\": \"one short {to} sentence on when to use it or what it is often confused with\",\n\
        \"examples\": [3 objects: {{\"sentence\": \"a natural {lang} sentence at {level} level that uses the pattern\", \
        \"target\": \"the part of the sentence that is the pattern, copied character for character from the sentence\", \
        \"translation\": \"{to} translation of the sentence\", \
        \"distractors\": [\"3 wrong {lang} fillers for the target's spot: similar patterns or wrong forms a learner might pick; each must make the sentence wrong or mean something else\"]}}],\n\
        \"mistake\": {{\"parts\": [\"a new {lang} sentence using the pattern, split into 3 to 5 chunks, where exactly one chunk contains a typical learner mistake with this pattern\"], \
        \"wrong\": index of the chunk with the mistake, \"fix\": \"that chunk written correctly\", \"translation\": \"{to} translation of the correct sentence\"}},\n\
        \"confusables\": [3 objects: {{\"pattern\": \"another {lang} pattern learners mix up with this one\", \"meaning\": \"its short {to} meaning\"}}]}}\n\n\
        No furigana, no romaji. Translate directly from {lang} into {to}, not via another language."
    );
    let learner_text = |raw: &str| {
        let d = parse_grammar_detail(Some(raw));
        let translations: Vec<&str> = d.examples.iter().map(|e| e.translation.as_str()).collect();
        format!("{} {} {}", d.meaning, d.note, translations.join(" "))
    };
    let raw = complete_explained(api_host, api_key, model, &prompt, explain, learner_text).await?;
    let detail = parse_grammar_detail(Some(&raw));
    if !detail.is_usable() {
        return Err(app_error("api_bad_response", raw));
    }
    serde_json::to_string(&detail).map_err(|e| e.to_string())
}

/// Pattern, level and explanation behind a grammar card, or `None` for a word.
fn grammar_source(conn: &Connection, content: &str) -> Result<Option<(String, String, String)>, String> {
    conn.query_row(
        "SELECT COALESCE(g.pattern, r.content), COALESCE(r.level, ''), COALESCE(g.detail, '')
         FROM review_items r LEFT JOIN grammar_items g ON g.id = r.grammar_id
         WHERE r.content = ?1 AND r.item_type = 'grammar'",
        params![content],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// Generates the back of any card: practice material for a grammar card, the vocabulary back for a word.
pub(crate) async fn generate_card_back(
    app: &tauri::AppHandle,
    api_host: &str,
    api_key: &str,
    model: &str,
    content: &str,
    language: &str,
    explain: &str,
) -> Result<String, String> {
    let source = grammar_source(&open_db(app)?, content)?;
    match source {
        Some((pattern, level, reference)) => {
            generate_grammar_text(api_host, api_key, model, &pattern, &level, &reference, language, explain).await
        }
        None => generate_detail_text(api_host, api_key, model, content, language, explain).await,
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn db_get_grammar_plan(app: tauri::AppHandle, language: String) -> Result<GrammarPlan, String> {
    let conn = open_db(&app)?;
    grammar_plan(&conn, &language)
}

/// Changes the learner's level and the daily grammar limits. Cards already in review keep their schedule.
#[tauri::command]
pub fn db_set_grammar_plan(
    app: tauri::AppHandle,
    language: String,
    level: Option<String>,
    new_per_day: Option<i64>,
    checks_per_day: Option<i64>,
    speed_up: Option<bool>,
) -> Result<GrammarPlan, String> {
    let conn = open_db(&app)?;
    if let Some(l) = level {
        if !level_order(&conn, &language)?.contains(&l) {
            return Err(app_error("unknown_level", l));
        }
        set_setting(&conn, &level_key(&language), &l)?;
    }
    if let Some(n) = new_per_day {
        set_setting(&conn, "grammar_new_per_day", &n.clamp(0, MAX_NEW_PER_DAY).to_string())?;
    }
    if let Some(n) = checks_per_day {
        set_setting(&conn, "grammar_checks_per_day", &n.clamp(0, MAX_CHECKS_PER_DAY).to_string())?;
    }
    if let Some(on) = speed_up {
        set_setting(&conn, "grammar_speed_up", if on { "1" } else { "0" })?;
    }
    grammar_plan(&conn, &language)
}

/// Stores a level picked before grammar review existed (e.g. the chat's JLPT level), unless one is set.
#[tauri::command]
pub fn db_seed_grammar_level(app: tauri::AppHandle, language: String, level: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    if level_order(&conn, &language)?.contains(&level) {
        conn.execute(
            "INSERT OR IGNORE INTO app_settings(key, value) VALUES (?1, ?2)",
            params![level_key(&language), level],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// "Add to review" on the Grammar page: puts a pattern into the deck now, as a new card.
#[tauri::command]
pub fn db_add_grammar_to_review(app: tauri::AppHandle, grammar_id: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    let original: String = conn
        .query_row(
            "SELECT COALESCE(source_id, id) FROM grammar_items WHERE id = ?1",
            params![grammar_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    match enroll(&conn, &original, false)? {
        Some(_) => Ok(()),
        None => Err(app_error("pattern_in_deck", "")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_run_from_easiest() {
        let mut levels = vec!["N1", "N3", "N5", "N2", "N4"];
        levels.sort_by_key(|l| level_rank(l));
        assert_eq!(levels, vec!["N5", "N4", "N3", "N2", "N1"]);
        let mut cefr = vec!["B1", "A2", "A1"];
        cefr.sort_by_key(|l| level_rank(l));
        assert_eq!(cefr, vec!["A1", "A2", "B1"]);
    }

    #[test]
    fn parsing_drops_material_questions_cant_use() {
        let d = parse_grammar_detail(Some(
            r#"```json
            {"title":"～てもいい","meaning":" may, it's okay to ","examples":[
              {"sentence":"ここで写真を撮ってもいいですか。","target":"撮ってもいい","translation":"May I take a photo here?","distractors":["撮ってはいけない","撮ってもいい"," ","撮らなければならない"]},
              {"sentence":"窓を開けます。","target":"開けてもいい","translation":"x"}
            ],
            "mistake":{"parts":["ここで","写真を","撮っては","いいですか。"],"wrong":2,"fix":"撮っても","translation":"May I take a photo here?"},
            "confusables":[{"pattern":"～てはいけない","meaning":"must not"},{"pattern":"x","meaning":""}]}
            ```"#,
        ));
        assert_eq!(d.meaning, "may, it's okay to");
        assert_eq!(d.examples.len(), 1);
        assert_eq!(d.examples[0].distractors, vec!["撮ってはいけない", "撮らなければならない"]);
        assert_eq!(d.mistake.as_ref().map(|m| m.wrong), Some(2));
        assert_eq!(d.confusables.len(), 1);
        assert!(d.is_usable());

        let bad = parse_grammar_detail(Some(r#"{"meaning":"m","examples":[],"mistake":{"parts":["a","b"],"wrong":5,"fix":"c"}}"#));
        assert!(bad.mistake.is_none());
        assert!(!bad.is_usable());
        assert!(!parse_grammar_detail(Some("to eat")).is_usable());
    }

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE app_settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE grammar_items(id TEXT PRIMARY KEY, pattern TEXT, detail TEXT DEFAULT '', language TEXT, level TEXT,
               is_default INTEGER DEFAULT 1, explain_lang TEXT DEFAULT 'en', ai_translated INTEGER DEFAULT 0, source_id TEXT);
             CREATE TABLE review_items(id TEXT PRIMARY KEY, content TEXT UNIQUE NOT NULL, EF REAL, I INTEGER, R INTEGER,
               state TEXT, step INTEGER, lapse_count INTEGER, next_review INTEGER, language TEXT, fsrs_stability REAL,
               fsrs_difficulty REAL, created_at INTEGER, item_type TEXT NOT NULL DEFAULT 'word', grammar_id TEXT, level TEXT,
               placement INTEGER NOT NULL DEFAULT 0, lemma TEXT);
             CREATE TABLE study_card(study_id INTEGER, review_item_id TEXT, kind TEXT);
             CREATE TABLE review_answers(language TEXT, placement INTEGER, correct INTEGER, answered_at INTEGER);",
        )
        .unwrap();
        for (level, n) in [("N5", 4), ("N4", 3), ("N3", 5), ("N2", 2)] {
            for i in 0..n {
                conn.execute(
                    "INSERT INTO grammar_items(id, pattern, language, level) VALUES (?1, ?2, 'ja', ?3)",
                    params![format!("{level}-{i}"), format!("～{level}{i}"), level],
                )
                .unwrap();
            }
        }
        // A translation of N5-0: not a pattern of its own.
        conn.execute(
            "INSERT INTO grammar_items(id, pattern, language, level, explain_lang, ai_translated, source_id)
             VALUES ('N5-0-id', '～N5 0 (id)', 'ja', 'N5', 'id', 1, 'N5-0')",
            [],
        )
        .unwrap();
        conn
    }

    fn cards(conn: &Connection, placement: bool) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT grammar_id FROM review_items WHERE item_type = 'grammar' AND placement = ?1 ORDER BY grammar_id")
            .unwrap();
        stmt.query_map(params![placement], |r| r.get(0)).unwrap().filter_map(|r| r.ok()).collect()
    }

    #[test]
    fn learners_level_is_taught_and_lower_levels_are_checked() {
        let conn = db();
        set_setting(&conn, &level_key("ja"), "N3").unwrap();
        set_setting(&conn, "grammar_new_per_day", "2").unwrap();
        set_setting(&conn, "grammar_checks_per_day", "5").unwrap();
        enroll_for_today(&conn, 1, "ja").unwrap();
        assert_eq!(cards(&conn, false), vec!["N3-0", "N3-1"]);
        // Easiest first: all of N5, then N4.
        assert_eq!(cards(&conn, true), vec!["N4-0", "N5-0", "N5-1", "N5-2", "N5-3"]);
        // Cards waiting outside today's queue fill the slots before anything new is enrolled.
        enroll_for_today(&conn, 1, "ja").unwrap();
        assert_eq!(cards(&conn, false).len(), 2);

        let plan = grammar_plan(&conn, "ja").unwrap();
        assert_eq!(plan.level.as_deref(), Some("N3"));
        let roles: Vec<(&str, &str)> = plan.levels.iter().map(|l| (l.level.as_str(), l.role)).collect();
        assert_eq!(roles, vec![("N5", "below"), ("N4", "below"), ("N3", "current"), ("N2", "above")]);
        assert_eq!(plan.to_introduce, 3);
        assert_eq!(plan.to_check, 2);
        assert_eq!(plan.days_to_introduce, Some(2));

        let statuses = review_statuses(&conn, "ja").unwrap();
        assert_eq!(statuses["N3-0"].status, "new");
        assert_eq!(statuses["N3-4"].status, "coming");
        assert_eq!(statuses["N4-2"].status, "coming_check");
        assert_eq!(statuses["N5-0"].status, "check");
        assert!(statuses["N2-0"].can_add && !statuses["N3-4"].can_add);
        assert!(!statuses.contains_key("N5-0-id"));
    }

    #[test]
    fn move_up_is_suggested_once_most_of_the_level_is_known() {
        let conn = db();
        set_setting(&conn, &level_key("ja"), "N4").unwrap();
        for i in 0..3 {
            enroll(&conn, &format!("N4-{i}"), false).unwrap();
        }
        conn.execute("UPDATE review_items SET state = 'review' WHERE grammar_id IN ('N4-0', 'N4-1')", []).unwrap();
        assert_eq!(grammar_plan(&conn, "ja").unwrap().move_up_to, None);
        conn.execute("UPDATE review_items SET state = 'review'", []).unwrap();
        assert_eq!(grammar_plan(&conn, "ja").unwrap().move_up_to.as_deref(), Some("N3"));
    }

    #[test]
    fn checks_speed_up_while_the_learner_keeps_passing() {
        let conn = db();
        let lim = Limits { new_per_day: 3, checks_per_day: 5, speed_up: true };
        for i in 0..10 {
            conn.execute(
                "INSERT INTO review_answers VALUES ('ja', 1, ?1, ?2)",
                params![i != 0, i],
            )
            .unwrap();
        }
        assert_eq!(checks_today(&conn, "ja", &lim), 10);
        conn.execute("INSERT INTO review_answers VALUES ('ja', 1, 0, 100), ('ja', 1, 0, 101)", []).unwrap();
        assert_eq!(checks_today(&conn, "ja", &lim), 5);
        assert_eq!(checks_today(&conn, "ja", &Limits { speed_up: false, ..lim }), 5);
    }

    #[test]
    fn due_grammar_keeps_its_review_slots_ahead_of_words() {
        use crate::srs::{ensure_study, top_up_queue};
        let conn = Connection::open_in_memory().unwrap();
        crate::db::prepare_db(&conn).unwrap();
        set_setting(&conn, "grammar_new_per_day", "0").unwrap();
        set_setting(&conn, "grammar_checks_per_day", "0").unwrap();
        // Words are due earlier, so they would fill the review limit on their own.
        for i in 0..10 {
            conn.execute(
                "INSERT INTO review_items(id, content, EF, I, R, state, step, lapse_count, next_review, language, created_at)
                 VALUES (?1, ?1, 2.5, 1, 1, 'review', 0, 0, ?2, 'ja', 0)",
                params![format!("w{i}"), i],
            )
            .unwrap();
        }
        for i in 0..5 {
            conn.execute(
                "INSERT INTO review_items(id, content, EF, I, R, state, step, lapse_count, next_review, language, created_at, item_type)
                 VALUES (?1, ?1, 2.5, 1, 1, 'review', 0, 0, 100, 'ja', 0, 'grammar')",
                params![format!("g{i}")],
            )
            .unwrap();
        }
        let queued = |item_type: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM study_card sc JOIN review_items r ON r.id = sc.review_item_id
                 WHERE sc.kind = 'review' AND r.item_type = ?1",
                params![item_type],
                |r| r.get(0),
            )
            .unwrap()
        };
        let study = ensure_study(&conn, "2026-01-01", "ja", 5, 0).unwrap();
        top_up_queue(&conn, study, "ja", i64::MAX, 5, 0).unwrap();
        assert_eq!((queued(ITEM_GRAMMAR), queued("word")), (GRAMMAR_REVIEWS_PER_DAY, 5 - GRAMMAR_REVIEWS_PER_DAY));
        // A bigger limit later today adds words, never more grammar.
        top_up_queue(&conn, study, "ja", i64::MAX, 20, 0).unwrap();
        assert_eq!((queued(ITEM_GRAMMAR), queued("word")), (GRAMMAR_REVIEWS_PER_DAY, 10));
    }

    #[test]
    fn todays_queue_mixes_words_and_grammar_on_the_real_schema() {
        use crate::quiz::{issue_task, KIND_GRAMMAR_GAP, KIND_INTRO_TF, KIND_NEEDS_MEANING};
        use crate::srs::{apply_session_grade, ensure_study, fetch_review_item, session_progress, top_up_queue};
        let conn = Connection::open_in_memory().unwrap();
        crate::db::prepare_db(&conn).unwrap();
        // Bundled Indonesian explanations are paired with the English rows they translate.
        let unlinked: i64 = conn
            .query_row("SELECT COUNT(*) FROM grammar_items WHERE ai_translated = 1 AND source_id IS NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(unlinked, 0);

        set_setting(&conn, &level_key("ja"), "N4").unwrap();
        conn.execute(
            "INSERT INTO review_items(id, content, EF, I, R, state, step, lapse_count, language, created_at)
             VALUES ('w1', '約束', -1.0, 0, 0, 'new', 0, 0, 'ja', 0)",
            [],
        )
        .unwrap();
        let study = ensure_study(&conn, "2026-01-01", "ja", 100, 20).unwrap();
        top_up_queue(&conn, study, "ja", i64::MAX, 100, 20).unwrap();
        let kinds: HashMap<String, i64> = conn
            .prepare("SELECT kind, COUNT(*) FROM study_card WHERE study_id = ?1 GROUP BY kind")
            .unwrap()
            .query_map(params![study], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(kinds.get("new"), Some(&1));
        assert_eq!(kinds.get(new_grammar_kind(false)), Some(&DEFAULT_NEW_PER_DAY));
        assert_eq!(kinds.get(new_grammar_kind(true)), Some(&DEFAULT_CHECKS_PER_DAY));
        // Topping up again adds nothing: today's slots are full.
        top_up_queue(&conn, study, "ja", i64::MAX, 100, 20).unwrap();
        let total: i64 = conn.query_row("SELECT COUNT(*) FROM study_card", [], |r| r.get(0)).unwrap();
        assert_eq!(total, 1 + DEFAULT_NEW_PER_DAY + DEFAULT_CHECKS_PER_DAY);

        // No practice material yet: every card asks for its back first.
        let progress = session_progress(&conn, study).unwrap();
        assert_eq!((progress.queue_done, progress.queue_total), (0, total as usize));
        assert_eq!((progress.grammar_done, progress.grammar_total), (0, DEFAULT_NEW_PER_DAY + DEFAULT_CHECKS_PER_DAY));
        assert_eq!(progress.next_task.unwrap().kind, KIND_NEEDS_MEANING);

        let back = r#"{"meaning":"m","examples":[{"sentence":"AXB","target":"X","translation":"t","distractors":["Y","Z"]}],
            "confusables":[{"pattern":"p","meaning":"other"}]}"#;
        conn.execute("UPDATE review_items SET detail = ?1, meaning_lang = 'en' WHERE item_type = 'grammar'", params![back]).unwrap();
        conn.execute("UPDATE study_card SET task = NULL", []).unwrap();
        let card_of = |placement: bool| -> String {
            conn.query_row(
                "SELECT id FROM review_items WHERE item_type = 'grammar' AND placement = ?1 LIMIT 1",
                params![placement],
                |r| r.get(0),
            )
            .unwrap()
        };
        let check = fetch_review_item(&conn, &card_of(true)).unwrap().unwrap();
        assert_eq!(check.level.as_deref(), Some("N5"));
        assert_eq!(issue_task(&conn, study, &check).unwrap().kind, KIND_GRAMMAR_GAP);
        let lesson = fetch_review_item(&conn, &card_of(false)).unwrap().unwrap();
        assert_eq!(lesson.level.as_deref(), Some("N4"));
        assert_eq!(issue_task(&conn, study, &lesson).unwrap().kind, KIND_INTRO_TF);

        // A passed check goes straight to review, about two weeks out.
        let srs = apply_session_grade(&conn, study, &check.id, "easy").unwrap();
        assert_eq!(srs.state, "review");
        assert!(srs.next_minutes > 7 * 24 * 60);
        conn.execute(
            "INSERT INTO review_answers(review_item_id, language, task, correct, elapsed_ms, grade, answered_at, placement)
             VALUES (?1, 'ja', 'grammar_gap', 1, 3000, 'easy', ?2, 1)",
            params![check.id, chrono::Utc::now().timestamp_millis()],
        )
        .unwrap();
        let progress = session_progress(&conn, study).unwrap();
        assert_eq!(progress.queue_done, 1);
        assert_eq!(progress.grammar_done, 1);

        let plan = grammar_plan(&conn, "ja").unwrap();
        assert_eq!(plan.levels[0].level, "N5");
        assert_eq!(plan.levels[0].known, 1);
        assert_eq!(plan.levels[1].role, "current");
    }
}
