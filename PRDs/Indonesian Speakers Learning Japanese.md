# Indonesian Speakers Learning Japanese

## Background

LangChef already teaches Japanese and Indonesian, but it quietly assumes the learner reads **English**:

- **App UI:** every label, button, toast, empty state and error is hardcoded English (no i18n layer; Ant Design runs with its default `en_US` locale).
- **Card backs:** `generate_detail_text` (`src-tauri/src/words.rs`) asks the LLM for `"meaning": "English translation"` and `"example_meaning": "English translation of the example"`. Every auto-graded quiz (`src-tauri/src/quiz.rs`) shows these meanings as answer options, so a learner who can't read English can't pass a single review.
- **Explanations:** grammar analysis (`grammar.rs`), message feedback (`feedback.rs`), reading answer checks (`reading.rs`) and the grade reasons in `compute_grade` ("slower than your usual 4s", "used furigana hint", "None of these: …") are written or requested in English.
- **Grammar content:** the bundled JLPT N5–N1 files in `grammar/ja/` explain Japanese in English ("Particle の shows possession… equivalent to the English possessive 's").
- **Chat:** the partner replies in Japanese only, and there is no "translate this" or "help me say this", so a beginner with no shared language with the app is stuck.
- **Setup:** the first thing a new user sees is an API host / key / model form with English-only provider docs.

Indonesia is one of the largest groups of Japanese learners in the world. Much of that demand comes from people going to work in Japan (Specified Skilled Worker / *Tokutei Ginou*, technical intern trainees / *magang*), who need roughly N4 / JFT-Basic (A2) level and often studied no English beyond school. Today they can't use LangChef at all.

## Goal

**A person who reads only Bahasa Indonesia can install LangChef, set it up, and learn Japanese from zero to JLPT N4, without seeing or needing a word of English.**

### Success criteria

| Metric | Target |
|---|---|
| Strings in core flows (onboarding, Home, Chats, Chat, Review, Words, Grammar, Reading, Stats, Settings) translated to Indonesian | 100% (enforced by a key check) |
| LLM explanations in Indonesian when the explanation language is Indonesian (sampled audit of card backs, feedback, grammar analysis, reading feedback) | ≥ 98% |
| Moderated test: 5 Indonesian-only speakers finish onboarding → first chat → first review session unaided | 5 / 5 |
| Bundled grammar explained in Indonesian | N5 + N4 at launch (P1), N3–N1 later |
| Quiz options in one card always share the same meaning language | 100% (unit-tested) |

### Non-goals

- Translating the UI into languages other than Indonesian and English. The design must allow it, but only `id` and `en` ship.
- Teaching Indonesian *to* Indonesian speakers (target = native is disallowed).
- Hosting a LangChef LLM proxy or bundling an API key. Users still bring their own OpenAI-compatible provider; we just make that understandable in Indonesian.
- Speech (TTS / speech recognition). Separate PRD.

---

## Solution

### Overview

Introduce a second language axis next to the existing **target language** (`ja` / `id`): the learner's **explanation language** (`en` / `id`), also called the "native language" in the UI ("Bahasa Anda"). It decides three things:

1. **UI language:** all app text comes from a translation dictionary keyed by the explanation language.
2. **LLM output language:** every prompt that produces meanings, explanations, feedback or translations carries an explicit "write explanations in Bahasa Indonesia" instruction.
3. **Content language:** card meanings, grammar explanations and quiz options are stored and picked by their meaning language.

On first run on a device whose locale is `id-*`, the explanation language defaults to Indonesian and the target language to Japanese.

Then add the learning aids a no-English beginner needs: translate any chat message, write in Indonesian and get it in Japanese ("Bantu saya menjawab"), Indonesian grammar notes, and optional romaji for absolute beginners.

### Key design decisions

1. **One setting, three effects.** A single `explain_lang` preference drives UI, prompts and content. Separate "UI language" and "meaning language" settings would let users end up with Indonesian buttons and English quiz options, the exact failure we're fixing. Advanced users can still switch it any time in Settings.

2. **No React context for i18n.** AGENT.md forbids global state and context providers. Translation is a plain module, `src/lib/i18n.ts`, which exports `t(key, params?)`. The dictionary is chosen once at startup from `localStorage.explain_lang` (falling back to `navigator.language`). Changing the language in Settings saves it and reloads the window. The language changes rarely, so a reload costs nothing and keeps the architecture unchanged. Ant Design's own `ConfigProvider locale` (its built-in mechanism, not app state) gets `id_ID` or `en_US` in `main.tsx`.

