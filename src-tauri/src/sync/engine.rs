//! Turning local changes into records (`export`), applying other devices' records with the merge
//! rules (`apply`), the first sync of a device (`initialize` / `adopt`) and catching up from a
//! backup snapshot (`merge_snapshot`).

use std::collections::HashMap;

use rusqlite::types::Value as Sql;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::schema::{backfill_all, filter_sql, meta_get, meta_set, row_json, synced_columns};
use super::spec::{table, Table, Target, TABLES};

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    /// Set these columns (creating the row if needed); last writer wins per column.
    U,
    /// Delete the row, unless a column was written after the delete.
    D,
    /// Add to counter columns.
    C,
    /// Counter value from before sync: the largest one seen counts once.
    B,
}

/// One change, as stored in the change files in Drive (one JSON object per line).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Record {
    /// Sequence number on the device that made it.
    pub s: u64,
    pub t: String,
    /// Key values on every device (`spec::Table::key`, references as global keys).
    pub k: Vec<Value>,
    pub o: Op,
    /// Version: hybrid clock in ms (wall clock, but always above anything the device has seen).
    pub v: i64,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub c: Map<String, Value>,
}

/// (version, device); a higher one wins. The device breaks ties the same way everywhere.
type Ver = (i64, String);

#[derive(PartialEq, Debug)]
enum Outcome {
    Applied,
    Skipped,
    /// Needs a row that hasn't arrived yet (a referenced message, the rest of a new row …).
    Parked,
}

#[derive(Default, Debug, Serialize)]
pub struct Stats {
    pub applied: usize,
    pub skipped: usize,
    pub parked: usize,
}

// ─── Small helpers ────────────────────────────────────────────────────────

pub fn key_string(k: &[Value]) -> String {
    serde_json::to_string(k).unwrap_or_default()
}

fn to_sql(v: &Value) -> Sql {
    match v {
        Value::Null => Sql::Null,
        Value::Bool(b) => Sql::Integer(*b as i64),
        Value::Number(n) => n.as_i64().map(Sql::Integer).unwrap_or_else(|| Sql::Real(n.as_f64().unwrap_or(0.0))),
        Value::String(s) => Sql::Text(s.clone()),
        other => Sql::Text(other.to_string()),
    }
}

fn as_i64(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n.as_i64().unwrap_or_else(|| n.as_f64().unwrap_or(0.0).round() as i64),
        _ => 0,
    }
}

fn clock_next(tx: &Connection, ts: i64) -> Result<i64, String> {
    let clock: i64 = meta_get(tx, "clock").and_then(|v| v.parse().ok()).unwrap_or(0);
    let v = ts.max(clock + 1);
    meta_set(tx, "clock", &v.to_string())?;
    Ok(v)
}

fn clock_observe(tx: &Connection, v: i64) -> Result<(), String> {
    let clock: i64 = meta_get(tx, "clock").and_then(|v| v.parse().ok()).unwrap_or(0);
    if v > clock {
        meta_set(tx, "clock", &v.to_string())?;
    }
    Ok(())
}

fn get_ver(tx: &Connection, t: &str, k: &str, col: &str) -> Result<Option<Ver>, String> {
    tx.query_row(
        "SELECT v, dev FROM sync_version WHERE tbl = ?1 AND k = ?2 AND col = ?3",
        params![t, k, col],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(err)
}

fn set_ver(tx: &Connection, t: &str, k: &str, col: &str, ver: &Ver) -> Result<(), String> {
    tx.execute(
        "INSERT OR REPLACE INTO sync_version(tbl, k, col, v, dev) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![t, k, col, ver.0, ver.1],
    )
    .map(|_| ())
    .map_err(err)
}

pub fn cursors(conn: &Connection) -> Result<HashMap<String, u64>, String> {
    let mut stmt = conn.prepare("SELECT dev, seq FROM sync_cursor").map_err(err)?;
    let r = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64)))
        .map_err(err)?
        .filter_map(Result::ok)
        .collect();
    Ok(r)
}

