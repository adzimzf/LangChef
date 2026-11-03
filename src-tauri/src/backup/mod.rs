//! Incremental SQLite backup (full + page deltas), ported from
//! https://github.com/adzimzf/sqlite-incremental-backup. `google_drive.rs` stores the files in
//! Google Drive and handles conflicts between devices.

pub mod delta;
pub mod engine;
pub mod manifest;
