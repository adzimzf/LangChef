# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

@AGENT.md

## Project Overview

LangChef — conversational language learning app (Japanese + Indonesian). LLM-driven chat with FSRS spaced repetition. Built with Tauri v2 (Rust backend + React frontend), targeting desktop and Android.

The app runs in English or Bahasa Indonesia ("explanation language", see below), so someone who reads only Indonesian can learn Japanese. Spec: `PRDs/Indonesian Speakers Learning Japanese.md`.

## Commands

```bash
npm run dev              # Vite dev server on :5173
npm run build            # Production build
npm run tauri:dev        # Tauri dev (Rust + frontend)
npm run tauri:build      # Tauri production build
npm run tauri:android:dev  # Android dev via Tauri
npm run tauri:android:build # Android release build
npx tsc --noEmit         # Type check only
npm run i18n:check       # en.json / id.json in sync, no hardcoded UI text
```

No test framework configured.

## Architecture

**Stack:** React 18 + TypeScript + Vite + Tailwind + Ant Design on frontend; Rust + Tauri v2 + SQLite (rusqlite) on backend.

**Frontend-Backend IPC:** All data flows through `invoke()` from `@tauri-apps/api/core`. Rust commands registered in `src-tauri/src/lib.rs`. Every persistent read/write is a Tauri command call — no global state, no context providers, no state management library.

**State:** Component-local `useState` + `localStorage` for API credentials and preferences. Each page independently loads data from SQLite via Tauri commands on mount.

**Explanation language (i18n):** `explain_lang` ∈ `en` | `id` is the language of the UI, of every AI explanation and of card meanings — one app-wide switch. Frontend: `src/lib/i18n.ts` exports `t(key, params)` (plain module, no context); dictionaries are `src/locales/en.json` (source of truth) and `id.json`; `{name}` placeholders, `key_one`/`key_other` plurals; `Intl` for dates/numbers. The value lives in `localStorage.explain_lang`, is mirrored into the `app_settings` table on startup (`db_set_explain_lang`), and changing it reloads the app. Backend: `src-tauri/src/locale.rs` (`explain_lang(conn)`, `explain_instruction()` appended to learner-facing prompts, `app_error(code, detail)`). Rust returns codes, React renders words: errors are `{"code","detail"}` JSON rendered with `errorText()`, grade reasons are `{code, ms}`, part of speech is a code (`pos.*`). Every user-visible string goes through `t()`.

**Meaning language:** `review_items.meaning_lang` records the language of `detail`. A meaning in another language than `explain_lang` counts as empty (`effective_detail_sql` / `review_cols` in SQL), so after switching language the card shows as `needs_meaning` and the user regenerates it. Quiz options only come from cards whose meaning is in the current language.

**SRS:** FSRS 5.2.0 (`fsrs` crate) in Rust (`src-tauri/src/srs.rs`, `schedule_fsrs` fn). Old SM-2 columns (EF, I, R) kept in DB for backward compat; SM-2 helpers (`compute_ef`, `format_next_review_minutes`) still used for display. Desired retention: 90%.

**Auto-graded review:** users don't rate themselves. `src-tauri/src/quiz.rs` gives each due card a question and turns the answer into the FSRS grade:
- Order (`fetch_next_pending` in `srs.rs`): today's queue is served in rounds. Each round first repeats what the previous round left over (`study_card.round`, set by `requeue_card`: new cards just met, meanings just saved, wrong answers), then hands out up to `ROUND_SIZE` (25) fresh cards, a new card (word or grammar) after every `REVIEWS_PER_NEW` (4) reviews. `study.round` / `round_fresh` track the current round.
- Question by card stage: new → intro + true/false; learning/relearning → pick the meaning; review → recall then confirm (sometimes "None of these" is the right answer); mature (review with stability ≥ 21 days) → rotates recall / pick the reading / meaning in context. Tiny decks fall back to true/false.
- Cards without a meaning in `detail` get `needs_meaning`: the user generates the back (`db_generate_detail` with `draft: true` writes `review_items.draft_detail`) or writes it, checks/edits it, and `db_session_save_meaning` saves it and moves the card to the next round without grading; it comes back as a real question (new cards skip the intro). "Skip for today" (`db_session_skip_card`) takes it out of today's queue ungraded. A session-start banner offers `db_generate_missing_meanings` for all of them. The self-graded flip (`classic`) only remains for decks with fewer than 2 words with meanings.
- Grade: wrong or "Don't know" → Again; hint used or slower than 2× the user's median time → Hard; otherwise Good. Easy only for fast answers on mature cards to recall or context questions. A wrong reading on a mature card is Hard (no lapse).
- The issued question is stored in `study_card.task` (answer hidden from the frontend) and checked against on answer. Wrong picks are recorded in `review_confusions` and those words are used as harder wrong options later. A meaning that shares a sense with the answer (`glosses` / `meanings_overlap`: "travel; trip" vs "travel; journey; trip") is never a wrong option, nor are two options that share one. One "I mis-tapped" fix per language per day restores the saved card state and re-grades.

