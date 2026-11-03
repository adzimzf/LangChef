//! Row-level sync between the learner's devices.
//!
//! - Every local write to a synced table is recorded by triggers in `sync_outbox` (`schema.rs`).
//! - `export` turns those into records: set columns / delete row / add to counter, each with a
//!   version from a hybrid clock (wall-clock ms, always above anything the device has seen).
//! - Each device uploads its records as its own change files; no device ever writes another
//!   device's file, so there is nothing to race on. Devices apply each other's records with
//!   `apply` (`google_drive.rs` moves the files).
//! - Merge rules (`spec.rs`): last writer wins per column; a delete loses to a later write;
//!   counters (activity, study time, daily limits, confusions) add up; the same word added on two
//!   devices becomes one card (`sync_alias`).
//! - Rows with integer ids (messages, review answers, reading and JLPT sessions) get a `gid`;
//!   references to them (a reply's parent, a message's analysis, a translation, breakdown parts)
//!   travel as that global key and are mapped back to local ids on arrival. Rows from before sync
//!   get a gid hashed from fields that never change, so devices sharing history agree on it.
//! - Bundled grammar gets the same id on every device (`bundled_grammar_id`).
//! - Not synced: today's study queue (`study`, `study_card`, rebuilt per device), the explanation
//!   language (follows each device's UI), legacy `telemetry_days`.

pub mod engine;
pub mod schema;
pub mod spec;

pub use engine::{adopt, apply, cursors, decode, device, encode, export, has_user_data, initialize, merge_snapshot, Record};
pub use schema::{bundled_grammar_id, prepare};

