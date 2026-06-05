# TRD: Reading Feature

## Overview

Add a `/reading` route where users read level-appropriate texts, see vocabulary colored by their review_items difficulty, toggle furigana (Japanese), and answer LLM-validated comprehension questions one at a time.

---

## PRD Requirements

1. Level picker before text loads — JLPT N5–N1 (JP) or CEFR A1–B2 (ID).
2. Real news scrape first, LLM-generated text as fallback.
3. Level-appropriate text length.
4. Furigana toggle for Japanese, defaulting to ON for N5–N3.
5. Vocabulary coloring from `review_items` difficulty.
6. 3–5 comprehension questions, one at a time, typed answers.
7. LLM validates each answer: correct → next; incorrect → retry required.
8. Session recorded as `reading` activity. Summary screen at end.

---

## Technical Changes

### 1. New Rust Commands (`src-tauri/src/lib.rs`)

#### 1a. `fetch_reading_text`

```rust
#[tauri::command]
async fn fetch_reading_text(
    language: String,   // "ja" | "id"
    level: String,      // "N5".."N1" | "A1".."B2"
    state: tauri::State<'_, AppState>,
) -> Result<ReadingText, String>
```

**Logic:**
1. Call `try_scrape_article(&language, &level)` with a 3-second timeout.
2. On success, optionally trim to level word-count range (see PRD).
3. On any error, call `generate_text_via_llm(&language, &level, &state)`.
4. For Japanese: run `annotate(&body)` from `furigana.rs` to embed `{kanji|reading}` markup.
5. Return `ReadingText`.

```rust
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ReadingText {
    pub title: String,
    pub body: String,           // furigana-annotated for JP
    pub plain_body: String,     // strip furigana, used for Q&A context
    pub source: String,         // "NHK Web Easy" | "Detik" | "AI Generated" | etc.
    pub language: String,
    pub level: String,
}
```

**`try_scrape_article` targets:**

Use **RSS feeds only** — not raw HTML scraping. RSS is intended for public consumption, lower legal risk, and doesn't require HTML parsing of article internals.

| Language | Source | RSS URL |
|----------|--------|---------|
| ja | NHK Web Easy | `https://www3.nhk.or.jp/rss/news/cat0.xml` — parse `<item>`, pick random entry, use `<description>` as body |
| id | Detik | `https://rss.detik.com/index.php/detikcom` — parse `<item>`, pick random entry, use `<description>` |

RSS `<description>` fields are typically article summaries (100–300 words) — suitable for N4/A2 and above. For N5/A1, trim to length. For N2/N1/B2 where a longer text is needed, take the summary and ask LLM to expand it while preserving the facts.

Use `reqwest` (already a dep) with a 3-second timeout. Parse XML with the `quick-xml` crate (add to `Cargo.toml`). No `scraper` crate needed.

**`generate_text_via_llm` prompt (Japanese N3 example):**
```
Write a {level}-level Japanese reading passage about a random everyday topic.
- Length: {min_chars}–{max_chars} characters
- Vocabulary: appropriate for {level}
- Include furigana in {kanji|reading} format for kanji above {level}
- Return JSON: { "title": "...", "body": "..." }
Use only the JSON — no markdown fences.
```

#### 1b. `generate_reading_questions`

```rust
#[tauri::command]
async fn generate_reading_questions(
    plain_body: String,
    language: String,
    level: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ReadingQuestion>, String>
```

**Prompt:**
```
You are a language teacher. Based on the following {level} {language} reading passage, generate {n} comprehension questions.
Questions must be answerable from the text alone.
Answers should be written in {language}.
Return JSON array: [{ "question": "...", "answer_hint": "..." }, ...]
Use only the JSON — no markdown fences.

Passage:
{plain_body}
```

`n` = 3 for N5/A1–A2, 4 for N4/N3/B1, 5 for N2/N1/B2.

```rust
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ReadingQuestion {
    pub question: String,
    pub answer_hint: String,  // internal only, not shown to user
}
```

#### 1c. `evaluate_reading_answer`

```rust
#[tauri::command]
async fn evaluate_reading_answer(
    question: String,
    answer_hint: String,
    user_answer: String,
    plain_body: String,
    language: String,
    state: tauri::State<'_, AppState>,
) -> Result<AnswerEvaluation, String>
```