**Grammar review** (`src-tauri/src/grammar_review.rs`, spec `PRDs/Grammar Review.md`): grammar patterns are cards in the same queue (`review_items.item_type = 'grammar'`, `grammar_id`, `level`).
- The learner's level per target language (`app_settings` `grammar_level:<lang>`) is taught as new (default 3/day, `study_card.kind = 'grammar_new'`); lower levels get one level check each (`placement`, default 5/day, `grammar_check`; right → Easy and known, wrong → taught); higher levels only via "Add to review". These slots are separate from new words; due grammar takes up to 3 review slots a day (`GRAMMAR_REVIEWS_PER_DAY`) out of the review limit before words, so word reviews can't crowd it out; more due grammar waits for a later day.
- Questions (`build_grammar_task` in `quiz.rs`): check → fill the gap; new → intro + true/false; learning → `grammar_meaning`; review → `grammar_gap`; mature → gap without translation / meaning / `grammar_mistake`. Options come only from other grammar cards plus the pattern's own `confusables`, never from words.
- The back is AI-made practice material (`GrammarDetail`: meaning, form, note, examples with exact `target` and wrong fillers, a mistake, confusables), made through the same draft → approve flow as word meanings (`generate_card_back`). `parse_grammar_detail` drops anything a question can't safely use.
- `db_get_grammar_plan` / `db_set_grammar_plan` (level, limits, speed up, progress per level, move-up suggestion); `db_list_grammar_items` returns each row's `review` status. Grammar cards are left out of the word list, chat word lists and mnemonics (`item_type = 'word'`).

**Lindera dependency:** Japanese tokenization uses Lindera with embedded IPADIC dictionary, vendored as a git submodule at `lindera/` (pinned to upstream tag `v4.0.1`) and referenced as a path dep (`../lindera/lindera`). Run `git submodule update --init` after cloning (or clone with `--recurse-submodules`) for Rust builds to succeed.

## Rust Modules (`src-tauri/src/`)

- `lib.rs` — module list and `generate_handler!` registration of all Tauri commands
- `db.rs` — `open_db()` / `prepare_db()`: schema, migrations, bundled grammar seeding (per explanation language), pairing bundled translations with their originals (`source_id`)
- `locale.rs` — explanation language setting, prompt language rule, error codes, `effective_detail_sql`
- `chat.rs` — `complete()`: the one LLM request helper (all prompts go through it; maps HTTP failures to error codes), `llm_chat`, `llm_translate` (cached in `translations`), `llm_help_reply`
- `words.rs` — deck commands, `generate_detail_text` (card back in the explanation language), mnemonics
- `grammar.rs` — grammar CRUD, per-language rows with English fallback, AI translation of grammar rows, analysis
- `grammar_review.rs` — grammar cards: the learner's level and daily grammar limits, enrolling patterns into the queue, level checks, practice material, progress per level
- `romaji.rs` — kana↔romaji, `to_romaji`, `romaji_to_kana_input`, kana starter deck
- `srs.rs` — FSRS scheduling, daily study queue (`study`, `study_card`), session commands
- `quiz.rs` — auto-graded review: question building (words and grammar), answer checking, grading, summary
- `breakdown.rs` — sentence breakdown ("Urai" tab): `split_sentences`, one small prompt per section (`SECTIONS`) generated by `llm_breakdown_section`, cached in `breakdown_parts`; check question answered via `db_check_breakdown_answer` (answer never sent to the frontend)
- `furigana.rs` — Lindera-based Japanese tokenization; `annotate()` produces `{kanji|reading}` format, `tokenize()` returns (surface, base_form) pairs
- `google_drive.rs` — Google Drive backup/restore, Drive file ops
- `google_auth/` — OAuth2 token management; desktop and mobile variants

## Key Data Flows

**Chat:** User message → `send()` builds system prompt (injects difficult words, known words, JLPT constraints, grammar patterns, optional 💡 hint line in the explanation language) → `invoke('llm_chat')` proxies OpenAI-compatible request through Rust → response rendered with furigana/grammar/word-tap highlighting, optional romaji, and a cached "Translate" (`llm_translate`) → tapping a word opens `WordSheet` (meaning in the explanation language, "Add to cards" → `db_upsert_review_item`). "Help me reply" (`HelpReplySheet` → `llm_help_reply`) turns what the learner writes in their language into a target-language reply.