fn set_cursor(tx: &Connection, dev: &str, seq: u64) -> Result<(), String> {
    tx.execute(
        "INSERT INTO sync_cursor(dev, seq) VALUES (?1, ?2)
         ON CONFLICT(dev) DO UPDATE SET seq = MAX(seq, excluded.seq)",
        params![dev, seq as i64],
    )
    .map(|_| ())
    .map_err(err)
}

/// The device this database syncs as (`None` before its first sync).
pub fn device(conn: &Connection) -> Option<String> {
    meta_get(conn, "device")
}

/// Whether the database holds anything the learner made (decides restore vs merge on first sync).
pub fn has_user_data(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT (SELECT COUNT(*) FROM messages) + (SELECT COUNT(*) FROM review_answers)
              + (SELECT COUNT(*) FROM reading_sessions) + (SELECT COUNT(*) FROM jlpt_sessions)",
        [],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(true)
}

fn key_of(spec: &Table, row: &Map<String, Value>) -> Option<Vec<Value>> {
    let k: Vec<Value> = spec.key.iter().map(|c| row.get(*c).cloned().unwrap_or(Value::Null)).collect();
    (!k.iter().any(Value::is_null)).then_some(k)
}

fn lww_cols<'a>(spec: &Table, cols: &'a [String]) -> impl Iterator<Item = &'a String> + 'a {
    let key = spec.key;
    let counters = spec.counters;
    cols.iter().filter(move |c| !key.contains(&c.as_str()) && !counters.contains(&c.as_str()))
}

// ─── Mapping global keys to this device ───────────────────────────────────

/// Key of the row on this device: a card the other device made separately for the same word maps
/// to ours.
fn local_key(tx: &Connection, spec: &Table, k: &[Value]) -> Result<Vec<Value>, String> {
    if spec.name == "review_items" {
        if let Some(a) = alias(tx, &key_string(k))? {
            return Ok(a);
        }
    }
    Ok(k.to_vec())
}

fn alias(tx: &Connection, remote: &str) -> Result<Option<Vec<Value>>, String> {
    let s: Option<String> = tx
        .query_row(
            "SELECT local_k FROM sync_alias WHERE tbl = 'review_items' AND remote_k = ?1",
            [remote],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    Ok(s.and_then(|s| serde_json::from_str(&s).ok()))
}

/// Local value of a column given its cross-device value. `None`: the referenced row isn't here yet.
fn resolve(tx: &Connection, spec: &Table, col: &str, v: &Value, kind: Option<&str>) -> Result<Option<Sql>, String> {
    if v.is_null() {
        return Ok(Some(Sql::Null));
    }
    let target = match spec.ref_target(col) {
        Some(Target::ByKind) => match kind {
            Some("message") => Some(Target::Messages),
            Some("reading") => Some(Target::Reading),
            _ => None,
        },
        t => t,
    };
    let by_gid = |table: &str| -> Result<Option<Sql>, String> {
        tx.query_row(&format!("SELECT id FROM {table} WHERE gid = ?1"), [v.as_str().unwrap_or("")], |r| r.get::<_, i64>(0))
            .optional()
            .map(|o| o.map(Sql::Integer))
            .map_err(err)
    };
    match target {
        None => Ok(Some(to_sql(v))),
        Some(Target::Messages) => by_gid("messages"),
        Some(Target::Reading) => by_gid("reading_sessions"),
        Some(Target::Breakdowns) => {
            let parts: Vec<Value> = serde_json::from_str(v.as_str().unwrap_or("[]")).unwrap_or_default();
            if parts.len() != 3 {
                return Ok(None);
            }
            tx.query_row(
                "SELECT id FROM breakdowns WHERE language = ?1 AND explain_lang = ?2 AND sentence = ?3",
                params_from_iter(parts.iter().map(to_sql)),
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .map(|o| o.map(Sql::Integer))
            .map_err(err)
        }
        Some(Target::Item) => Ok(Some(match alias(tx, &key_string(std::slice::from_ref(v)))? {
            Some(a) => to_sql(&a[0]),
            None => to_sql(v),
        })),
        Some(Target::ByKind) => unreachable!(),
    }
}

fn kind_of<'a>(spec: &Table, k: &'a [Value]) -> Option<&'a str> {
    spec.key.iter().position(|c| *c == "kind").and_then(|i| k[i].as_str())
}

