# AGENT.md

Agent-specific guidance for autonomous work in this repo. Supplements CLAUDE.md.

## Verification

No test suite. Use these to verify changes before finishing:

```bash
npx tsc --noEmit                    # Frontend type check
npm run i18n:check                  # Dictionaries in sync, no hardcoded UI text
cd src-tauri && GOOGLE_CLIENT_ID=test GOOGLE_CLIENT_SECRET=test cargo check    # Rust compile check (no binary)
cd src-tauri && GOOGLE_CLIENT_ID=test GOOGLE_CLIENT_SECRET=test cargo clippy   # Rust lint
```

`GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET` must be set at compile time (they use `env!()` in `google_auth/mod.rs`). Use `test` as a placeholder for local verification — real values are only needed for Google Auth functionality.

Rust builds require the `lindera/` git submodule — if `cargo check` fails with a path error, run `git submodule update --init`. Lindera releases after `v4.0.1` change the `lindera::tokenizer` API used by `furigana.rs`, so bumping the submodule also means updating that file.

## Adding a Tauri Command

Three steps — all must be done together:

1. **Rust** (`src-tauri/src/lib.rs`): implement `#[tauri::command] fn your_fn(...)` and add it to `.invoke_handler(tauri::generate_handler![..., your_fn])`
2. **Frontend**: call via `invoke('your_fn', { ...args })`
3. **Type**: add corresponding TypeScript type in `src/lib/types.ts` if a new struct is returned

## User-Visible Text (i18n)

The app ships in English and Bahasa Indonesia. Never hardcode UI text:

- **Frontend:** use `t('area.key', { params })` from `src/lib/i18n.ts` and add the key to **both** `src/locales/en.json` and `id.json` (plain, friendly Indonesian; "Anda"). Format dates/numbers/relative times with the helpers in `i18n.ts` (`formatDate`, `formatInMinutes`, …), never hand-built English.
- **Backend:** don't return English sentences. Return codes the frontend translates: errors via `locale::app_error(code, detail)` (add `error.<code>` keys), grade reasons as `GradeReason { code, ms }`.
- **LLM prompts:** prompts stay in English, but anything the learner reads must use `chat::complete_explained(...)` (appends `explain_instruction`); the reply to a chat stays in the target language. Use `locale::lang_name` instead of a new `"ja" => "Japanese"` match.
- **Card meanings:** every write of `review_items.detail` also sets `meaning_lang` to `explain_lang(conn)`; read `detail` through `review_cols()` / `effective_detail_sql()`.

## DB Schema Changes

All schema changes go in `open_db()` in `src-tauri/src/lib.rs`. Use `ALTER TABLE ... ADD COLUMN IF NOT EXISTS` for additive migrations — the function runs on every app start and must be idempotent.

## SRS Changes

FSRS 5.2.0 in `src-tauri/src/srs.rs` is the engine; SM-2 columns (EF, I, R) are legacy display-only. Desired retention constant: 90%. Review grades come from `src-tauri/src/quiz.rs` (`compute_grade`), not from the user; its rules are covered by `cargo test --lib quiz`.

## Grammar Files

Adding a grammar level: create `grammar/<lang>/<LEVEL>.md` and matching `.csv`, then add an `include_str!()` entry in `lib.rs` and register it in `seed_bundled_grammar_if_empty()`. Requires a Rust rebuild.

## Code Reuse — Mandatory

Before writing any new function or component, search for an existing one that covers the same concern.

- **Reuse first**: if an existing function or component almost fits, extend it to support the new case — don't duplicate it.
- **Refactor to generalize**: if the existing code is too rigid, make it accept a param or callback rather than forking it. Prefer one flexible function over two nearly-identical ones.
- **New code only as a last resort**: create something new only when the existing code is fundamentally incompatible (different domain, incompatible contract).

Apply this to both Rust functions in `lib.rs` and React components in `src/`.

## Frontend vs Backend Responsibility

Frontend (React/TypeScript) is for **rendering only**:
- Transform Tauri response data into display values (formatting, labels, colors).
- Local UI state (loading flags, selected tab, open modal).
- Calling `invoke()` and passing results to components.

Backend (Rust) owns all **logic**:
- Filtering, sorting, aggregating, scoring, pagination.
- Any computation that touches the DB or SRS state.
- Business rules (difficulty thresholds, retention targets, JLPT constraints).

If you find yourself writing a `filter()`, `reduce()`, or conditional branch on data in a React component, stop and move it to a Rust Tauri command instead.

## What to Avoid

- Don't add global state, React context, or a state management library — architecture is intentionally local state + Tauri IPC.
- Don't mock Tauri `invoke` calls — there's no test harness for it.
- Don't modify the `telemetry_days` table — it's legacy; write to `user_activity` instead.
