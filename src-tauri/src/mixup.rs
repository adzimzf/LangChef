//! "Don't confuse": when the learner picks another card's meaning, the result screen compares the
//! two words as a diff. Each word is split into its parts (kanji, or the components of one kanji,
//! or a root and affixes), the parts only one word has are marked, and one short rule says how to
//! tell them apart. AI writes that once per pair and explanation language; it is saved in
//! `confusion_diffs` (synced) and reused. Words, readings and meanings come from the cards, and the
//! mix-up count from `review_confusions`, never from the AI.

use crate::chat::{complete_explained, json_part};
use crate::db::open_db;
use crate::locale::{app_error, explain_lang, lang_name};
use crate::quiz::parse_detail;
use crate::srs::{fetch_review_item, ReviewItem};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How the two words relate (`mixup.kind.<kind>` on the frontend). Anything else becomes `meaning`.
pub const KINDS: &[&str] = &["order", "shape", "sound", "meaning"];
const MAX_PARTS: usize = 5;
const MAX_PART_CHARS: usize = 12;
const MAX_MEANING_CHARS: usize = 40;
const MAX_NOTE_CHARS: usize = 220;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DiffPart {
    pub text: String,
    pub meaning: String,
    /// Only this word has the part (in this place).
    pub differs: bool,
}

#[derive(Serialize, Debug)]
pub struct DiffWord {
    pub id: String,
    pub word: String,
    pub reading: String,
    pub meaning: String,
    pub parts: Vec<DiffPart>,
}

#[derive(Serialize, Debug)]
pub struct ConfusionDiff {
    pub kind: String,
    pub note: String,
    /// The card that was asked first, then the word that was picked instead.
    pub words: Vec<DiffWord>,
    /// How often the two were mixed up, both ways.
    pub count: i64,
}

/// What the AI wrote for a pair, already cleaned.
#[derive(Debug, PartialEq)]
struct Comparison {
    kind: String,
    note: String,
    a: Vec<DiffPart>,
    b: Vec<DiffPart>,
}

/// How often two cards were mixed up, in either direction.
pub fn pair_mixups(conn: &Connection, a: &str, b: &str) -> Result<i64, String> {
    conn.query_row(
        "SELECT COALESCE(SUM(count), 0) FROM review_confusions
         WHERE (review_item_id = ?1 AND confused_with_id = ?2) OR (review_item_id = ?2 AND confused_with_id = ?1)",
        params![a, b],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}

fn clip(s: &str, max: usize) -> String {
    s.trim().chars().take(max).collect()
}

fn clean_parts(v: Option<&Value>) -> Vec<DiffPart> {
    v.and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| {
                    let text = clip(p.get("part").or_else(|| p.get("text")).and_then(Value::as_str)?, MAX_PART_CHARS + 1);
                    if text.is_empty() || text.chars().count() > MAX_PART_CHARS {
                        return None;
                    }
                    Some(DiffPart {
                        text,
                        meaning: clip(p.get("meaning").and_then(Value::as_str).unwrap_or(""), MAX_MEANING_CHARS),
                        differs: p.get("differs").and_then(Value::as_bool).unwrap_or(false),
                    })
                })
                .take(MAX_PARTS)
                .collect()
        })
        .unwrap_or_default()
}

/// Reads the AI's JSON. Without a rule, or without parts for a pair that should have them, the
/// answer is no use and counts as a bad response.
fn clean_comparison(raw: &str) -> Result<Comparison, String> {
    let v: Value = serde_json::from_str(json_part(raw)).map_err(|_| app_error("api_bad_response", ""))?;
    let kind = v
        .get("kind")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|k| KINDS.contains(k))
        .unwrap_or("meaning")
        .to_string();
    let note = clip(v.get("note").and_then(Value::as_str).unwrap_or(""), MAX_NOTE_CHARS);
    let (mut a, mut b) = (clean_parts(v.get("a")), clean_parts(v.get("b")));
    if note.is_empty() || (kind != "meaning" && (a.is_empty() || b.is_empty())) {
        return Err(app_error("api_bad_response", ""));
    }
    if kind == "order" {
        // Same kanji in another order: no part is the odd one out, the order is.
        for p in a.iter_mut().chain(b.iter_mut()) {
            p.differs = false;
        }
    } else {
        // A part both words have in the same place is never the difference.
        let shared = |p: &DiffPart, other: &[DiffPart]| other.iter().any(|o| o.text == p.text);
        let (a0, b0) = (a.clone(), b.clone());
        for p in a.iter_mut() {
            p.differs = p.differs && !shared(p, &b0);
        }
        for p in b.iter_mut() {
            p.differs = p.differs && !shared(p, &a0);
        }
    }
    Ok(Comparison { kind, note, a, b })
}

