//! Auto-graded review. Instead of the user rating themselves, each card gets a small question
//! (true/false, pick the meaning, recall then confirm, pick the reading, meaning in context).
//! The answer is checked here and correctness, answer time and hints are turned into the FSRS grade.

use crate::db::open_db;
use crate::srs::{
    apply_session_grade, ensure_study, fetch_review_item, format_next_review_minutes,
    session_progress, ReviewItem, SessionProgress,
};
use crate::words::is_cjk;
use rand::seq::SliceRandom;
use rand::Rng;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const KIND_CLASSIC: &str = "classic";
pub const KIND_INTRO_TF: &str = "intro_tf";
pub const KIND_TRUE_FALSE: &str = "true_false";
pub const KIND_CHOICE: &str = "choice";
pub const KIND_RECALL: &str = "recall_confirm";
pub const KIND_READING: &str = "reading";
pub const KIND_CONTEXT: &str = "context";

/// Answer value for "None of these" in recall → confirm.
pub const NONE_ANSWER: &str = "__none";

/// FSRS stability (days) from which a review card counts as mature.
const MATURE_DAYS: f64 = 21.0;
/// Chance that recall → confirm leaves the right meaning out, so "None of these" is correct.
const NONE_CORRECT_CHANCE: f64 = 0.15;
/// Mis-tap fixes allowed per language per day.
const OVERRIDES_PER_DAY: i64 = 1;
/// Past answers needed before the user's own usual time replaces the default.
const MIN_TIMING_SAMPLES: usize = 8;

// ---------------------------------------------------------------------------
// Card detail parsing
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub(crate) struct CardDetail {
    pub reading: String,
    pub meaning: String,
    pub part_of_speech: String,
    pub example: String,
    pub example_meaning: String,
}

/// Parses `review_items.detail`: the JSON object written by `db_generate_detail`, or the plain
/// text meaning from a CSV import (`word,meaning`).
pub(crate) fn parse_detail(detail: Option<&str>) -> CardDetail {
    let Some(d) = detail.map(str::trim).filter(|d| !d.is_empty()) else {
        return CardDetail::default();
    };
    let json = match (d.find('{'), d.rfind('}')) {
        (Some(s), Some(e)) if e > s => serde_json::from_str::<serde_json::Value>(&d[s..=e]).ok(),
        _ => None,
    };
    match json {
        Some(v) => {
            let field = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
            CardDetail {
                reading: field("reading"),
                meaning: field("meaning"),
                part_of_speech: field("part_of_speech"),
                example: field("example"),
                example_meaning: field("example_meaning"),
            }
        }
        None => CardDetail {
            meaning: d.lines().next().unwrap_or("").trim().to_string(),
            ..Default::default()
        },
    }
}

fn normalize(s: &str) -> String {
    s.trim().to_lowercase()
}

fn is_mature(card: &ReviewItem) -> bool {
    let stability = card.fsrs_stability.map(|s| s as f64).unwrap_or(card.I as f64);
    card.state == "review" && stability >= MATURE_DAYS
}

// ---------------------------------------------------------------------------
// Questions
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone)]
pub struct QuizOption {
    pub text: String,
    /// The card this option's text came from, so a wrong pick can be recorded as a confusion.
    pub item_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SentenceParts {
    pub before: String,
    pub target: String,
    pub after: String,
    pub translation: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct QuizTask {
    pub kind: String,
    pub word: String,
    pub reading: String,
    pub meaning: String,
    pub options: Vec<QuizOption>,
    /// True/false: the meaning shown next to the word.
    pub statement: Option<QuizOption>,
    pub sentence: Option<SentenceParts>,
    /// Furigana can be revealed as a hint (counts against the grade).
    pub hint_available: bool,
    pub mature: bool,
    #[serde(skip_serializing, default)]
    answer: String,
    #[serde(skip_serializing, default)]
    statement_true: bool,
    #[serde(skip_serializing, default)]
    easy_allowed: bool,
}

impl QuizTask {
    fn base(kind: &str, card: &ReviewItem, detail: &CardDetail) -> Self {
        QuizTask {
            kind: kind.to_string(),
            word: card.content.clone(),
            reading: detail.reading.clone(),
            meaning: detail.meaning.clone(),
            options: vec![],
            statement: None,
            sentence: None,
            hint_available: false,
            mature: false,
            answer: detail.meaning.clone(),
            statement_true: false,
            easy_allowed: false,
        }
    }
}

struct Candidate {
    id: String,
    content: String,
    detail: CardDetail,
}

fn load_candidates(conn: &Connection, card: &ReviewItem) -> Result<Vec<Candidate>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, content, detail FROM review_items
             WHERE COALESCE(language, '') = COALESCE(?1, '') AND id != ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![card.language, card.id], |row| {
            Ok(Candidate {
                id: row.get(0)?,
                content: row.get(1)?,
                detail: parse_detail(row.get::<_, Option<String>>(2)?.as_deref()),
            })
        })
        .map_err(|e| e.to_string())?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// How often each other card was mixed up with this one, in either direction.
fn load_confusions(conn: &Connection, id: &str) -> Result<HashMap<String, i64>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT confused_with_id, count FROM review_confusions WHERE review_item_id = ?1
             UNION ALL
             SELECT review_item_id, count FROM review_confusions WHERE confused_with_id = ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
        .map_err(|e| e.to_string())?;
    let mut map = HashMap::new();
    for (other, count) in rows.filter_map(|r| r.ok()) {
        *map.entry(other).or_insert(0) += count;
    }
    Ok(map)
}

