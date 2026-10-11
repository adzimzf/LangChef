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

**Lindera dependency:** Japanese tokenization uses Lindera with embedded IPADIC dictionary, vendored as a git submodule at `lindera/` (pinned to upstream tag `v4.0.1`) and referenced as a path dep (`../lindera/lindera`). Run `git submodule update --init` after cloning (or clone with `--recurse-submodules`) for Rust builds to succeed.

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

<!-- rtk-instructions v2 -->
# RTK (Rust Token Killer) - Token-Optimized Commands

## Golden Rule

**Always prefix commands with `rtk`**. If RTK has a dedicated filter, it uses it. If not, it passes through unchanged. This means RTK is always safe to use.

**Important**: Even in command chains with `&&`, use `rtk`:
```bash
# ❌ Wrong
git add . && git commit -m "msg" && git push

# ✅ Correct
rtk git add . && rtk git commit -m "msg" && rtk git push
```

## RTK Commands by Workflow

### Build & Compile (80-90% savings)
```bash
rtk cargo build         # Cargo build output
rtk cargo check         # Cargo check output
rtk cargo clippy        # Clippy warnings grouped by file (80%)
rtk tsc                 # TypeScript errors grouped by file/code (83%)
rtk lint                # ESLint/Biome violations grouped (84%)
rtk prettier --check    # Files needing format only (70%)
rtk next build          # Next.js build with route metrics (87%)
```

### Test (60-99% savings)
```bash
rtk cargo test          # Cargo test failures only (90%)
rtk go test             # Go test failures only (90%)
rtk jest                # Jest failures only (99.5%)
rtk vitest              # Vitest failures only (99.5%)
rtk playwright test     # Playwright failures only (94%)
rtk pytest              # Python test failures only (90%)
rtk rake test           # Ruby test failures only (90%)
rtk rspec               # RSpec test failures only (60%)
rtk test <cmd>          # Generic test wrapper - failures only
```

### Git (59-80% savings)
```bash
rtk git status          # Compact status
rtk git log             # Compact log (works with all git flags)
rtk git diff            # Compact diff (80%)
rtk git show            # Compact show (80%)
rtk git add             # Ultra-compact confirmations (59%)
rtk git commit          # Ultra-compact confirmations (59%)
rtk git push            # Ultra-compact confirmations
rtk git pull            # Ultra-compact confirmations
rtk git branch          # Compact branch list
rtk git fetch           # Compact fetch
rtk git stash           # Compact stash
rtk git worktree        # Compact worktree
```

Note: Git passthrough works for ALL subcommands, even those not explicitly listed.

### GitHub (26-87% savings)
```bash
rtk gh pr view <num>    # Compact PR view (87%)
rtk gh pr checks        # Compact PR checks (79%)
rtk gh run list         # Compact workflow runs (82%)
rtk gh issue list       # Compact issue list (80%)
rtk gh api              # Compact API responses (26%)
```

### JavaScript/TypeScript Tooling (70-90% savings)
```bash
rtk pnpm list           # Compact dependency tree (70%)
rtk pnpm outdated       # Compact outdated packages (80%)
rtk pnpm install        # Compact install output (90%)
rtk npm run <script>    # Compact npm script output
rtk npx <cmd>           # Compact npx command output
rtk prisma              # Prisma without ASCII art (88%)
```

### Files & Search (60-75% savings)
```bash
rtk ls <path>           # Tree format, compact (65%)
rtk read <file>         # Code reading with filtering (60%)
rtk grep <pattern>      # Search grouped by file (75%)
rtk find <pattern>      # Find grouped by directory (70%)
```

### Analysis & Debug (70-90% savings)
```bash
rtk err <cmd>           # Filter errors only from any command
rtk log <file>          # Deduplicated logs with counts
rtk json <file>         # JSON structure without values
rtk deps                # Dependency overview
rtk env                 # Environment variables compact
rtk summary <cmd>       # Smart summary of command output
rtk diff                # Ultra-compact diffs
```

### Infrastructure (85% savings)
```bash
rtk docker ps           # Compact container list
rtk docker images       # Compact image list
rtk docker logs <c>     # Deduplicated logs
rtk kubectl get         # Compact resource list
rtk kubectl logs        # Deduplicated pod logs
```

### Network (65-70% savings)
```bash
rtk curl <url>          # Compact HTTP responses (70%)
rtk wget <url>          # Compact download output (65%)
```

### Meta Commands
```bash
rtk gain                # View token savings statistics
rtk gain --history      # View command history with savings
rtk discover            # Analyze Claude Code sessions for missed RTK usage
rtk proxy <cmd>         # Run command without filtering (for debugging)
rtk init                # Add RTK instructions to CLAUDE.md
rtk init --global       # Add RTK to ~/.claude/CLAUDE.md
```

## Token Savings Overview

| Category | Commands | Typical Savings |
|----------|----------|-----------------|
| Tests | vitest, playwright, cargo test | 90-99% |
| Build | next, tsc, lint, prettier | 70-87% |
| Git | status, log, diff, add, commit | 59-80% |
| GitHub | gh pr, gh run, gh issue | 26-87% |
| Package Managers | pnpm, npm, npx | 70-90% |
| Files | ls, read, grep, find | 60-75% |
| Infrastructure | docker, kubectl | 85% |
| Network | curl, wget | 65-70% |

Overall average: **60-90% token reduction** on common development operations.
<!-- /rtk-instructions -->