#[cfg(test)]
mod tests {
    use super::engine::*;
    use rusqlite::{params, Connection};

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::prepare_db(&c).unwrap();
        c
    }

    struct Device {
        name: &'static str,
        conn: Connection,
        seq: u64,
    }

    impl Device {
        fn new(name: &'static str) -> Self {
            let mut d = Device { name, conn: db(), seq: 1 };
            d.init();
            d
        }
        fn from(name: &'static str, conn: Connection) -> Self {
            Device { name, conn, seq: 1 }
        }
        fn init(&mut self) -> Vec<Record> {
            initialize(&mut self.conn, self.name, &mut self.seq).unwrap()
        }
        fn export(&mut self) -> Vec<Record> {
            export(&mut self.conn, self.name, &mut self.seq).unwrap()
        }
        fn exec(&self, sql: &str) {
            self.conn.execute_batch(sql).unwrap();
        }
        fn count(&self, sql: &str) -> i64 {
            self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
        }
        fn text(&self, sql: &str) -> Option<String> {
            self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
        }
    }

    /// Sends everything new on `from` to `to`.
    fn send(from: &mut Device, to: &mut Device) {
        let recs = from.export();
        // Through the file format, like the real transport.
        let (dev, recs) = decode(&encode(from.name, &recs).unwrap()).unwrap();
        apply(&mut to.conn, &dev, &recs).unwrap();
    }

    fn sync_both(a: &mut Device, b: &mut Device) {
        send(a, b);
        send(b, a);
    }

    fn add_card(d: &Device, id: &str, content: &str) {
        d.conn
            .execute(
                "INSERT INTO review_items(id, content, EF, I, R) VALUES (?1, ?2, -1.0, 0, 0)",
                params![id, content],
            )
            .unwrap();
    }

    fn answer(d: &Device, item: &str, at: i64) {
        d.conn
            .execute(
                "INSERT INTO review_answers(review_item_id, task, correct, elapsed_ms, grade, answered_at)
                 VALUES (?1, 'recall', 1, 900, 'good', ?2)",
                params![item, at],
            )
            .unwrap();
    }

    #[test]
    fn both_devices_end_up_with_everything() {
        let (mut a, mut b) = (Device::new("dev-a"), Device::new("dev-b"));
        // First exchange: the bundled grammar is identical on both, nothing else.
        let ia = a.init();
        let ib = b.init();
        apply(&mut b.conn, "dev-a", &ia).unwrap();
        apply(&mut a.conn, "dev-b", &ib).unwrap();
        let grammar = a.count("SELECT COUNT(*) FROM grammar_items");
        assert!(grammar > 0);
        assert_eq!(b.count("SELECT COUNT(*) FROM grammar_items"), grammar, "bundled grammar not duplicated");

        // A: a chat with a reply chain and a cached analysis. B: another chat.
        a.exec(
            "INSERT INTO chat_sessions(id, title, created_at, last_activity) VALUES ('s1', 'A chat', 1, 1);
             INSERT INTO messages(role, content, created_at, session_id) VALUES ('user', 'こんにちは', 1, 's1');
             INSERT INTO messages(role, content, created_at, session_id, parent_message_id)
               VALUES ('assistant', 'はい', 2, 's1', (SELECT id FROM messages WHERE content = 'こんにちは'));
             INSERT INTO message_grammar_analysis(message_id, analysis)
               VALUES ((SELECT id FROM messages WHERE content = 'はい'), 'analysis');",
        );
        b.exec(
            "INSERT INTO chat_sessions(id, title, created_at, last_activity) VALUES ('s2', 'B chat', 1, 1);
             INSERT INTO messages(role, content, created_at, session_id) VALUES ('user', 'halo', 1, 's2');",
        );
        sync_both(&mut a, &mut b);

        for d in [&a, &b] {
            assert_eq!(d.count("SELECT COUNT(*) FROM messages"), 3);
            // Integer ids differ per device; the reply still points at its own parent.
            assert_eq!(
                d.text(
                    "SELECT p.content FROM messages m JOIN messages p ON p.id = m.parent_message_id WHERE m.content = 'はい'"
                )
                .as_deref(),
                Some("こんにちは")
            );
            assert_eq!(
                d.text(
                    "SELECT g.analysis FROM message_grammar_analysis g JOIN messages m ON m.id = g.message_id WHERE m.content = 'はい'"
                )
                .as_deref(),
                Some("analysis")
            );
        }
        // B's own message got id 1, so A's two messages are 2 and 3 there.
        assert_eq!(b.count("SELECT id FROM messages WHERE content = 'halo'"), 1);
    }

    #[test]
    fn same_word_on_two_devices_is_one_card() {
        let (mut a, mut b) = (Device::new("dev-a"), Device::new("dev-b"));
        add_card(&a, "id-a", "犬");
        answer(&a, "id-a", 100);
        add_card(&b, "id-b", "犬");
        answer(&b, "id-b", 200);
        answer(&b, "id-b", 300);
        sync_both(&mut a, &mut b);

        assert_eq!(a.count("SELECT COUNT(*) FROM review_items WHERE content = '犬'"), 1);
        assert_eq!(b.count("SELECT COUNT(*) FROM review_items WHERE content = '犬'"), 1);
        // All three answers, each pointing at the one local card.
        assert_eq!(a.count("SELECT COUNT(*) FROM review_answers WHERE review_item_id = 'id-a'"), 3);
        assert_eq!(b.count("SELECT COUNT(*) FROM review_answers WHERE review_item_id = 'id-b'"), 3);

        // Later edits on either side still reach the other side's copy.
        b.exec("UPDATE review_items SET mnemonic = 'woof' WHERE id = 'id-b'");
        sync_both(&mut a, &mut b);
        assert_eq!(a.text("SELECT mnemonic FROM review_items WHERE id = 'id-a'").as_deref(), Some("woof"));
    }

    #[test]
    fn columns_merge_and_latest_write_wins() {
        let (mut a, mut b) = (Device::new("dev-a"), Device::new("dev-b"));
        add_card(&a, "c1", "猫");
        sync_both(&mut a, &mut b);

        // Different columns at the same time: both kept.
        a.exec("UPDATE review_items SET detail = 'cat' WHERE id = 'c1'");
        b.exec("UPDATE review_items SET mnemonic = 'neko' WHERE id = 'c1'");
        sync_both(&mut a, &mut b);
        for d in [&a, &b] {
            assert_eq!(d.text("SELECT detail FROM review_items WHERE id = 'c1'").as_deref(), Some("cat"));
            assert_eq!(d.text("SELECT mnemonic FROM review_items WHERE id = 'c1'").as_deref(), Some("neko"));
        }

        // Same column: the edit made after seeing the other one wins on both.
        a.exec("UPDATE review_items SET detail = 'cat (A)' WHERE id = 'c1'");
        send(&mut a, &mut b);
        b.exec("UPDATE review_items SET detail = 'cat (B, later)' WHERE id = 'c1'");
        sync_both(&mut a, &mut b);
        for d in [&a, &b] {
            assert_eq!(d.text("SELECT detail FROM review_items WHERE id = 'c1'").as_deref(), Some("cat (B, later)"));
        }
    }

    #[test]
    fn counters_add_up_and_deletes_travel() {
        let (mut a, mut b) = (Device::new("dev-a"), Device::new("dev-b"));
        a.exec("INSERT INTO user_activity(date, activity_type, count) VALUES ('2026-11-03', 'review', 2)");
        b.exec("INSERT INTO user_activity(date, activity_type, count) VALUES ('2026-11-03', 'review', 3)");
        sync_both(&mut a, &mut b);
        a.exec("UPDATE user_activity SET count = count + 1 WHERE date = '2026-11-03'");
        sync_both(&mut a, &mut b);
        for d in [&a, &b] {
            assert_eq!(d.count("SELECT count FROM user_activity WHERE date = '2026-11-03'"), 6);
        }

        a.exec("INSERT INTO known_words(word) VALUES ('水')");
        sync_both(&mut a, &mut b);
        assert_eq!(b.count("SELECT COUNT(*) FROM known_words WHERE word = '水'"), 1);
        b.exec("DELETE FROM known_words WHERE word = '水'");
        sync_both(&mut a, &mut b);
        assert_eq!(a.count("SELECT COUNT(*) FROM known_words WHERE word = '水'"), 0);
    }

    #[test]
    fn shared_history_is_not_duplicated() {
        // One device used before sync existed; the second restored its backup back then.
        let a_conn = db();
        a_conn
            .execute_batch(
                "INSERT INTO chat_sessions(id, title, created_at, last_activity) VALUES ('s1', 'old', 1, 1);
                 INSERT INTO messages(role, content, created_at, session_id) VALUES ('user', 'old message', 5, 's1');
                 INSERT INTO user_activity(date, activity_type, count) VALUES ('2026-01-01', 'chat', 4);",
            )
            .unwrap();
        let mut b_conn = Connection::open_in_memory().unwrap();
        rusqlite::backup::Backup::new(&a_conn, &mut b_conn).unwrap().run_to_completion(100, std::time::Duration::ZERO, None).unwrap();
        crate::db::prepare_db(&b_conn).unwrap();

        let (mut a, mut b) = (Device::from("dev-a", a_conn), Device::from("dev-b", b_conn));
        // Both diverge, then both start syncing.
        a.exec("INSERT INTO messages(role, content, created_at, session_id) VALUES ('user', 'from A', 6, 's1')");
        b.exec("INSERT INTO messages(role, content, created_at, session_id) VALUES ('user', 'from B', 6, 's1')");
        let ia = a.init();
        let ib = b.init();
        apply(&mut b.conn, "dev-a", &ia).unwrap();
        apply(&mut a.conn, "dev-b", &ib).unwrap();
        for d in [&a, &b] {
            assert_eq!(d.count("SELECT COUNT(*) FROM messages WHERE content = 'old message'"), 1);
            assert_eq!(d.count("SELECT COUNT(*) FROM messages"), 3);
            // The shared count of 4 is counted once.
            assert_eq!(d.count("SELECT count FROM user_activity WHERE date = '2026-01-01'"), 4);
        }
    }

    #[test]
    fn joining_device_merges_a_snapshot() {
        let (mut a, mut b) = (Device::new("dev-a"), Device::new("dev-b"));
        a.exec("INSERT INTO chat_sessions(id, title, created_at, last_activity) VALUES ('s1', 'A', 1, 1)");
        add_card(&a, "a1", "山");
        sync_both(&mut a, &mut b);
        // C was used offline, never synced, and joins from A's backup.
        let mut c = Device::from("dev-c", db());
        add_card(&c, "c1", "川");
        add_card(&c, "c2", "山");
        let init = c.init();
        let stats = merge_snapshot(&mut c.conn, &a.conn, "dev-c").unwrap();
        assert!(stats.applied > 0);
        assert_eq!(c.count("SELECT COUNT(*) FROM chat_sessions WHERE id = 's1'"), 1);
        assert_eq!(c.count("SELECT COUNT(*) FROM review_items WHERE content IN ('山', '川')"), 2);
        // C's own data reaches A and B; 山 stays one card.
        apply(&mut a.conn, "dev-c", &init).unwrap();
        assert_eq!(a.count("SELECT COUNT(*) FROM review_items WHERE content IN ('山', '川')"), 2);
        // Records A sent before the snapshot are not applied twice.
        assert_eq!(cursors(&c.conn).unwrap().get("dev-b").copied(), cursors(&a.conn).unwrap().get("dev-b").copied());
    }

    #[test]
    fn bundled_grammar_gets_stable_ids() {
        let c = db();
        let (id, pattern): (String, String) = c
            .query_row("SELECT id, pattern FROM grammar_items WHERE is_default = 1 ORDER BY rowid LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert!(id.starts_with("b-"));
        // An install from before stable ids: a random id, used by a card, a translation and a chat.
        c.execute_batch(&format!(
            "UPDATE grammar_items SET id = 'old-uuid' WHERE id = '{id}';
             INSERT INTO grammar_items(id, pattern, language, level, explain_lang, source_id) VALUES ('tr', 'x', 'ja', 'N5', 'id', 'old-uuid');
             INSERT INTO review_items(id, content, EF, I, R, item_type, grammar_id) VALUES ('g1', '{pattern}', -1.0, 0, 0, 'grammar', 'old-uuid');
             INSERT INTO chat_sessions(id, created_at, last_activity, selected_grammar) VALUES ('s', 1, 1, '[\"old-uuid\"]');
             DELETE FROM sync_meta WHERE k = 'grammar_ids';"
        ))
        .unwrap();
        super::prepare(&c).unwrap();
        let get = |sql: &str| c.query_row(sql, [], |r| r.get::<_, String>(0)).unwrap();
        assert_eq!(get("SELECT grammar_id FROM review_items WHERE id = 'g1'"), id);
        assert_eq!(get("SELECT source_id FROM grammar_items WHERE id = 'tr'"), id);
        assert_eq!(get("SELECT selected_grammar FROM chat_sessions WHERE id = 's'"), format!("[\"{id}\"]"));
    }

    #[test]
    fn nothing_recorded_before_sync_and_while_applying() {
        let mut d = Device::from("dev-a", db());
        d.exec("INSERT INTO known_words(word) VALUES ('x')");
        assert_eq!(d.count("SELECT COUNT(*) FROM sync_outbox"), 0);
        d.init();
        d.exec("INSERT INTO known_words(word) VALUES ('y')");
        assert_eq!(d.count("SELECT COUNT(*) FROM sync_outbox"), 1);
        assert_eq!(d.export().len(), 1);
        assert_eq!(d.count("SELECT COUNT(*) FROM sync_outbox"), 0);
        // explain_lang stays per device.
        d.exec("INSERT OR REPLACE INTO app_settings(key, value) VALUES ('explain_lang', 'id')");
        assert_eq!(d.count("SELECT COUNT(*) FROM sync_outbox"), 0);
    }
}
