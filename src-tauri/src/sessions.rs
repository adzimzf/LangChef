use crate::db::open_db;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub created_at: i64,
    pub parent_message_id: Option<i64>,
    pub is_active_path: i64,
}

#[derive(Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    pub title: Option<String>,
    pub language: Option<String>,
    pub level: Option<String>,
    pub created_at: i64,
    pub last_activity: i64,
}

#[derive(Serialize)]
pub struct SiblingMessage {
    pub id: i64,
    pub content: String,
    pub is_active_path: i64,
}

#[tauri::command]
pub fn db_add_message(app: tauri::AppHandle, role: String, content: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "INSERT INTO messages(role, content, created_at) VALUES (?1, ?2, ?3)",
        params![role, content, now],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn db_new_session(app: tauri::AppHandle, title: Option<String>, language: Option<String>, level: Option<String>) -> Result<String, String> {
    let conn = open_db(&app)?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "INSERT INTO chat_sessions(id, title, language, level, created_at, last_activity) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, title, language, level, now, now],
    ).map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command]
pub fn db_list_sessions(app: tauri::AppHandle) -> Result<Vec<ChatSession>, String> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare("SELECT id, title, language, level, created_at, last_activity FROM chat_sessions ORDER BY last_activity DESC")
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok(ChatSession {
            id: row.get(0)?,
            title: row.get(1)?,
            language: row.get(2)?,
            level: row.get(3)?,
            created_at: row.get(4)?,
            last_activity: row.get(5)?,
        })
    }).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
pub fn db_add_message_in_session(app: tauri::AppHandle, session_id: String, role: String, content: String, parent_message_id: Option<i64>) -> Result<i64, String> {
    let conn = open_db(&app)?;
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "INSERT INTO messages(role, content, created_at, session_id, parent_message_id, is_active_path) VALUES (?1, ?2, ?3, ?4, ?5, 1)",
        params![role, content, now, session_id, parent_message_id],
    ).map_err(|e| e.to_string())?;
    conn.execute("UPDATE chat_sessions SET last_activity=?1 WHERE id=?2", params![now, session_id])
        .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

#[tauri::command]
pub fn db_list_session_messages(app: tauri::AppHandle, session_id: String) -> Result<Vec<Message>, String> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare("SELECT id, role, content, created_at, parent_message_id, is_active_path FROM messages WHERE session_id=?1 ORDER BY created_at ASC")
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![session_id], |row| {
        Ok(Message {
            id: row.get(0)?,
            role: row.get(1)?,
            content: row.get(2)?,
            created_at: row.get(3)?,
            parent_message_id: row.get(4)?,
            is_active_path: row.get(5)?,
        })
    }).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
pub fn db_update_message_content(app: tauri::AppHandle, message_id: i64, content: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute(
        "UPDATE messages SET content=?1 WHERE id=?2",
        params![content, message_id],
    ).map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM user_message_feedback WHERE message_id=?1",
        params![message_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn db_deactivate_message_branch(app: tauri::AppHandle, message_id: i64) -> Result<(), String> {
    let conn = open_db(&app)?;
    let mut to_deactivate = vec![message_id];
    let mut idx = 0;
    while idx < to_deactivate.len() {
        let current = to_deactivate[idx];
        let mut stmt = conn.prepare("SELECT id FROM messages WHERE parent_message_id=?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![current], |row| row.get::<_, i64>(0))
            .map_err(|e| e.to_string())?;
        for r in rows {
            to_deactivate.push(r.map_err(|e| e.to_string())?);
        }
        idx += 1;
    }
    for id in to_deactivate {
        conn.execute("UPDATE messages SET is_active_path=0 WHERE id=?1", params![id])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn db_get_message_siblings(app: tauri::AppHandle, message_id: i64) -> Result<Vec<SiblingMessage>, String> {
    let conn = open_db(&app)?;
    let parent_id: Option<i64> = conn.query_row(
        "SELECT parent_message_id FROM messages WHERE id=?1",
        params![message_id],
        |row| row.get(0),
    ).map_err(|e| e.to_string())?;

    let mut stmt = conn.prepare(
        "SELECT id, content, is_active_path FROM messages WHERE parent_message_id=?1 AND role='user' ORDER BY created_at ASC"
    ).map_err(|e| e.to_string())?;

    let parent_param = match parent_id {
        Some(id) => id as i64,
        None => 0i64,
    };

    let rows = if parent_id.is_none() {
        let mut stmt2 = conn.prepare(
            "SELECT id, content, is_active_path FROM messages WHERE parent_message_id IS NULL AND role='user' AND session_id=(SELECT session_id FROM messages WHERE id=?1) ORDER BY created_at ASC"
        ).map_err(|e| e.to_string())?;
        let mapped = stmt2.query_map(params![message_id], |row| {
            Ok(SiblingMessage {
                id: row.get(0)?,
                content: row.get(1)?,
                is_active_path: row.get(2)?,
            })
        }).map_err(|e| e.to_string())?;
        mapped.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
    } else {
        let mapped = stmt.query_map(params![parent_param], |row| {
            Ok(SiblingMessage {
                id: row.get(0)?,
                content: row.get(1)?,
                is_active_path: row.get(2)?,
            })
        }).map_err(|e| e.to_string())?;
        mapped.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
    };

    Ok(rows)
}

#[tauri::command]
pub fn db_get_bot_response_for_user(app: tauri::AppHandle, user_message_id: i64) -> Result<Option<Message>, String> {
    let conn = open_db(&app)?;
    let result = conn.query_row(
        "SELECT id, role, content, created_at, parent_message_id, is_active_path FROM messages WHERE parent_message_id=?1 AND role='assistant' ORDER BY created_at DESC LIMIT 1",
        params![user_message_id],
        |row| Ok(Message {
            id: row.get(0)?,
            role: row.get(1)?,
            content: row.get(2)?,
            created_at: row.get(3)?,
            parent_message_id: row.get(4)?,
            is_active_path: row.get(5)?,
        }),
    );
    match result {
        Ok(msg) => Ok(Some(msg)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub fn db_rename_session(app: tauri::AppHandle, session_id: String, title: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute("UPDATE chat_sessions SET title=?1 WHERE id=?2", params![title, session_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn db_set_session_topic(app: tauri::AppHandle, session_id: String, topic: Option<String>) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute("UPDATE chat_sessions SET topic=?1 WHERE id=?2", params![topic, session_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn db_get_session_topic(app: tauri::AppHandle, session_id: String) -> Result<Option<String>, String> {
    let conn = open_db(&app)?;
    let result = conn.query_row(
        "SELECT topic FROM chat_sessions WHERE id=?1",
        params![session_id],
        |row| row.get(0),
    );
    match result {
        Ok(topic) => Ok(topic),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub fn db_delete_session(app: tauri::AppHandle, session_id: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute("DELETE FROM messages WHERE session_id=?1", params![session_id])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM chat_sessions WHERE id=?1", params![session_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}