/// Wrong meanings that are hard to rule out: words mixed up before, words sharing a kanji,
/// and words with the same part of speech come first.
fn meaning_distractors(
    card: &ReviewItem,
    detail: &CardDetail,
    candidates: &[Candidate],
    confusions: &HashMap<String, i64>,
    n: usize,
    rng: &mut impl Rng,
) -> Vec<QuizOption> {
    let target_meaning = normalize(&detail.meaning);
    let target_chars: HashSet<char> = card.content.chars().filter(|c| is_cjk(*c)).collect();
    let target_pos = normalize(&detail.part_of_speech);

    let mut seen: HashSet<String> = HashSet::new();
    seen.insert(target_meaning);
    let mut scored: Vec<(f64, QuizOption)> = Vec::new();
    for c in candidates {
        let key = normalize(&c.detail.meaning);
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        let mut score: f64 = rng.gen_range(0.0..3.0);
        score += 100.0 * *confusions.get(&c.id).unwrap_or(&0) as f64;
        score += 10.0 * c.content.chars().filter(|ch| target_chars.contains(ch)).count() as f64;
        if !target_pos.is_empty() && normalize(&c.detail.part_of_speech) == target_pos {
            score += 5.0;
        }
        scored.push((score, QuizOption { text: c.detail.meaning.clone(), item_id: Some(c.id.clone()) }));
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(n).map(|(_, o)| o).collect()
}

const VOICING_PAIRS: &str = "かがきぎくぐけげこごさざしじすずせぜそぞただちぢつづてでとどはばひびふぶへべほぼ";
const SOKUON_BEFORE: &str = "かきくけこさしすせそたちつてとぱぴぷぺぽ";
const LONG_O_ENDINGS: &str = "おこそとのほもよろごぞどぼぽょ";

/// Plausible misreadings: voicing flipped, small っ or a long vowel added or dropped, ゅ/ょ swapped.
fn reading_variants(reading: &str) -> Vec<String> {
    let chars: Vec<char> = reading.chars().collect();
    let pairs: Vec<char> = VOICING_PAIRS.chars().collect();
    let mut out = Vec::new();
    let with = |v: Vec<char>| v.into_iter().collect::<String>();
    for (i, c) in chars.iter().enumerate() {
        if let Some(p) = pairs.iter().position(|x| x == c) {
            let mut v = chars.clone();
            v[i] = if p % 2 == 0 { pairs[p + 1] } else { pairs[p - 1] };
            out.push(with(v));
        }
        if matches!(c, 'っ' | 'う' | 'ー') && chars.len() > 2 {
            let mut v = chars.clone();
            v.remove(i);
            out.push(with(v));
        }
        let swap = match c {
            'ゃ' | 'ゅ' => Some('ょ'),
            'ょ' => Some('ゅ'),
            _ => None,
        };
        if let Some(sw) = swap {
            let mut v = chars.clone();
            v[i] = sw;
            out.push(with(v));
        }
        if i > 0 && SOKUON_BEFORE.contains(*c) && chars[i - 1] != 'っ' {
            let mut v = chars.clone();
            v.insert(i, 'っ');
            out.push(with(v));
        }
    }
    if let Some(last) = chars.last() {
        if LONG_O_ENDINGS.contains(*last) {
            out.push(format!("{}う", reading));
        }
    }
    out
}

fn is_kana(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| ('\u{3040}'..='\u{30FF}').contains(&c))
}

fn reading_distractors(reading: &str, candidates: &[Candidate], n: usize, rng: &mut impl Rng) -> Vec<String> {
    let mut variants = reading_variants(reading);
    variants.shuffle(rng);
    let len = reading.chars().count() as i64;
    let mut others: Vec<String> = candidates
        .iter()
        .map(|c| c.detail.reading.clone())
        .filter(|r| is_kana(r) && (r.chars().count() as i64 - len).abs() <= 1)
        .collect();
    others.shuffle(rng);

    let mut seen: HashSet<String> = HashSet::new();
    seen.insert(reading.to_string());
    variants
        .into_iter()
        .chain(others)
        .filter(|r| seen.insert(r.clone()))
        .take(n)
        .collect()
}