fn resolve_key(tx: &Connection, spec: &Table, lk: &[Value]) -> Result<Option<Vec<Sql>>, String> {
    let kind = kind_of(spec, lk);
    let mut out = Vec::with_capacity(lk.len());
    for (col, v) in spec.key.iter().zip(lk) {
        match resolve(tx, spec, col, v, kind)? {
            Some(s) => out.push(s),
            None => return Ok(None),
        }
    }
    Ok(Some(out))
}

fn where_key(spec: &Table, start: usize) -> String {
    spec.key
        .iter()
        .enumerate()
        .map(|(i, c)| format!("\"{c}\" IS ?{}", start + i))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn row_exists(tx: &Connection, spec: &Table, keyvals: &[Sql]) -> Result<bool, String> {
    tx.query_row(
        &format!("SELECT 1 FROM \"{}\" WHERE {}", spec.name, where_key(spec, 1)),
        params_from_iter(keyvals.iter()),
        |_| Ok(()),
    )
    .optional()
    .map(|o| o.is_some())
    .map_err(err)
}

/// Inserts a row from its key (and given columns). Errors are returned as text for the caller.
fn insert_row(tx: &Connection, spec: &Table, keyvals: &[Sql], cols: &[(String, Sql)]) -> Result<(), String> {
    let names: Vec<String> = spec.key.iter().map(|c| format!("\"{c}\"")).chain(cols.iter().map(|(c, _)| format!("\"{c}\""))).collect();
    let marks: Vec<String> = (1..=names.len()).map(|i| format!("?{i}")).collect();
    let values: Vec<Sql> = keyvals.iter().cloned().chain(cols.iter().map(|(_, v)| v.clone())).collect();
    tx.execute(
        &format!("INSERT INTO \"{}\" ({}) VALUES ({})", spec.name, names.join(", "), marks.join(", ")),
        params_from_iter(values.iter()),
    )
    .map(|_| ())
    .map_err(err)
}

// ─── Merge rules ──────────────────────────────────────────────────────────

/// Last writer wins per column. `cols` carry their own versions.
fn upsert(tx: &Connection, spec: &Table, k: &[Value], cols: &[(String, Value, Ver)]) -> Result<Outcome, String> {
    let lk = local_key(tx, spec, k)?;
    let lks = key_string(&lk);
    let Some(keyvals) = resolve_key(tx, spec, &lk)? else {
        return Ok(Outcome::Parked);
    };
    let tomb = get_ver(tx, spec.name, &lks, "_del")?;
    let mut winners = Vec::new();
    for (c, v, ver) in cols {
        if tomb.as_ref().is_some_and(|t| ver <= t) {
            continue;
        }
        if get_ver(tx, spec.name, &lks, c)?.is_some_and(|cur| *ver <= cur) {
            continue;
        }
        winners.push((c, v, ver));
    }
    // A row with no other columns (known_words): existence only.
    let existence_only = cols.is_empty();
    if winners.is_empty() && !existence_only {
        return Ok(Outcome::Skipped);
    }
    let kind = kind_of(spec, &lk);
    let mut values = Vec::with_capacity(winners.len());
    for (c, v, _) in &winners {
        match resolve(tx, spec, c, v, kind)? {
            Some(s) => values.push(((*c).clone(), s)),
            None => return Ok(Outcome::Parked),
        }
    }

    if row_exists(tx, spec, &keyvals)? {
        if !values.is_empty() {
            let sets: Vec<String> = values.iter().enumerate().map(|(i, (c, _))| format!("\"{c}\" = ?{}", i + 1)).collect();
            let all: Vec<Sql> = values.iter().map(|(_, v)| v.clone()).chain(keyvals.iter().cloned()).collect();
            if let Err(e) = tx.execute(
                &format!("UPDATE \"{}\" SET {} WHERE {}", spec.name, sets.join(", "), where_key(spec, values.len() + 1)),
                params_from_iter(all.iter()),
            ) {
                eprintln!("[sync] update {} failed: {e}", spec.name);
                return Ok(Outcome::Skipped);
            }
        }
    } else if let Err(e) = insert_row(tx, spec, &keyvals, &values) {
        if spec.name == "review_items" && e.contains("review_items.content") && lk == k {
            // The same word, added separately on two devices: one card, ours.
            let content = values.iter().find(|(c, _)| c == "content").map(|(_, v)| v.clone());
            let local: Option<String> = match content {
                Some(c) => tx
                    .query_row("SELECT id FROM review_items WHERE content = ?1", [c], |r| r.get(0))
                    .optional()
                    .map_err(err)?,
                None => None,
            };
            let Some(local) = local else { return Ok(Outcome::Skipped) };
            tx.execute(
                "INSERT OR REPLACE INTO sync_alias(tbl, remote_k, local_k) VALUES ('review_items', ?1, ?2)",
                params![key_string(k), key_string(&[Value::String(local)])],
            )
            .map_err(err)?;
            return upsert(tx, spec, k, cols);
        }
        if e.contains("NOT NULL") {
            return Ok(Outcome::Parked);
        }
        eprintln!("[sync] insert into {} failed: {e}", spec.name);
        return Ok(Outcome::Skipped);
    }
    for (c, _, ver) in &winners {
        set_ver(tx, spec.name, &lks, c, ver)?;
    }
    Ok(Outcome::Applied)
}

/// A delete loses to any column written after it.
fn delete(tx: &Connection, spec: &Table, k: &[Value], ver: &Ver) -> Result<Outcome, String> {
    let lk = local_key(tx, spec, k)?;
    let lks = key_string(&lk);
    let newest: Option<Ver> = tx
        .query_row(
            "SELECT v, dev FROM sync_version WHERE tbl = ?1 AND k = ?2 ORDER BY v DESC, dev DESC LIMIT 1",
            params![spec.name, lks],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(err)?;
    if newest.is_some_and(|n| n >= *ver) {
        return Ok(Outcome::Skipped);
    }
    if let Some(keyvals) = resolve_key(tx, spec, &lk)? {
        tx.execute(&format!("DELETE FROM \"{}\" WHERE {}", spec.name, where_key(spec, 1)), params_from_iter(keyvals.iter()))
            .map_err(err)?;
    }
    set_ver(tx, spec.name, &lks, "_del", ver)?;
    Ok(Outcome::Applied)
}

fn ensure_counter_row(tx: &Connection, spec: &Table, keyvals: &[Sql]) -> Result<bool, String> {
    if row_exists(tx, spec, keyvals)? {
        return Ok(true);
    }
    let zeros: Vec<(String, Sql)> = spec.counters.iter().map(|c| (c.to_string(), Sql::Integer(0))).collect();
    Ok(insert_row(tx, spec, keyvals, &zeros).is_ok())
}

fn add_counter(tx: &Connection, spec: &Table, keyvals: &[Sql], col: &str, delta: i64) -> Result<(), String> {
    let mut all = vec![Sql::Integer(delta)];
    all.extend(keyvals.iter().cloned());
    tx.execute(
        &format!("UPDATE \"{}\" SET \"{col}\" = COALESCE(\"{col}\", 0) + ?1 WHERE {}", spec.name, where_key(spec, 2)),
        params_from_iter(all.iter()),
    )
    .map(|_| ())
    .map_err(err)
}

fn counters(tx: &Connection, spec: &Table, k: &[Value], c: &Map<String, Value>, base: bool) -> Result<Outcome, String> {
    let lk = local_key(tx, spec, k)?;
    let lks = key_string(&lk);
    let Some(keyvals) = resolve_key(tx, spec, &lk)? else {
        return Ok(Outcome::Parked);
    };
    if !ensure_counter_row(tx, spec, &keyvals)? {
        return Ok(Outcome::Parked);
    }
    for (col, v) in c {
        if !spec.is_counter(col) {
            continue;
        }
        let n = as_i64(Some(v));
        if base {
            let old: i64 = tx
                .query_row(
                    "SELECT base FROM sync_counter_base WHERE tbl = ?1 AND k = ?2 AND col = ?3",
                    params![spec.name, lks, col],
                    |r| r.get(0),
                )
                .optional()
                .map_err(err)?
                .unwrap_or(0);
            if n > old {
                add_counter(tx, spec, &keyvals, col, n - old)?;
                tx.execute(
                    "INSERT OR REPLACE INTO sync_counter_base(tbl, k, col, base) VALUES (?1, ?2, ?3, ?4)",
                    params![spec.name, lks, col, n],
                )
                .map_err(err)?;
            }
        } else if n != 0 {
            add_counter(tx, spec, &keyvals, col, n)?;
        }
    }
    Ok(Outcome::Applied)
}

fn apply_one(tx: &Connection, dev: &str, rec: &Record) -> Result<Outcome, String> {
    let Some(spec) = table(&rec.t) else {
        return Ok(Outcome::Skipped);
    };
    let ver: Ver = (rec.v, dev.to_string());
    match rec.o {
        Op::U => {
            let cols: Vec<(String, Value, Ver)> = rec.c.iter().map(|(c, v)| (c.clone(), v.clone(), ver.clone())).collect();
            upsert(tx, spec, &rec.k, &cols)
        }
        Op::D => delete(tx, spec, &rec.k, &ver),
        Op::C => counters(tx, spec, &rec.k, &rec.c, false),
        Op::B => counters(tx, spec, &rec.k, &rec.c, true),
    }
}

fn park(tx: &Connection, dev: &str, rec: &Record) -> Result<(), String> {
    tx.execute(
        "INSERT INTO sync_parked(dev, rec) VALUES (?1, ?2)",
        params![dev, serde_json::to_string(rec).map_err(err)?],
    )
    .map(|_| ())
    .map_err(err)
}

/// Retries records that waited for another row, until nothing more goes through.
fn retry_parked(tx: &Connection, stats: &mut Stats) -> Result<(), String> {
    loop {
        let parked: Vec<(i64, String, String)> = {
            let mut stmt = tx.prepare("SELECT id, dev, rec FROM sync_parked ORDER BY id").map_err(err)?;
            let r = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map_err(err)?
                .filter_map(Result::ok)
                .collect();
            r
        };
        let mut progress = false;
        for (id, dev, rec) in parked {
            let Ok(rec) = serde_json::from_str::<Record>(&rec) else {
                tx.execute("DELETE FROM sync_parked WHERE id = ?1", [id]).map_err(err)?;
                continue;
            };
            if apply_one(tx, &dev, &rec)? != Outcome::Parked {
                tx.execute("DELETE FROM sync_parked WHERE id = ?1", [id]).map_err(err)?;
                stats.parked = stats.parked.saturating_sub(1);
                stats.applied += 1;
                progress = true;
            }
        }
        if !progress {
            return Ok(());
        }
    }
}

fn begin_apply(conn: &mut Connection) -> Result<Transaction<'_>, String> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(err)?;
    meta_set(&tx, "applying", "1")?;
    Ok(tx)
}

fn end_apply(tx: Transaction<'_>) -> Result<(), String> {
    tx.execute("DELETE FROM sync_meta WHERE k = 'applying'", []).map_err(err)?;
    tx.commit().map_err(err)
}

// ─── Public operations ────────────────────────────────────────────────────

/// Applies another device's (or this device's own, after a restore) records with sequence numbers
/// past what this database has seen from that device.
pub fn apply(conn: &mut Connection, dev: &str, records: &[Record]) -> Result<Stats, String> {
    let tx = begin_apply(conn)?;
    let mut last = cursors(&tx)?.get(dev).copied().unwrap_or(0);
    let mut stats = Stats::default();
    for rec in records {
        if rec.s <= last {
            continue;
        }
        clock_observe(&tx, rec.v)?;
        match apply_one(&tx, dev, rec)? {
            Outcome::Applied => stats.applied += 1,
            Outcome::Skipped => stats.skipped += 1,
            Outcome::Parked => {
                park(&tx, dev, rec)?;
                stats.parked += 1;
            }
        }
        last = rec.s;
    }
    set_cursor(&tx, dev, last)?;
    retry_parked(&tx, &mut stats)?;
    end_apply(tx)?;
    Ok(stats)
}

fn push(out: &mut Vec<Record>, next_seq: &mut u64, t: &str, k: Vec<Value>, o: Op, v: i64, c: Map<String, Value>) {
    out.push(Record { s: *next_seq, t: t.to_string(), k, o, v, c });
    *next_seq += 1;
}

/// Records for every local change since the last export. `next_seq` is this device's sequence
/// counter (kept outside the database so it never goes back after a restore).
pub fn export(conn: &mut Connection, me: &str, next_seq: &mut u64) -> Result<Vec<Record>, String> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(err)?;
    type OutboxRow = (i64, String, String, Option<String>, Option<String>, i64);
    let rows: Vec<OutboxRow> = {
        let mut stmt = tx.prepare("SELECT seq, tbl, op, row, old, ts FROM sync_outbox ORDER BY seq").map_err(err)?;
        let r = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))
            .map_err(err)?
            .filter_map(Result::ok)
            .collect();
        r
    };
    let parse = |s: &Option<String>| s.as_deref().and_then(|s| serde_json::from_str::<Map<String, Value>>(s).ok());
    let mut out = Vec::new();
    let mut max_seq = 0;
    for (seq, tbl, op, row, old, ts) in &rows {
        max_seq = *seq;
        let Some(spec) = table(tbl) else { continue };
        let cols = synced_columns(&tx, spec)?;
        let (row, old) = (parse(row), parse(old));
        let v = clock_next(&tx, *ts)?;
        let full = |out: &mut Vec<Record>, next_seq: &mut u64, row: &Map<String, Value>| {
            let Some(k) = key_of(spec, row) else { return };
            let c: Map<String, Value> = lww_cols(spec, &cols).map(|c| (c.clone(), row.get(c).cloned().unwrap_or(Value::Null))).collect();
            push(out, next_seq, tbl, k.clone(), Op::U, v, c);
            let counters: Map<String, Value> = spec
                .counters
                .iter()
                .filter(|c| as_i64(row.get(**c)) != 0)
                .map(|c| (c.to_string(), row.get(*c).cloned().unwrap_or(Value::Null)))
                .collect();
            if !counters.is_empty() {
                push(out, next_seq, tbl, k, Op::C, v, counters);
            }
        };
        match (op.as_str(), &row, &old) {
            ("I", Some(row), _) => full(&mut out, next_seq, row),
            ("U", Some(row), Some(old)) => {
                let (kn, ko) = (key_of(spec, row), key_of(spec, old));
                if kn != ko {
                    if let Some(ko) = ko {
                        push(&mut out, next_seq, tbl, ko, Op::D, v, Map::new());
                    }
                    full(&mut out, next_seq, row);
                } else if let Some(k) = kn {
                    let c: Map<String, Value> = lww_cols(spec, &cols)
                        .filter(|c| row.get(*c) != old.get(*c))
                        .map(|c| (c.clone(), row.get(c).cloned().unwrap_or(Value::Null)))
                        .collect();
                    if !c.is_empty() {
                        push(&mut out, next_seq, tbl, k.clone(), Op::U, v, c);
                    }
                    let d: Map<String, Value> = spec
                        .counters
                        .iter()
                        .map(|c| (c.to_string(), as_i64(row.get(*c)) - as_i64(old.get(*c))))
                        .filter(|(_, d)| *d != 0)
                        .map(|(c, d)| (c, Value::from(d)))
                        .collect();
                    if !d.is_empty() {
                        push(&mut out, next_seq, tbl, k, Op::C, v, d);
                    }
                }
            }
            ("D", _, Some(old)) => {
                if let Some(k) = key_of(spec, old) {
                    push(&mut out, next_seq, tbl, k, Op::D, v, Map::new());
                }
            }
            _ => {}
        }
    }
    // This device's own writes are the newest it knows.
    for rec in &out {
        let spec = table(&rec.t).expect("known table");
        let lks = key_string(&local_key(&tx, spec, &rec.k)?);
        let ver: Ver = (rec.v, me.to_string());
        match rec.o {
            Op::U => {
                for c in rec.c.keys() {
                    set_ver(&tx, &rec.t, &lks, c, &ver)?;
                }
            }
            Op::D => set_ver(&tx, &rec.t, &lks, "_del", &ver)?,
            _ => {}
        }
    }
    tx.execute("DELETE FROM sync_outbox WHERE seq <= ?1", [max_seq]).map_err(err)?;
    if let Some(last) = out.last() {
        set_cursor(&tx, me, last.s)?;
    }
    tx.commit().map_err(err)?;
    Ok(out)
}

