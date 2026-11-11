//! Full / incremental backup engine, ported from `adzimzf/sqlite-incremental-backup`
//! (`src/backup_engine.rs`, `full_backup.rs`, `incremental_backup.rs`, `restore.rs`).
//!
//! Same decisions as upstream — a full backup when the chain is empty or has `max_delta_chain`
//! deltas, otherwise a delta of the pages that changed since the last backup — with these changes
//! for an app that keeps its backups in the cloud:
//! - Pages are compared between two consistent snapshots taken with SQLite's online backup API
//!   (page-for-page copies), not by reading the live database file, and the app's database is
//!   never switched to WAL.
//! - Only the latest state (`baseline.db`) stays on the device; full and delta files are made in
//!   a work folder, uploaded, then removed. Restoring rebuilds the chain from downloaded files.
//! - Full backups are stored zstd-compressed, and a restore truncates the file to the page count
//!   the last delta recorded, so a database that shrank comes back at its real size.

use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::backup::Backup;
use rusqlite::Connection;

use super::delta::{self, DeltaPageEntry};
use super::manifest::{BackupEntry, BackupType, Manifest};

pub const MAX_DELTA_CHAIN: u32 = 10;
pub const COMPRESSION_LEVEL: i32 = 3;

pub struct Options<'a> {
    pub max_delta_chain: u32,
    pub compression_level: i32,
    /// Start a new chain even if a delta would do (used when this device overwrites the cloud).
    pub force_full: bool,
    pub device_id: &'a str,
    pub device_os: &'a str,
}

/// A backup file ready to upload. `commit` makes its snapshot the new baseline once it is stored.
pub struct Prepared {
    pub entry: BackupEntry,
    pub file: PathBuf,
    pub page_size: u32,
    snapshot: PathBuf,
}

pub enum Plan {
    /// Nothing changed since the baseline.
    NoChanges,
    Upload(Box<Prepared>),
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn pragma_u32(conn: &Connection, name: &str) -> Result<u32, String> {
    conn.query_row(&format!("PRAGMA {name}"), [], |r| r.get::<_, i64>(0))
        .map(|v| v as u32)
        .map_err(err)
}

fn quick_check(conn: &Connection) -> Result<(), String> {
    let r: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0)).map_err(err)?;
    if r == "ok" {
        Ok(())
    } else {
        Err(format!("integrity check failed: {r}"))
    }
}

/// Copies `db_path` to `dest` with SQLite's online backup API and checks the copy.
/// Returns (page_size, page_count).
pub fn snapshot(db_path: &Path, dest: &Path) -> Result<(u32, u32), String> {
    let _ = fs::remove_file(dest);
    let src = Connection::open(db_path).map_err(err)?;
    src.busy_timeout(Duration::from_secs(10)).map_err(err)?;
    {
        let mut dst = Connection::open(dest).map_err(err)?;
        Backup::new(&src, &mut dst)
            .map_err(err)?
            .run_to_completion(1024, Duration::ZERO, None)
            .map_err(err)?;
    }
    let dst = Connection::open(dest).map_err(err)?;
    quick_check(&dst)?;
    Ok((pragma_u32(&dst, "page_size")?, pragma_u32(&dst, "page_count")?))
}

/// Bytes of the database header that SQLite rewrites on every copy (change counter,
/// version-valid-for, library version). A page 1 that differs only there holds no change.
fn header_only_change(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len()
        && a.len() >= 100
        && a.iter()
            .zip(b)
            .enumerate()
            .all(|(i, (x, y))| x == y || (24..28).contains(&i) || (92..100).contains(&i))
}

