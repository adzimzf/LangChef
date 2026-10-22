# Grammar Review

## Background

Review only drilled words. Grammar patterns lived on the Grammar page and in the chat prompt, but nothing checked whether the learner still knew them, and the "level" was a setting the learner picked (onboarding, chat picker) that never changed on its own. Adding a pattern as a word card didn't work either: the card back is built for vocabulary (reading, part of speech), and the word questions test matching a short meaning, not using a pattern.

## Solution

Grammar patterns become review cards in the **same daily queue as words**, with their own questions, their own daily limits, and progress per level.

### Your level

- One level per target language (`ja`: N5–N1, `id`: A1–A2), stored in `app_settings` as `grammar_level:<lang>`.
- Set in onboarding (the "N5 / N4 / N3" step), in **Settings → Grammar review**, or from the Grammar page. Existing installs start from the JLPT level saved for chat (`jlpt_level`). Until one is set, the easiest level is used.
- Changing level never touches cards already in review.

### Which patterns enter review

| Level relative to yours | How it enters | Daily limit (default) |
|---|---|---|
| Your level | As a **new** pattern: intro, then questions | 3 new grammar per day |
| Below your level | As a **level check**: one fill-the-gap question | 5 checks per day |
| Above your level | Only when the learner taps **Add to review** | — |

- Lower levels are checked easiest first (all of N5, then N4).
- **Level check:** right → graded Easy, straight to review (about two weeks out), counts as known. Wrong or "Don't know" → Again, and from then on it is taught like a new pattern; the result screen shows how the pattern works.
- **Speed up easy levels** (on by default): while at least 9 of the last 10 checks were right, checks per day double.
- Grammar has its own daily slots (`study_card.kind` `grammar_new` / `grammar_check`), so it never uses up new-word slots. Due grammar shares the review limit with words.

### Questions by card stage

| Stage | Question |
|---|---|
| Level check | Fill the gap (with translation) |
| New | Intro (meaning, how to form it, example, note), then true/false on the meaning |
| Learning / relearning | What the highlighted pattern means in a sentence |
| Review | Fill the gap (with translation) |
| Mature (stability ≥ 21 days) | Rotates: fill the gap without translation, meaning in a sentence, find the mistake (tap the wrong chunk) |

Grading reuses `compute_grade` (wrong → Again, hint or slow → Hard, else Good, Easy only for fast answers on mature cards), plus the level-check rule above. "I mis-tapped" works as for words.

### Practice material (the card's back)

A grammar card has no hand-written back. The AI makes it once, in the explanation language, and the learner checks it like a word's meaning draft (only the meaning can be edited):

- `title` (pattern with English words translated), `meaning`, `form`, `note`
- 3 `examples`: sentence, the exact `target` substring, translation, 3 wrong fillers
- one `mistake`: the sentence in chunks, which chunk is wrong, the fix
- 3 `confusables`: look-alike patterns and their meanings (extra wrong options while the deck has little grammar)

Rust drops anything a question can't safely use (target not in the sentence, a filler equal to the answer, an invalid mistake). The session-start "Make all" banner also prepares grammar cards. Switching the explanation language empties the back, as for words.

### Level progress

- **Grammar page:** each level chip shows % known; the selected level shows known / learning / total, its role (your level, quick checks, later), a "Move up to N3?" banner once 80% of your level is known, "Practice in review", and per pattern a status (Known, Learning, In review · new, Check due, Coming soon, Check coming, Not in review) with **Add to review** for patterns above your level.
- **Settings → Grammar review:** level picker, the two daily limits, speed up, and an "At this pace" estimate (days to introduce your level, days to check the levels below).
- **Session summary:** words vs grammar answered today.

## Backend

- Module `src-tauri/src/grammar_review.rs`: level order, plan and progress (`grammar_plan`), per-row status (`review_statuses`), enrolling patterns (`enroll_for_today`, called from `srs::top_up_queue`), practice material (`GrammarDetail`, `parse_grammar_detail`, `generate_card_back`).
- Commands: `db_get_grammar_plan`, `db_set_grammar_plan`, `db_seed_grammar_level`, `db_add_grammar_to_review`. `db_list_grammar_items` now returns `review` per row; `db_generate_detail` and `db_generate_missing_meanings` make practice material for grammar cards.
- `quiz.rs`: `build_grammar_task`, kinds `grammar_meaning`, `grammar_gap`, `grammar_mistake`; `QuizTask.grammar` (what the card teaches; examples only where they give nothing away) and `QuizTask.placement`.
- Schema: `review_items.item_type` (`word` | `grammar`), `grammar_id`, `level`, `placement`; `review_answers.placement`; `grammar_items.source_id` (a translated row points at its original; bundled translations are paired by position on startup). Grammar cards are excluded from the word list, chat word lists and mnemonics.
- `db::prepare_db` runs the schema on any connection, so tests use the real schema in memory.

## Not in this version

- **Grammar used in chat.** The chat feedback already marks each pattern the learner used as right or wrong; feeding that into progress (and turning a pattern used wrong into a card) is the next step.
- **Writing a sentence with the pattern**, graded by the AI, for mature cards.
- **Vocabulary per level.** Words have no JLPT tag yet, so level progress is grammar only.
