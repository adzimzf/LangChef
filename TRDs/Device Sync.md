# Device Sync (row-level merge)

Keeps every device signed in to the same Google account in step: words, reviews, chats, readings,
breakdowns, JLPT sessions, grammar, settings and activity from all of them are combined. Nothing is
overwritten as a whole; each row merges by the rules below. Code: `src-tauri/src/sync/` (merge) and
`src-tauri/src/google_drive.rs` (transport).

## Drive layout

Folder `LangChef Backup` (`drive.file` scope):

| File | Written by | Purpose |
|---|---|---|
| `chg_<device>_<first>_<last>.jsonl.zst` | that device only | its row changes, records `first..last` |
| `manifest_<device>.json` + `full_*.db.zst` / `delta_*.bin` | that device only | its snapshots (incremental backup engine, 2 chains kept) |

Because every file has a single writer, devices never race on a shared file.

## Recording changes

`sync::prepare` (run from `db::prepare_db`) creates triggers on every synced table. Once the
database has synced (`sync_meta.device` set) and while no remote change is being applied
(`sync_meta.applying` unset), each insert / update / delete writes the row (and the old row) as
JSON into `sync_outbox`, with references already mapped to global keys. Updates that change no
synced column are not recorded. Triggers are rebuilt automatically when columns change.

`sync::export` turns the outbox into records:

| Op | Meaning |
|---|---|
| `u` | set these columns (create the row if missing) |
| `d` | delete the row |
| `c` | add to counter columns |
| `b` | counter value from before sync (the largest one seen counts, once) |

Each record has a version from a hybrid clock: wall-clock ms, but always above any version the
device has seen, so an edit made after receiving another device's edit always wins over it. Ties
are broken by device id, the same way on every device. The device's sequence number lives in
`google_sync_config.json` (not the database), so it never goes back after a restore.

## Merge rules

- **Columns:** last writer wins per column (`sync_version` holds each column's version), so two
  devices editing different columns of the same row both keep their edit.
- **Deletes:** a delete wins only over columns written before it; a later write brings the row back.
- **Counters** add up: `user_activity.count`, `daily_active_time.total_seconds`,
  `study_limits.reviews_completed` / `new_cards_seen`, `review_confusions.count`.
- **Same word on two devices:** `review_items.content` is unique. When another device's card
  collides with a local one, its id is mapped to the local card (`sync_alias`) and everything that
  refers to it (answers, confusions, later edits) lands on the local card.
- **Card scheduling** (FSRS state) is a set of columns written together on every review, so the
  most recent review's state wins. Every review answer from every device is kept in
  `review_answers`.
- Records that need a row that hasn't arrived yet (a reply before its parent, part of a row that
  doesn't exist here) wait in `sync_parked` and are retried after each batch.

## Identity of rows

| Table | Key on every device |
|---|---|
| `messages`, `review_answers`, `reading_sessions`, `jlpt_sessions` | new `gid` column |
| `breakdowns` | `[language, explain_lang, sentence]` |
| `review_items`, `chat_sessions`, `grammar_items` | their text `id` (cards collide by content → alias) |
| others | their natural primary key, with references mapped |

References to integer-id rows travel as the target's key and are mapped back on arrival:
`messages.parent_message_id`, `message_grammar_analysis.message_id`,
`user_message_feedback.message_id`, `translations.ref_id` (message or reading by `kind`),
`breakdown_parts.breakdown_id`.

**Existing data with integer ids:** rows written while sync was off get a `gid` hashed from
fields that never change (including the local id). Two devices that share history because one
restored the other's backup give the shared rows the same gid, so they are not duplicated. New
rows get a random gid from the insert trigger.

**Bundled grammar** used to get a random id per device. It now has a stable id
(`bundled_grammar_id`: language, level, explanation language, pattern). Existing rows are renamed
once, along with grammar cards, translated rows and chats' selected patterns.

## Not synced

`study` / `study_card` (each device builds its own daily queue; at session start
`srs::absorb_remote_answers` counts cards answered on another device today as done, so a day
finished on one device is finished on all of them and the daily limits are shared), `app_settings.explain_lang`
(follows each device's UI language), `telemetry_days` (legacy).

## One sync (`google_drive::sync_now`)

1. **Join** if this database hasn't synced as this device:
   - restored from another device → `adopt` (its rows are already known everywhere);
   - fresh install and a snapshot exists → start from the newest snapshot, then `adopt`;
   - has its own data → `initialize` (send all of it, old versions so real edits win) and
     `merge_snapshot` the newest snapshot of another device row by row.
2. **Export** the outbox to `backup/outgoing/`, then **upload** pending files.
3. **Pull:** for each device, download change files past `sync_cursor[device]` and `apply` them.
   If older files were already deleted (gap), merge the newest snapshot first.
4. **Snapshot** this device's database (only changed pages are uploaded).
5. **Prune** this device's own change files older than 60 days once it has a newer snapshot.

**Restore** sends unsynced changes first, rebuilds the newest snapshot of any device, puts it in
place, `adopt`s it and syncs again, which brings back every change made after that snapshot,
including this device's own.

## Known limits

- Counter values from before sync on two devices that were used separately are combined with
  the larger value, not the sum (needed so shared history isn't counted twice).
- Two devices reviewing the same card while offline: the later review's scheduling wins; the
  earlier review is kept in the history but doesn't change the schedule.
- A change written in the moment between a device's export and its snapshot may have its counter
  part counted twice by a device that later joins from that snapshot.
- Changes are exchanged only during a sync, not live. Automatic sync runs only while the app is
  open: right after opening it (or returning to it on Android), then on the chosen interval
  (5 minutes at least). Nothing syncs while the app is closed or in the background.