/// Pages of `current` that differ from `baseline` (or are past its end).
fn diff_pages(current: &Path, baseline: &Path, page_size: u32, page_count: u32) -> Result<Vec<DeltaPageEntry>, String> {
    let mut cur = BufReader::new(File::open(current).map_err(err)?);
    let mut base = BufReader::new(File::open(baseline).map_err(err)?);
    let ps = page_size as usize;
    let mut a = vec![0u8; ps];
    let mut b = vec![0u8; ps];
    let mut base_ok = true;
    let mut changed = Vec::new();
    for pgno in 1..=page_count {
        cur.read_exact(&mut a).map_err(err)?;
        if base_ok && base.read_exact(&mut b).is_err() {
            base_ok = false;
        }
        if !base_ok || a != b {
            changed.push(DeltaPageEntry { page_number: pgno, data: a.clone() });
        }
    }
    // Page 1 alone with only header bookkeeping changed: nothing to back up.
    if changed.len() == 1 && changed[0].page_number == 1 && base_ok {
        let mut base_first = vec![0u8; ps];
        File::open(baseline).map_err(err)?.read_exact(&mut base_first).map_err(err)?;
        if header_only_change(&changed[0].data, &base_first) {
            changed.clear();
        }
    }
    Ok(changed)
}

fn checksum_file(path: &Path) -> Result<String, String> {
    let mut h = blake3::Hasher::new();
    std::io::copy(&mut File::open(path).map_err(err)?, &mut h).map_err(err)?;
    Ok(h.finalize().to_hex().to_string())
}

/// Decides full vs incremental like upstream `run_backup`, and writes the file to upload into
/// `work_dir`. `baseline` is the state of `manifest.latest()` on this device, if known.
pub fn prepare_backup(
    db_path: &Path,
    work_dir: &Path,
    manifest: &Manifest,
    baseline: Option<&Path>,
    opts: &Options,
) -> Result<Plan, String> {
    fs::create_dir_all(work_dir).map_err(err)?;
    let snap = work_dir.join("snapshot.db");
    let (page_size, page_count) = snapshot(db_path, &snap)?;

    let id = manifest.next_id();
    let tag: String = opts.device_id.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
    let baseline = baseline.filter(|p| p.exists());
    let baseline_page_size = match baseline {
        Some(p) => Some(pragma_u32(&Connection::open(p).map_err(err)?, "page_size")?),
        None => None,
    };
    let full = opts.force_full
        || manifest.should_create_full_backup(opts.max_delta_chain)
        || manifest.page_size != page_size
        || baseline_page_size != Some(page_size);

    let (file_name, pages_changed, parent_id, backup_type) = if full {
        let name = format!("full_{id:04}_{tag}.db.zst");
        let mut out = File::create(work_dir.join(&name)).map_err(err)?;
        zstd::stream::copy_encode(File::open(&snap).map_err(err)?, &mut out, opts.compression_level).map_err(err)?;
        out.sync_all().map_err(err)?;
        (name, None, None, BackupType::Full)
    } else {
        let latest = manifest.latest().expect("non-empty chain");
        let pages = diff_pages(&snap, baseline.expect("checked"), page_size, page_count)?;
        if pages.is_empty() {
            let _ = fs::remove_file(&snap);
            return Ok(Plan::NoChanges);
        }
        let data = delta::serialize_delta(&pages, page_count, page_size, latest.id, opts.compression_level)?;
        let name = format!("delta_{id:04}_{tag}.bin");
        let mut out = File::create(work_dir.join(&name)).map_err(err)?;
        out.write_all(&data).map_err(err)?;
        out.sync_all().map_err(err)?;
        (name, Some(pages.len() as u32), Some(latest.id), BackupType::Delta)
    };

    let file = work_dir.join(&file_name);
    let entry = BackupEntry {
        id,
        backup_type,
        timestamp: chrono::Utc::now(),
        file_size_bytes: fs::metadata(&file).map_err(err)?.len(),
        blake3_checksum: checksum_file(&file)?,
        file_name,
        pages_changed,
        parent_id,
        page_count,
        device_id: opts.device_id.to_string(),
        device_os: opts.device_os.to_string(),
        drive_file_id: None,
    };
    Ok(Plan::Upload(Box::new(Prepared { entry, file, page_size, snapshot: snap })))
}