fn split_sentence(example: &str, word: &str, translation: &str) -> Option<SentenceParts> {
    let at = example.find(word)?;
    Some(SentenceParts {
        before: example[..at].to_string(),
        target: word.to_string(),
        after: example[at + word.len()..].to_string(),
        translation: translation.to_string(),
    })
}

fn build_task(conn: &Connection, card: &ReviewItem, rng: &mut impl Rng) -> Result<QuizTask, String> {
    let detail = parse_detail(card.detail.as_deref());
    if detail.meaning.is_empty() {
        return Ok(QuizTask::base(KIND_CLASSIC, card, &detail));
    }

    let candidates = load_candidates(conn, card)?;
    let confusions = load_confusions(conn, &card.id)?;
    let pool = meaning_distractors(card, &detail, &candidates, &confusions, 4, rng);
    let mature = is_mature(card);
    let hint_available = card.language.as_deref() == Some("ja")
        && card.content.chars().any(is_cjk)
        && is_kana(&detail.reading);
    let sentence = split_sentence(&detail.example, &card.content, &detail.example_meaning);
    let readings = if hint_available { reading_distractors(&detail.reading, &candidates, 3, rng) } else { vec![] };

    let kind = if pool.is_empty() {
        KIND_CLASSIC
    } else if card.state == "new" {
        KIND_INTRO_TF
    } else if pool.len() < 3 {
        KIND_TRUE_FALSE
    } else if card.state != "review" {
        KIND_CHOICE
    } else if mature {
        // About half recall, a quarter reading, a quarter context, when those are possible.
        let mut kinds = vec![KIND_RECALL, KIND_RECALL];
        if readings.len() == 3 {
            kinds.push(KIND_READING);
        }
        if sentence.is_some() {
            kinds.push(KIND_CONTEXT);
        }
        *kinds.choose(rng).unwrap_or(&KIND_RECALL)
    } else {
        KIND_RECALL
    };

    let mut task = QuizTask::base(kind, card, &detail);
    task.mature = mature;
    task.hint_available = hint_available && kind != KIND_READING;
    task.easy_allowed = mature && (kind == KIND_RECALL || kind == KIND_CONTEXT);
    let correct = QuizOption { text: detail.meaning.clone(), item_id: Some(card.id.clone()) };

    match kind {
        KIND_INTRO_TF | KIND_TRUE_FALSE => {
            task.statement_true = rng.gen_bool(0.5);
            task.statement = Some(if task.statement_true { correct } else { pool[0].clone() });
            task.answer = task.statement_true.to_string();
        }
        KIND_CHOICE | KIND_CONTEXT => {
            task.options = pool.into_iter().take(3).chain(std::iter::once(correct)).collect();
            task.options.shuffle(rng);
            if kind == KIND_CONTEXT {
                task.sentence = sentence;
            }
        }
        KIND_RECALL => {
            let leave_out = pool.len() >= 4 && rng.gen_bool(NONE_CORRECT_CHANCE);
            task.options = if leave_out {
                task.answer = NONE_ANSWER.to_string();
                pool
            } else {
                pool.into_iter().take(3).chain(std::iter::once(correct)).collect()
            };
            task.options.shuffle(rng);
        }
        KIND_READING => {
            task.options = readings
                .into_iter()
                .chain(std::iter::once(detail.reading.clone()))
                .map(|text| QuizOption { text, item_id: None })
                .collect();
            task.options.shuffle(rng);
            task.answer = detail.reading.clone();
        }
        _ => {}
    }
    Ok(task)
}

fn load_task(conn: &Connection, study_id: i64, card_id: &str) -> Result<Option<QuizTask>, String> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT task FROM study_card WHERE study_id = ?1 AND review_item_id = ?2",
            params![study_id, card_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    Ok(stored.and_then(|s| serde_json::from_str(&s).ok()))
}

/// Returns the open question for a card, creating and storing one if needed. Storing it means a
/// reload shows the same question and the answer is checked against what was actually shown.
pub(crate) fn issue_task(conn: &Connection, study_id: i64, card: &ReviewItem) -> Result<QuizTask, String> {
    if let Some(task) = load_task(conn, study_id, &card.id)? {
        return Ok(task);
    }
    let task = build_task(conn, card, &mut rand::thread_rng())?;
    let json = serde_json::to_string(&StoredTask(&task)).map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE study_card SET task = ?1 WHERE study_id = ?2 AND review_item_id = ?3",
        params![json, study_id, card.id],
    )
    .map_err(|e| e.to_string())?;
    Ok(task)
}

