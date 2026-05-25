# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

@AGENT.md

## Project Overview

LangChef — conversational language learning app (Japanese + Indonesian). LLM-driven chat with FSRS spaced repetition. Built with Tauri v2 (Rust backend + React frontend), targeting desktop and Android.

## Commands

```bash
npm run dev              # Vite dev server on :5173
npm run build            # Production build
npm run tauri:dev        # Tauri dev (Rust + frontend)
npm run tauri:build      # Tauri production build
npm run tauri:android:dev  # Android dev via Tauri
npm run tauri:android:build # Android release build
npx tsc --noEmit         # Type check only
```

No test framework configured.

## Architecture

**Stack:** React 18 + TypeScript + Vite + Tailwind + Ant Design on frontend; Rust + Tauri v2 + SQLite (rusqlite) on backend.

**Frontend-Backend IPC:** All data flows through `invoke()` from `@tauri-apps/api/core`. Rust commands registered in `src-tauri/src/lib.rs`. Every persistent read/write is a Tauri command call — no global state, no context providers, no state management library.

**State:** Component-local `useState` + `localStorage` for API credentials and preferences. Each page independently loads data from SQLite via Tauri commands on mount.

**SRS duplication:** FSRS 5.2.0 (`fsrs` crate) in Rust (`src-tauri/src/lib.rs`, `schedule_card` fn) for persistence. TypeScript (`src/lib/srs.ts`) mirrors scheduling logic for immediate UI updates. Old SM-2 columns (EF, I, R) kept in DB for backward compat; SM-2 helpers (`compute_ef`, `format_next_review_minutes`) still used for display. Must keep TS/Rust in sync when modifying. Desired retention: 90%.

**Lindera dependency:** Japanese tokenization uses Lindera with embedded IPADIC dictionary, referenced as a local path dep (`../../lindera/lindera`). The `lindera/` directory must exist as a sibling to the repo root for Rust builds to succeed.

## Rust Modules (`src-tauri/src/`)

- `lib.rs` — all Tauri commands (~3500 lines), DB schema/migrations, SRS logic, grammar seeding
- `furigana.rs` — Lindera-based Japanese tokenization; `annotate()` produces `{kanji|reading}` format, `tokenize()` returns (surface, base_form) pairs
- `google_drive.rs` — Google Drive backup/restore, Drive file ops
- `google_auth/` — OAuth2 token management; desktop and mobile variants

## Key Data Flows

**Chat:** User message → `send()` builds system prompt (injects difficult words, known words, JLPT constraints, grammar patterns) → `invoke('llm_chat')` proxies OpenAI-compatible request through Rust → response rendered with furigana/grammar/word-tap highlighting → tapping word calls `invoke('db_upsert_review_item')`.

**Review:** `invoke('db_get_review_items')` → local SRS scheduling in TS → `invoke('db_update_srs')` persists grade (Rust also computes FSRS independently).

**Message branching:** Messages support `parent_message_id` and `is_active_path` for branching conversations. `db_deactivate_message_branch` + `db_get_message_siblings` manage tree traversal.

**Grammar seeding:** `grammar/` markdown files embedded via `include_str!()` at Rust compile time, auto-seeded into `grammar_items` table on first DB open via `seed_bundled_grammar_if_empty()`.

**Active time tracking:** `useActiveTimeHeartbeat` hook pings `db_record_active_time` every 30s during active use.

## Text Processing Pipeline (Japanese)

1. LLM returns text with `{kanji|reading}` furigana + `[G:pattern]...[/G]` grammar markers
2. `stripFurigana()` → plain text for segmentation
3. `splitTappable()` → `Intl.Segmenter` for CJK word boundaries
4. `parseGrammarMarkers()` → grammar-highlighted segments
5. Words checked against "difficult" set, color-coded
6. `renderFurigana()` → `<ruby>` HTML elements

## Routes

All wrapped in `Layout` (bottom tab nav). Chat page hides bottom nav.

| Path | Page | Purpose |
|------|------|---------|
| `/` | Home | Dashboard, stats, quick actions |
| `/chats` | Chats | Session list, language/JLPT picker |
| `/chat` | Chat | Active conversation (query: `sid`, `lang`, `jlpt`) |
| `/review` | Review | FSRS flashcard review |
| `/words` | WordsList | Browsable word deck |
| `/grammar` | Grammar | Grammar pattern CRUD + CSV import |
| `/stats` | UserStats | Stats dashboard (charts: activity, mastery, proficiency) |
| `/settings` | Settings | API provider config, review limits |

`/telemetry` redirects to `/stats`.

## Database Schema (SQLite)

DB path: `~/.app-data/langchef/langchef.db`. Schema created in `open_db()` in `src-tauri/src/lib.rs`.

| Table | Key columns |
|-------|------------|
| `messages` | id, role, content, session_id, parent_message_id, is_active_path |
| `chat_sessions` | id, title, language, level, selected_grammar |
| `review_items` | id, content, EF, I, R, state, step, lapse_count, next_review, tags, language, detail, mnemonic, stability, difficulty |
| `known_words` | word (PK) |
| `grammar_items` | id, pattern, detail, language, level, is_default |
| `user_activity` | date, activity_type, count |
| `daily_active_time` | date, total_seconds |
| `message_grammar_analysis` | message_id, analysis |
| `user_message_feedback` | message_id, score, feedback |
| `study_limits` | date, language, reviews_completed, new_cards_seen |
| `telemetry_days` | legacy, migrated to user_activity |

## Important Notes

- API config stored in `localStorage` keys: `api_host`, `api_key`, `api_model`
- Grammar data in `grammar/` dir — `ja/` has N5–N1, `id/` has A1–A2. MD for display, CSV for import.
- `src/lib/intl-segmenter.d.ts` provides TS declarations for `Intl.Segmenter`
- PRD is in `PRD.md`; feature specs in `PRDs/`, technical specs in `TRDs/`
- `.keystore` files in `.gitignore` — signing key at project root for Android
- Google OAuth tokens stored at `~/.app-data/langchef/google_tokens.json`