**Review:** `invoke('db_session_start')` (which also enrolls today's grammar patterns) → returns `next_card` + `next_task` (the question) → user answers → `invoke('db_session_answer')` checks it in Rust, grades, records the answer and returns the verdict plus the next card. `db_session_override` handles the mis-tap fix, `db_session_summary` feeds the end-of-session screen, `db_session_save_meaning` / `db_session_skip_card` / `db_generate_missing_meanings` handle cards without a meaning, and `db_session_grade` is the self-graded path for `classic` cards. UI in `src/components/ReviewQuiz.tsx`.

**Sentence breakdown:** user pastes text on `/breakdown` → `breakdown_split_sentences` → picks one if several → `db_start_sentence_breakdown` returns the cached sections and the `missing` ones → the page requests every missing section at once with `llm_breakdown_section` and shows each as it arrives: `chunks` (role codes, colored by `ROLE_STYLE` in `src/pages/Breakdown.tsx`), `core`, `layers`, `order`, `notes`, `gloss`, `check`. A failed section gets its own retry. Spec: `PRDs/Sentence Breakdown.md`.

**One card per word (Japanese):** `db_upsert_review_item` saves the dictionary form (`FuriganaAnnotator::lemma`: 下がります → 下がる) and returns the existing card when another form is already in the deck. `db_merge_duplicate_words` (run from `Layout` once the dictionary is ready) fills `review_items.lemma` and folds duplicates into the strongest card. `load_candidates` in `quiz.rs` never offers a card with the same lemma or reading as a wrong option.

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
| `/reading` | Reading | Level-appropriate texts + comprehension questions |
| `/breakdown` | Breakdown | Paste a sentence, get its structure explained ("Urai" tab) |
| `/words` | WordsList | Browsable word deck |
| `/grammar` | Grammar | Grammar patterns per level, review status and level progress, CRUD + CSV import |
| `/stats` | UserStats | Stats dashboard (charts: activity, mastery, proficiency) |
| `/settings` | Settings | Your language, learning aids, API provider (OpenRouter preset), review limits, grammar review (level, daily limits) |

First run (no API key, `onboarding_done` unset) shows `Onboarding` instead of the routes: language → target & level → connect AI.

`/telemetry` redirects to `/stats`.

## Database Schema (SQLite)

DB path: `~/.app-data/langchef/langchef.db`. Schema created in `open_db()` in `src-tauri/src/lib.rs`.

| Table | Key columns |
|-------|------------|
| `messages` | id, role, content, session_id, parent_message_id, is_active_path |
| `chat_sessions` | id, title, language, level, selected_grammar |
| `review_items` | id, content, EF, I, R, state, step, lapse_count, next_review, tags, language, detail, draft_detail, mnemonic, stability, difficulty, item_type (`word`/`grammar`), grammar_id, level, placement, lemma (Japanese dictionary form) |
| `known_words` | word (PK) |
| `grammar_items` | id, pattern, detail, language, level, is_default, source_id (translated row → original) |
| `user_activity` | date, activity_type, count |
| `daily_active_time` | date, total_seconds |
| `message_grammar_analysis` | message_id, analysis |
| `user_message_feedback` | message_id, score, feedback |
| `study_limits` | date, language, reviews_completed, new_cards_seen |
| `study`, `study_card` | daily queue per language; `study_card.task` holds the open question, `study_card.round` / `study.round` the round order |
| `review_answers` | review_item_id, task, correct, elapsed_ms, hint_used, chosen, grade, mature, prev_state, overridden, placement |
| `review_confusions` | review_item_id, confused_with_id, count |
| `app_settings` | key, value (`explain_lang`, `grammar_level:<lang>`, `grammar_new_per_day`, `grammar_checks_per_day`, `grammar_speed_up`) |
| `translations` | kind (`message` / `reading`), ref_id, lang, text |
| `breakdowns` | id, language, explain_lang, sentence (unique together), checked, created_at |
| `breakdown_parts` | breakdown_id, section, data (JSON; the check part holds the hidden answer) |
| `telemetry_days` | legacy, migrated to user_activity |

`review_items.meaning_lang`, `grammar_items.explain_lang` / `ai_translated`, and `lang` on `message_grammar_analysis` / `user_message_feedback` (cached AI output is reused only in the same language).

## Important Notes

- API config stored in `localStorage` keys: `api_host`, `api_key`, `api_model`; provider presets (OpenRouter default) in `src/lib/providers.ts`
- Learning aids in `localStorage` (`src/lib/prefs.ts`): `chat_hints`, `romaji_enabled`, `romaji_input`, `reading_answer_native`
- Grammar data in `grammar/` dir — `ja/` has N5–N1, `id/` has A1–A2, `ja/id/` has Indonesian explanations for N5–N4 (AI-translated, shown with a "Diterjemahkan oleh AI" badge). MD for display, CSV for import.
- `src/lib/intl-segmenter.d.ts` provides TS declarations for `Intl.Segmenter`
- PRD is in `PRD.md`; feature specs in `PRDs/`, technical specs in `TRDs/`
- Android release signing: keystore and credentials are never committed. Copy `keystore.properties.example` to `keystore.properties` (gitignored) or set `LANGCHEF_KEYSTORE_FILE` / `LANGCHEF_KEYSTORE_PASSWORD` / `LANGCHEF_KEY_ALIAS` / `LANGCHEF_KEY_PASSWORD`. Without them the release APK is built unsigned.
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