pub(crate) fn clear_task(conn: &Connection, study_id: i64, card_id: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE study_card SET task = NULL WHERE study_id = ?1 AND review_item_id = ?2",
        params![study_id, card_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Serializes every field, including the answer that is hidden from the frontend.
struct StoredTask<'a>(&'a QuizTask);

impl Serialize for StoredTask<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let t = self.0;
        let mut st = s.serialize_struct("QuizTask", 13)?;
        st.serialize_field("kind", &t.kind)?;
        st.serialize_field("word", &t.word)?;
        st.serialize_field("reading", &t.reading)?;
        st.serialize_field("meaning", &t.meaning)?;
        st.serialize_field("options", &t.options)?;
        st.serialize_field("statement", &t.statement)?;
        st.serialize_field("sentence", &t.sentence)?;
        st.serialize_field("hint_available", &t.hint_available)?;
        st.serialize_field("mature", &t.mature)?;
        st.serialize_field("answer", &t.answer)?;
        st.serialize_field("statement_true", &t.statement_true)?;
        st.serialize_field("easy_allowed", &t.easy_allowed)?;
        st.end()
    }
}

// ---------------------------------------------------------------------------
// Grading
// ---------------------------------------------------------------------------

fn default_usual_ms(kind: &str) -> i64 {
    match kind {
        KIND_INTRO_TF | KIND_TRUE_FALSE => 2500,
        KIND_CONTEXT => 5000,
        _ => 3500,
    }
}

/// The user's median answer time for this kind of question (correct, unhinted answers).
fn usual_ms(conn: &Connection, language: &str, kind: &str) -> i64 {
    let mut times: Vec<i64> = conn
        .prepare(
            "SELECT elapsed_ms FROM review_answers
             WHERE task = ?1 AND COALESCE(language, '') = ?2 AND correct = 1 AND hint_used = 0
             ORDER BY answered_at DESC LIMIT 50",
        )
        .and_then(|mut stmt| {
            stmt.query_map(params![kind, language], |row| row.get::<_, i64>(0))
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default();
    if times.len() < MIN_TIMING_SAMPLES {
        return default_usual_ms(kind);
    }
    times.sort_unstable();
    times[times.len() / 2]
}

fn seconds(ms: i64) -> String {
    format!("{:.1}s", ms as f64 / 1000.0)
}

struct GradeInput<'a> {
    kind: &'a str,
    correct: bool,
    dont_know: bool,
    elapsed_ms: i64,
    hint_used: bool,
    mature: bool,
    easy_allowed: bool,
    usual_ms: i64,
}

fn compute_grade(g: &GradeInput) -> (&'static str, Vec<String>) {
    if g.dont_know {
        return ("again", vec!["didn't know".into()]);
    }
    if !g.correct {
        // A mature card tests the meaning; a misread shouldn't reset months of memory.
        if g.kind == KIND_READING && g.mature {
            return ("hard", vec!["reading slip".into(), "meaning still known".into(), "no lapse on a mature card".into()]);
        }
        let reason = if g.kind == KIND_CONTEXT { "wrong meaning in this sentence" } else { "wrong answer" };
        return ("again", vec![reason.into()]);
    }
    let verb = if g.kind == KIND_RECALL { "recalled" } else { "answered" };
    let mut reasons = vec![format!("{} in {}", verb, seconds(g.elapsed_ms))];
    if g.hint_used {
        reasons.push("used furigana hint".into());
        return ("hard", reasons);
    }
    if g.elapsed_ms >= g.usual_ms * 2 {
        reasons.push(format!("slower than your usual {}", seconds(g.usual_ms)));
        return ("hard", reasons);
    }
    reasons.push("no hints".into());
    let fast = g.elapsed_ms <= g.usual_ms * 6 / 10;
    if fast && g.easy_allowed {
        reasons.push(format!("faster than your usual {}", seconds(g.usual_ms)));
        reasons.push("mature card".into());
        return ("easy", reasons);
    }
    if fast {
        reasons.push(if g.mature { "capped at Good for this question" } else { "Easy needs a mature card" }.into());
    }
    ("good", reasons)
}

fn grade_label(grade: &str) -> &'static str {
    match grade {
        "again" => "Again",
        "hard" => "Hard",
        "easy" => "Easy",
        _ => "Good",
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct QuizAnswer {
    #[serde(default)]
    pub dont_know: bool,
    /// Picked option text, or `NONE_ANSWER`.
    pub choice: Option<String>,
    /// True/false answer.
    pub tf: Option<bool>,
    pub elapsed_ms: i64,
    #[serde(default)]
    pub hint_used: bool,
}

#[derive(Serialize)]
pub struct ConfusedWord {
    pub word: String,
    pub reading: String,
    pub meaning: String,
}

#[derive(Serialize)]
pub struct AnswerVerdict {
    pub answer_id: i64,
    pub grade: String,
    pub label: String,
    pub correct: bool,
    pub dont_know: bool,
    pub correct_answer: String,
    pub chosen: Option<String>,
    pub reasons: Vec<String>,
    pub next_review_formatted: String,
    pub can_override: bool,
    pub overridden: bool,
    pub confused_with: Option<ConfusedWord>,
}

#[derive(Serialize)]
pub struct AnswerResult {
    pub verdict: AnswerVerdict,
    /// The answered card after grading, for showing its details on the result screen.
    pub card: Option<ReviewItem>,
    pub graduated: bool,
    pub lapsed: bool,
    #[serde(flatten)]
    pub progress: SessionProgress,
}

/// Returns (correct, chosen text, card the wrong pick belongs to).
fn check_answer(task: &QuizTask, a: &QuizAnswer) -> (bool, Option<String>, Option<String>) {
    if a.dont_know {
        return (false, None, None);
    }
    match task.kind.as_str() {
        KIND_INTRO_TF | KIND_TRUE_FALSE => {
            let said = a.tf.unwrap_or(false);
            let correct = said == task.statement_true;
            // Accepting a wrong meaning means the two words are mixed up.
            let other = if !correct && said { task.statement.as_ref().and_then(|s| s.item_id.clone()) } else { None };
            (correct, Some(if said { "true" } else { "false" }.to_string()), other)
        }
        _ => {
            let choice = a.choice.clone().unwrap_or_default();
            let correct = choice == task.answer;
            let other = if correct {
                None
            } else {
                task.options.iter().find(|o| o.text == choice).and_then(|o| o.item_id.clone())
            };
            (correct, Some(choice), other)
        }
    }
}

fn correct_answer_text(task: &QuizTask) -> String {
    match task.kind.as_str() {
        KIND_READING => task.reading.clone(),
        KIND_RECALL if task.answer == NONE_ANSWER => format!("None of these: {}", task.meaning),
        _ => task.meaning.clone(),
    }
}

fn bump_confusion(conn: &Connection, card_id: &str, other_id: &str, delta: i64) -> Result<(), String> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "INSERT INTO review_confusions (review_item_id, confused_with_id, count, last_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(review_item_id, confused_with_id) DO UPDATE SET count = count + ?3, last_at = ?4",
        params![card_id, other_id, delta, now_ms],
    )
    .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM review_confusions WHERE count <= 0", []).map_err(|e| e.to_string())?;
    Ok(())
}

