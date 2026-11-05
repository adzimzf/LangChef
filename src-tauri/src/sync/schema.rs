//! Sync bookkeeping tables, global ids for rows with integer ids, deterministic ids for bundled
//! grammar, and the triggers that record every local change in `sync_outbox`.

use std::collections::HashMap;

use rusqlite::types::ValueRef;
use rusqlite::{params, Connection, OptionalExtension};

use super::spec::{export_expr, Table, TABLES};

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub const ACTIVE: &str = "EXISTS (SELECT 1 FROM sync_meta WHERE k = 'device') \
     AND NOT EXISTS (SELECT 1 FROM sync_meta WHERE k = 'applying')";

pub const NOW_MS: &str = "CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)";

pub fn meta_get(conn: &Connection, k: &str) -> Option<String> {
    conn.query_row("SELECT v FROM sync_meta WHERE k = ?1", [k], |r| r.get(0)).optional().ok().flatten()
}

pub fn meta_set(conn: &Connection, k: &str, v: &str) -> Result<(), String> {
    conn.execute("INSERT OR REPLACE INTO sync_meta(k, v) VALUES (?1, ?2)", [k, v]).map(|_| ()).map_err(err)
}

/// Columns of `table` that travel between devices (all but the device-local ones).
pub fn synced_columns(conn: &Connection, table: &Table) -> Result<Vec<String>, String> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info(\"{}\")", table.name)).map_err(err)?;
    let cols = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(err)?
        .filter_map(Result::ok)
        .filter(|c| !table.skip.contains(&c.as_str()))
        .collect();
    Ok(cols)
}

/// `json_object(...)` of a row's synced columns, references mapped to global keys.
pub fn row_json(table: &Table, cols: &[String], r: &str) -> String {
    let parts: Vec<String> = cols.iter().map(|c| format!("'{c}', {}", export_expr(table, c, r))).collect();
    format!("json_object({})", parts.join(", "))
}

pub fn filter_sql(table: &Table, r: &str) -> String {
    table.filter.map(|f| f.replace("{R}", r)).unwrap_or_else(|| "1".into())
}

/// Stable id for a bundled grammar pattern, the same on every device. `n` numbers repeats of
/// the same pattern within one bundled file.
pub fn bundled_grammar_id(lang: &str, level: &str, explain: &str, pattern: &str, n: usize) -> String {
    let h = blake3::hash(format!("{lang}\x1f{level}\x1f{explain}\x1f{pattern}\x1f{n}").as_bytes());
    format!("b-{}", &h.to_hex()[..24])
}

fn value_text(v: ValueRef) -> String {
    match v {
        ValueRef::Null => "\u{2205}".into(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => format!("{f}"),
        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
        ValueRef::Blob(b) => blake3::hash(b).to_hex().to_string(),
    }
}

/// Rows written while sync was off get a gid from fields that never change, so two devices that
/// share history (one restored the other's backup) give the shared rows the same gid.
fn backfill_gids(conn: &Connection, table: &Table) -> Result<(), String> {
    let fields = table.legacy_fields.iter().map(|f| format!("\"{f}\"")).collect::<Vec<_>>().join(", ");
    let mut stmt = conn
        .prepare(&format!("SELECT rowid, {fields} FROM \"{}\" WHERE gid IS NULL", table.name))
        .map_err(err)?;
    let mut rows = stmt.query([]).map_err(err)?;
    let mut updates = Vec::new();
    while let Some(row) = rows.next().map_err(err)? {
        let rowid: i64 = row.get(0).map_err(err)?;
        let mut text = String::from(table.name);
        for i in 0..table.legacy_fields.len() {
            text.push('\x1f');
            text.push_str(&value_text(row.get_ref(i + 1).map_err(err)?));
        }
        updates.push((rowid, format!("L{}", &blake3::hash(text.as_bytes()).to_hex()[..31])));
    }
    drop(rows);
    for (rowid, gid) in updates {
        conn.execute(&format!("UPDATE \"{}\" SET gid = ?1 WHERE rowid = ?2", table.name), params![gid, rowid])
            .map_err(err)?;
    }
    Ok(())
}

/// Gids for rows written while sync was off (also run by `prepare` on every open).
pub fn backfill_all(conn: &Connection) -> Result<(), String> {
    for table in TABLES.iter().filter(|t| t.gid) {
        backfill_gids(conn, table)?;
    }
    Ok(())
}

/// Gives bundled grammar rows made before stable ids existed their stable id, and moves every
/// reference to them (cards, translated rows, a chat's selected patterns).
fn migrate_bundled_grammar_ids(conn: &Connection) -> Result<(), String> {
    if meta_get(conn, "grammar_ids").is_some() {
        return Ok(());
    }
    let rows: Vec<(String, String, String, String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, language, level, explain_lang, pattern FROM grammar_items
                 WHERE is_default = 1 ORDER BY rowid",
            )
            .map_err(err)?;
        let r = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .map_err(err)?
            .filter_map(Result::ok)
            .collect();
        r
    };
    let mut seen: HashMap<(String, String, String, String), usize> = HashMap::new();
    for (old, lang, level, explain, pattern) in rows {
        let n = seen.entry((lang.clone(), level.clone(), explain.clone(), pattern.clone())).or_insert(0);
        let new = bundled_grammar_id(&lang, &level, &explain, &pattern, *n);
        *n += 1;
        if old == new {
            continue;
        }
        conn.execute("UPDATE grammar_items SET id = ?1 WHERE id = ?2", params![new, old]).map_err(err)?;
        conn.execute("UPDATE grammar_items SET source_id = ?1 WHERE source_id = ?2", params![new, old]).map_err(err)?;
        conn.execute("UPDATE review_items SET grammar_id = ?1 WHERE grammar_id = ?2", params![new, old]).map_err(err)?;
        conn.execute(
            "UPDATE chat_sessions SET selected_grammar = REPLACE(selected_grammar, '\"' || ?2 || '\"', '\"' || ?1 || '\"')
             WHERE instr(selected_grammar, ?2) > 0",
            params![new, old],
        )
        .map_err(err)?;
    }
    meta_set(conn, "grammar_ids", "1")
}

