//! Auto-graded review. Instead of the user rating themselves, each card gets a small question
//! (true/false, pick the meaning, recall then confirm, pick the reading, meaning in context;
//! for grammar cards: meaning in a sentence, fill the gap, find the mistake).
//! The answer is checked here and correctness, answer time and hints are turned into the FSRS grade.

use crate::db::open_db;
use crate::grammar_review::{generate_card_back, parse_grammar_detail, GrammarDetail, GrammarExample, ITEM_GRAMMAR};
use crate::locale::{app_error, effective_detail_sql, explain_lang};
use crate::srs::{
    apply_session_grade, ensure_study, fetch_review_item, format_next_review_minutes,
    requeue_card, session_progress, set_card_status, ReviewItem, SessionProgress,
};
use crate::words::is_cjk;
use rand::seq::SliceRandom;
use rand::Rng;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Self-graded flip; only when the deck has no other meanings to build wrong options from.
pub const KIND_CLASSIC: &str = "classic";
/// The card has no meaning yet: generate or write the back, approve it, and it comes back later.
pub const KIND_NEEDS_MEANING: &str = "needs_meaning";
pub const KIND_INTRO_TF: &str = "intro_tf";
pub const KIND_TRUE_FALSE: &str = "true_false";
pub const KIND_CHOICE: &str = "choice";
pub const KIND_RECALL: &str = "recall_confirm";
pub const KIND_READING: &str = "reading";
pub const KIND_CONTEXT: &str = "context";
/// Grammar: what the highlighted pattern means in a sentence.
pub const KIND_GRAMMAR_MEANING: &str = "grammar_meaning";
/// Grammar: pick the filler for the gap where the pattern goes.
pub const KIND_GRAMMAR_GAP: &str = "grammar_gap";
/// Grammar: tap the part of the sentence with the mistake.
pub const KIND_GRAMMAR_MISTAKE: &str = "grammar_mistake";

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

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct CardDetail {
    pub reading: String,
    pub meaning: String,
    pub part_of_speech: String,
    pub example: String,
    pub example_meaning: String,
}