fn confused_word(conn: &Connection, other_id: Option<&str>) -> Result<Option<ConfusedWord>, String> {
    let Some(id) = other_id else { return Ok(None) };
    Ok(fetch_review_item(conn, id)?.map(|c| {
        let d = parse_detail(c.detail.as_deref());
        ConfusedWord { word: c.content, reading: d.reading, meaning: d.meaning }
    }))
}

fn start_of_today_ms() -> i64 {
    let midnight = chrono::Local::now().date_naive().and_hms_opt(0, 0, 0).unwrap_or_default();
    midnight
        .and_local_timezone(chrono::Local)
        .earliest()
        .map(|t| t.timestamp_millis())
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis() - 86_400_000)
}

fn overrides_left(conn: &Connection, language: &str) -> Result<bool, String> {
    let used: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM review_answers
             WHERE overridden = 1 AND COALESCE(language, '') = ?1 AND answered_at >= ?2",
            params![language, start_of_today_ms()],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(used < OVERRIDES_PER_DAY)
}

#[derive(Serialize, Deserialize)]
struct CardState {
    ef: f64,
    i: i64,
    r: i64,
    state: String,
    step: i64,
    lapse_count: i64,
    next_review: Option<i64>,
    stability: Option<f32>,
    difficulty: Option<f32>,
    mastered_at: Option<i64>,
}

fn snapshot_state(conn: &Connection, id: &str) -> Result<String, String> {
    let s = conn
        .query_row(
            "SELECT EF, I, R, state, step, lapse_count, next_review, fsrs_stability, fsrs_difficulty, mastered_at
             FROM review_items WHERE id = ?1",
            params![id],
            |row| {
                Ok(CardState {
                    ef: row.get(0)?,
                    i: row.get(1)?,
                    r: row.get(2)?,
                    state: row.get(3)?,
                    step: row.get(4)?,
                    lapse_count: row.get(5)?,
                    next_review: row.get(6)?,
                    stability: row.get(7)?,
                    difficulty: row.get(8)?,
                    mastered_at: row.get(9)?,
                })
            },
        )
        .map_err(|e| e.to_string())?;
    serde_json::to_string(&s).map_err(|e| e.to_string())
}