/// After the upload: the snapshot becomes the baseline for the next delta.
pub fn commit(prepared: Prepared, baseline: &Path) -> Result<(), String> {
    fs::rename(&prepared.snapshot, baseline).map_err(err)?;
    let _ = fs::remove_file(&prepared.file);
    Ok(())
}

/// Throws a prepared backup away (upload failed or the cloud changed meanwhile).
pub fn discard(prepared: Prepared) {
    let _ = fs::remove_file(&prepared.file);
    let _ = fs::remove_file(&prepared.snapshot);
}

/// Checks a downloaded file against the checksum the manifest recorded.
pub fn verify(entry: &BackupEntry, data: &[u8]) -> Result<(), String> {
    if blake3::hash(data).to_hex().as_str() == entry.blake3_checksum {
        Ok(())
    } else {
        Err(format!("checksum mismatch for backup #{} ({})", entry.id, entry.file_name))
    }
}

/// Writes a delta's pages into `db_path` and cuts the file to the page count it records.
/// `parent_id` is the backup the delta must follow.
pub fn apply_delta_to_file(db_path: &Path, data: &[u8], parent_id: u64) -> Result<(), String> {
    use std::io::{Seek, SeekFrom};
    let (header, pages) = delta::parse_delta(data)?;
    if header.parent_backup_id != parent_id {
        return Err(format!("delta follows backup #{}, not #{parent_id}", header.parent_backup_id));
    }
    let ps = header.page_size as u64;
    let mut f = fs::OpenOptions::new().read(true).write(true).open(db_path).map_err(err)?;
    for p in &pages {
        f.seek(SeekFrom::Start((p.page_number as u64 - 1) * ps)).map_err(err)?;
        f.write_all(&p.data).map_err(err)?;
    }
    f.set_len(header.source_page_count as u64 * ps).map_err(err)?;
    f.sync_all().map_err(err)
}

