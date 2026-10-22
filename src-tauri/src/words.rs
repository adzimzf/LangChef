use crate::chat::{complete_explained, json_part};
use crate::db::open_db;
use crate::grammar_review::generate_card_back;
use crate::locale::{app_error, effective_detail_sql, explain_lang, lang_name};
use crate::quiz::{parse_detail, CardDetail};
use crate::srs::{review_cols, row_to_review_item, ReviewItem};
use rusqlite::params;
use serde::{Deserialize, Serialize};

struct KnownWord {
    word: String,
    reading: String,
    meaning: String,
}

#[derive(Debug, Deserialize)]
pub struct ImportItem {
    pub content: String,
    pub language: Option<String>,
    pub detail: Option<String>,
    /// Language of `detail`; defaults to the learner's explanation language.
    pub meaning_lang: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateReviewItem {
    pub id: String,
    pub content: Option<String>,
    pub language: Option<String>,
}

#[derive(Serialize)]
pub struct WordInfo {
    pub content: String,
    pub ef: f64,
    pub r: i64,
    pub is_new: bool,
    pub difficulty_bg_class: String,
}

#[derive(Serialize)]
pub struct WordClassification {
    pub difficult: Vec<WordInfo>,
    pub known: Vec<String>,
}

#[derive(Serialize)]
pub struct SearchResult {
    pub items: Vec<ReviewItem>,
    pub total: i64,
}

fn find_mnemonic_matches(
    app: &tauri::AppHandle,
    language: &str,
    target_content: &str,
    target_reading: &str,
) -> Result<(Vec<KnownWord>, Vec<KnownWord>), String> {
    let conn = open_db(app)?;

    let mut all_words: Vec<KnownWord> = Vec::new();

    let mut stmt = conn.prepare(&format!(
        "SELECT content, EF, R, {}, fsrs_difficulty FROM review_items WHERE language=?1 AND item_type='word'",
        effective_detail_sql("")
    )).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![language], |row| {
        let content: String = row.get(0)?;
        let ef: f64 = row.get(1)?;
        let r_val: i64 = row.get(2)?;
        let detail: Option<String> = row.get(3)?;
        let fsrs_diff: Option<f32> = row.get(4).unwrap_or(None);
        Ok((content, ef, r_val, detail, fsrs_diff))
    }).map_err(|e| e.to_string())?;
    for r in rows {
        let (content, ef, r_val, detail, fsrs_diff) = r.map_err(|e| e.to_string())?;
        let is_known = if ef == -1.0 {
            fsrs_diff.unwrap_or(5.0) < 3.0
        } else {
            ef >= 2.7 && r_val >= 5
        };
        if !is_known { continue; }
        if content == target_content { continue; }

        let CardDetail { reading, meaning, .. } = parse_detail(detail.as_deref());

        all_words.push(KnownWord { word: content, reading, meaning });
    }

    let mut matched: Vec<KnownWord> = Vec::new();

    for kw in &all_words {
        let mut is_match = false;

        let target_chars: std::collections::HashSet<char> = target_content.chars()
            .filter(|c| is_cjk(*c))
            .collect();
        let kw_chars: std::collections::HashSet<char> = kw.word.chars()
            .filter(|c| is_cjk(*c))
            .collect();
        if !target_chars.is_disjoint(&kw_chars) {
            is_match = true;
        }

        if !target_reading.is_empty() && !kw.reading.is_empty() {
            let t_mora = split_mora(&target_reading.to_lowercase());
            let k_mora = split_mora(&kw.reading.to_lowercase());
            let (shorter, longer) = if t_mora.len() <= k_mora.len() {
                (&t_mora, &k_mora)
            } else {
                (&k_mora, &t_mora)
            };
            if shorter.len() >= 2 {
                'outer: for i in 0..=shorter.len()-2 {
                    let sub = &shorter[i..i+2];
                    for j in 0..=longer.len().saturating_sub(2) {
                        if longer[j] == sub[0] && longer[j+1] == sub[1] {
                            is_match = true;
                            break 'outer;
                        }
                    }
                }
            }
        }