fn restore_state(conn: &Connection, id: &str, json: &str) -> Result<(), String> {
    let s: CardState = serde_json::from_str(json).map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE review_items SET EF = ?1, I = ?2, R = ?3, state = ?4, step = ?5, lapse_count = ?6,
         next_review = ?7, fsrs_stability = ?8, fsrs_difficulty = ?9, mastered_at = ?10 WHERE id = ?11",
        params![s.ef, s.i, s.r, s.state, s.step, s.lapse_count, s.next_review, s.stability, s.difficulty, s.mastered_at, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn today() -> String {
    chrono::Local::now().date_naive().to_string()
}

/// Checks an answer to the card's open question, grades the card and returns the next one.
#[tauri::command]
pub fn db_session_answer(
    app: tauri::AppHandle,
    language: String,
    card_id: String,
    answer: QuizAnswer,
    max_reviews: i64,
    max_new_cards: i64,
) -> Result<AnswerResult, String> {
    let conn = open_db(&app)?;
    let study_id = ensure_study(&conn, &today(), &language, max_reviews, max_new_cards)?;
    let task = load_task(&conn, study_id, &card_id)?
        .ok_or("This card has no open question. Pull to refresh the review.")?;
    if task.kind == KIND_CLASSIC {
        return Err("This card has no meaning to check against. Grade it yourself.".into());
    }

    let (correct, chosen, other_id) = check_answer(&task, &answer);
    let usual = usual_ms(&conn, &language, &task.kind);
    let elapsed_ms = answer.elapsed_ms.max(0);
    let (grade, reasons) = compute_grade(&GradeInput {
        kind: &task.kind,
        correct,
        dont_know: answer.dont_know,
        elapsed_ms,
        hint_used: answer.hint_used,
        mature: task.mature,
        easy_allowed: task.easy_allowed,
        usual_ms: usual,
    });

    let prev_state = snapshot_state(&conn, &card_id)?;
    let srs = apply_session_grade(&conn, study_id, &card_id, grade)?;
    clear_task(&conn, study_id, &card_id)?;
    // Reading slips are about kana, not about two words being confused.
    let record_confusion = task.kind != KIND_READING;
    if let (false, true, Some(other)) = (correct, record_confusion, other_id.as_deref()) {
        bump_confusion(&conn, &card_id, other, 1)?;
    }

    conn.execute(
        "INSERT INTO review_answers (review_item_id, language, task, correct, dont_know, elapsed_ms, hint_used,
            chosen, chosen_item_id, grade, easy_allowed, mature, prev_state, answered_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            card_id, language, task.kind, correct, answer.dont_know, elapsed_ms, answer.hint_used,
            chosen, other_id, grade, task.easy_allowed, task.mature, prev_state,
            chrono::Utc::now().timestamp_millis(),
        ],
    )
    .map_err(|e| e.to_string())?;
    let answer_id = conn.last_insert_rowid();

    let verdict = AnswerVerdict {
        answer_id,
        grade: grade.to_string(),
        label: grade_label(grade).to_string(),
        correct,
        dont_know: answer.dont_know,
        correct_answer: correct_answer_text(&task),
        chosen,
        reasons,
        next_review_formatted: format_next_review_minutes(srs.next_minutes),
        can_override: !correct && !answer.dont_know && overrides_left(&conn, &language)?,
        overridden: false,
        confused_with: if record_confusion { confused_word(&conn, other_id.as_deref())? } else { None },
    };

    Ok(AnswerResult {
        verdict,
        card: fetch_review_item(&conn, &card_id)?,
        graduated: srs.graduated,
        lapsed: srs.lapsed,
        progress: session_progress(&conn, study_id)?,
    })
}

/// "I mis-tapped": re-grades a wrong answer as if it were right. Limited per day.
#[tauri::command]
pub fn db_session_override(
    app: tauri::AppHandle,
    language: String,
    answer_id: i64,
    max_reviews: i64,
    max_new_cards: i64,
) -> Result<AnswerResult, String> {
    let conn = open_db(&app)?;
    #[allow(clippy::type_complexity)]
    let row: (String, String, bool, bool, i64, bool, Option<String>, bool, bool, Option<String>, bool) = conn
        .query_row(
            "SELECT review_item_id, task, correct, dont_know, elapsed_ms, hint_used, chosen_item_id,
                    easy_allowed, mature, prev_state, overridden
             FROM review_answers WHERE id = ?1 AND COALESCE(language, '') = ?2",
            params![answer_id, language],
            |r| {
                Ok((
                    r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?,
                    r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("That answer no longer exists.")?;
    let (card_id, kind, correct, dont_know, elapsed_ms, hint_used, other_id, easy_allowed, mature, prev_state, overridden) = row;

    if correct || dont_know || overridden {
        return Err("Only a wrong pick can be marked as a mis-tap.".into());
    }
    if !overrides_left(&conn, &language)? {
        return Err("Today's mis-tap fix is already used. It resets tomorrow.".into());
    }
    let prev_state = prev_state.ok_or("This answer can't be undone.")?;

    let study_id = ensure_study(&conn, &today(), &language, max_reviews, max_new_cards)?;
    let usual = usual_ms(&conn, &language, &kind);
    let (grade, mut reasons) = compute_grade(&GradeInput {
        kind: &kind,
        correct: true,
        dont_know: false,
        elapsed_ms,
        hint_used,
        mature,
        easy_allowed,
        usual_ms: usual,
    });
    reasons.insert(0, "marked as mis-tap".into());

    restore_state(&conn, &card_id, &prev_state)?;
    let srs = apply_session_grade(&conn, study_id, &card_id, grade)?;
    clear_task(&conn, study_id, &card_id)?;
    if let (true, Some(other)) = (kind != KIND_READING, other_id.as_deref()) {
        bump_confusion(&conn, &card_id, other, -1)?;
    }
    conn.execute(
        "UPDATE review_answers SET correct = 1, grade = ?1, overridden = 1 WHERE id = ?2",
        params![grade, answer_id],
    )
    .map_err(|e| e.to_string())?;

    let card = fetch_review_item(&conn, &card_id)?;
    let correct_answer = card
        .as_ref()
        .map(|c| {
            let d = parse_detail(c.detail.as_deref());
            if kind == KIND_READING { d.reading } else { d.meaning }
        })
        .unwrap_or_default();

    Ok(AnswerResult {
        verdict: AnswerVerdict {
            answer_id,
            grade: grade.to_string(),
            label: grade_label(grade).to_string(),
            correct: true,
            dont_know: false,
            correct_answer,
            chosen: None,
            reasons,
            next_review_formatted: format_next_review_minutes(srs.next_minutes),
            can_override: false,
            overridden: true,
            confused_with: None,
        },
        card,
        graduated: srs.graduated,
        lapsed: srs.lapsed,
        progress: session_progress(&conn, study_id)?,
    })
}

#[derive(Serialize)]
pub struct MixedUp {
    pub word: String,
    pub other: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct ReadingSlip {
    pub word: String,
    pub reading: String,
    pub picked: String,
}

#[derive(Serialize)]
pub struct SessionSummary {
    pub total: i64,
    pub correct: i64,
    pub avg_ms: Option<i64>,
    pub hints: i64,
    pub again: i64,
    pub hard: i64,
    pub good: i64,
    pub easy: i64,
    pub mature_total: i64,
    /// Share of mature cards not forgotten today (0–100), compared against the 90% target.
    pub mature_retention: Option<i64>,
    pub mixed_up: Vec<MixedUp>,
    pub reading_slips: Vec<ReadingSlip>,
    pub context_misses: Vec<MixedUp>,
}

/// Today's auto-graded answers for a language, for the end-of-session screen.
#[tauri::command]
pub fn db_session_summary(app: tauri::AppHandle, language: String) -> Result<SessionSummary, String> {
    let conn = open_db(&app)?;
    let since = start_of_today_ms();
    let lang = language.as_str();

    let (total, correct, avg_ms, hints, again, hard, good, easy, mature_total, mature_kept) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(correct), 0),
                    CAST(AVG(CASE WHEN correct = 1 THEN elapsed_ms END) AS INTEGER),
                    COALESCE(SUM(hint_used), 0),
                    COALESCE(SUM(grade = 'again'), 0), COALESCE(SUM(grade = 'hard'), 0),
                    COALESCE(SUM(grade = 'good'), 0), COALESCE(SUM(grade = 'easy'), 0),
                    COALESCE(SUM(mature), 0), COALESCE(SUM(mature = 1 AND grade != 'again'), 0)
             FROM review_answers WHERE COALESCE(language, '') = ?1 AND answered_at >= ?2",
            params![lang, since],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?, r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?, r.get::<_, i64>(5)?, r.get::<_, i64>(6)?, r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?, r.get::<_, i64>(9)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?;

    let pairs = |task_filter: &str| -> Result<Vec<MixedUp>, String> {
        let sql = format!(
            "SELECT a.content, b.content, COUNT(*) FROM review_answers ra
             JOIN review_items a ON a.id = ra.review_item_id
             JOIN review_items b ON b.id = ra.chosen_item_id
             WHERE COALESCE(ra.language, '') = ?1 AND ra.answered_at >= ?2 AND ra.correct = 0 AND {}
             GROUP BY 1, 2 ORDER BY 3 DESC LIMIT 5",
            task_filter
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![lang, since], |r| {
                Ok(MixedUp { word: r.get(0)?, other: r.get(1)?, count: r.get(2)? })
            })
            .map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    };
    let mixed_up = pairs("ra.task != 'context'")?;
    let context_misses = pairs("ra.task = 'context'")?;

    let mut stmt = conn
        .prepare(
            "SELECT a.content, a.detail, ra.chosen FROM review_answers ra
             JOIN review_items a ON a.id = ra.review_item_id
             WHERE COALESCE(ra.language, '') = ?1 AND ra.answered_at >= ?2 AND ra.correct = 0
               AND ra.task = 'reading' AND ra.chosen IS NOT NULL
             ORDER BY ra.answered_at DESC LIMIT 5",
        )
        .map_err(|e| e.to_string())?;
    let reading_slips = stmt
        .query_map(params![lang, since], |r| {
            Ok(ReadingSlip {
                word: r.get(0)?,
                reading: parse_detail(r.get::<_, Option<String>>(1)?.as_deref()).reading,
                picked: r.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(SessionSummary {
        total,
        correct,
        avg_ms,
        hints,
        again,
        hard,
        good,
        easy,
        mature_total,
        mature_retention: (mature_total > 0).then(|| mature_kept * 100 / mature_total),
        mixed_up,
        reading_slips,
        context_misses,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(kind: &str, correct: bool, elapsed_ms: i64, mature: bool) -> GradeInput<'_> {
        GradeInput {
            kind,
            correct,
            dont_know: false,
            elapsed_ms,
            hint_used: false,
            mature,
            easy_allowed: mature && (kind == KIND_RECALL || kind == KIND_CONTEXT),
            usual_ms: 3500,
        }
    }

    #[test]
    fn grades_follow_the_rules() {
        assert_eq!(compute_grade(&input(KIND_RECALL, false, 1000, true)).0, "again");
        assert_eq!(compute_grade(&input(KIND_RECALL, true, 1000, true)).0, "easy");
        assert_eq!(compute_grade(&input(KIND_RECALL, true, 1000, false)).0, "good");
        assert_eq!(compute_grade(&input(KIND_RECALL, true, 3000, true)).0, "good");
        assert_eq!(compute_grade(&input(KIND_RECALL, true, 8000, true)).0, "hard");
        assert_eq!(compute_grade(&input(KIND_READING, true, 1000, true)).0, "good");
        assert_eq!(compute_grade(&input(KIND_READING, false, 1000, true)).0, "hard");
        assert_eq!(compute_grade(&input(KIND_READING, false, 1000, false)).0, "again");
        assert_eq!(compute_grade(&input(KIND_CONTEXT, false, 1000, true)).0, "again");
        let mut hinted = input(KIND_RECALL, true, 1000, true);
        hinted.hint_used = true;
        assert_eq!(compute_grade(&hinted).0, "hard");
        let mut dk = input(KIND_RECALL, true, 1000, true);
        dk.dont_know = true;
        assert_eq!(compute_grade(&dk).0, "again");
    }

    #[test]
    fn parses_json_and_plain_detail() {
        let d = parse_detail(Some("```json\n{\"reading\":\"やくそく\",\"meaning\":\"promise\"}\n```"));
        assert_eq!(d.reading, "やくそく");
        assert_eq!(d.meaning, "promise");
        assert_eq!(parse_detail(Some("to eat")).meaning, "to eat");
        assert!(parse_detail(None).meaning.is_empty());
    }

    #[test]
    fn reading_variants_are_plausible_and_different() {
        let v = reading_variants("かいぎ");
        assert!(v.contains(&"かいき".to_string()));
        assert!(v.contains(&"がいぎ".to_string()));
        assert!(!v.contains(&"かいぎ".to_string()));
        assert!(reading_variants("ふつう").contains(&"ふっつう".to_string()));
    }

    #[test]
    fn stored_task_keeps_hidden_answer() {
        let t = QuizTask {
            kind: KIND_RECALL.into(),
            word: "約束".into(),
            reading: "やくそく".into(),
            meaning: "promise".into(),
            options: vec![],
            statement: None,
            sentence: None,
            hint_available: true,
            mature: true,
            answer: NONE_ANSWER.into(),
            statement_true: false,
            easy_allowed: true,
        };
        let public = serde_json::to_string(&t).unwrap();
        assert!(!public.contains("easy_allowed") && !public.contains(NONE_ANSWER));
        let stored: QuizTask = serde_json::from_str(&serde_json::to_string(&StoredTask(&t)).unwrap()).unwrap();
        assert_eq!(stored.answer, NONE_ANSWER);
        assert!(stored.easy_allowed);
    }
}