fn trigger_sql(conn: &Connection, table: &Table) -> Result<String, String> {
    let cols = synced_columns(conn, table)?;
    let t = table.name;
    let changed = cols.iter().map(|c| format!("OLD.\"{c}\" IS NOT NEW.\"{c}\"")).collect::<Vec<_>>().join(" OR ");
    let set_gid = if table.gid {
        format!("UPDATE \"{t}\" SET gid = lower(hex(randomblob(16))) WHERE rowid = NEW.rowid AND gid IS NULL;")
    } else {
        String::new()
    };
    let gid_guard = if table.gid { " AND OLD.gid IS NOT NULL" } else { "" };
    Ok(format!(
        "CREATE TRIGGER sync_{t}_i AFTER INSERT ON \"{t}\" WHEN {ACTIVE} AND {fnew} BEGIN
           {set_gid}
           INSERT INTO sync_outbox(tbl, op, row, old, ts)
             SELECT '{t}', 'I', {row_r}, NULL, {NOW_MS} FROM \"{t}\" r WHERE r.rowid = NEW.rowid;
         END;
         CREATE TRIGGER sync_{t}_u AFTER UPDATE ON \"{t}\" WHEN {ACTIVE}{gid_guard} AND {fnew} AND {fold} AND ({changed}) BEGIN
           INSERT INTO sync_outbox(tbl, op, row, old, ts) VALUES ('{t}', 'U', {row_new}, {row_old}, {NOW_MS});
         END;
         CREATE TRIGGER sync_{t}_d AFTER DELETE ON \"{t}\" WHEN {ACTIVE} AND {fold} BEGIN
           INSERT INTO sync_outbox(tbl, op, row, old, ts) VALUES ('{t}', 'D', NULL, {row_old}, {NOW_MS});
         END;",
        fnew = filter_sql(table, "NEW"),
        fold = filter_sql(table, "OLD"),
        row_r = row_json(table, &cols, "r"),
        row_new = row_json(table, &cols, "NEW"),
        row_old = row_json(table, &cols, "OLD"),
    ))
}

/// Runs on every database open (from `db::prepare_db`) and must stay idempotent.
pub fn prepare(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sync_meta(k TEXT PRIMARY KEY, v TEXT);
         CREATE TABLE IF NOT EXISTS sync_outbox(
           seq INTEGER PRIMARY KEY AUTOINCREMENT, tbl TEXT NOT NULL, op TEXT NOT NULL,
           row TEXT, old TEXT, ts INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS sync_version(
           tbl TEXT NOT NULL, k TEXT NOT NULL, col TEXT NOT NULL, v INTEGER NOT NULL, dev TEXT NOT NULL,
           PRIMARY KEY (tbl, k, col));
         CREATE TABLE IF NOT EXISTS sync_cursor(dev TEXT PRIMARY KEY, seq INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS sync_alias(tbl TEXT NOT NULL, remote_k TEXT NOT NULL, local_k TEXT NOT NULL,
           PRIMARY KEY (tbl, remote_k));
         CREATE TABLE IF NOT EXISTS sync_counter_base(tbl TEXT NOT NULL, k TEXT NOT NULL, col TEXT NOT NULL,
           base INTEGER NOT NULL, PRIMARY KEY (tbl, k, col));
         CREATE TABLE IF NOT EXISTS sync_parked(id INTEGER PRIMARY KEY AUTOINCREMENT, dev TEXT NOT NULL, rec TEXT NOT NULL);",
    )
    .map_err(err)?;

    for table in TABLES.iter().filter(|t| t.gid) {
        let _ = conn.execute(&format!("ALTER TABLE \"{}\" ADD COLUMN gid TEXT", table.name), []);
        backfill_gids(conn, table)?;
        conn.execute(
            &format!("CREATE UNIQUE INDEX IF NOT EXISTS sync_gid_{0} ON \"{0}\"(gid)", table.name),
            [],
        )
        .map_err(err)?;
    }
    migrate_bundled_grammar_ids(conn)?;

    // Triggers follow the current columns; rebuilt only when that changes.
    let mut sql = String::new();
    for table in TABLES {
        sql.push_str(&trigger_sql(conn, table)?);
    }
    let sig = blake3::hash(sql.as_bytes()).to_hex().to_string();
    if meta_get(conn, "triggers").as_deref() != Some(sig.as_str()) {
        let mut drop = String::new();
        for table in TABLES {
            for op in ["i", "u", "d"] {
                drop.push_str(&format!("DROP TRIGGER IF EXISTS sync_{}_{op};", table.name));
            }
        }
        conn.execute_batch(&format!("BEGIN; {drop} {sql} COMMIT;")).map_err(err)?;
        meta_set(conn, "triggers", &sig)?;
    }
    Ok(())
}
