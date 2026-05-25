# AGENT.md

Agent-specific guidance for autonomous work in this repo. Supplements CLAUDE.md.

## Verification

No test suite. Use these to verify changes before finishing:

```bash
npx tsc --noEmit                    # Frontend type check
cd src-tauri && cargo check         # Rust compile check (no binary)
cd src-tauri && cargo clippy        # Rust lint
```

Rust builds require `lindera/` as a sibling directory to the repo root — if `cargo check` fails with a path error, the Lindera dep is missing.

## Adding a Tauri Command

Three steps — all must be done together:

1. **Rust** (`src-tauri/src/lib.rs`): implement `#[tauri::command] fn your_fn(...)` and add it to `.invoke_handler(tauri::generate_handler![..., your_fn])`
2. **Frontend**: call via `invoke('your_fn', { ...args })`
3. **Type**: add corresponding TypeScript type in `src/lib/types.ts` if a new struct is returned

## DB Schema Changes

All schema changes go in `open_db()` in `src-tauri/src/lib.rs`. Use `ALTER TABLE ... ADD COLUMN IF NOT EXISTS` for additive migrations — the function runs on every app start and must be idempotent.

## SRS Changes

`src/lib/srs.ts` and the `schedule_card` fn in `src-tauri/src/lib.rs` must stay in sync. FSRS 5.2.0 is the engine; SM-2 columns (EF, I, R) are legacy display-only. Desired retention constant: 90%.

## Grammar Files

Adding a grammar level: create `grammar/<lang>/<LEVEL>.md` and matching `.csv`, then add an `include_str!()` entry in `lib.rs` and register it in `seed_bundled_grammar_if_empty()`. Requires a Rust rebuild.

## What to Avoid

- Don't add global state, React context, or a state management library — architecture is intentionally local state + Tauri IPC.
- Don't mock Tauri `invoke` calls — there's no test harness for it.
- Don't modify the `telemetry_days` table — it's legacy; write to `user_activity` instead.
