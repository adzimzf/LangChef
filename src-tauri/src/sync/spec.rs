//! Which tables are merged between devices, how their rows are identified across devices and how
//! each column merges. See `mod.rs` for the overall design.

/// What a reference column points at. Integer ids are local to a device, so references to rows
/// with integer ids travel as that row's global key and are mapped back on the other device.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Target {
    /// `messages.id` → `messages.gid`
    Messages,
    /// `reading_sessions.id` → `reading_sessions.gid`
    Reading,
    /// `breakdowns.id` → `[language, explain_lang, sentence]`
    Breakdowns,
    /// `translations.ref_id`: a message or a reading text, depending on `kind`
    ByKind,
    /// `review_items.id` (text, but the same word added on two devices gets two ids: mapped
    /// through `sync_alias` on the receiving side)
    Item,
}

pub struct Table {
    pub name: &'static str,
    /// Columns that identify the row on every device (after reference mapping).
    pub key: &'static [&'static str],
    /// The table has an integer primary key and a `gid` column (the key) added for sync.
    pub gid: bool,
    /// Immutable columns hashed into the `gid` of rows that existed before sync (see `schema.rs`).
    pub legacy_fields: &'static [&'static str],
    pub refs: &'static [(&'static str, Target)],
    /// Columns that add up across devices (each device sends how much it added).
    pub counters: &'static [&'static str],
    /// Device-local columns that never leave the device.
    pub skip: &'static [&'static str],
    /// Rows not synced: SQL over `{R}` (`NEW`, `OLD` or a table alias) that is true for rows to sync.
    pub filter: Option<&'static str>,
}

const NONE: &[&str] = &[];

pub const TABLES: &[Table] = &[
    Table { name: "chat_sessions", key: &["id"], gid: false, legacy_fields: NONE, refs: &[], counters: NONE, skip: NONE, filter: None },
    Table {
        name: "messages",
        key: &["gid"],
        gid: true,
        legacy_fields: &["id", "session_id", "role", "created_at", "content"],
        refs: &[("parent_message_id", Target::Messages)],
        counters: NONE,
        skip: &["id"],
        filter: None,
    },
    Table { name: "grammar_items", key: &["id"], gid: false, legacy_fields: NONE, refs: &[], counters: NONE, skip: NONE, filter: None },
    Table { name: "review_items", key: &["id"], gid: false, legacy_fields: NONE, refs: &[], counters: NONE, skip: NONE, filter: None },
    Table { name: "known_words", key: &["word"], gid: false, legacy_fields: NONE, refs: &[], counters: NONE, skip: NONE, filter: None },
    Table { name: "user_activity", key: &["date", "activity_type"], gid: false, legacy_fields: NONE, refs: &[], counters: &["count"], skip: NONE, filter: None },
    Table { name: "daily_active_time", key: &["date"], gid: false, legacy_fields: NONE, refs: &[], counters: &["total_seconds"], skip: NONE, filter: None },
    Table {
        name: "study_limits",
        key: &["date", "language"],
        gid: false,
        legacy_fields: NONE,
        refs: &[],
        counters: &["reviews_completed", "new_cards_seen"],
        skip: NONE,
        filter: None,
    },
    Table {
        name: "message_grammar_analysis",
        key: &["message_id"],
        gid: false,
        legacy_fields: NONE,
        refs: &[("message_id", Target::Messages)],
        counters: NONE,
        skip: NONE,
        filter: None,
    },
    Table {
        name: "user_message_feedback",
        key: &["message_id"],
        gid: false,
        legacy_fields: NONE,
        refs: &[("message_id", Target::Messages)],
        counters: NONE,
        skip: NONE,
        filter: None,
    },
    Table {
        name: "review_answers",
        key: &["gid"],
        gid: true,
        legacy_fields: &["id", "review_item_id", "answered_at", "task"],
        refs: &[("review_item_id", Target::Item), ("chosen_item_id", Target::Item)],
        counters: NONE,
        skip: &["id"],
        filter: None,
    },
    Table {
        name: "review_confusions",
        key: &["review_item_id", "confused_with_id"],
        gid: false,
        legacy_fields: NONE,
        refs: &[("review_item_id", Target::Item), ("confused_with_id", Target::Item)],
        counters: &["count"],
        skip: NONE,
        filter: None,
    },
    // The explanation language follows each device's own UI setting (localStorage).
    Table { name: "app_settings", key: &["key"], gid: false, legacy_fields: NONE, refs: &[], counters: NONE, skip: NONE, filter: Some("{R}.key <> 'explain_lang'") },
    Table {
        name: "translations",
        key: &["kind", "ref_id", "lang"],
        gid: false,
        legacy_fields: NONE,
        refs: &[("ref_id", Target::ByKind)],
        counters: NONE,
        skip: NONE,
        filter: None,
    },
    Table {
        name: "reading_sessions",
        key: &["gid"],
        gid: true,
        legacy_fields: &["id", "created_at", "title", "language", "level"],
        refs: &[],
        counters: NONE,
        skip: &["id"],
        filter: None,
    },
    Table { name: "breakdowns", key: &["language", "explain_lang", "sentence"], gid: false, legacy_fields: NONE, refs: &[], counters: NONE, skip: &["id"], filter: None },
    Table {
        name: "breakdown_parts",
        key: &["breakdown_id", "section"],
        gid: false,
        legacy_fields: NONE,
        refs: &[("breakdown_id", Target::Breakdowns)],
        counters: NONE,
        skip: NONE,
        filter: None,
    },
    Table {
        name: "jlpt_sessions",
        key: &["gid"],
        gid: true,
        legacy_fields: &["id", "created_at", "level", "section"],
        refs: &[],
        counters: NONE,
        skip: &["id"],
        filter: None,
    },
];

pub fn table(name: &str) -> Option<&'static Table> {
    TABLES.iter().find(|t| t.name == name)
}

impl Table {
    pub fn ref_target(&self, col: &str) -> Option<Target> {
        self.refs.iter().find(|(c, _)| *c == col).map(|(_, t)| *t)
    }

    pub fn is_counter(&self, col: &str) -> bool {
        self.counters.contains(&col)
    }
}

/// SQL giving the cross-device value of `{R}.col` (references mapped to global keys).
pub fn export_expr(table: &Table, col: &str, r: &str) -> String {
    let plain = format!("{r}.\"{col}\"");
    match table.ref_target(col) {
        None | Some(Target::Item) => plain,
        Some(Target::Messages) => format!("(SELECT gid FROM messages WHERE id = {plain})"),
        Some(Target::Reading) => format!("(SELECT gid FROM reading_sessions WHERE id = {plain})"),
        Some(Target::Breakdowns) => {
            format!("(SELECT json_array(language, explain_lang, sentence) FROM breakdowns WHERE id = {plain})")
        }
        Some(Target::ByKind) => format!(
            "CASE {r}.kind WHEN 'message' THEN (SELECT gid FROM messages WHERE id = {plain}) \
             WHEN 'reading' THEN (SELECT gid FROM reading_sessions WHERE id = {plain}) ELSE {plain} END"
        ),
    }
}
