# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

LangChef — conversational language learning app (Japanese + Indonesian). LLM-driven chat with SM-2 spaced repetition. Built with Tauri v2 (Rust backend + React frontend), targeting desktop and Android.

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

**SRS duplication:** SM-2 algorithm implemented twice — TypeScript (`src/lib/srs.ts`) for immediate UI updates, Rust (`src-tauri/src/lib.rs`, `schedule_card` fn) for persistence. Must keep in sync when modifying.

## Key Data Flows

**Chat:** User message → `send()` builds system prompt (injects difficult words, known words, JLPT constraints, grammar patterns) → `invoke('llm_chat')` proxies OpenAI-compatible request through Rust → response rendered with furigana/grammar/word-tap highlighting → tapping word calls `invoke('db_upsert_review_item')`.

**Review:** `invoke('db_get_review_items')` → local SRS scheduling in TS → `invoke('db_update_srs')` persists grade (Rust also computes SM-2 independently).

**Grammar seeding:** `grammar/` markdown files embedded via `include_str!()` at Rust compile time, auto-seeded into `grammar_items` table on first DB open.

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
| `/review` | Review | SM-2 flashcard review |
| `/words` | WordsList | Browsable word deck |
| `/grammar` | Grammar | Grammar pattern CRUD + CSV import |
| `/telemetry` | Telemetry | Stats dashboard |
| `/settings` | Settings | API provider config, review limits |

## Database Schema (SQLite)

Tables: `messages`, `review_items`, `known_words`, `telemetry_days`, `chat_sessions`, `grammar_items`. Schema created in `open_db()` in `src-tauri/src/lib.rs`. DB path: `~/.app-data/langchef/langchef.db`.

## Important Notes

- API config stored in `localStorage` keys: `api_host`, `api_key`, `api_model`
- Grammar data in `grammar/` dir — MD for display, CSV for import. Auto-seeded by Rust on first run.
- `src/lib/intl-segmenter.d.ts` provides TS declarations for `Intl.Segmenter`
- PRD is in `PRD.md`, not README
- `.keystore` files in `.gitignore` — signing key at project root for Android