fn prompt(language: &str, a: &DiffWord, b: &DiffWord) -> String {
    let line = |w: &DiffWord| {
        let reading = if w.reading.is_empty() || w.reading == w.word { String::new() } else { format!(" ({})", w.reading) };
        let meaning = if w.meaning.is_empty() { String::new() } else { format!(": {}", w.meaning) };
        format!("{}{}{}", w.word, reading, meaning)
    };
    format!(
        "A learner of {lang} keeps mixing up these two words:\nA: {a}\nB: {b}\n\n\
         Compare them so the learner sees at a glance what is different. Reply with JSON only:\n\
         {{\"kind\": \"order|shape|sound|meaning\", \"a\": [{{\"part\": \"…\", \"meaning\": \"…\", \"differs\": true}}], \"b\": [...], \"note\": \"…\"}}\n\n\
         - kind: \"order\" when both are made of the same kanji in another order; \"shape\" when they look alike and \
         a part is different; \"sound\" when they are read the same or almost the same; \"meaning\" otherwise.\n\
         - a, b: each word split into 1 to 4 parts that matter for telling them apart, in writing order. Japanese: \
         its kanji, or for a single-kanji word the components of that kanji (radical and the rest, e.g. 待 → 彳 + 寺); \
         leave out kana endings unless they are the difference. Other languages: root and affixes. \
         \"part\" is exactly as written (max 12 characters), \"meaning\" is 1 to 3 words, \"differs\" is true only \
         when the other word does not have that part.\n\
         - note: one short sentence (max 20 words) on how to tell them apart, naming the part that differs.",
        lang = lang_name(language),
        a = line(a),
        b = line(b),
    )
}

fn word_of(item: &ReviewItem) -> DiffWord {
    let d = parse_detail(item.detail.as_deref());
    DiffWord { id: item.id.clone(), word: item.content.clone(), reading: d.reading, meaning: d.meaning, parts: Vec::new() }
}