/// First sync of a database with data of its own: everything in it becomes records (old
/// versions, so any real edit elsewhere wins; counters as before-sync values). Turns on change
/// recording.
pub fn initialize(conn: &mut Connection, me: &str, next_seq: &mut u64) -> Result<Vec<Record>, String> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(err)?;
    tx.execute("DELETE FROM sync_outbox", []).map_err(err)?;
    backfill_all(&tx)?;
    let mut out = Vec::new();
    for spec in TABLES {
        let cols = synced_columns(&tx, spec)?;
        // A card's state counts from its last review, so the more recently studied copy wins.
        let version = if spec.name == "review_items" {
            "COALESCE((SELECT MAX(answered_at) FROM review_answers ra WHERE ra.review_item_id = r.id), 1)"
        } else {
            "1"
        };
        let sql = format!(
            "SELECT {}, {version} FROM \"{}\" r WHERE {}",
            row_json(spec, &cols, "r"),
            spec.name,
            filter_sql(spec, "r")
        );
        let rows: Vec<(String, i64)> = {
            let mut stmt = tx.prepare(&sql).map_err(err)?;
            let r = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.filter_map(Result::ok).collect();
            r
        };
        for (json, v) in rows {
            let Ok(row) = serde_json::from_str::<Map<String, Value>>(&json) else { continue };
            let Some(k) = key_of(spec, &row) else { continue };
            let lks = key_string(&k);
            let c: Map<String, Value> = lww_cols(spec, &cols).map(|c| (c.clone(), row.get(c).cloned().unwrap_or(Value::Null))).collect();
            for col in c.keys() {
                tx.execute(
                    "INSERT OR IGNORE INTO sync_version(tbl, k, col, v, dev) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![spec.name, lks, col, v, me],
                )
                .map_err(err)?;
            }
            push(&mut out, next_seq, spec.name, k.clone(), Op::U, v, c);
            let base: Map<String, Value> = spec
                .counters
                .iter()
                .filter(|c| as_i64(row.get(**c)) > 0)
                .map(|c| (c.to_string(), row.get(*c).cloned().unwrap_or(Value::Null)))
                .collect();
            for (col, n) in &base {
                tx.execute(
                    "INSERT INTO sync_counter_base(tbl, k, col, base) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(tbl, k, col) DO UPDATE SET base = MAX(base, excluded.base)",
                    params![spec.name, lks, col, as_i64(Some(n))],
                )
                .map_err(err)?;
            }
            if !base.is_empty() {
                push(&mut out, next_seq, spec.name, k, Op::B, v, base);
            }
        }
    }
    meta_set(&tx, "device", me)?;
    if let Some(last) = out.last() {
        set_cursor(&tx, me, last.s)?;
    }
    tx.commit().map_err(err)?;
    Ok(out)
}