**Prompt:**
```
You are a language teacher evaluating a student's reading comprehension answer.

Reading passage:
{plain_body}

Question: {question}
Expected answer (hint, not exact): {answer_hint}
Student's answer: {user_answer}

Evaluate the answer as one of:
- "correct": answer is factually right and sufficiently detailed
- "partial": answer has the right idea but is incomplete or slightly wrong
- "incorrect": answer is wrong or shows misunderstanding

Return JSON: { "result": "correct"|"partial"|"incorrect", "feedback": "..." }
Feedback must be in {language} and 1–2 sentences max. No markdown.
```

```rust
#[derive(serde::Serialize, serde::Deserialize)]
pub struct AnswerEvaluation {
    pub result: String,   // "correct" | "partial" | "incorrect"
    pub feedback: String,
}
```

#### 1d. Register all three commands in `invoke_handler`

```rust
tauri::generate_handler![
    // ... existing handlers ...
    fetch_reading_text,
    generate_reading_questions,
    evaluate_reading_answer,
]
```

#### 1e. `Cargo.toml` additions

```toml
[dependencies]
quick-xml = { version = "0.36", features = ["serialize"] }
```

`reqwest` is already present. Confirm `features = ["json"]` is set.

---

### 2. New Types (`src/lib/types.ts`)

```typescript
export interface ReadingText {
  title: string;
  body: string;           // furigana-annotated (JP) or plain (ID)
  plain_body: string;
  source: string;
  language: string;
  level: string;
}

export interface ReadingQuestion {
  question: string;
  answer_hint: string;
}

export interface AnswerEvaluation {
  result: 'correct' | 'partial' | 'incorrect';
  feedback: string;
}
```

---

### 3. New Route (`src/App.tsx`)

```tsx
import Reading from './pages/Reading';
// ...
<Route path="/reading" element={<Reading />} />
```