3. **Dictionaries as flat JSON files.** `src/locales/en.json` and `src/locales/id.json`, flat dotted keys (`review.dontKnow`, `chat.translate`), `{param}` interpolation, plural via `_one` / `_other` suffix. English is the source of truth. `npm run i18n:check` (a small Node script) fails if `id.json` is missing a key or has an extra one, or if a `.tsx` file contains a JSX text literal outside an allowlist. There's no test framework, so this script is the guard.

4. **Rust returns codes, React renders words.** Today `compute_grade` builds English reason strings and the summary formats "None of these: …". Per AGENT.md the backend owns logic and the frontend owns rendering, so Rust returns structured reasons (`{ "code": "slower_than_usual", "ms": 4200, "usual_ms": 2100 }`, `{ "code": "none_of_these" }`) and the frontend turns them into text with `t()`. Same for errors: Rust maps known failures to codes (`api_unauthorized`, `api_rate_limited`, `api_unreachable`, `api_bad_response`, `db_error`) and keeps the raw message in a `detail` field shown under "Detail teknis". This keeps all language-specific text in one place, the dictionaries.

5. **Meanings carry their language.** A card's back gets a meaning language. New `review_items.meaning_lang` column (`'en'` default for existing rows, so no data changes meaning silently). `generate_detail_text` takes `explain_lang` and stamps it. The quiz only builds wrong options from cards with the same `meaning_lang` as the card being asked, so it never mixes English and Indonesian options. Cards whose meaning language differs from the user's current one are treated like cards without a meaning: the existing `needs_meaning` flow offers "Terjemahkan arti" (generate a translated back for approval), and the session-start banner offers a batch translation (an extension of `db_generate_missing_meanings`).

6. **Grammar keeps one row per explanation language.** New `grammar_items.explain_lang` column (`'en'` for existing rows). Bundled Indonesian explanations live at `grammar/ja/id/N5.md` … and are seeded by an extended `seed_bundled_grammar_if_empty`, which checks `(language, level, explain_lang)` instead of `(language, level)`. Grammar list, chat grammar picker and reading show rows in the user's explanation language. If a level has none, they fall back to English rows with a "Terjemahkan dengan AI" button. User-added grammar items get the user's current explanation language.

7. **Bundled Indonesian grammar is AI-drafted, human-reviewed.** N5 and N4 (~200 patterns) are drafted with an LLM from the English files, then reviewed by a native Indonesian speaker who knows Japanese before they ship. Examples keep the Japanese sentence and replace the English gloss with Indonesian. Explanations compare with Indonesian where it helps (e.g. は vs が against *-nya* / *yang*, Japanese SOV vs Indonesian SVO, no tenses in Indonesian vs Japanese past ～ました, counters vs Indonesian *buah / orang / ekor*) instead of English.

8. **All LLM prompts go through one helper.** Add `fn explain_instruction(explain_lang: &str) -> &'static str` next to the existing `lang_name` matches (reused rather than adding another match). Every prompt that writes something meant for the learner appends it. Prompts whose output must stay in the target language (the chat reply itself, reading passages, reading questions) don't get it. The Japanese chat partner still speaks Japanese, and only the help text is Indonesian.

9. **Translations are cached.** Translating a chat message or reading passage calls the LLM once. The result is saved (`message_translations`, `reading_sessions.translation`) so tapping again is instant and free.

---

## User flows

### First run (new)

```
Launch (device locale id-ID)
 └─ Selamat datang di LangChef
      ├─ Bahasa Anda: [Bahasa Indonesia ✓] [English]
      ├─ Saya ingin belajar: [Jepang 🇯🇵 ✓]            (Indonesian hidden when Bahasa Anda = Indonesia)
      ├─ Level saya: [Belum bisa sama sekali] [Bisa hiragana] [N5] [N4] [N3+]
      │     "Belum bisa sama sekali" → turns romaji on + suggests Kana starter (P2)
      ├─ Hubungkan AI: provider presets + step-by-step guide in Indonesian
      │     [Tes koneksi] → localized success / error ("Kunci API salah", "Kuota habis", …)
      └─ Mulai → Home
```

Existing users skip onboarding. Their `explain_lang` is set to `en`, so nothing changes until they switch.

