# Sentence Breakdown ("Urai kalimat")

Persona and journey: `PRDs/Sentence Comprehension Persona.md`.

## Background

Intermediate learners often know every word in a sentence and still can't follow it: describing clauses placed before the noun, omitted subjects, particles that do what word order does in Indonesian or English, stacked verb endings, clauses nested several levels deep. The existing aids don't fix this. **Translate** gives the answer without the structure, **Analyze grammar** lists patterns rather than showing how this sentence fits together, and review only drills single words.

## Solution

A dedicated **Urai** / **Break down** tab (`/breakdown`) in the bottom nav.

1. **Input.** The learner types or pastes a sentence or a paragraph (with a Paste button) and taps **Uraikan** / **Break it down**. Picks a target language when more than one is available.
2. **Pick a sentence.** If the text has several sentences, they are listed and the learner taps the confusing one. Splitting is done in Rust (`split_sentences`): it splits on 。！？!? and line breaks, a `.` only when followed by a space, never inside 「」.
3. **Breakdown page**, everything explained in the explanation language:
   - The sentence colored by chunk role (topic, subject, object, describing clause, purpose/reason/condition, time/place, main verb), with unspoken subjects shown as dashed chips and a legend for the roles that appear.
   - **Core of the sentence:** the bare main clause, what the rest adds, and a reading tip.
   - **How it nests:** boxes inside boxes, from the main clause inwards.
   - **Word order compared:** the parts in the sentence's order vs the natural order of the learner's language, in the same role colors.
   - **Parts of the sentence:** each chunk with its role label and meaning.
   - **Particles & endings:** what each one does in this sentence.
   - **Word for word:** gloss strip (with readings for Japanese).
   - **Check yourself:** one multiple-choice question that can only be answered by reading the structure right, graded in Rust, with an explanation pointing at the part of the sentence that gives the answer.
   - Full translation behind a button, last.
   Each section is its own small AI request; all are sent at once and each section appears as soon as it arrives (a skeleton until then, its own retry if it fails). The sentence itself shows immediately and gets its colors when the chunks arrive.
4. **History.** Breakdowns are cached per sentence + target language + explanation language and listed under "Recently broken down" with their structure tags and check result. They can be reopened (no new AI call) or deleted.

## Backend

- Module `src-tauri/src/breakdown.rs`.
- Sections (`SECTIONS`): `chunks`, `core` (translation, core, note, tip, structure tags), `layers`, `order`, `notes`, `gloss`, `check`. One shared intro plus a short per-section instruction, so each answer is small and fast.
- Commands: `breakdown_split_sentences`, `db_start_sentence_breakdown` (find or create; returns cached parts and the `missing` sections), `llm_breakdown_section`, `db_list_sentence_breakdowns`, `db_get_sentence_breakdown`, `db_check_breakdown_answer`, `db_delete_sentence_breakdown`.
- Tables `breakdowns(id, language, explain_lang, sentence, checked, created_at)`, unique on `(language, explain_lang, sentence)`, and `breakdown_parts(breakdown_id, section, data)`. The check part keeps its answer and explanation in the backend until the learner answers.
- `clean_section` maps unknown roles to `other` and rejects an unusable answer (no chunks, no core, invalid check) so the page offers a retry. `chunks_match` says whether the chunks spell out the sentence; if they don't, the page shows the plain sentence instead of the colored one.
- Uses `complete_explained`, so learner-facing text follows `explain_lang`; the Indonesian retry now repeats only the section that came back in English.
- `open_db` sets a 5 s busy timeout so the parallel section writes wait for the lock instead of failing.

## Not in this version

- **Sentence cards in Review** (structure questions on saved sentences, FSRS-scheduled). These need a new question kind in `quiz.rs` and must stay out of the word-meaning option pools. Planned as the next step.
- Entry points from Chat and Reading (long-press a sentence → open this page with the text filled in).
- Furigana on the colored sentence.