Add **Reading** icon + tab to `src/components/Layout.tsx` bottom nav (between Review and Words, or after Words — designer's call).

---

### 4. New Page (`src/pages/Reading.tsx`)

State machine with four phases: `level-pick` → `loading` → `reading` → `qa`.

```
type Phase = 'level-pick' | 'loading' | 'reading' | 'qa' | 'summary';
```

**`level-pick` phase:**
- Language toggle (Japanese / Indonesian) if not determinable from last session.
- Level buttons (N5–N1 or A1–B2).
- "Start Reading" calls `invoke('fetch_reading_text', { language, level })` → transitions to `loading`.
- Persists `lastReadingLanguage` + `lastReadingLevel` to `localStorage`.

**`loading` phase:**
- Spinner + "Fetching article…" text.
- On success → call `invoke('generate_reading_questions', {...})` → transition to `reading` once both resolve.
- On error (both scrape and LLM failed) → show error message + retry button.

**`reading` phase:**
- `<ReadingText>` component with text + furigana toggle (JP only).
- Source badge.
- "Start Questions" button → transitions to `qa`.

**`qa` phase:**
- Shows question index (e.g. "Question 2 / 4").
- `<QuestionCard>` with current question, textarea, submit button.
- Collapsible "View passage" section showing the original text (furigana state preserved).
- On submit → `invoke('evaluate_reading_answer', {...})` → show result inline.
  - correct: green checkmark + feedback + auto-advance after 1.5s (or "Next" button).
  - partial: yellow warning + feedback + "Retry" or "Continue" buttons.
  - incorrect: red X + feedback + "Try Again" (re-enables textarea with previous answer cleared).

**`summary` phase:**
- Total questions, total retries, time taken.
- "Read Another" button resets to `level-pick`.
- Triggers `invoke('db_record_activity', { activity_type: 'reading' })`.

---

### 5. New Components (`src/components/reading/`)

#### `ReadingText.tsx`

Props: `text: ReadingText`, `reviewItems: ReviewItem[]`, `showFurigana: boolean`, `onToggleFurigana: () => void`

**Furigana toggle:** Same `renderFurigana()` utility already used in Chat. When `showFurigana=false`, call `stripFurigana(text.body)` before render.

**Vocab coloring:** Before rendering, call `splitTappable(plainWord)` then check each token against `reviewItems` — same coloring logic as `WordsList`. Wrap colored tokens in `<span style={{ color: difficultyColor(item) }}>`. Words not in review_items render unstyled.

**Implementation note:** The text contains furigana markup `{kanji|reading}`. Coloring must operate on the kanji/surface form, not the reading. Parse `{kanji|reading}` pairs, color by `kanji`, reconstruct the ruby markup.

#### `QuestionCard.tsx`

Props: `question: string`, `onSubmit: (answer: string) => Promise<void>`, `evaluation: AnswerEvaluation | null`, `submitting: boolean`

- Controlled textarea, disabled while `submitting=true` or `evaluation?.result === 'correct'`.
- Clears textarea on retry (parent passes `key` to force remount on retry).

#### `ReadingSummary.tsx`

Props: `totalQuestions: number`, `totalRetries: number`, `elapsedSeconds: number`, `onRestart: () => void`

---

### 6. Vocabulary Coloring Integration

`Reading.tsx` fetches `review_items` on mount via existing `invoke('db_get_review_items', {})` — same call used by Chat and Review pages. Passes the array down to `ReadingText`.

Difficulty color mapping reuses the existing helper from `src/pages/WordsList.tsx` — extract it to `src/lib/difficulty.ts` if not already shared.

---

### 7. Furigana Default by Level

```typescript
function defaultFurigana(level: string): boolean {
  return ['N5', 'N4', 'N3'].includes(level);
}
```

---

### 8. Text Length Enforcement (Rust)

After LLM generation or scrape, trim to level range in Rust:

```rust
fn enforce_length(text: &str, language: &str, level: &str) -> String {
    let (min, max) = level_char_range(language, level);
    if text.chars().count() <= max {
        return text.to_string();
    }
    // Truncate at last sentence boundary within max
    truncate_at_sentence(text, max)
}
```

For scraped articles that are too short (< min), request LLM to expand; for LLM-generated text, include length in the prompt so this is rarely needed.

---

### 9. Scraping Failure Modes & Fallback

| Failure | Behavior |
|---------|----------|
| DNS / network error | Immediately fall back to LLM |
| HTTP 4xx / 5xx | Fall back to LLM |
| Parsing yields empty string | Fall back to LLM |
| Timeout > 3s | Cancel request, fall back to LLM |
| LLM also fails | Return `Err("Failed to load article. Check your API settings.")` |

The frontend shows `source = "AI Generated"` whenever the scrape path was not used.

---

### 10. Session Persistence

Users navigate away (e.g. back to Home, then return to /reading) and expect to resume where they left off.

#### DB Schema — `reading_sessions` table

Added in `open_db()` alongside existing `ALTER TABLE` migrations:

```sql
CREATE TABLE IF NOT EXISTS reading_sessions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    language    TEXT NOT NULL,
    level       TEXT NOT NULL,
    title       TEXT NOT NULL,
    body        TEXT NOT NULL,        -- furigana-annotated
    plain_body  TEXT NOT NULL,
    source      TEXT NOT NULL,
    questions   TEXT NOT NULL,        -- JSON: Vec<ReadingQuestion>
    answers     TEXT NOT NULL DEFAULT '[]', -- JSON: Vec<SavedAnswer>
    phase       TEXT NOT NULL DEFAULT 'reading', -- 'reading'|'qa'|'complete'
    current_q   INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL DEFAULT (date('now'))
);
```

Only one active (non-complete) session is kept per language. On `db_save_reading_session`, delete any previous incomplete session for the same language before inserting.

#### New Rust Commands

```rust
#[tauri::command]
fn db_save_reading_session(
    language: String, level: String,
    title: String, body: String, plain_body: String, source: String,
    questions: String,  // serialized JSON
    conn: tauri::State<'_, DbConn>,
) -> Result<i64, String>   // returns new session id
```

```rust
#[tauri::command]
fn db_get_active_reading_session(
    language: String,
    conn: tauri::State<'_, DbConn>,
) -> Result<Option<ReadingSession>, String>
```

```rust
#[tauri::command]
fn db_update_reading_session(
    id: i64,
    phase: String,
    current_q: i64,
    answers: String,   // serialized JSON
    conn: tauri::State<'_, DbConn>,
) -> Result<(), String>
```

```rust
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ReadingSession {
    pub id: i64,
    pub language: String,
    pub level: String,
    pub title: String,
    pub body: String,
    pub plain_body: String,
    pub source: String,
    pub questions: Vec<ReadingQuestion>,
    pub answers: Vec<SavedAnswer>,
    pub phase: String,
    pub current_q: usize,
    pub created_at: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct SavedAnswer {
    pub question_index: usize,
    pub user_answer: String,
    pub result: String,      // "correct"|"partial"|"incorrect"
    pub retries: u32,
}
```

#### Frontend — Restore on Mount

`Reading.tsx` `useEffect` on mount:

```typescript
// On mount, check for an active session for the current language
const saved = await invoke<ReadingSession | null>('db_get_active_reading_session', { language });
if (saved && saved.phase !== 'complete') {
  // Restore: skip level-pick, go straight to 'reading' or 'qa'
  setReadingText({ title: saved.title, body: saved.body, plain_body: saved.plain_body, source: saved.source, language: saved.language, level: saved.level });
  setQuestions(saved.questions);
  setCurrentQ(saved.current_q);
  setAnswers(saved.answers);
  setPhase(saved.phase as Phase);
  setSessionId(saved.id);
} else {
  setPhase('level-pick');
}
```

**On new session start:** After `fetch_reading_text` + `generate_reading_questions` both succeed, call `db_save_reading_session` and store the returned `id` in state.

**On each answer submit:** After evaluation resolves, call `db_update_reading_session` with updated `phase`, `current_q`, and appended `answers`.

**On summary screen:** Call `db_update_reading_session` with `phase = 'complete'`. The "Read Another" button resets state to `level-pick` (a new session will be created on next fetch).

**"Start Fresh" button:** Show on the restored-session banner — calls `db_update_reading_session(id, 'complete', ...)` to discard the saved session, then resets to `level-pick`.

---

### 11. Navigation

`src/components/Layout.tsx` — add Reading tab:

```tsx
{ path: '/reading', label: 'Read', icon: <BookOpenIcon /> }
```

Use a book/newspaper icon from the existing icon set (Heroicons or Lucide, whichever is in use).

---

## File Changelist

| File | Change |
|------|--------|
| `src-tauri/src/lib.rs` | Add `fetch_reading_text`, `generate_reading_questions`, `evaluate_reading_answer`, `db_save_reading_session`, `db_get_active_reading_session`, `db_update_reading_session`; add `reading_sessions` table in `open_db()`; register all in handler |
| `src-tauri/Cargo.toml` | Add `quick-xml = "0.36"` |
| `src/lib/types.ts` | Add `ReadingText`, `ReadingQuestion`, `AnswerEvaluation`, `ReadingSession`, `SavedAnswer` |
| `src/lib/difficulty.ts` | Extract difficulty color helper (if not already shared) |
| `src/App.tsx` | Add `/reading` route |
| `src/components/Layout.tsx` | Add Reading tab to bottom nav |
| `src/pages/Reading.tsx` | New — main page, state machine + session restore on mount |
| `src/components/reading/ReadingText.tsx` | New |
| `src/components/reading/QuestionCard.tsx` | New |
| `src/components/reading/ReadingSummary.tsx` | New |

---

## Open Questions

1. **NHK RSS article length** — RSS `<description>` summaries are ~100–200 chars. Fine for N5/N4 but N1/N2 need 700–2000 chars. Plan: for high levels, LLM expands the RSS summary into a full article using the original facts. Confirm this is acceptable vs. always using LLM for N1/N2.
2. **Indonesian source** — Detik RSS covers general news; may not produce A1/A2-appropriate vocabulary. Consider switching to LLM-only for Indonesian A1/A2 and using RSS only for B1/B2.
3. **Partial correct UX** — Allow user to continue without retrying on `partial`? PRD says "retry or continue" — confirm with product owner.
4. **Session restore banner** — When restoring a saved session, show a banner "Resuming yesterday's article — [Start Fresh]"? Or silently restore? Decide on UX.