### Chat (Japanese target, Indonesian explanation)

```
Partner message (Japanese, furigana)
 ├─ tap word → popup: reading, arti (Indonesian), [Tambah ke kartu]
 ├─ [Terjemahkan] → Indonesian translation under the bubble (cached)
 └─ [Tata bahasa] → grammar analysis in Indonesian

Input box
 ├─ type Japanese → send (unchanged)
 └─ [Bantu saya menjawab] → type in Indonesian → AI suggests a Japanese reply at the
      session's JLPT level with furigana + Indonesian gloss → [Pakai] puts it in the input
      for the learner to edit and send
Feedback on the learner's message → summary, notes and suggestion in Indonesian
```

### Review

```
Quiz options in Indonesian ("makan", "minum", "tidur", "Tidak ada yang benar")
Buttons: "Tidak tahu", "Petunjuk", "Saya salah tekan"
Result reasons: "Dijawab dalam 3 dtk · tanpa petunjuk"
Card meaning in English while explanation language is Indonesian →
   "Arti kartu ini masih dalam bahasa Inggris" [Terjemahkan arti] [Lewati hari ini]
Session-start banner: "12 kartu masih berbahasa Inggris" [Terjemahkan semua]
```

---

## Requirements

### Functional requirements

#### FR1: Explanation language setting
- **FR1.1** The app SHALL store an explanation language `explain_lang` ∈ {`en`, `id`} in `localStorage` and pass it to every Tauri command that produces learner-facing text or content.
- **FR1.2** On first launch, `explain_lang` SHALL default to `id` when `navigator.language` starts with `id`, otherwise `en`. Existing installs (any of `api_host`, `api_key` present) SHALL default to `en`.
- **FR1.3** Settings SHALL let the user change it ("Bahasa Anda / Your language"). Changing it SHALL save and reload the app.
- **FR1.4** The target-language picker SHALL hide the target that equals the explanation language (no Indonesian → Indonesian).

#### FR2: Interface translation
- **FR2.1** Every user-visible string in `src/` (pages, components, toasts, modals, empty states, placeholders, `aria-label`s, chart labels, date and number formats) SHALL come from `t()`.
- **FR2.2** Dates and numbers SHALL be formatted with `Intl` using `id-ID` or `en-US`. Relative times ("2 jam lagi") SHALL use `Intl.RelativeTimeFormat`, replacing hand-built English strings (incl. what `format_next_review_minutes` returns: Rust SHALL return minutes and the frontend formats them).
- **FR2.3** Ant Design components SHALL use the matching built-in locale.
- **FR2.4** `npm run i18n:check` SHALL fail on missing / extra keys between `en.json` and `id.json` and on untranslated JSX text literals.
- **FR2.5** Indonesian copy SHALL use plain, friendly everyday Indonesian ("Anda", short sentences), avoid English loanwords where a common Indonesian word exists ("Pengaturan", not "Settings"; "Kartu", not "Flashcard"), and keep unavoidable technical terms (API, JLPT) with a short explanation the first time they appear.
- **FR2.6** Language and level names SHALL be localized (Jepang, Indonesia; "N5 — Dasar: salam, perkenalan diri"). The `description` fields in `src/lib/jlpt.ts` move into the dictionaries. `promptRule` stays English because the LLM reads it.