/// This database came from another device's backup (restore): its rows are already known to
/// every device, so only take it over as this device.
pub fn adopt(conn: &mut Connection, me: &str) -> Result<(), String> {
    let tx = conn.transaction().map_err(err)?;
    tx.execute("DELETE FROM sync_outbox", []).map_err(err)?;
    tx.execute("DELETE FROM sync_parked", []).map_err(err)?;
    meta_set(&tx, "device", me)?;
    tx.commit().map_err(err)
}

/// Merges another device's backup snapshot into this database row by row (a device joining
/// with data of its own, or one that was offline longer than change files are kept).
pub fn merge_snapshot(conn: &mut Connection, snap: &Connection, me: &str) -> Result<Stats, String> {
    let snap_dev = device(snap).unwrap_or_else(|| "snapshot".into());
    let tx = begin_apply(conn)?;
    let mut stats = Stats::default();
    fn note(o: Outcome, stats: &mut Stats) {
        match o {
            Outcome::Applied => stats.applied += 1,
            Outcome::Skipped => stats.skipped += 1,
            Outcome::Parked => stats.parked += 1,
        }
    }
    for spec in TABLES {
        let cols = synced_columns(snap, spec)?;
        let local_cols = synced_columns(&tx, spec)?;
        let cols: Vec<String> = cols.into_iter().filter(|c| local_cols.contains(c)).collect();
        let sql = format!("SELECT {} FROM \"{}\" r WHERE {}", row_json(spec, &cols, "r"), spec.name, filter_sql(spec, "r"));
        let rows: Vec<String> = {
            let mut stmt = snap.prepare(&sql).map_err(err)?;
            let r = stmt.query_map([], |r| r.get(0)).map_err(err)?.filter_map(Result::ok).collect();
            r
        };
        for json in rows {
            let Ok(row) = serde_json::from_str::<Map<String, Value>>(&json) else { continue };
            let Some(k) = key_of(spec, &row) else { continue };
            let ks = key_string(&k);
            let mut cv = Vec::new();
            for c in lww_cols(spec, &cols) {
                let ver = get_ver(snap, spec.name, &ks, c)?.unwrap_or((1, snap_dev.clone()));
                cv.push((c.clone(), row.get(c).cloned().unwrap_or(Value::Null), ver));
            }
            let o = upsert(&tx, spec, &k, &cv)?;
            if o == Outcome::Parked {
                // Park one record per version so the retry uses the same rules.
                let mut groups: HashMap<Ver, Map<String, Value>> = HashMap::new();
                for (c, v, ver) in cv {
                    groups.entry(ver).or_default().insert(c, v);
                }
                for ((v, dev), c) in groups {
                    park(&tx, &dev, &Record { s: 0, t: spec.name.into(), k: k.clone(), o: Op::U, v, c })?;
                }
            }
            note(o, &mut stats);
            let base: Map<String, Value> = spec
                .counters
                .iter()
                .filter(|c| as_i64(row.get(**c)) > 0)
                .map(|c| (c.to_string(), row.get(*c).cloned().unwrap_or(Value::Null)))
                .collect();
            if !base.is_empty() {
                let rec = Record { s: 0, t: spec.name.into(), k: k.clone(), o: Op::B, v: 1, c: base };
                let o = apply_one(&tx, &snap_dev, &rec)?;
                if o == Outcome::Parked {
                    park(&tx, &snap_dev, &rec)?;
                }
                note(o, &mut stats);
            }
        }
        // Deletes the snapshot knows about.
        let tombs: Vec<(String, i64, String)> = {
            let mut stmt = snap
                .prepare("SELECT k, v, dev FROM sync_version WHERE tbl = ?1 AND col = '_del'")
                .map_err(err)?;
            let r = stmt
                .query_map([spec.name], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map_err(err)?
                .filter_map(Result::ok)
                .collect();
            r
        };
        for (ks, v, dev) in tombs {
            if let Ok(k) = serde_json::from_str::<Vec<Value>>(&ks) {
                let o = delete(&tx, spec, &k, &(v, dev))?;
                note(o, &mut stats);
            }
        }
    }
    for (dev, seq) in cursors(snap)? {
        if dev != me {
            set_cursor(&tx, &dev, seq)?;
        }
    }
    clock_observe(&tx, meta_get(snap, "clock").and_then(|v| v.parse().ok()).unwrap_or(0))?;
    retry_parked(&tx, &mut stats)?;
    end_apply(tx)?;
    Ok(stats)
}

/// Records as a compressed change file: a header line, then one record per line.
pub fn encode(dev: &str, records: &[Record]) -> Result<Vec<u8>, String> {
    let mut text = serde_json::json!({ "device": dev, "format": 1 }).to_string();
    text.push('\n');
    for r in records {
        text.push_str(&serde_json::to_string(r).map_err(err)?);
        text.push('\n');
    }
    zstd::encode_all(text.as_bytes(), 3).map_err(err)
}

pub fn decode(data: &[u8]) -> Result<(String, Vec<Record>), String> {
    let text = String::from_utf8(zstd::decode_all(data).map_err(err)?).map_err(err)?;
    let mut lines = text.lines();
    let header: Value = serde_json::from_str(lines.next().unwrap_or("{}")).map_err(err)?;
    let dev = header["device"].as_str().unwrap_or("").to_string();
    let records = lines.filter(|l| !l.is_empty()).map(serde_json::from_str).collect::<Result<Vec<Record>, _>>().map_err(err)?;
    Ok((dev, records))
}
