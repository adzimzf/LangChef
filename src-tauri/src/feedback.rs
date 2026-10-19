use crate::chat::{complete_explained, json_part, ChatMessage};
use crate::db::open_db;
use crate::locale::{explain_lang, lang_name};
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct UserMessageFeedback {
    pub message_id: i64,
    pub score: i64,
    pub feedback: String,
    pub created_at: i64,
}

fn extract_score_from_json(feedback: &str) -> i64 {
    let cleaned = feedback
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    if let Ok(val) = serde_json::from_str::<serde_json::Value>(cleaned) {
        if let Some(score) = val.get("score").and_then(|s| s.as_i64()) {
            if (1..=5).contains(&score) {
                return score;
            }
        }
    }

    for pattern in &["Score: ", "score: ", "**Score:** ", "# Score: "] {
        if let Some(idx) = feedback.find(pattern) {
            let after = &feedback[idx + pattern.len()..];
            let num_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = num_str.parse::<i64>() {
                if (1..=5).contains(&n) {
                    return n;
                }
            }
        }
    }

    3
}

/// The in-prompt example, in the learner's language so it doesn't pull the model back to English.
fn feedback_example(explain: &str) -> &'static str {
    match explain {
        "id" => r#"{"score":1,"summary":"Jawaban belum lengkap, hanya satu kata tanya.","vocab":[{"word":"何","reading_meaning":"nani / apa","ok":false,"note":"Hanya potongan kata; satu kata tanya saja bukan jawaban yang lengkap."}],"grammar":[{"pattern":"何","ok":false,"note":"Belum menjadi kalimat yang lengkap."}],"target_words":[{"word":"気をつける","used":false,"note":"Belum dipakai. Coba: はい、気をつけます。"},{"word":"はい","used":false,"note":"Belum dipakai. Coba: はい、気をつけます。"}],"suggestion":"はい、気をつけます。"}"#,
        _ => r#"{"score":1,"summary":"Incomplete response with a single question word.","vocab":[{"word":"何","reading_meaning":"nani / what","ok":false,"note":"Fragment used incorrectly; a single question word is not a proper response."}],"grammar":[{"pattern":"何","ok":false,"note":"Not a grammatically complete response."}],"target_words":[{"word":"気をつける","used":false,"note":"Expected but not used. Try: はい、気をつけます。"},{"word":"はい","used":false,"note":"Expected but not used. Try: はい、気をつけます。"}],"suggestion":"はい、気をつけます。"}"#,
    }
}

#[tauri::command]
pub fn db_get_user_feedback(app: tauri::AppHandle, message_id: i64) -> Result<Option<UserMessageFeedback>, String> {
    let conn = open_db(&app)?;
    let result = conn.query_row(
        "SELECT f.message_id, f.score, f.feedback, COALESCE(m.created_at, 0)
         FROM user_message_feedback f
         LEFT JOIN messages m ON m.id = f.message_id
         WHERE f.message_id=?1 AND f.lang=?2",
        params![message_id, explain_lang(&conn)],
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
pub fn db_list_all_feedback(app: tauri::AppHandle) -> Result<Vec<UserMessageFeedback>, String> {
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
pub async fn db_evaluate_user_message(
    app: tauri::AppHandle,
    api_host: String,
    api_key: String,
    model: String,
    message_id: i64,
    message_content: String,
    language: String,
    level: String,
    context_messages: Vec<ChatMessage>,
    target_words: Vec<String>,
) -> Result<UserMessageFeedback, String> {
    let conn = open_db(&app)?;
    let explain = explain_lang(&conn);
    if let Ok(cached) = conn.query_row(
        "SELECT f.message_id, f.score, f.feedback, COALESCE(m.created_at, 0)
         FROM user_message_feedback f
         LEFT JOIN messages m ON m.id = f.message_id
         WHERE f.message_id=?1 AND f.lang=?2",
        params![message_id, explain],
        |row| Ok(UserMessageFeedback {
            message_id: row.get(0)?,
            score: row.get(1)?,
            feedback: row.get(2)?,
            created_at: row.get(3)?,
        }),
    ) {
        return Ok(cached);
    }

    let lang_name = lang_name(&language);
    let level_context = if level.is_empty() {
        String::new()
    } else {
        format!("\nJLPT/Level context: {}", level)
    };

    let target_words_json = if target_words.is_empty() {
        "[]".to_string()
    } else {
        serde_json::to_string(&target_words).unwrap_or_else(|_| "[]".to_string())
    };

    let context_json = if context_messages.is_empty() {
        "[]".to_string()
    } else {
        serde_json::to_string(&context_messages).unwrap_or_else(|_| "[]".to_string())
    };

    let prompt = format!(
        "Evaluate this {lang_name} learner's message.{level_context}\n\
         Message: \"{message_content}\"\n\n\
         Target words: {target_words_json}\n\
         Context: {context_json}\n\n\
         Return ONLY a JSON object (no markdown, no explanation):\n\
         {{\"score\":1-5,\"summary\":\"...\",\"vocab\":[...],\"grammar\":[...],\"target_words\":[...],\"suggestion\":\"...\"}}\n\n\
         - \"vocab\": analyze words the learner ACTUALLY WROTE — check spelling, usage, word choice errors.\n\
         - \"target_words\": check whether each assigned target word was used in the message.\n\
         - Do NOT put target word usage in \"vocab\". These are separate concerns.\n\n\
         Example — if the learner wrote \"何？\" and target words were [\"気をつける\", \"はい\"]:\n\
         {example}\n\n\
         \"summary\" and every \"note\" are read by the learner; \"suggestion\" is a corrected message in {lang_name}.\n\
         Score: 1=many errors, 2=frequent errors, 3=some errors, 4=minor issues, 5=natural.\n\
         Be concise. Omit empty arrays.",
        lang_name = lang_name,
        level_context = level_context,
        message_content = message_content,
        target_words_json = target_words_json,
        context_json = context_json,
        example = feedback_example(&explain),
    );

    let learner_text = |raw: &str| {
        serde_json::from_str::<serde_json::Value>(json_part(raw))
            .ok()
            .and_then(|v| v.get("summary").and_then(|x| x.as_str()).map(String::from))
            .unwrap_or_default()
    };
    let feedback = complete_explained(&api_host, &api_key, &model, &prompt, &explain, learner_text).await?;

    let score = extract_score_from_json(&feedback);

    conn.execute(
        "INSERT OR REPLACE INTO user_message_feedback(message_id, score, feedback, lang) VALUES (?1, ?2, ?3, ?4)",
        params![message_id, score, feedback, explain],
    ).map_err(|e| e.to_string())?;

    let msg_created_at: i64 = conn.query_row(
        "SELECT COALESCE(created_at, 0) FROM messages WHERE id=?1",
        params![message_id],
        |row| row.get(0),
    ).unwrap_or(0);

    Ok(UserMessageFeedback { message_id, score, feedback, created_at: msg_created_at })
}