#### FR3: Backend codes instead of English text
- **FR3.1** `compute_grade` and the session summary SHALL return reason codes with numeric params instead of English sentences. Existing `cargo test --lib quiz` tests SHALL be updated to assert codes.
- **FR3.2** Commands that fail SHALL return `{ code, detail }` for known cases (HTTP 401/403 → `api_unauthorized`, 429 → `api_rate_limited`, connect/timeout → `api_unreachable`, JSON parse → `api_bad_response`). The frontend shows the translated message plus a collapsible raw `detail`.
- **FR3.3** Sentinels (`NONE_ANSWER = "__none"`, don't-know) stay language-neutral. Only their labels are translated.

#### FR4: Indonesian card meanings
- **FR4.1** `generate_detail_text` / `db_generate_detail` / `db_generate_missing_meanings` SHALL take `explain_lang` and request `meaning` and `example_meaning` in that language (for Indonesian: "arti dalam Bahasa Indonesia", short, the way an Indonesian dictionary would gloss it, e.g. 食べる → "makan").
- **FR4.2** `part_of_speech` SHALL be returned as a fixed code (`noun`, `verb`, `i_adj`, `na_adj`, `adverb`, `particle`, `expression`, …) and rendered via `t()` (kata benda, kata kerja, kata sifat-i, …).
- **FR4.3** `review_items` SHALL get `meaning_lang TEXT NOT NULL DEFAULT 'en'`, set on every save of a detail (generated, edited in `EditWordForm`, saved via `db_session_save_meaning`, CSV import).
- **FR4.4** CSV import (`word,meaning`) SHALL take the meaning language from an import-dialog choice that defaults to `explain_lang`.
- **FR4.5** `meaning_distractors` SHALL only use candidate cards with the same `meaning_lang` as the asked card. The "fewer than 2 words with meanings → classic" fallback SHALL count per meaning language.
- **FR4.6** A card whose `meaning_lang` ≠ `explain_lang` SHALL be treated as `needs_meaning`, with a "translate" variant of the existing UI. The draft (`draft_detail`) is a translation of the existing back. Approving saves it with the new `meaning_lang` and keeps the old detail in a new `review_items.detail_prev` column so it can be undone.
- **FR4.7** The session-start banner SHALL count and batch-translate such cards, reusing the `db_generate_missing_meanings` loop with a `translate_from` mode.
- **FR4.8** Words list SHALL show the meaning in the user's language and offer "Terjemahkan semua arti" for the deck.

#### FR5: LLM explanations in Indonesian
- **FR5.1** These prompts SHALL append `explain_instruction(explain_lang)`: card detail (`words.rs`), grammar analysis (`grammar.rs`), message feedback `summary` / `note` fields (`feedback.rs`, keeping `suggestion` in the target language), reading answer feedback (`reading.rs`), and the new translate / help-me-reply commands.
- **FR5.2** The in-prompt examples in `feedback.rs` SHALL have an Indonesian variant so the model isn't steered back to English by an English example.
- **FR5.3** The chat system prompt SHALL stay target-language-only for the partner's reply. When the session level is N5 and the learner chose "Belum bisa sama sekali" / "Bisa hiragana", the system prompt MAY allow one short Indonesian hint in parentheses per reply (toggle "Petunjuk bahasa Indonesia di chat", default on for those learners, off otherwise).
- **FR5.4** If a response comes back in the wrong language (cheap check: for `id`, a feedback `summary` with no common Indonesian function words and mostly English stopwords), the command SHALL retry once with a stronger instruction before returning.

#### FR6: Chat aids
- **FR6.1** Each assistant message SHALL have a "Terjemahkan" action that calls new command `llm_translate(text, from_lang, to_lang)` and caches the result in new table `message_translations(message_id, lang, text)`.
- **FR6.2** The word-tap popup SHALL show the meaning in `explain_lang` (from the card if it exists in that language, otherwise generated on demand).
- **FR6.3** "Bantu saya menjawab": the learner types in Indonesian. New command `llm_help_reply(intent, context_messages, level, target_lang, explain_lang)` returns `{ reply, reply_furigana, gloss }`. The reply respects the session's JLPT `promptRule` and selected grammar. The learner must press "Pakai" and may edit before sending. Usage is recorded as activity `help_reply`.
- **FR6.4** Chat topic presets SHALL include contexts relevant to Indonesian learners: *Perkenalan diri*, *Wawancara kerja*, *Di tempat kerja (pabrik, perawatan lansia / kaigo, restoran)*, *Belanja & konbini*, *Di stasiun*, *Ke dokter*, *Menelepon*. Shown in Indonesian, sent to the LLM in English.

#### FR7: Grammar in Indonesian
- **FR7.1** `grammar_items` SHALL get `explain_lang TEXT NOT NULL DEFAULT 'en'`.
- **FR7.2** Bundle `grammar/ja/id/N5.md` and `N4.md` (+ `.csv`), seeded per `(language, level, explain_lang)`. Existing English rows stay.
- **FR7.3** `db_get_grammar_*` commands SHALL take `explain_lang` and return rows in that language, falling back per level to `en` with a `fallback: true` flag.
- **FR7.4** Fallback rows SHALL offer "Terjemahkan dengan AI", which saves a translated copy as a new row with `explain_lang='id'` and `is_default=0`.
- **FR7.5** Each Indonesian grammar entry SHALL have: pattern, *Arti* (one line), *Cara pakai* (formation, e.g. "KK bentuk-te + います"), *Contoh* (2–3 Japanese sentences with furigana + Indonesian translation), and *Catatan* (common mistakes for Indonesian speakers) where useful.
- **FR7.6** N3, N2, N1 Indonesian files follow in a later release using the same process.

#### FR8: Reading in Indonesian
- **FR8.1** Reading answer feedback (`evaluate_reading_answer`) SHALL be in `explain_lang`. Questions and expected answers stay in Japanese.
- **FR8.2** The reading view SHALL offer "Lihat terjemahan" (whole passage, cached in `reading_sessions.translation`), shown only after the learner presses it.
- **FR8.3** At N5/N4 the learner MAY answer comprehension questions in Indonesian (toggle). The evaluator then checks understanding, not Japanese production, and the result is marked so it doesn't count as a production answer in stats.

#### FR9: AI provider setup in Indonesian
- **FR9.1** Onboarding and Settings SHALL include an Indonesian step-by-step guide for getting an API key from each preset provider, with a "Tes koneksi" button.
- **FR9.2** Provider presets SHALL fill `api_host` and suggest a model so the learner only pastes a key. Presets are data (a JSON list in the frontend), so adding one needs no code change.
- **FR9.3** Errors during setup SHALL use the FR3.2 codes with Indonesian guidance ("Kunci API salah — salin ulang dari halaman provider", "Kuota habis — tunggu atau isi saldo").

#### FR10: Beginner support (P2)
- **FR10.1** Romaji toggle: show Hepburn romaji under Japanese text (chat, cards, reading), generated from the Lindera readings already produced by `furigana.rs`. Default on only for "Belum bisa sama sekali", with a reminder to switch to furigana after N5 kana is learned.
- **FR10.2** Kana starter: a hiragana then katakana deck (46 + 46 + dakuten/combos) seeded as review items with Indonesian-friendly hints (e.g. し = "shi", ち = "chi", つ = "tsu" explained with Indonesian sounds: "chi" dibaca seperti "ci" dalam "cinta"). Reviewed with the existing quiz (pick the reading).
- **FR10.3** Romaji input: chat input MAY convert typed romaji to kana live (for Android keyboards without a Japanese IME), toggled in Settings.

### Non-functional requirements

- **NFR1 Architecture:** no React context, global store or state library. `t()` is a plain module, and all filtering and selection logic (meaning-language matching, grammar fallback, wrong-language retry) is in Rust.
- **NFR2 Migrations:** every schema change goes in `open_db()` (`src-tauri/src/db.rs`) and is idempotent. Existing rows default to `'en'`, so upgrading changes no data until the user opts in.
- **NFR3 Backward compatibility:** with `explain_lang='en'` the app looks and behaves exactly as today (same English copy, same prompts apart from the explicit English instruction).
- **NFR4 Cost:** translations and generated backs are cached. Batch translation shows the card count before running and runs in chunks so it can be cancelled.
- **NFR5 Text fit:** Indonesian strings run ~20–30% longer than English. Every screen SHALL be checked at 360 px width (Android) with the Indonesian dictionary: no clipped buttons in the bottom nav, quiz options wrap.
- **NFR6 Backup:** Google Drive backup/restore SHALL include the new columns and tables. `explain_lang` and the other preferences SHALL be included in the backup so a restore on a new phone keeps the language.
- **NFR7 Verification:** `npx tsc --noEmit`, `npm run i18n:check`, `cargo check`, `cargo clippy`, and `cargo test --lib quiz` (extended with meaning-language tests) pass.

---

## Data model changes

| Table | Change |
|---|---|
| `review_items` | `+ meaning_lang TEXT NOT NULL DEFAULT 'en'`, `+ detail_prev TEXT` |
| `grammar_items` | `+ explain_lang TEXT NOT NULL DEFAULT 'en'` |
| `message_translations` | new: `message_id TEXT, lang TEXT, text TEXT, created_at`, PK `(message_id, lang)` |
| `reading_sessions` | `+ translation TEXT` |
| `user_activity` | new activity types: `translate`, `help_reply` (no schema change) |

`localStorage`: new `explain_lang`, `romaji_enabled`, `chat_id_hints`, `romaji_input`, `onboarding_done`.

## Commands

| Command | Change |
|---|---|
| `db_generate_detail`, `db_generate_missing_meanings` | `+ explain_lang`; `db_generate_missing_meanings` `+ translate_from` mode |
| `db_session_start`, `db_session_answer`, `db_session_summary` | `+ explain_lang` (to detect wrong-language cards); reasons returned as codes |
| `db_session_save_meaning`, `db_upsert_review_item`, CSV import | write `meaning_lang` |
| `analyze_grammar`, feedback, `evaluate_reading_answer` | `+ explain_lang` |
| `db_get_grammar_*` | `+ explain_lang`, return `fallback` flag |
| `llm_translate` | **new**: `(text, from_lang, to_lang, message_id?)` → `String`, cached |
| `llm_help_reply` | **new**: `(intent, context_messages, level, target_lang, explain_lang)` → `{ reply, reply_furigana, gloss }` |
| `db_translate_grammar_item` | **new**: copy a grammar row into `explain_lang` via LLM |

New TS types in `src/lib/types.ts`: `GradeReason`, `AppError`, `HelpReply`, `ExplainLang`.

## Files affected (main)

- **New:** `src/lib/i18n.ts`, `src/locales/en.json`, `src/locales/id.json`, `scripts/i18n-check.mjs`, `src/pages/Onboarding.tsx`, `src/lib/providers.ts`, `grammar/ja/id/N5.{md,csv}`, `grammar/ja/id/N4.{md,csv}`
- **Rust:** `db.rs` (migrations, seeding), `words.rs`, `quiz.rs`, `srs.rs`, `grammar.rs`, `feedback.rs`, `reading.rs`, `chat.rs` (translate / help-reply), `google_drive.rs` (backup), `lib.rs` (register commands)
- **Frontend:** every page and component for `t()`, plus `main.tsx` (antd locale), `App.tsx` (onboarding route), `Chat.tsx` (translate, help-reply, topics), `ReviewQuiz.tsx` / `Review.tsx` (codes, translate-meaning flow), `WordsList.tsx`, `EditWordForm.tsx`, `Grammar.tsx`, `Reading.tsx`, `Settings.tsx`, `jlpt.ts`

---

## Phasing

| Phase | Scope | Exit criteria |
|---|---|---|
| **P0: Usable without English** | FR1, FR2, FR3, FR4, FR5, FR9 | Indonesian-only tester can onboard, chat (with tap-to-see meaning), and finish a review session with Indonesian options |
| **P1: Learnable without English** | FR6 (translate, help-reply, topics), FR7 (N5 + N4 Indonesian grammar, human-reviewed), FR8 | A beginner can follow a chat, understand grammar notes and get reading feedback in Indonesian |
| **P2: From zero** | FR10 (romaji, kana starter, romaji input), FR7.6 (N3–N1 grammar) | Someone who can't read kana can start on day one |

---

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| Smaller / cheaper models drift back to English | Explicit instruction in every prompt, Indonesian in-prompt examples, one automatic retry (FR5.4), and the onboarding recommends models that handle Indonesian well |
| Bad Indonesian glosses for Japanese words (LLM translating via English, e.g. picking an odd sense) | Prompt asks for direct JA→ID glosses; the user approves every generated back (existing approval flow); "Laporkan arti salah" opens the edit form |
| Mixed-language decks confuse the quiz | `meaning_lang` filter on wrong options (FR4.5) with unit tests |
| AI-drafted grammar notes contain errors | Native-speaker review before bundling; marked "Diperiksa" in UI; AI-translated user copies are marked "Terjemahan AI" |
| Hardcoded strings creep back | `i18n:check` lints JSX literals; PR checklist item |
| Text overflow on small Android screens | NFR5 check at 360 px; shorter Indonesian labels for the bottom nav (Beranda, Chat, Ulas, Kata, Statistik) |
| API key setup still too hard for non-technical users | Indonesian guide with screenshots per preset; error codes with concrete fixes. Out of scope here but noted: a hosted option would remove this step entirely |

## Open questions

1. Which providers go in the presets list for launch? Ideally ones with a free tier usable from Indonesia and good Indonesian output.
2. Who does the native-speaker review of the N5/N4 grammar notes and the UI copy? Budget and timeline.
3. Should the chat Indonesian hints (FR5.3) be per-session or a global preference?
4. Should we add JFT-Basic as a level option next to JLPT (it maps roughly to A2 / between N5 and N4), given how many Indonesian learners target it for work visas?
5. When a user switches explanation language back to English, should translated cards offer "restore English meaning" from `detail_prev`, or regenerate?
