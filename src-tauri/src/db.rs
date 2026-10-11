use rusqlite::{Connection, params};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

pub fn db_file(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("langchef.db"))
}

pub fn open_db(app: &tauri::AppHandle) -> Result<Connection, String> {
    let path = db_file(app)?;
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS messages(
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          role TEXT NOT NULL,
          content TEXT NOT NULL,
          created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS review_items(
          id TEXT PRIMARY KEY,
          content TEXT UNIQUE NOT NULL,
          EF REAL NOT NULL,
          I INTEGER NOT NULL,
          R INTEGER NOT NULL,
          state TEXT NOT NULL DEFAULT 'new',
          step INTEGER NOT NULL DEFAULT 0,
          lapse_count INTEGER NOT NULL DEFAULT 0,
          next_review INTEGER,
          tags TEXT,
          language TEXT,
          detail TEXT,
          mnemonic TEXT,
          fsrs_stability REAL,
          fsrs_difficulty REAL
        );
        CREATE TABLE IF NOT EXISTS known_words(
          word TEXT PRIMARY KEY
        );
        CREATE TABLE IF NOT EXISTS telemetry_days(
          date TEXT PRIMARY KEY,
          sessions INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS user_activity(
          date TEXT NOT NULL,
          activity_type TEXT NOT NULL,
          count INTEGER NOT NULL DEFAULT 0,
          PRIMARY KEY (date, activity_type)
        );
        CREATE TABLE IF NOT EXISTS daily_active_time(
          date TEXT PRIMARY KEY,
          total_seconds INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS chat_sessions(
          id TEXT PRIMARY KEY,
          title TEXT,
          language TEXT,
          created_at INTEGER NOT NULL,
          last_activity INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS grammar_items(
          id TEXT PRIMARY KEY,
          pattern TEXT NOT NULL,
          detail TEXT NOT NULL DEFAULT '',
          language TEXT NOT NULL,
          level TEXT NOT NULL DEFAULT '',
          is_default INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS message_grammar_analysis(
            message_id INTEGER PRIMARY KEY,
            analysis TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS user_message_feedback(
            message_id INTEGER PRIMARY KEY,
            score INTEGER NOT NULL,
            feedback TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS study_limits(
            date TEXT NOT NULL,
            language TEXT NOT NULL DEFAULT '',
            reviews_completed INTEGER NOT NULL DEFAULT 0,
            new_cards_seen INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (date, language)
        );
        CREATE TABLE IF NOT EXISTS study(
            study_id INTEGER PRIMARY KEY AUTOINCREMENT,
            date TEXT NOT NULL,
            language TEXT NOT NULL,
            max_review INTEGER NOT NULL,
            max_new INTEGER NOT NULL,
            UNIQUE(date, language)
        );
        CREATE TABLE IF NOT EXISTS study_card(
            study_id INTEGER NOT NULL REFERENCES study(study_id),
            review_item_id TEXT NOT NULL REFERENCES review_items(id),
            kind TEXT NOT NULL,
            status TEXT NOT NULL,
            create_time INTEGER NOT NULL,
            update_time INTEGER NOT NULL,
            PRIMARY KEY (study_id, review_item_id)
        );
        CREATE INDEX IF NOT EXISTS idx_study_card_status ON study_card(study_id, status);
        CREATE TABLE IF NOT EXISTS review_answers(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            review_item_id TEXT NOT NULL,
            language TEXT,
            task TEXT NOT NULL,
            correct INTEGER NOT NULL,
            dont_know INTEGER NOT NULL DEFAULT 0,
            elapsed_ms INTEGER NOT NULL,
            hint_used INTEGER NOT NULL DEFAULT 0,
            chosen TEXT,
            chosen_item_id TEXT,
            grade TEXT NOT NULL,
            easy_allowed INTEGER NOT NULL DEFAULT 0,
            prev_state TEXT,
            overridden INTEGER NOT NULL DEFAULT 0,
            answered_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_review_answers_lang_time ON review_answers(language, answered_at);
        CREATE TABLE IF NOT EXISTS review_confusions(
            review_item_id TEXT NOT NULL,
            confused_with_id TEXT NOT NULL,
            count INTEGER NOT NULL DEFAULT 1,
            last_at INTEGER NOT NULL,
            PRIMARY KEY (review_item_id, confused_with_id)
        );
        CREATE TABLE IF NOT EXISTS reading_sessions(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            language TEXT NOT NULL,
            level TEXT NOT NULL,
            title TEXT NOT NULL,
            body TEXT NOT NULL,
            plain_body TEXT NOT NULL,
            source TEXT NOT NULL,
            questions TEXT NOT NULL DEFAULT '[]',
            answers TEXT NOT NULL DEFAULT '[]',
            phase TEXT NOT NULL DEFAULT 'reading',
            current_q INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (date('now'))
        );
        "
    ).map_err(|e| e.to_string())?;
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN session_id TEXT", []);
    let _ = conn.execute("ALTER TABLE chat_sessions ADD COLUMN language TEXT", []);
    let _ = conn.execute("ALTER TABLE chat_sessions ADD COLUMN selected_grammar TEXT", []);
    let _ = conn.execute("ALTER TABLE chat_sessions ADD COLUMN level TEXT", []);
    let _ = conn.execute("ALTER TABLE chat_sessions ADD COLUMN topic TEXT", []);
    let _ = conn.execute("ALTER TABLE review_items ADD COLUMN language TEXT", []);
    let _ = conn.execute("ALTER TABLE review_items ADD COLUMN detail TEXT", []);
    let _ = conn.execute("ALTER TABLE review_items ADD COLUMN state TEXT DEFAULT 'new'", []);
    let _ = conn.execute("ALTER TABLE review_items ADD COLUMN step INTEGER DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE review_items ADD COLUMN lapse_count INTEGER DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE review_items ADD COLUMN mnemonic TEXT", []);
    let _ = conn.execute("ALTER TABLE study_card ADD COLUMN task TEXT", []);
    let _ = conn.execute("ALTER TABLE review_items ADD COLUMN fsrs_stability REAL", []);
    let _ = conn.execute("ALTER TABLE review_items ADD COLUMN fsrs_difficulty REAL", []);
    let _ = conn.execute("ALTER TABLE review_items ADD COLUMN created_at INTEGER", []);
    let _ = conn.execute(
        "UPDATE review_items SET created_at = CAST(strftime('%s', 'now') * 1000 AS INTEGER)
         WHERE created_at IS NULL",
        [],
    );
    let _ = conn.execute("ALTER TABLE review_items ADD COLUMN mastered_at INTEGER", []);
    let _ = conn.execute(
        "UPDATE review_items SET mastered_at = CAST(strftime('%s', 'now') * 1000 AS INTEGER)
         WHERE mastered_at IS NULL AND state = 'review'",
        [],
    );
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN parent_message_id INTEGER", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN is_active_path INTEGER DEFAULT 1", []);
    let _ = conn.execute("UPDATE review_items SET state = 'review' WHERE state IS NULL OR state = ''", []);
    let _ = conn.execute("UPDATE review_items SET state = 'new' WHERE state IS NULL OR state = ''", []);
    seed_bundled_grammar_if_empty(&conn);
    migrate_sm2_to_fsrs(&conn);
    migrate_telemetry_to_activity(&conn)?;
    Ok(conn)
}

fn migrate_telemetry_to_activity(conn: &Connection) -> Result<(), String> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM user_activity",
        [],
        |row| row.get(0),
    ).map_err(|e| e.to_string())?;
    if count > 0 {
        return Ok(());
    }
    let telemetry_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM telemetry_days",
        [],
        |row| row.get(0),
    ).map_err(|e| e.to_string())?;
    if telemetry_count == 0 {
        return Ok(());
    }
    conn.execute("BEGIN TRANSACTION", []).map_err(|e| e.to_string())?;
    let result = conn.execute(
        "INSERT INTO user_activity (date, activity_type, count)
         SELECT date, 'chat', sessions FROM telemetry_days",
        [],
    );
    match result {
        Ok(_) => {
            conn.execute("COMMIT", []).map_err(|e| e.to_string())?;
            Ok(())
        }
        Err(e) => {
            let _ = conn.execute("ROLLBACK", []);
            Err(e.to_string())
        }
    }
}

fn migrate_sm2_to_fsrs(conn: &Connection) {
    let migrated_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM review_items WHERE EF = -1.0",
        [],
        |row| row.get(0),
    ).unwrap_or(0);
    if migrated_count > 0 {
        return;
    }

    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM review_items WHERE EF != -1.0",
        [],
        |row| row.get(0),
    ).unwrap_or(0);
    if total == 0 {
        return;
    }

    let fsrs = match fsrs::FSRS::new(Some(&[])) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[fsrs] Failed to create FSRS instance for migration: {}", e);
            return;
        }
    };

    let mut stmt = match conn.prepare("SELECT id, EF, I FROM review_items WHERE EF != -1.0") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[fsrs] Failed to prepare migration query: {}", e);
            return;
        }
    };

    let rows = match stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?, row.get::<_, i64>(2)?))
    }) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[fsrs] Failed to query cards for migration: {}", e);
            return;
        }
    };

    let mut update_stmt = match conn.prepare(
        "UPDATE review_items SET EF = -1.0, fsrs_stability = ?1, fsrs_difficulty = ?2 WHERE id = ?3"
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[fsrs] Failed to prepare update statement: {}", e);
            return;
        }
    };

    let mut success = 0u32;
    let mut failures = 0u32;

    for row in rows {
        let (id, ef, interval) = match row {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[fsrs] Error reading row: {}", e);
                failures += 1;
                continue;
            }
        };

        match fsrs.memory_state_from_sm2(ef as f32, interval as f32, 0.9) {
            Ok(memory_state) => {
                if let Err(e) = update_stmt.execute(params![
                    memory_state.stability,
                    memory_state.difficulty,
                    id,
                ]) {
                    eprintln!("[fsrs] Failed to update card {}: {}", id, e);
                    failures += 1;
                } else {
                    success += 1;
                }
            }
            Err(e) => {
                eprintln!("[fsrs] Failed to convert card {} (EF={}, I={}): {}", id, ef, interval, e);
                failures += 1;
            }
        }
    }

    eprintln!("[fsrs] Migration complete: {} succeeded, {} failed", success, failures);
}