/// Rebuilds a database at `out` from a chain (a full backup, then its deltas in order), each
/// entry with its file's bytes. Checks every file's checksum and the result's integrity.
pub fn restore_chain(chain: &[(BackupEntry, Vec<u8>)], out: &Path) -> Result<(), String> {
    let (first, rest) = chain.split_first().ok_or("empty backup chain")?;
    if first.0.backup_type != BackupType::Full {
        return Err("backup chain does not start with a full backup".into());
    }
    verify(&first.0, &first.1)?;
    let _ = fs::remove_file(out);
    {
        let mut f = File::create(out).map_err(err)?;
        zstd::stream::copy_decode(first.1.as_slice(), &mut f).map_err(err)?;
        f.sync_all().map_err(err)?;
    }
    let mut prev = first.0.id;
    for (entry, data) in rest {
        if entry.backup_type != BackupType::Delta {
            return Err("unexpected full backup inside a chain".into());
        }
        verify(entry, data)?;
        apply_delta_to_file(out, data, prev)?;
        prev = entry.id;
    }
    let conn = Connection::open(out).map_err(err)?;
    let r: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0)).map_err(err)?;
    if r != "ok" {
        return Err(format!("restored database failed integrity_check: {r}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("lc-backup-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn opts(force_full: bool) -> Options<'static> {
        Options {
            max_delta_chain: 3,
            compression_level: 3,
            force_full,
            device_id: "dev-a",
            device_os: "test",
        }
    }

    fn rows(path: &Path) -> Vec<(i64, String)> {
        let c = Connection::open(path).unwrap();
        let mut s = c.prepare("SELECT id, v FROM t ORDER BY id").unwrap();
        let r = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        r.map(|x| x.unwrap()).collect()
    }

    /// Runs one backup like the sync code does and keeps the uploaded file's bytes.
    fn backup(db: &Path, dir: &Path, m: &mut Manifest, store: &mut Vec<(BackupEntry, Vec<u8>)>) -> bool {
        let baseline = dir.join("baseline.db");
        match prepare_backup(db, &dir.join("work"), m, Some(&baseline), &opts(false)).unwrap() {
            Plan::NoChanges => false,
            Plan::Upload(p) => {
                store.push((p.entry.clone(), fs::read(&p.file).unwrap()));
                m.backups.push(p.entry.clone());
                commit(*p, &baseline).unwrap();
                true
            }
        }
    }

    #[test]
    fn full_then_deltas_restore_latest_state() {
        let dir = tmp();
        let db = dir.join("app.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE t(id INTEGER PRIMARY KEY, v TEXT);").unwrap();
        for i in 0..500 {
            conn.execute("INSERT INTO t(v) VALUES (?1)", [format!("row {i} {}", "x".repeat(50))]).unwrap();
        }
        let mut m = Manifest::new(4096);
        let mut store = Vec::new();

        assert!(backup(&db, &dir, &mut m, &mut store));
        assert_eq!(m.backups[0].backup_type, BackupType::Full);

        // No writes: nothing to upload.
        assert!(!backup(&db, &dir, &mut m, &mut store));

        conn.execute("UPDATE t SET v = 'changed' WHERE id = 7", []).unwrap();
        assert!(backup(&db, &dir, &mut m, &mut store));
        let d = &m.backups[1];
        assert_eq!(d.backup_type, BackupType::Delta);
        assert!(d.pages_changed.unwrap() < 5, "only a few pages change");

        conn.execute("DELETE FROM t WHERE id > 100", []).unwrap();
        conn.execute_batch("VACUUM").unwrap(); // database shrinks
        assert!(backup(&db, &dir, &mut m, &mut store));

        let out = dir.join("restored.db");
        restore_chain(m.restore_chain().map(|c| &store[store.len() - c.len()..]).unwrap(), &out).unwrap();
        assert_eq!(rows(&out), rows(&db));
        assert_eq!(fs::metadata(&out).unwrap().len(), fs::metadata(&db).unwrap().len());

        // Chain limit reached → next change starts a new full backup.
        conn.execute("UPDATE t SET v = 'again' WHERE id = 1", []).unwrap();
        assert!(backup(&db, &dir, &mut m, &mut store));
        conn.execute("UPDATE t SET v = 'more' WHERE id = 2", []).unwrap();
        assert!(backup(&db, &dir, &mut m, &mut store));
        assert_eq!(m.latest().unwrap().backup_type, BackupType::Full);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tampered_file_is_rejected() {
        let dir = tmp();
        let db = dir.join("app.db");
        Connection::open(&db).unwrap().execute_batch("CREATE TABLE t(id INTEGER PRIMARY KEY, v TEXT);").unwrap();
        let mut m = Manifest::new(4096);
        let mut store = Vec::new();
        backup(&db, &dir, &mut m, &mut store);
        store[0].1[10] ^= 0xff;
        assert!(restore_chain(&store, &dir.join("out.db")).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_keeps_newest_chains() {
        let e = |id, t| BackupEntry {
            id,
            backup_type: t,
            timestamp: chrono::Utc::now(),
            file_name: String::new(),
            file_size_bytes: 1,
            blake3_checksum: String::new(),
            pages_changed: None,
            parent_id: None,
            page_count: 1,
            device_id: String::new(),
            device_os: String::new(),
            drive_file_id: None,
        };
        use BackupType::*;
        let mut m = Manifest::new(4096);
        m.backups = vec![e(1, Full), e(2, Delta), e(3, Full), e(4, Delta), e(5, Full), e(6, Delta)];
        let dropped = m.prune(2);
        assert_eq!(dropped.iter().map(|e| e.id).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(m.restore_chain().unwrap().iter().map(|e| e.id).collect::<Vec<_>>(), vec![5, 6]);
    }
}