/// Parses `review_items.detail`: the JSON object written by `db_generate_detail`, or the plain
/// text meaning from a CSV import (`word,meaning`).
pub fn parse_detail(detail: Option<&str>) -> CardDetail {
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

/// The separate senses in a meaning: "to travel; trip (abroad)" → {travel, trip}.
/// Notes in brackets and a leading "to"/"a"/"the" don't count.
fn glosses(meaning: &str) -> HashSet<String> {
    let mut plain = String::new();
    let mut depth = 0;
    for c in meaning.chars() {
        match c {
            '(' | '（' | '[' => depth += 1,
            ')' | '）' | ']' => depth = (depth - 1).max(0),
            _ if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    plain
        .split([';', ',', '/', '、', '；', '，'])
        .map(|g| {
            let g = g.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
            let g = ["to ", "a ", "an ", "the "].iter().find_map(|p| g.strip_prefix(p)).map(str::to_string).unwrap_or(g);
            g.trim().to_string()
        })
        .filter(|g| !g.is_empty())
        .collect()
}

/// Two meanings sharing a sense ("travel; trip" and "travel; journey; trip") are both right,
/// so one is never a wrong option for the other.
fn meanings_overlap(a: &HashSet<String>, b: &HashSet<String>) -> bool {
    !a.is_disjoint(b)
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
pub struct ExampleView {
    pub sentence: String,
    pub target: String,
    pub translation: String,
}

/// What a grammar card teaches, for the intro and the result screen.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct GrammarInfo {
    pub level: String,
    pub meaning: String,
    pub form: String,
    pub note: String,
    /// Only where showing them gives nothing away (intro, true/false, a draft waiting for approval).
    pub examples: Vec<ExampleView>,
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
    /// Needs-meaning: an AI-generated back waiting for approval.
    #[serde(default)]
    pub draft: Option<CardDetail>,
    /// Grammar cards only.
    #[serde(default)]
    pub grammar: Option<GrammarInfo>,
    /// A level check: a grammar card below the learner's level, asked once before it is taught.
    #[serde(default)]
    pub placement: bool,
    #[serde(skip_serializing, default)]
    answer: String,
    /// Find the mistake: the corrected chunk.
    #[serde(skip_serializing, default)]
    fix: String,
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
            draft: None,
            grammar: None,
            placement: false,
            answer: detail.meaning.clone(),
            fix: String::new(),
            statement_true: false,
            easy_allowed: false,
        }
    }
}

struct Candidate {
    id: String,
    content: String,
    detail: CardDetail,
    raw: Option<String>,
    level: Option<String>,
}

fn load_candidates(conn: &Connection, card: &ReviewItem) -> Result<Vec<Candidate>, String> {
    // Only meanings in the learner's language: options in one question never mix languages.
    // Words get wrong options from words, grammar from grammar.
    // Another form or spelling of the same word (下がります for 下がる, さげる for 下げる) is never a wrong option.
    let mut stmt = conn
        .prepare(&format!(
            "SELECT id, content, {}, level FROM review_items
             WHERE COALESCE(language, '') = COALESCE(?1, '') AND id != ?2 AND COALESCE(item_type, 'word') = ?3
               AND COALESCE(lemma, content) != COALESCE((SELECT COALESCE(lemma, content) FROM review_items WHERE id = ?2), '')",
            effective_detail_sql("")
        ))
        .map_err(|e| e.to_string())?;
    let reading = if card.is_grammar() { String::new() } else { parse_detail(card.detail.as_deref()).reading };
    let rows = stmt
        .query_map(params![card.language, card.id, card.item_type], |row| {
            let raw: Option<String> = row.get(2)?;
            Ok(Candidate {
                id: row.get(0)?,
                content: row.get(1)?,
                detail: parse_detail(raw.as_deref()),
                raw,
                level: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;
    Ok(rows
        .filter_map(|r| r.ok())
        .filter(|c| reading.is_empty() || (c.detail.reading != reading && c.content != reading))
        .collect())
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
/// words with the same part of speech and grammar of the same level come first.
/// A meaning sharing a sense with the answer (or with another option) is never offered,
/// so a shared kanji only ranks up words that mean something else (旅館 for 旅行, not 旅).
fn meaning_distractors(
    card: &ReviewItem,
    detail: &CardDetail,
    candidates: &[Candidate],
    confusions: &HashMap<String, i64>,
    n: usize,
    rng: &mut impl Rng,
) -> Vec<QuizOption> {
    let target_glosses = glosses(&detail.meaning);
    let target_chars: HashSet<char> = card.content.chars().filter(|c| is_cjk(*c)).collect();
    let target_pos = normalize(&detail.part_of_speech);

    let mut scored: Vec<(f64, HashSet<String>, QuizOption)> = Vec::new();
    for c in candidates {
        let g = glosses(&c.detail.meaning);
        if g.is_empty() || meanings_overlap(&g, &target_glosses) {
            continue;
        }
        let mut score: f64 = rng.gen_range(0.0..3.0);
        score += 100.0 * *confusions.get(&c.id).unwrap_or(&0) as f64;
        score += 10.0 * c.content.chars().filter(|ch| target_chars.contains(ch)).count() as f64;
        if !target_pos.is_empty() && normalize(&c.detail.part_of_speech) == target_pos {
            score += 5.0;
        }
        if card.level.is_some() && c.level == card.level {
            score += 5.0;
        }
        scored.push((score, g, QuizOption { text: c.detail.meaning.clone(), item_id: Some(c.id.clone()) }));
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut taken: Vec<HashSet<String>> = Vec::new();
    let mut out = Vec::new();
    for (_, g, option) in scored {
        if out.len() == n {
            break;
        }
        if taken.iter().any(|t| meanings_overlap(t, &g)) {
            continue;
        }
        taken.push(g);
        out.push(option);
    }
    out
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

fn draft_text(conn: &Connection, id: &str) -> Result<Option<String>, String> {
    Ok(conn
        .query_row("SELECT draft_detail FROM review_items WHERE id = ?1", params![id], |row| row.get(0))
        .optional()
        .map_err(|e| e.to_string())?
        .flatten())
}

fn load_draft(conn: &Connection, id: &str) -> Result<Option<CardDetail>, String> {
    Ok(Some(parse_detail(draft_text(conn, id)?.as_deref())).filter(|d| !d.meaning.is_empty()))
}

/// Whether a card's back (or draft) has what its questions need.
fn has_back(item_type: &str, raw: Option<&str>) -> bool {
    if item_type == ITEM_GRAMMAR {
        parse_grammar_detail(raw).is_usable()
    } else {
        !parse_detail(raw).meaning.is_empty()
    }
}

/// `meaning_just_shown`: the user has just approved this card's back, so a new card skips the intro.
fn build_task(conn: &Connection, card: &ReviewItem, meaning_just_shown: bool, rng: &mut impl Rng) -> Result<QuizTask, String> {
    if card.is_grammar() {
        return build_grammar_task(conn, card, meaning_just_shown, rng);
    }
    let detail = parse_detail(card.detail.as_deref());
    if detail.meaning.is_empty() {
        let mut task = QuizTask::base(KIND_NEEDS_MEANING, card, &detail);
        task.draft = load_draft(conn, &card.id)?;
        return Ok(task);
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
    } else if card.state == "new" && !meaning_just_shown {
        KIND_INTRO_TF
    } else if card.state == "new" || pool.len() < 3 {
        // New card whose meaning was just approved, or too few words for 4 options.
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

fn grammar_info(card: &ReviewItem, g: &GrammarDetail, examples: &[&GrammarExample]) -> GrammarInfo {
    GrammarInfo {
        level: card.level.clone().unwrap_or_default(),
        meaning: g.meaning.clone(),
        form: g.form.clone(),
        note: g.note.clone(),
        examples: examples
            .iter()
            .map(|e| ExampleView { sentence: e.sentence.clone(), target: e.target.clone(), translation: e.translation.clone() })
            .collect(),
    }
}

/// Wrong fillers for a gap: the ones made with the example, then patterns of other grammar cards.
fn gap_fillers(example: &GrammarExample, candidates: &[Candidate], rng: &mut impl Rng) -> Vec<QuizOption> {
    let mut seen: HashSet<String> = HashSet::from([example.target.clone()]);
    let mut out: Vec<QuizOption> = example
        .distractors
        .iter()
        .filter(|d| seen.insert((*d).clone()))
        .map(|d| QuizOption { text: d.clone(), item_id: None })
        .collect();
    if out.len() < 3 {
        let mut others: Vec<QuizOption> = candidates
            .iter()
            .flat_map(|c| {
                parse_grammar_detail(c.raw.as_deref())
                    .examples
                    .into_iter()
                    .map(|e| QuizOption { text: e.target, item_id: Some(c.id.clone()) })
            })
            .collect();
        others.shuffle(rng);
        out.extend(others.into_iter().filter(|o| seen.insert(o.text.clone())));
    }
    out.truncate(3);
    out
}

/// Grammar questions by stage: a level check is one fill-the-gap; new → intro, then true/false on the
/// meaning; learning → what the pattern means in a sentence; review → fill the gap; mature → fill the gap
/// without the translation, find the mistake, or the meaning in a sentence.
fn build_grammar_task(conn: &Connection, card: &ReviewItem, meaning_just_shown: bool, rng: &mut impl Rng) -> Result<QuizTask, String> {
    let g = parse_grammar_detail(card.detail.as_deref());
    let detail = CardDetail { meaning: g.meaning.clone(), ..Default::default() };
    if !g.is_usable() {
        let mut task = QuizTask::base(KIND_NEEDS_MEANING, card, &detail);
        let draft = Some(parse_grammar_detail(draft_text(conn, &card.id)?.as_deref())).filter(|d| d.is_usable());
        if let Some(d) = draft {
            let examples: Vec<&GrammarExample> = d.examples.iter().collect();
            task.grammar = Some(grammar_info(card, &d, &examples));
            task.draft = Some(CardDetail { meaning: d.meaning.clone(), ..Default::default() });
            if !d.title.is_empty() {
                task.word = d.title.clone();
            }
        }
        return Ok(task);
    }

    let candidates = load_candidates(conn, card)?;
    let confusions = load_confusions(conn, &card.id)?;
    let mut pool = meaning_distractors(card, &detail, &candidates, &confusions, 4, rng);
    // Too little grammar in the deck yet: the pattern's own look-alikes fill in.
    let mut taken: Vec<HashSet<String>> = pool.iter().map(|o| glosses(&o.text)).collect();
    taken.push(glosses(&g.meaning));
    for c in &g.confusables {
        let cg = glosses(&c.meaning);
        if pool.len() < 4 && !cg.is_empty() && !taken.iter().any(|t| meanings_overlap(t, &cg)) {
            pool.push(QuizOption { text: c.meaning.clone(), item_id: None });
            taken.push(cg);
        }
    }
    let example = g.examples.choose(rng).cloned().unwrap_or_default();
    let fillers = gap_fillers(&example, &candidates, rng);
    let mature = is_mature(card);
    let new = card.state == "new";

    let preferred = if new && card.placement {
        KIND_GRAMMAR_GAP
    } else if new && !meaning_just_shown {
        KIND_INTRO_TF
    } else if new {
        KIND_TRUE_FALSE
    } else if card.state != "review" {
        KIND_GRAMMAR_MEANING
    } else if mature {
        let mut kinds = vec![KIND_GRAMMAR_GAP, KIND_GRAMMAR_GAP, KIND_GRAMMAR_MEANING];
        if g.mistake.is_some() {
            kinds.push(KIND_GRAMMAR_MISTAKE);
        }
        *kinds.choose(rng).unwrap_or(&KIND_GRAMMAR_GAP)
    } else {
        KIND_GRAMMAR_GAP
    };
    let possible = |k: &str| match k {
        KIND_GRAMMAR_GAP => !fillers.is_empty(),
        KIND_GRAMMAR_MEANING => pool.len() >= 3,
        KIND_GRAMMAR_MISTAKE => g.mistake.is_some(),
        _ => !pool.is_empty(),
    };
    let kind = [preferred, KIND_GRAMMAR_GAP, KIND_GRAMMAR_MEANING, KIND_TRUE_FALSE]
        .into_iter()
        .find(|k| possible(k))
        .unwrap_or(KIND_CLASSIC);

    let mut task = QuizTask::base(kind, card, &detail);
    if !g.title.is_empty() {
        task.word = g.title.clone();
    }
    task.mature = mature;
    task.placement = new && card.placement;
    task.easy_allowed = mature && [KIND_GRAMMAR_GAP, KIND_GRAMMAR_MEANING, KIND_GRAMMAR_MISTAKE].contains(&kind);
    let shown: Vec<&GrammarExample> = if kind == KIND_INTRO_TF || kind == KIND_TRUE_FALSE { vec![&example] } else { vec![] };
    task.grammar = Some(grammar_info(card, &g, &shown));
    let correct = QuizOption { text: g.meaning.clone(), item_id: Some(card.id.clone()) };

    match kind {
        KIND_INTRO_TF | KIND_TRUE_FALSE => {
            task.statement_true = rng.gen_bool(0.5);
            task.statement = Some(if task.statement_true { correct } else { pool[0].clone() });
            task.answer = task.statement_true.to_string();
        }
        KIND_GRAMMAR_MEANING => {
            task.options = pool.into_iter().take(3).chain(std::iter::once(correct)).collect();
            task.options.shuffle(rng);
            task.sentence = split_sentence(&example.sentence, &example.target, &example.translation);
        }
        KIND_GRAMMAR_GAP => {
            task.options = fillers
                .into_iter()
                .chain(std::iter::once(QuizOption { text: example.target.clone(), item_id: Some(card.id.clone()) }))
                .collect();
            task.options.shuffle(rng);
            task.answer = example.target.clone();
            // Mature cards get no translation to lean on.
            let translation = if mature { "" } else { example.translation.as_str() };
            task.sentence = split_sentence(&example.sentence, &example.target, translation)
                .map(|s| SentenceParts { target: String::new(), ..s });
        }
        KIND_GRAMMAR_MISTAKE => {
            if let Some(m) = &g.mistake {
                task.options = m.parts.iter().map(|p| QuizOption { text: p.clone(), item_id: None }).collect();
                task.answer = m.parts[m.wrong].clone();
                task.fix = m.fix.clone();
                task.sentence = Some(SentenceParts {
                    before: String::new(),
                    target: String::new(),
                    after: String::new(),
                    translation: m.translation.clone(),
                });
            }
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
    match load_task(conn, study_id, &card.id)? {
        // Cards without a quiz are re-checked each time: a meaning or more cards may have been added since.
        Some(task) if task.kind != KIND_NEEDS_MEANING && task.kind != KIND_CLASSIC => Ok(task),
        _ => store_new_task(conn, study_id, card, false),
    }
}

fn store_new_task(conn: &Connection, study_id: i64, card: &ReviewItem, meaning_just_shown: bool) -> Result<QuizTask, String> {
    let task = build_task(conn, card, meaning_just_shown, &mut rand::thread_rng())?;
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
        let mut st = s.serialize_struct("QuizTask", 17)?;
        st.serialize_field("kind", &t.kind)?;
        st.serialize_field("word", &t.word)?;
        st.serialize_field("reading", &t.reading)?;
        st.serialize_field("meaning", &t.meaning)?;
        st.serialize_field("options", &t.options)?;
        st.serialize_field("statement", &t.statement)?;
        st.serialize_field("sentence", &t.sentence)?;
        st.serialize_field("hint_available", &t.hint_available)?;
        st.serialize_field("mature", &t.mature)?;
        st.serialize_field("draft", &t.draft)?;
        st.serialize_field("grammar", &t.grammar)?;
        st.serialize_field("placement", &t.placement)?;
        st.serialize_field("answer", &t.answer)?;
        st.serialize_field("fix", &t.fix)?;
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
        KIND_GRAMMAR_MEANING | KIND_GRAMMAR_GAP => 6000,
        KIND_GRAMMAR_MISTAKE => 8000,
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

/// Why a card got its grade. `code` is translated by the frontend; `ms` fills in times.
#[derive(Serialize, Debug, PartialEq)]
pub struct GradeReason {
    pub code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms: Option<i64>,
}

fn reason(code: &'static str) -> GradeReason {
    GradeReason { code, ms: None }
}

fn timed(code: &'static str, ms: i64) -> GradeReason {
    GradeReason { code, ms: Some(ms) }
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
    /// A level check: right means already known, wrong means it gets taught.
    placement: bool,
}

fn compute_grade(g: &GradeInput) -> (&'static str, Vec<GradeReason>) {
    if g.dont_know {
        return ("again", vec![reason("didnt_know")]);
    }
    if g.placement {
        return if g.correct {
            ("easy", vec![timed("answered_in", g.elapsed_ms), reason("check_passed")])
        } else {
            ("again", vec![reason("check_missed")])
        };
    }
    if !g.correct {
        // A mature card tests the meaning; a misread shouldn't reset months of memory.
        if g.kind == KIND_READING && g.mature {
            return ("hard", vec![reason("reading_slip"), reason("meaning_still_known"), reason("no_lapse_mature")]);
        }
        let code = if g.kind == KIND_CONTEXT || g.kind == KIND_GRAMMAR_MEANING { "wrong_in_sentence" } else { "wrong_answer" };
        return ("again", vec![reason(code)]);
    }
    let verb = if g.kind == KIND_RECALL { "recalled_in" } else { "answered_in" };
    let mut reasons = vec![timed(verb, g.elapsed_ms)];
    if g.hint_used {
        reasons.push(reason("used_hint"));
        return ("hard", reasons);
    }
    if g.elapsed_ms >= g.usual_ms * 2 {
        reasons.push(timed("slower_than_usual", g.usual_ms));
        return ("hard", reasons);
    }
    reasons.push(reason("no_hints"));
    let fast = g.elapsed_ms <= g.usual_ms * 6 / 10;
    if fast && g.easy_allowed {
        reasons.push(timed("faster_than_usual", g.usual_ms));
        reasons.push(reason("mature_card"));
        return ("easy", reasons);
    }
    if fast {
        reasons.push(reason(if g.mature { "capped_good" } else { "easy_needs_mature" }));
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
    /// Recall → confirm where the right meaning was not among the options ("None of these").
    pub none_correct: bool,
    pub chosen: Option<String>,
    pub reasons: Vec<GradeReason>,
    pub next_review_formatted: String,
    /// Minutes until the card is due again, for the frontend to format.
    pub next_minutes: i64,
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
        KIND_GRAMMAR_GAP => task.answer.clone(),
        KIND_GRAMMAR_MISTAKE => format!("{} → {}", task.answer, task.fix),
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
        if c.is_grammar() {
            let g = parse_grammar_detail(c.detail.as_deref());
            let word = if g.title.is_empty() { c.content } else { g.title };
            return ConfusedWord { word, reading: String::new(), meaning: g.meaning };
        }
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
    #[serde(default)]
    placement: bool,
}

fn snapshot_state(conn: &Connection, id: &str) -> Result<String, String> {
    let s = conn
        .query_row(
            "SELECT EF, I, R, state, step, lapse_count, next_review, fsrs_stability, fsrs_difficulty, mastered_at,
                    COALESCE(placement, 0)
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
                    placement: row.get(10)?,
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
         next_review = ?7, fsrs_stability = ?8, fsrs_difficulty = ?9, mastered_at = ?10, placement = ?11 WHERE id = ?12",
        params![s.ef, s.i, s.r, s.state, s.step, s.lapse_count, s.next_review, s.stability, s.difficulty, s.mastered_at, s.placement, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn today() -> String {
    chrono::Local::now().date_naive().to_string()
}

/// After its level check a grammar card is an ordinary card: known, or taught from here on.
fn end_level_check(conn: &Connection, card_id: &str) -> Result<(), String> {
    conn.execute("UPDATE review_items SET placement = 0 WHERE id = ?1", params![card_id])
        .map_err(|e| e.to_string())?;
    Ok(())
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
        .ok_or_else(|| app_error("no_open_question", ""))?;
    if task.kind == KIND_CLASSIC || task.kind == KIND_NEEDS_MEANING {
        return Err(app_error("no_question_yet", ""));
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
        placement: task.placement,
    });

    let prev_state = snapshot_state(&conn, &card_id)?;
    let srs = apply_session_grade(&conn, study_id, &card_id, grade)?;
    clear_task(&conn, study_id, &card_id)?;
    if task.placement {
        end_level_check(&conn, &card_id)?;
    }
    // Reading slips are about kana, not about two words being confused.
    let record_confusion = task.kind != KIND_READING;
    if let (false, true, Some(other)) = (correct, record_confusion, other_id.as_deref()) {
        bump_confusion(&conn, &card_id, other, 1)?;
    }

    conn.execute(
        "INSERT INTO review_answers (review_item_id, language, task, correct, dont_know, elapsed_ms, hint_used,
            chosen, chosen_item_id, grade, easy_allowed, mature, prev_state, answered_at, placement)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            card_id, language, task.kind, correct, answer.dont_know, elapsed_ms, answer.hint_used,
            chosen, other_id, grade, task.easy_allowed, task.mature, prev_state,
            chrono::Utc::now().timestamp_millis(), task.placement,
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
        none_correct: task.kind == KIND_RECALL && task.answer == NONE_ANSWER,
        chosen,
        reasons,
        next_review_formatted: format_next_review_minutes(srs.next_minutes),
        next_minutes: srs.next_minutes,
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
    let row: (String, String, bool, bool, i64, bool, Option<String>, bool, bool, Option<String>, bool, bool) = conn
        .query_row(
            "SELECT review_item_id, task, correct, dont_know, elapsed_ms, hint_used, chosen_item_id,
                    easy_allowed, mature, prev_state, overridden, placement
             FROM review_answers WHERE id = ?1 AND COALESCE(language, '') = ?2",
            params![answer_id, language],
            |r| {
                Ok((
                    r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?,
                    r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| app_error("answer_gone", ""))?;
    let (card_id, kind, correct, dont_know, elapsed_ms, hint_used, other_id, easy_allowed, mature, prev_state, overridden, placement) = row;

    if correct || dont_know || overridden {
        return Err(app_error("mistap_only_wrong", ""));
    }
    if !overrides_left(&conn, &language)? {
        return Err(app_error("mistap_used", ""));
    }
    let prev_state = prev_state.ok_or_else(|| app_error("cannot_undo", ""))?;

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
        placement,
    });
    reasons.insert(0, reason("marked_mistap"));

    restore_state(&conn, &card_id, &prev_state)?;
    let srs = apply_session_grade(&conn, study_id, &card_id, grade)?;
    clear_task(&conn, study_id, &card_id)?;
    if placement {
        end_level_check(&conn, &card_id)?;
    }
    if let (true, Some(other)) = (kind != KIND_READING, other_id.as_deref()) {
        bump_confusion(&conn, &card_id, other, -1)?;
    }
    conn.execute(
        "UPDATE review_answers SET correct = 1, grade = ?1, overridden = 1 WHERE id = ?2",
        params![grade, answer_id],
    )
    .map_err(|e| e.to_string())?;

    let card = fetch_review_item(&conn, &card_id)?;
    // The question is gone; for gap and mistake questions the pick itself was the right answer.
    let chosen: Option<String> = conn
        .query_row("SELECT chosen FROM review_answers WHERE id = ?1", params![answer_id], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let correct_answer = card
        .as_ref()
        .map(|c| {
            let d = parse_detail(c.detail.as_deref());
            match kind.as_str() {
                KIND_READING => d.reading,
                KIND_GRAMMAR_GAP | KIND_GRAMMAR_MISTAKE => chosen.clone().unwrap_or_default(),
                _ => d.meaning,
            }
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
            none_correct: false,
            chosen: None,
            reasons,
            next_review_formatted: format_next_review_minutes(srs.next_minutes),
            next_minutes: srs.next_minutes,
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

/// Saves the approved back of a card that had no meaning and moves it to the end of today's
/// queue. Not graded: it comes back later in the session as a real question.
#[tauri::command]
pub fn db_session_save_meaning(
    app: tauri::AppHandle,
    language: String,
    card_id: String,
    meaning: CardDetail,
    max_reviews: i64,
    max_new_cards: i64,
) -> Result<SessionProgress, String> {
    let clean = |s: &str| s.trim().to_string();
    let detail = CardDetail {
        reading: clean(&meaning.reading),
        meaning: clean(&meaning.meaning),
        part_of_speech: clean(&meaning.part_of_speech),
        example: clean(&meaning.example),
        example_meaning: clean(&meaning.example_meaning),
    };
    if detail.meaning.is_empty() {
        return Err(app_error("meaning_required", ""));
    }
    let conn = open_db(&app)?;
    let item_type: String = conn
        .query_row("SELECT COALESCE(item_type, 'word') FROM review_items WHERE id = ?1", params![card_id], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| app_error("card_gone", ""))?;
    let json = if item_type == ITEM_GRAMMAR {
        // Practice material can't be written by hand: keep the AI draft, with the meaning as edited.
        let mut g = parse_grammar_detail(draft_text(&conn, &card_id)?.as_deref());
        if !g.is_usable() {
            return Err(app_error("practice_required", ""));
        }
        g.meaning = detail.meaning.clone();
        serde_json::to_string(&g)
    } else {
        serde_json::to_string(&detail)
    }
    .map_err(|e| e.to_string())?;

    conn.execute(
        "UPDATE review_items SET detail = ?1, meaning_lang = ?2, draft_detail = NULL WHERE id = ?3",
        params![json, explain_lang(&conn), card_id],
    )
    .map_err(|e| e.to_string())?;

    let study_id = ensure_study(&conn, &today(), &language, max_reviews, max_new_cards)?;
    let card = fetch_review_item(&conn, &card_id)?.ok_or_else(|| app_error("card_gone", ""))?;
    store_new_task(&conn, study_id, &card, true)?;
    // It comes back in the next round, while the meaning is still fresh.
    requeue_card(&conn, study_id, &card_id)?;
    session_progress(&conn, study_id)
}

/// "Skip for today": takes a card out of today's queue without grading it.
#[tauri::command]
pub fn db_session_skip_card(
    app: tauri::AppHandle,
    language: String,
    card_id: String,
    max_reviews: i64,
    max_new_cards: i64,
) -> Result<SessionProgress, String> {
    let conn = open_db(&app)?;
    let study_id = ensure_study(&conn, &today(), &language, max_reviews, max_new_cards)?;
    clear_task(&conn, study_id, &card_id)?;
    set_card_status(&conn, study_id, &card_id, "skipped")?;
    session_progress(&conn, study_id)
}

/// Cards still waiting in today's queue that have neither a meaning (in the learner's language) nor a draft.
pub(crate) fn cards_missing_meaning(conn: &Connection, study_id: i64) -> Result<Vec<(String, String)>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT r.id, r.content, {}, r.draft_detail, COALESCE(r.item_type, 'word') FROM study_card sc
             JOIN review_items r ON r.id = sc.review_item_id
             WHERE sc.study_id = ?1 AND sc.status = 'pending'
             ORDER BY sc.update_time",
            effective_detail_sql("r.")
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![study_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    Ok(rows
        .filter_map(|r| r.ok())
        .filter(|(_, _, detail, draft, item_type)| {
            !has_back(item_type, detail.as_deref()) && !has_back(item_type, draft.as_deref())
        })
        .map(|(id, content, _, _, _)| (id, content))
        .collect())
}

#[derive(Serialize)]
pub struct GenerateAllResult {
    pub generated: usize,
    pub failed: Vec<String>,
}

/// "Generate all": drafts a back for every card in today's queue that has no meaning. Each draft
/// still waits for the user's approval when its card comes up.
#[tauri::command]
pub async fn db_generate_missing_meanings(
    app: tauri::AppHandle,
    api_host: String,
    api_key: String,
    model: String,
    language: String,
    max_reviews: i64,
    max_new_cards: i64,
) -> Result<GenerateAllResult, String> {
    let (missing, explain) = {
        let conn = open_db(&app)?;
        let study_id = ensure_study(&conn, &today(), &language, max_reviews, max_new_cards)?;
        (cards_missing_meaning(&conn, study_id)?, explain_lang(&conn))
    };
    let mut generated = 0;
    let mut failed = Vec::new();
    for (id, content) in missing {
        match generate_card_back(&app, &api_host, &api_key, &model, &content, &language, &explain).await {
            Ok(text) if !parse_detail(Some(&text)).meaning.is_empty() => {
                let conn = open_db(&app)?;
                conn.execute("UPDATE review_items SET draft_detail = ?1 WHERE id = ?2", params![text, id])
                    .map_err(|e| e.to_string())?;
                generated += 1;
            }
            _ => failed.push(content),
        }
    }
    Ok(GenerateAllResult { generated, failed })
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
    /// Answers today on word cards and on grammar cards.
    pub words: i64,
    pub grammar: i64,
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
    let (words, grammar): (i64, i64) = conn
        .query_row(
            "SELECT COALESCE(SUM(COALESCE(r.item_type, 'word') != 'grammar'), 0), COALESCE(SUM(r.item_type = 'grammar'), 0)
             FROM review_answers ra JOIN review_items r ON r.id = ra.review_item_id
             WHERE COALESCE(ra.language, '') = ?1 AND ra.answered_at >= ?2",
            params![lang, since],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
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
        words,
        grammar,
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
            placement: false,
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
    fn reasons_are_codes_with_times() {
        let (_, reasons) = compute_grade(&input(KIND_RECALL, true, 8000, true));
        assert_eq!(reasons, vec![timed("recalled_in", 8000), timed("slower_than_usual", 3500)]);
        let (_, reasons) = compute_grade(&input(KIND_READING, false, 1000, true));
        assert_eq!(reasons[0], reason("reading_slip"));
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

    fn card(id: &str, content: &str, state: &str, detail: Option<&str>) -> ReviewItem {
        ReviewItem {
            id: id.into(),
            content: content.into(),
            EF: -1.0,
            I: 0,
            R: 0,
            state: state.into(),
            step: 0,
            lapse_count: 0,
            next_review: None,
            language: Some("ja".into()),
            detail: detail.map(String::from),
            mnemonic: None,
            fsrs_stability: None,
            fsrs_difficulty: None,
            difficulty_bg_class: String::new(),
            difficulty_label: String::new(),
            state_label: String::new(),
            next_review_formatted: String::new(),
            item_type: "word".into(),
            level: None,
            placement: false,
        }
    }

    fn deck() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE app_settings(key TEXT PRIMARY KEY, value TEXT);
             CREATE TABLE review_items(id TEXT PRIMARY KEY, content TEXT, language TEXT, detail TEXT, draft_detail TEXT, meaning_lang TEXT DEFAULT 'en',
               item_type TEXT DEFAULT 'word', level TEXT, lemma TEXT);
             CREATE TABLE review_confusions(review_item_id TEXT, confused_with_id TEXT, count INTEGER, last_at INTEGER);
             INSERT INTO review_items(id, content, language, detail, draft_detail) VALUES
               ('a', '散歩', 'ja', NULL, '{\"reading\":\"さんぽ\",\"meaning\":\"walk\"}'),
               ('b', '先生', 'ja', 'teacher', NULL),
               ('c', '学生', 'ja', 'student', NULL);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn card_without_meaning_asks_for_one_and_shows_draft() {
        let conn = deck();
        let t = build_task(&conn, &card("a", "散歩", "new", None), false, &mut rand::thread_rng()).unwrap();
        assert_eq!(t.kind, KIND_NEEDS_MEANING);
        assert_eq!(t.draft.unwrap().meaning, "walk");
    }

    #[test]
    fn new_card_skips_intro_after_meaning_was_approved() {
        let conn = deck();
        let c = card("b", "先生", "new", Some("teacher"));
        assert_eq!(build_task(&conn, &c, false, &mut rand::thread_rng()).unwrap().kind, KIND_INTRO_TF);
        assert_eq!(build_task(&conn, &c, true, &mut rand::thread_rng()).unwrap().kind, KIND_TRUE_FALSE);
    }

    #[test]
    fn options_never_mix_meaning_languages() {
        let conn = deck();
        conn.execute_batch(
            "INSERT INTO review_items(id, content, language, detail, meaning_lang) VALUES
               ('d', '本', 'ja', 'buku', 'id'),
               ('e', '水', 'ja', 'air', 'id');",
        )
        .unwrap();
        let c = card("b", "先生", "review", Some("teacher"));
        let cands = load_candidates(&conn, &c).unwrap();
        let meanings: Vec<String> = cands.iter().map(|x| x.detail.meaning.clone()).filter(|m| !m.is_empty()).collect();
        assert_eq!(meanings, vec!["student".to_string()]);
        conn.execute("INSERT INTO app_settings VALUES ('explain_lang', 'id')", []).unwrap();
        let cands = load_candidates(&conn, &c).unwrap();
        let mut meanings: Vec<String> = cands.iter().map(|x| x.detail.meaning.clone()).filter(|m| !m.is_empty()).collect();
        meanings.sort();
        assert_eq!(meanings, vec!["air".to_string(), "buku".to_string()]);
    }

    #[test]
    fn other_forms_of_the_word_are_never_options() {
        let conn = deck();
        conn.execute_batch(
            "INSERT INTO review_items(id, content, language, detail, lemma) VALUES
               ('f', '下がる', 'ja', 'to go down', '下がる'),
               ('g', '下がります', 'ja', 'to drop', '下がる'),
               ('h', 'さがる', 'ja', 'to decrease', 'さがる');",
        )
        .unwrap();
        let c = card("f", "下がる", "review", Some(r#"{"reading":"さがる","meaning":"to go down"}"#));
        let ids: Vec<String> = load_candidates(&conn, &c).unwrap().into_iter().map(|x| x.id).collect();
        assert!(!ids.contains(&"g".to_string()), "another form of the word");
        assert!(!ids.contains(&"h".to_string()), "the word in kana");
        assert!(ids.contains(&"b".to_string()));
    }

    #[test]
    fn meanings_sharing_a_sense_are_never_wrong_options() {
        let g = glosses("To travel; trip (abroad), a journey");
        assert_eq!(g, ["travel", "trip", "journey"].iter().map(|s| s.to_string()).collect::<HashSet<String>>());
        assert!(meanings_overlap(&glosses("travel; trip"), &glosses("travel; journey; trip")));
        assert!(!meanings_overlap(&glosses("travel; trip"), &glosses("inn; hotel")));

        let conn = deck();
        conn.execute_batch(
            "INSERT INTO review_items(id, content, language, detail) VALUES
               ('t', '旅行', 'ja', '{\"reading\":\"りょこう\",\"meaning\":\"travel; trip\"}'),
               ('u', '旅', 'ja', '{\"reading\":\"たび\",\"meaning\":\"travel; journey; trip\"}'),
               ('v', '旅館', 'ja', '{\"reading\":\"りょかん\",\"meaning\":\"inn\"}'),
               ('w', '歩く', 'ja', '{\"reading\":\"あるく\",\"meaning\":\"to walk\"}'),
               ('x', '散策', 'ja', '{\"reading\":\"さんさく\",\"meaning\":\"stroll; walk\"}');",
        )
        .unwrap();
        let c = card("t", "旅行", "review", Some(r#"{"reading":"りょこう","meaning":"travel; trip"}"#));
        let detail = parse_detail(c.detail.as_deref());
        let cands = load_candidates(&conn, &c).unwrap();
        // 旅 was confused before: a shared sense still keeps it out.
        let confusions = HashMap::from([("u".to_string(), 3)]);
        for _ in 0..20 {
            let opts = meaning_distractors(&c, &detail, &cands, &confusions, 4, &mut rand::thread_rng());
            let ids: Vec<String> = opts.iter().filter_map(|o| o.item_id.clone()).collect();
            assert!(!ids.contains(&"u".to_string()), "same meaning as the answer");
            assert_eq!(ids[0], "v", "shared kanji, other meaning ranks first");
            assert!(!(ids.contains(&"w".to_string()) && ids.contains(&"x".to_string())), "two options with one sense");
        }
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
            draft: None,
            grammar: None,
            placement: false,
            answer: NONE_ANSWER.into(),
            fix: String::new(),
            statement_true: false,
            easy_allowed: true,
        };
        let public = serde_json::to_string(&t).unwrap();
        assert!(!public.contains("easy_allowed") && !public.contains(NONE_ANSWER));
        let stored: QuizTask = serde_json::from_str(&serde_json::to_string(&StoredTask(&t)).unwrap()).unwrap();
        assert_eq!(stored.answer, NONE_ANSWER);
        assert!(stored.easy_allowed);
    }

    const GRAMMAR_BACK: &str = r#"{"title":"～てもいい","meaning":"may, it's okay to","form":"te-form + もいい",
        "examples":[{"sentence":"ここで写真を撮ってもいいですか。","target":"撮ってもいい","translation":"May I take a photo here?",
          "distractors":["撮ってはいけない","撮らなければならない","撮ってみる"]}],
        "mistake":{"parts":["ここで","写真を","撮っては","いいですか。"],"wrong":2,"fix":"撮っても","translation":"May I take a photo here?"},
        "confusables":[{"pattern":"～てはいけない","meaning":"must not"},{"pattern":"～なければならない","meaning":"must"},{"pattern":"～てみる","meaning":"try doing"}]}"#;

    fn grammar_card(state: &str, placement: bool, stability: Option<f32>) -> ReviewItem {
        let mut c = card("g", "～てもいい", state, Some(GRAMMAR_BACK));
        c.item_type = ITEM_GRAMMAR.into();
        c.level = Some("N4".into());
        c.placement = placement;
        c.fsrs_stability = stability;
        c
    }

    #[test]
    fn grammar_questions_follow_the_card_stage() {
        let conn = deck();
        let mut rng = rand::thread_rng();
        let kind = |c: &ReviewItem| build_task(&conn, c, false, &mut rand::thread_rng()).unwrap().kind;
        assert_eq!(kind(&grammar_card("new", true, None)), KIND_GRAMMAR_GAP);
        assert_eq!(kind(&grammar_card("new", false, None)), KIND_INTRO_TF);
        assert_eq!(build_task(&conn, &grammar_card("new", false, None), true, &mut rng).unwrap().kind, KIND_TRUE_FALSE);
        assert_eq!(kind(&grammar_card("learning", false, None)), KIND_GRAMMAR_MEANING);
        assert_eq!(kind(&grammar_card("review", false, Some(5.0))), KIND_GRAMMAR_GAP);
        for _ in 0..20 {
            let k = kind(&grammar_card("review", false, Some(40.0)));
            assert!([KIND_GRAMMAR_GAP, KIND_GRAMMAR_MEANING, KIND_GRAMMAR_MISTAKE].contains(&k.as_str()));
        }
    }

    #[test]
    fn level_check_is_a_gap_with_the_pattern_hidden() {
        let conn = deck();
        let t = build_task(&conn, &grammar_card("new", true, None), false, &mut rand::thread_rng()).unwrap();
        assert!(t.placement);
        assert_eq!(t.word, "～てもいい");
        assert_eq!(t.answer, "撮ってもいい");
        let s = t.sentence.as_ref().unwrap();
        assert_eq!((s.before.as_str(), s.target.as_str(), s.after.as_str()), ("ここで写真を", "", "ですか。"));
        assert_eq!(s.translation, "May I take a photo here?");
        let mut options: Vec<&str> = t.options.iter().map(|o| o.text.as_str()).collect();
        options.sort();
        assert_eq!(options, vec!["撮ってはいけない", "撮ってみる", "撮ってもいい", "撮らなければならない"]);
        // Examples would give the gap away, so the card only carries what it teaches.
        assert!(t.grammar.as_ref().unwrap().examples.is_empty());
        let public = serde_json::to_string(&t).unwrap();
        assert!(!public.contains("\"answer\""));
    }

    #[test]
    fn grammar_meaning_options_come_from_grammar_and_look_alikes() {
        let conn = deck();
        let t = build_task(&conn, &grammar_card("learning", false, None), false, &mut rand::thread_rng()).unwrap();
        let mut options: Vec<&str> = t.options.iter().map(|o| o.text.as_str()).collect();
        options.sort();
        // The deck's words (walk, teacher, student) never show up for a grammar card.
        assert_eq!(options, vec!["may, it's okay to", "must", "must not", "try doing"]);
        assert_eq!(t.sentence.unwrap().target, "撮ってもいい");
    }

    #[test]
    fn mistake_answer_names_the_fix() {
        let conn = deck();
        let mut t = build_task(&conn, &grammar_card("review", false, Some(40.0)), false, &mut rand::thread_rng()).unwrap();
        while t.kind != KIND_GRAMMAR_MISTAKE {
            t = build_task(&conn, &grammar_card("review", false, Some(40.0)), false, &mut rand::thread_rng()).unwrap();
        }
        let answer = QuizAnswer { dont_know: false, choice: Some("撮っては".into()), tf: None, elapsed_ms: 3000, hint_used: false };
        assert!(check_answer(&t, &answer).0);
        assert_eq!(correct_answer_text(&t), "撮っては → 撮っても");
    }

    #[test]
    fn level_checks_grade_known_or_taught() {
        let mut g = input(KIND_GRAMMAR_GAP, true, 20_000, false);
        g.placement = true;
        assert_eq!(compute_grade(&g).0, "easy");
        g.correct = false;
        assert_eq!(compute_grade(&g), ("again", vec![reason("check_missed")]));
    }

    #[test]
    fn grammar_card_without_practice_asks_for_it() {
        let conn = deck();
        let mut c = grammar_card("new", false, None);
        c.detail = None;
        assert_eq!(build_task(&conn, &c, false, &mut rand::thread_rng()).unwrap().kind, KIND_NEEDS_MEANING);
        assert!(!has_back(ITEM_GRAMMAR, Some(r#"{"meaning":"may"}"#)));
        assert!(has_back(ITEM_GRAMMAR, Some(GRAMMAR_BACK)));
        assert!(has_back("word", Some("to eat")));
    }
}