static BUNDLED_GRAMMAR: &[(&str, &str, &str)] = &[
    ("ja", "N5", include_str!("../../grammar/ja/N5.md")),
    ("ja", "N4", include_str!("../../grammar/ja/N4.md")),
    ("ja", "N3", include_str!("../../grammar/ja/N3.md")),
    ("ja", "N2", include_str!("../../grammar/ja/N2.md")),
    ("ja", "N1", include_str!("../../grammar/ja/N1.md")),
    ("id", "A1", include_str!("../../grammar/id/A1.md")),
    ("id", "A2", include_str!("../../grammar/id/A2.md")),
];

fn parse_grammar_md(content: &str) -> Vec<(String, String)> {
    let mut items = Vec::new();
    let sections = content.split("\n---\n");

    for section in sections {
        let section = section.trim();
        if section.is_empty() {
            continue;
        }

        let mut pattern = String::new();
        let mut detail_start = 0;
        for (i, line) in section.lines().enumerate() {
            if let Some(rest) = line.strip_prefix("## ") {
                pattern = rest.trim().to_string();
                detail_start = i + 1;
                break;
            }
        }

        if pattern.is_empty() {
            continue;
        }

        let detail: String = section
            .lines()
            .skip(detail_start)
            .collect::<Vec<&str>>()
            .join("\n")
            .trim()
            .to_string();

        items.push((pattern, detail));
    }
    items
}

fn seed_bundled_grammar_if_empty(conn: &Connection) {
    for &(lang, level, csv) in BUNDLED_GRAMMAR {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM grammar_items WHERE language=?1 AND level=?2",
            params![lang, level],
            |row| row.get(0),
        ).unwrap_or(0);

        if count > 0 {
            continue;
        }

        let items = parse_grammar_md(csv);
        eprintln!("[seed] grammar/{}/{level}: parsed {} items", lang, items.len());
        for (pattern, detail) in items {
            let id = uuid::Uuid::new_v4().to_string();
            match conn.execute(
                "INSERT INTO grammar_items(id, pattern, detail, language, level, is_default) VALUES (?1, ?2, ?3, ?4, ?5, 1)",
                params![id, pattern, detail, lang, level],
            ) {
                Ok(_) => {}
                Err(e) => eprintln!("[seed] FAILED to insert '{}': {}", pattern, e),
            }
        }
    }
}