        if is_match {
            matched.push(KnownWord {
                word: kw.word.clone(),
                reading: kw.reading.clone(),
                meaning: kw.meaning.clone(),
            });
        }
    }

    matched.truncate(10);
    let mut all_words = all_words;
    all_words.truncate(30);

    Ok((matched, all_words))
}

fn split_mora(reading: &str) -> Vec<String> {
    let chars: Vec<char> = reading.chars().collect();
    let mut mora: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if is_small_kana(c) {
            if let Some(last) = mora.last_mut() {
                last.push(c);
            } else {
                mora.push(c.to_string());
            }
        } else {
            mora.push(c.to_string());
        }
        i += 1;
    }
    mora
}

fn is_small_kana(c: char) -> bool {
    matches!(c, 'ゃ' | 'ゅ' | 'ょ' | 'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' | 'ャ' | 'ュ' | 'ョ' | 'ァ' | 'ィ' | 'ゥ' | 'ェ' | 'ォ')
}

pub(crate) fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}' |
        '\u{3400}'..='\u{4DBF}' |
        '\u{F900}'..='\u{FAFF}'
    )
}

#[tauri::command]
pub fn db_list_known_words(app: tauri::AppHandle) -> Result<Vec<String>, String> {
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
pub fn db_add_known_word(app: tauri::AppHandle, word: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute("INSERT OR IGNORE INTO known_words(word) VALUES (?1)", params![word.to_lowercase()])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn db_generate_mnemonic(
    app: tauri::AppHandle,
    api_host: String,
    api_key: String,
    model: String,
    id: String,
    content: String,
    language: String,
    detail: Option<String>,
) -> Result<String, String> {
    let CardDetail { reading: target_reading, meaning: target_meaning, .. } = parse_detail(detail.as_deref());

    let target_info = format!(
        "{}{}{}",
        content,
        if target_reading.is_empty() { String::new() } else { format!(" (reading: {})", target_reading) },
        if target_meaning.is_empty() { String::new() } else { format!(" — meaning: {}", target_meaning) },
    );

    let (matched, _all_known) = find_mnemonic_matches(
        &app, &language, &content, &target_reading
    )?;

    let explain = explain_lang(&open_db(&app)?);
    let lang_name = lang_name(&language);
    let explain_name = lang_name_of(&explain);

    let prompt = if matched.is_empty() {
        format!(
            "Create a mnemonic to help remember the {} word: \"{}\"\n\n\
            No known words share sounds or kanji with this word.\n\
            Use a sound-alike from {explain_name} or another familiar word that sounds like the reading.\n\n\
            Example: 公式 (こうしき, formula) — sounds like \"COZY-sheeki\" — imagine a COZY formula you can solve in bed.\n\n\
            RULES:\n\
            - Base the mnemonic on how the reading SOUNDS — find a {explain_name} word or phrase that sounds similar\n\
            - Always show the target word's reading in the mnemonic\n\
            - Keep it to 1-2 short sentences — natural and easy to recall\n\
            - Write in {explain_name}\n\
            - Return ONLY the mnemonic text. No JSON, no labels, no quotes, no explanation.",
            lang_name, target_info
        )
    } else {
        let matches_str: Vec<String> = matched.iter().map(|kw| {
            if kw.reading.is_empty() {
                format!("{} (meaning: {})", kw.word, kw.meaning)
            } else {
                format!("{} (reading: {}, meaning: {})", kw.word, kw.reading, kw.meaning)
            }
        }).collect();

        format!(
            "Create a mnemonic to help remember the {} word: \"{}\"\n\n\
            The following known words share sounds or kanji with the target:\n{}\n\n\
            Pick the BEST match from the list above and build a mnemonic that explains the connection.\n\
            The connection must be based on the REAL overlap — shared kanji or shared reading syllables.\n\n\
            Example: Target 食堂 (しょくどう, canteen). Match: 食べる (reading: たべる, meaning: to eat).\n\
            Mnemonic: \"You already know 食べる (たべる). The kanji 食 can also be read as しょ (onyomi). So 食堂 しょくどう is a HALL where you EAT — a canteen!\"\n\n\
            RULES:\n\
            - You MUST use one of the matched words listed above — explain the shared kanji or reading\n\
            - Always show both the target's reading and the matched word's reading in the mnemonic\n\
            - Keep it to 1-2 short sentences — natural and easy to recall\n\
            - Write in {explain_name}\n\
            - Return ONLY the mnemonic text. No JSON, no labels, no quotes, no explanation.",
            lang_name, target_info,
            matches_str.join("\n")
        )
    };

    let mnemonic = complete_explained(&api_host, &api_key, &model, &prompt, &explain, |t| t.to_string()).await?;

    let conn = open_db(&app)?;
    conn.execute("UPDATE review_items SET mnemonic=?1 WHERE id=?2", params![mnemonic, id])
        .map_err(|e| e.to_string())?;

    Ok(mnemonic)
}

#[tauri::command]
pub fn db_update_mnemonic(app: tauri::AppHandle, id: String, mnemonic: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute("UPDATE review_items SET mnemonic=?1 WHERE id=?2", params![mnemonic, id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn db_update_review_detail(app: tauri::AppHandle, id: String, detail: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute(
        "UPDATE review_items SET detail=?1, meaning_lang=?2 WHERE id=?3",
        params![detail, explain_lang(&conn), id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn db_get_review_items_by_language(app: tauri::AppHandle, language: String) -> Result<Vec<ReviewItem>, String> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare(&format!("SELECT {} FROM review_items WHERE language=?1", review_cols(""))).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![language], row_to_review_item).map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    for r in rows {
        items.push(r.map_err(|e| e.to_string())?);
    }
    Ok(items)
}

#[tauri::command]
pub fn db_search_review_items(
    app: tauri::AppHandle,
    query: Option<String>,
    language: Option<String>,
    state: Option<String>,
    difficulty: Option<String>,
    sort: Option<String>,
    offset: Option<i64>,
    limit: Option<i64>,
) -> Result<SearchResult, String> {
    use rusqlite::types::ToSql;
    let conn = open_db(&app)?;

    // The word list; grammar cards are shown on the Grammar page.
    let mut conditions: Vec<String> = vec!["item_type = 'word'".to_string()];
    let mut filter_params: Vec<String> = Vec::new();

    if let Some(ref lang) = language {
        conditions.push(format!("language = ?{}", filter_params.len() + 1));
        filter_params.push(lang.clone());
    }
    if let Some(ref s) = state {
        conditions.push(format!("state = ?{}", filter_params.len() + 1));
        filter_params.push(s.clone());
    }
    if let Some(ref q) = query {
        let q = q.trim().to_string();
        if !q.is_empty() {
            conditions.push(format!("LOWER(content) LIKE '%' || LOWER(?{}) || '%'", filter_params.len() + 1));
            filter_params.push(q);
        }
    }
    if let Some(ref d) = difficulty {
        match d.as_str() {
            "hard" => conditions.push("((EF = -1.0 AND fsrs_difficulty >= 7.0) OR (EF != -1.0 AND EF < 2.0))".to_string()),
            "medium" => conditions.push("((EF = -1.0 AND fsrs_difficulty >= 3.0 AND fsrs_difficulty < 7.0) OR (EF != -1.0 AND EF >= 2.0 AND (EF < 2.7 OR R < 5)))".to_string()),
            "easy" => conditions.push("((EF = -1.0 AND fsrs_difficulty < 3.0) OR (EF != -1.0 AND EF >= 2.7 AND R >= 5))".to_string()),
            _ => {}
        }
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let order_clause = match sort.as_deref() {
        Some("next_review") => " ORDER BY next_review ASC NULLS LAST".to_string(),
        Some("hardest") => " ORDER BY COALESCE(fsrs_difficulty, (11.0 - EF)) DESC, lapse_count DESC".to_string(),
        _ => " ORDER BY COALESCE(fsrs_difficulty, (11.0 - EF)) ASC, lapse_count ASC".to_string(),
    };

    let count_sql = format!("SELECT COUNT(*) FROM review_items{}", where_clause);
    let param_refs: Vec<&dyn ToSql> = filter_params.iter().map(|s| s as &dyn ToSql).collect();
    let total: i64 = conn.query_row(&count_sql, param_refs.as_slice(), |row| row.get(0))
        .map_err(|e| e.to_string())?;

    let data_sql = format!(
        "SELECT {} FROM review_items{}{} LIMIT ?{} OFFSET ?{}",
        review_cols(""), where_clause, order_clause,
        filter_params.len() + 1, filter_params.len() + 2,
    );

    let limit_val = limit.unwrap_or(20).max(1);
    let offset_val = offset.unwrap_or(0).max(0);

    let mut all_params: Vec<Box<dyn ToSql>> = filter_params.into_iter().map(|s| Box::new(s) as Box<dyn ToSql>).collect();
    all_params.push(Box::new(limit_val));
    all_params.push(Box::new(offset_val));
    let all_refs: Vec<&dyn ToSql> = all_params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&data_sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(all_refs.as_slice(), row_to_review_item).map_err(|e| e.to_string())?;

    let mut items = Vec::new();
    for r in rows {
        items.push(r.map_err(|e| e.to_string())?);
    }

    Ok(SearchResult { items, total })
}

#[tauri::command]
pub fn db_get_due_review_items(
    app: tauri::AppHandle,
    language: Option<String>,
    limit: Option<i64>,
    new_cards_remaining: Option<i64>,
    max_next_review: Option<i64>,
    max_new_cards_per_day: Option<i64>,
    max_reviews_remaining: Option<i64>,
) -> Result<Vec<ReviewItem>, String> {
    let conn = open_db(&app)?;
    let today = chrono::Local::now().date_naive().to_string();

    let limits_row = conn.query_row(
        "SELECT
            COALESCE(SUM(CASE WHEN sc.kind='review' AND sc.status='graded' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN sc.kind='new'    AND sc.status='graded' THEN 1 ELSE 0 END), 0)
         FROM study s JOIN study_card sc ON sc.study_id = s.study_id
         WHERE s.date = ?1 AND s.language = ?2",
        params![today, language.clone().unwrap_or_default()],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    );
    let (backend_reviews_done, backend_new_seen) = limits_row.unwrap_or((0, 0));

    let authoritative_new_remaining = match max_new_cards_per_day {
        Some(max_new) => {
            let backend_remaining = i64::max(0, max_new - backend_new_seen);
            match new_cards_remaining {
                Some(frontend_rem) => Some(i64::min(frontend_rem, backend_remaining)),
                None => Some(backend_remaining),
            }
        }
        None => new_cards_remaining,
    };

    let authoritative_review_remaining: Option<i64> = max_reviews_remaining
        .map(|frontend_rem| i64::max(0, frontend_rem - backend_reviews_done));

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

    let mut sql = format!(
        "SELECT {}
         FROM review_items
         WHERE (next_review IS NULL OR next_review <= ?1)",
        review_cols("")
    );

    if language.is_some() {
        sql.push_str(" AND language = ?2");
    }

    sql.push_str(" ORDER BY CASE WHEN state='new' OR state='' OR state IS NULL THEN 1 ELSE 0 END, next_review ASC NULLS FIRST LIMIT ?");

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut items = Vec::new();

    let rows = if let Some(ref lang) = language {
        stmt.query_map(params![end_of_day, lang, limit_val], row_to_review_item).map_err(|e| e.to_string())?
    } else {
        stmt.query_map(params![end_of_day, limit_val], row_to_review_item).map_err(|e| e.to_string())?
    };

    for r in rows {
        items.push(r.map_err(|e| e.to_string())?);
    }

    if let Some(review_cap) = authoritative_review_remaining {
        if review_cap <= 0 {
            items.retain(|item| item.state != "review");
        } else {
            let mut review_count = 0i64;
            items.retain(|item| {
                if item.state == "review" {
                    if review_count >= review_cap {
                        return false;
                    }
                    review_count += 1;
                }
                true
            });
        }
    }

    if let Some(new_cap) = authoritative_new_remaining {
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
pub fn db_get_difficult_and_known_words(
    app: tauri::AppHandle,
    language: Option<String>,
) -> Result<WordClassification, String> {
    let conn = open_db(&app)?;

    let cols = review_cols("");

    let (difficult_sql, known_sql) = if language.is_some() {
        (
            format!(
                "SELECT {cols} FROM review_items \
                 WHERE language=?1 AND item_type='word' AND COALESCE(fsrs_difficulty, 5.0) >= 5.0 \
                 ORDER BY COALESCE(fsrs_difficulty, 5.0) DESC LIMIT 20"
            ),
            "SELECT content FROM review_items \
             WHERE language=?1 AND item_type='word' AND COALESCE(fsrs_difficulty, 5.0) < 5.0 \
             ORDER BY COALESCE(fsrs_difficulty, 5.0) ASC LIMIT 20"
                .to_string(),
        )
    } else {
        (
            format!(
                "SELECT {cols} FROM review_items \
                 WHERE item_type='word' AND COALESCE(fsrs_difficulty, 5.0) >= 5.0 \
                 ORDER BY COALESCE(fsrs_difficulty, 5.0) DESC LIMIT 20"
            ),
            "SELECT content FROM review_items \
             WHERE item_type='word' AND COALESCE(fsrs_difficulty, 5.0) < 5.0 \
             ORDER BY COALESCE(fsrs_difficulty, 5.0) ASC LIMIT 20"
                .to_string(),
        )
    };

    let mut stmt_d = conn.prepare(&difficult_sql).map_err(|e| e.to_string())?;
    let difficult_rows = if let Some(ref lang) = language {
        stmt_d.query_map(params![lang], row_to_review_item)
    } else {
        stmt_d.query_map([], row_to_review_item)
    }
    .map_err(|e| e.to_string())?;

    let mut difficult: Vec<WordInfo> = Vec::new();
    for r in difficult_rows {
        let item = r.map_err(|e| e.to_string())?;
        let is_new = item.state == "new" || item.state.is_empty();
        difficult.push(WordInfo {
            content: item.content.clone(),
            ef: item.EF,
            r: item.R,
            is_new,
            difficulty_bg_class: item.difficulty_bg_class.clone(),
        });
    }

    let mut stmt_k = conn.prepare(&known_sql).map_err(|e| e.to_string())?;
    let map_string = |row: &rusqlite::Row<'_>| -> rusqlite::Result<String> { row.get(0) };
    let known_rows = if let Some(ref lang) = language {
        stmt_k.query_map(params![lang], map_string)
    } else {
        stmt_k.query_map([], map_string)
    }
    .map_err(|e| e.to_string())?;

    let mut known: Vec<String> = Vec::new();
    for r in known_rows {
        known.push(r.map_err(|e| e.to_string())?.to_lowercase());
    }

    Ok(WordClassification { difficult, known })
}

#[tauri::command]
pub fn db_delete_review_item(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let mut conn = open_db(&app)?;
    // study_card references review_items (foreign keys are on in bundled SQLite),
    // so a card that was ever in a daily queue has to leave it first.
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM study_card WHERE review_item_id=?1", params![id])
        .map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM review_confusions WHERE review_item_id=?1 OR confused_with_id=?1",
        params![id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM review_items WHERE id=?1", params![id])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn db_update_review_item(app: tauri::AppHandle, item: UpdateReviewItem) -> Result<(), String> {
    let conn = open_db(&app)?;
    if let Some(content) = item.content {
        let existing_id: Option<String> = conn.query_row(
            "SELECT id FROM review_items WHERE content=?1 AND id!=?2",
            params![content, item.id],
            |row| row.get(0),
        ).ok();
        if existing_id.is_some() {
            return Err(app_error("word_exists", content));
        }
        conn.execute("UPDATE review_items SET content=?1 WHERE id=?2", params![content, item.id])
            .map_err(|e| e.to_string())?;
    }
    if let Some(language) = item.language {
        conn.execute("UPDATE review_items SET language=?1 WHERE id=?2", params![language, item.id])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn db_import_review_items(app: tauri::AppHandle, items: Vec<ImportItem>) -> Result<String, String> {
    let conn = open_db(&app)?;
    let explain = explain_lang(&conn);
    let mut imported = 0u32;
    for item in items {
        let id = uuid::Uuid::new_v4().to_string();
        let meaning_lang = item.meaning_lang.unwrap_or_else(|| explain.clone());
        let result = conn.execute(
            "INSERT INTO review_items(id, content, EF, I, R, state, step, lapse_count, next_review, language, detail, meaning_lang, fsrs_stability, fsrs_difficulty, created_at)
             VALUES (?1, ?2, -1.0, 0, 0, 'new', 0, 0, NULL, ?3, ?4, ?5, NULL, NULL, CAST(strftime('%s', 'now') * 1000 AS INTEGER))
             ON CONFLICT(content) DO NOTHING",
            params![id, item.content, item.language, item.detail, meaning_lang],
        );
        if let Ok(n) = result {
            if n > 0 { imported += 1; }
        }
    }
    Ok(imported.to_string())
}

/// Part-of-speech codes the frontend translates; the LLM must pick one of these.
pub const PART_OF_SPEECH_CODES: &[&str] = &[
    "noun", "verb", "i_adjective", "na_adjective", "adjective", "adverb", "particle", "pronoun",
    "counter", "conjunction", "interjection", "expression", "prefix", "suffix", "other",
];

fn lang_name_of(code: &str) -> String {
    lang_name(code).to_string()
}

/// Asks the LLM for a card's back (reading, meaning, part of speech, example) as a JSON string,
/// with the meaning and example translation in the learner's explanation language.
pub(crate) async fn generate_detail_text(api_host: &str, api_key: &str, model: &str, content: &str, language: &str, explain: &str) -> Result<String, String> {
    let lang_name = lang_name(language);
    let explain_name = lang_name_of(explain);
    let meaning_hint = match explain {
        "id" => "short Indonesian meaning, the way an Indonesian dictionary glosses it (e.g. 食べる → makan, 駅 → stasiun)".to_string(),
        _ => format!("short {} translation", explain_name),
    };
    let prompt = format!(
        "You are a language learning assistant. For the {lang_name} word/phrase \"{content}\", provide a vocabulary card in this exact JSON format:\n\
        {{\"reading\": \"hiragana reading for Japanese (e.g., たべる for 食べる, しょくどう for 食堂), or IPA/romaji for non-Japanese\", \"meaning\": \"{meaning_hint}\", \"part_of_speech\": \"one of: {pos}\", \"example\": \"a natural example sentence in {lang_name}\", \"example_meaning\": \"{explain_name} translation of the example\"}}\n\
        Translate directly from {lang_name} into {explain_name}, not via another language.\n\
        Return ONLY the JSON object, nothing else.",
        pos = PART_OF_SPEECH_CODES.join(", "),
    );
    let meaning_of = |raw: &str| {
        serde_json::from_str::<serde_json::Value>(json_part(raw))
            .ok()
            .map(|v| {
                let f = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                format!("{} {}", f("meaning"), f("example_meaning"))
            })
            .unwrap_or_default()
    };
    complete_explained(api_host, api_key, model, &prompt, explain, meaning_of).await
}

/// Generates a card's back (practice material for a grammar card). With `draft`, it is kept aside for the user to approve in review
/// instead of being saved as the card's detail.
#[tauri::command]
pub async fn db_generate_detail(
    app: tauri::AppHandle,
    api_host: String,
    api_key: String,
    model: String,
    content: String,
    language: String,
    draft: Option<bool>,
) -> Result<String, String> {
    let explain = explain_lang(&open_db(&app)?);
    let text = generate_card_back(&app, &api_host, &api_key, &model, &content, &language, &explain).await?;
    let conn = open_db(&app)?;
    if draft.unwrap_or(false) {
        conn.execute("UPDATE review_items SET draft_detail=?1 WHERE content=?2", params![text, content])
    } else {
        conn.execute(
            "UPDATE review_items SET detail=?1, meaning_lang=?2 WHERE content=?3",
            params![text, explain, content],
        )
    }
    .map_err(|e| e.to_string())?;
    Ok(text)
}
