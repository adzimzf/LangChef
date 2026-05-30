use crate::db::open_db;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct TelemetryDay {
    pub date: String,
    pub sessions: i64,
}

#[derive(Serialize, Deserialize)]
pub struct UserActivity {
    pub date: String,
    pub activity_type: String,
    pub count: i64,
}

#[derive(Serialize, Deserialize)]
pub struct DailyActiveTime {
    pub date: String,
    pub total_seconds: i64,
}

#[derive(Serialize)]
pub struct MasteryDay {
    pub date: String,
    pub mastered: i64,
}

#[derive(Serialize)]
pub struct CardsAddedDay {
    pub date: String,
    pub total: i64,
}

#[tauri::command]
pub fn db_increment_session_today(app: tauri::AppHandle) -> Result<(), String> {
    db_record_activity(app, "chat".to_string())
}

#[tauri::command]
pub fn db_get_telemetry_days(app: tauri::AppHandle) -> Result<Vec<TelemetryDay>, String> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare("SELECT date, sessions FROM telemetry_days ORDER BY date ASC")
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok(TelemetryDay {
            date: row.get(0)?,
            sessions: row.get(1)?,
        })
    }).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
pub fn db_record_activity(app: tauri::AppHandle, activity_type: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    let today = chrono::Local::now().date_naive().to_string();
    conn.execute(
        "INSERT INTO user_activity(date, activity_type, count) VALUES (?1, ?2, 1)
         ON CONFLICT(date, activity_type) DO UPDATE SET count = count + 1",
        params![today, activity_type],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn db_get_user_activity(app: tauri::AppHandle, date_from: String, date_to: String) -> Result<Vec<UserActivity>, String> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare(
        "SELECT date, activity_type, count FROM user_activity
         WHERE date >= ?1 AND date <= ?2
         ORDER BY date ASC"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![date_from, date_to], |row| {
        Ok(UserActivity {
            date: row.get(0)?,
            activity_type: row.get(1)?,
            count: row.get(2)?,
        })
    }).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
pub fn db_record_active_time(app: tauri::AppHandle) -> Result<(), String> {
    let conn = open_db(&app)?;
    let today = chrono::Local::now().date_naive().to_string();
    conn.execute(
        "INSERT INTO daily_active_time(date, total_seconds) VALUES (?1, 30)
         ON CONFLICT(date) DO UPDATE SET total_seconds = MIN(total_seconds + 30, 57600)",
        params![today],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn db_get_active_time(app: tauri::AppHandle, date_from: String, date_to: String) -> Result<Vec<DailyActiveTime>, String> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare(
        "SELECT date, total_seconds FROM daily_active_time
         WHERE date >= ?1 AND date <= ?2
         ORDER BY date ASC"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![date_from, date_to], |row| {
        Ok(DailyActiveTime {
            date: row.get(0)?,
            total_seconds: row.get(1)?,
        })
    }).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
pub fn db_get_mastery_history(app: tauri::AppHandle, date_from: String, date_to: String) -> Result<Vec<MasteryDay>, String> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare(
        "SELECT date(mastered_at / 1000, 'unixepoch', 'localtime') as day, COUNT(*) as cnt
         FROM review_items
         WHERE mastered_at IS NOT NULL
         GROUP BY day
         ORDER BY day ASC"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    }).map_err(|e| e.to_string())?;

    use std::collections::HashMap;
    let mut daily_map: HashMap<String, i64> = HashMap::new();
    for r in rows {
        let (day, cnt) = r.map_err(|e| e.to_string())?;
        daily_map.insert(day, cnt);
    }

    let start = chrono::NaiveDate::parse_from_str(&date_from, "%Y-%m-%d")
        .map_err(|e| e.to_string())?;
    let end = chrono::NaiveDate::parse_from_str(&date_to, "%Y-%m-%d")
        .map_err(|e| e.to_string())?;

    let baseline: i64 = conn.query_row(
        "SELECT COUNT(*) FROM review_items
         WHERE mastered_at IS NOT NULL
           AND date(mastered_at / 1000, 'unixepoch', 'localtime') < ?1",
        params![date_from],
        |row| row.get(0),
    ).unwrap_or(0);

    let mut out = Vec::new();
    let mut cumulative = baseline;
    let mut d = start;
    while d <= end {
        let ds = d.to_string();
        cumulative += daily_map.get(&ds).copied().unwrap_or(0);
        out.push(MasteryDay { date: ds, mastered: cumulative });
        d = d.succ_opt().unwrap_or(end);
    }
    Ok(out)
}

#[tauri::command]
pub fn db_get_cards_added_history(app: tauri::AppHandle, date_from: String, date_to: String) -> Result<Vec<CardsAddedDay>, String> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare(
        "SELECT date(created_at / 1000, 'unixepoch', 'localtime') as day, COUNT(*) as cnt
         FROM review_items
         WHERE created_at IS NOT NULL
         GROUP BY day
         ORDER BY day ASC"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    }).map_err(|e| e.to_string())?;

    use std::collections::HashMap;
    let mut daily_map: HashMap<String, i64> = HashMap::new();
    for r in rows {
        let (day, cnt) = r.map_err(|e| e.to_string())?;
        daily_map.insert(day, cnt);
    }

    let start = chrono::NaiveDate::parse_from_str(&date_from, "%Y-%m-%d")
        .map_err(|e| e.to_string())?;
    let end = chrono::NaiveDate::parse_from_str(&date_to, "%Y-%m-%d")
        .map_err(|e| e.to_string())?;

    let baseline: i64 = conn.query_row(
        "SELECT COUNT(*) FROM review_items
         WHERE created_at IS NOT NULL
           AND date(created_at / 1000, 'unixepoch', 'localtime') < ?1",
        params![date_from],
        |row| row.get(0),
    ).unwrap_or(0);

    let mut out = Vec::new();
    let mut cumulative = baseline;
    let mut d = start;
    while d <= end {
        let ds = d.to_string();
        cumulative += daily_map.get(&ds).copied().unwrap_or(0);
        out.push(CardsAddedDay { date: ds, total: cumulative });
        d = d.succ_opt().unwrap_or(end);
    }
    Ok(out)
}