/// The saved comparison, as (kind, note, parts of `first`, parts of `second`).
fn stored(conn: &Connection, first: &str, second: &str, explain: &str) -> Result<Option<Comparison>, String> {
    let row = conn
        .query_row(
            "SELECT item_a, kind, note, parts_a, parts_b FROM confusion_diffs
             WHERE explain_lang = ?3 AND ((item_a = ?1 AND item_b = ?2) OR (item_a = ?2 AND item_b = ?1))
             ORDER BY created_at DESC LIMIT 1",
            params![first, second, explain],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(row.map(|(item_a, kind, note, pa, pb)| {
        let parse = |s: &str| serde_json::from_str::<Vec<DiffPart>>(s).unwrap_or_default();
        let (pa, pb) = (parse(&pa), parse(&pb));
        let (a, b) = if item_a == first { (pa, pb) } else { (pb, pa) };
        Comparison { kind, note, a, b }
    }))
}

fn save(conn: &Connection, first: &str, second: &str, explain: &str, c: &Comparison) -> Result<(), String> {
    // One row per pair: the smaller id goes first, whichever card was asked.
    let (item_a, item_b, pa, pb) = if first <= second { (first, second, &c.a, &c.b) } else { (second, first, &c.b, &c.a) };
    let json = |p: &Vec<DiffPart>| serde_json::to_string(p).unwrap_or_else(|_| "[]".into());
    conn.execute(
        "DELETE FROM confusion_diffs WHERE explain_lang = ?3 AND ((item_a = ?1 AND item_b = ?2) OR (item_a = ?2 AND item_b = ?1))",
        params![item_a, item_b, explain],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO confusion_diffs (item_a, item_b, explain_lang, kind, note, parts_a, parts_b, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![item_a, item_b, explain, c.kind, c.note, json(pa), json(pb), chrono::Utc::now().timestamp_millis()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn assemble(mut a: DiffWord, mut b: DiffWord, c: Comparison, count: i64) -> ConfusionDiff {
    a.parts = c.a;
    b.parts = c.b;
    ConfusionDiff { kind: c.kind, note: c.note, words: vec![a, b], count }
}

/// The comparison of a card with the word picked instead: the saved one, or a new one from AI
/// (`refresh` always asks again). Word cards only.
#[tauri::command]
pub async fn llm_confusion_diff(
    app: tauri::AppHandle,
    api_host: String,
    api_key: String,
    model: String,
    card_id: String,
    other_id: String,
    refresh: Option<bool>,
) -> Result<ConfusionDiff, String> {
    let (a, b, language, explain, count) = {
        let conn = open_db(&app)?;
        let card = fetch_review_item(&conn, &card_id)?.ok_or_else(|| app_error("card_gone", ""))?;
        let other = fetch_review_item(&conn, &other_id)?.ok_or_else(|| app_error("card_gone", ""))?;
        if card.is_grammar() || other.is_grammar() {
            return Err(app_error("mixup_not_words", ""));
        }
        let explain = explain_lang(&conn);
        let count = pair_mixups(&conn, &card_id, &other_id)?;
        let (a, b) = (word_of(&card), word_of(&other));
        if !refresh.unwrap_or(false) {
            if let Some(c) = stored(&conn, &card_id, &other_id, &explain)? {
                return Ok(assemble(a, b, c, count));
            }
        }
        (a, b, card.language.unwrap_or_else(|| "ja".into()), explain, count)
    };
    if api_host.trim().is_empty() || api_key.trim().is_empty() {
        return Err(app_error("no_ai", ""));
    }
    let raw = complete_explained(&api_host, &api_key, &model, &prompt(&language, &a, &b), &explain, |r| {
        serde_json::from_str::<Value>(json_part(r))
            .ok()
            .and_then(|v| v.get("note").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default()
    })
    .await?;
    let c = clean_comparison(&raw)?;
    save(&open_db(&app)?, &card_id, &other_id, &explain, &c)?;
    Ok(assemble(a, b, c, count))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(text: &str, differs: bool) -> DiffPart {
        DiffPart { text: text.into(), meaning: String::new(), differs }
    }

    #[test]
    fn shared_parts_never_count_as_the_difference() {
        let raw = r#"```json
        {"kind":"shape","a":[{"part":"彳","meaning":"road","differs":true},{"part":"寺","meaning":"temple","differs":true}],
         "b":[{"part":"扌","meaning":"hand","differs":true},{"part":"寺","meaning":"temple","differs":false}],
         "note":"Look at the left: 彳 road vs 扌 hand."}
        ```"#;
        let c = clean_comparison(raw).unwrap();
        assert_eq!(c.kind, "shape");
        assert_eq!(c.a.iter().map(|p| p.differs).collect::<Vec<_>>(), vec![true, false]);
        assert_eq!(c.b.iter().map(|p| p.differs).collect::<Vec<_>>(), vec![true, false]);
    }

    #[test]
    fn swapped_order_marks_no_part_and_unknown_kinds_fall_back() {
        let c = clean_comparison(r#"{"kind":"order","a":[{"part":"会","differs":true},{"part":"社"}],"b":[{"part":"社"},{"part":"会"}],"note":"The last kanji says what it is."}"#).unwrap();
        assert!(c.a.iter().chain(c.b.iter()).all(|p| !p.differs));
        let c = clean_comparison(r#"{"kind":"synonym","a":[],"b":[],"note":"Different things."}"#).unwrap();
        assert_eq!(c.kind, "meaning");
    }

    #[test]
    fn unusable_answers_are_refused() {
        assert!(clean_comparison("not json").is_err());
        assert!(clean_comparison(r#"{"kind":"shape","a":[{"part":"彳"}],"b":[{"part":"扌"}],"note":""}"#).is_err());
        assert!(clean_comparison(r#"{"kind":"shape","a":[],"b":[{"part":"扌"}],"note":"x"}"#).is_err());
        let long = clean_comparison(r#"{"kind":"meaning","a":[{"part":"this part is far too long"}],"b":[],"note":"x"}"#).unwrap();
        assert!(long.a.is_empty());
    }

    #[test]
    fn a_pair_is_saved_once_and_read_back_from_either_card() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE confusion_diffs(item_a TEXT, item_b TEXT, explain_lang TEXT, kind TEXT, note TEXT,
               parts_a TEXT, parts_b TEXT, created_at INTEGER, PRIMARY KEY (item_a, item_b, explain_lang));
             CREATE TABLE review_confusions(review_item_id TEXT, confused_with_id TEXT, count INTEGER, last_at INTEGER);
             INSERT INTO review_confusions VALUES ('y', 'x', 2, 1), ('x', 'y', 1, 2), ('x', 'z', 5, 3);",
        )
        .unwrap();
        let c = Comparison { kind: "shape".into(), note: "n".into(), a: vec![part("扌", true)], b: vec![part("彳", true)] };
        save(&conn, "y", "x", "en", &c).unwrap();
        save(&conn, "y", "x", "en", &c).unwrap();
        let rows: i64 = conn.query_row("SELECT COUNT(*) FROM confusion_diffs", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 1);
        let from_y = stored(&conn, "y", "x", "en").unwrap().unwrap();
        assert_eq!(from_y.a[0].text, "扌");
        let from_x = stored(&conn, "x", "y", "en").unwrap().unwrap();
        assert_eq!(from_x.a[0].text, "彳", "parts follow the card that was asked");
        assert!(stored(&conn, "x", "y", "id").unwrap().is_none(), "another explanation language needs its own");
        assert_eq!(pair_mixups(&conn, "x", "y").unwrap(), 3);
    }
}
