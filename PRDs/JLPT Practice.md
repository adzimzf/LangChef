# JLPT Practice

Timed practice that looks and feels like the real JLPT, cut into short sessions so learners practise regularly instead of sitting a 2-hour mock test.

Mockup: `mockups/jlpt-practice.html`. Code: `src-tauri/src/jlpt.rs`, `src/pages/JlptPractice.tsx`.

## Where it lives

Not a daily feature, so it is **not in the bottom tab bar**. It is a row in Home's quick actions ("JLPT Practice", route `/jlpt`) that shows what comes next in the rotation, or that today's session is done.

## Sessions

One session = one section of the real test at one level, with fewer questions:

| Section | N5 / N4 | N3 | N2 | N1 |
|---|---|---|---|---|
| 文字・語彙 | 10 q: 漢字読み, 表記, 文脈規定, 言い換え (+ 用法 at N4) | as N4 | adds 語形成 | 漢字読み, 文脈規定, 言い換え, 用法 |
| 文法 | 10 q: 文の文法1 ×5, ★ order ×2, 文章の文法 ×3 | same | same | same |
| 読解 | 4 q: short, mid ×2, info search | 6 q: adds long ×2 | 6 q: short, mid ×2, 統合, 主張, info | 6 q: one of each |
| 聴解 | 5 q: 課題, ポイント, 発話表現, 即時応答 ×2 | 6 q: adds 概要 | 6 q: 課題, ポイント, 概要, 即時応答 ×2, 統合 | as N2 |

Each 問題 type keeps the real test's order and printed Japanese instructions; N5/N4 sentences have spaces between phrases like the real booklet. The time limit keeps roughly the real test's pace per question (`plan()` in `jlpt.rs`; e.g. N5 文字・語彙 10 q in 6:00).

## Rotation

N5/N4: 文字・語彙 → 文法 → 読解 → 聴解 → repeat. From N3 up reading is a third of the score on its own, so it comes twice: 文字・語彙 → 読解 → 聴解 → 文法 → 読解. Finishing the section that is due moves the rotation on (`app_settings` `jlpt_rotation:<level>`, `jlpt_cycle:<level>`); any section can be practised out of turn.

## Exam rules (time pressure)

- 3 · 2 · 1, then 「はじめてください」. The clock runs from the stored start time and never pauses, also in the background.
- Amber at 1:00, red at 0:30, the timer shakes in the last 10 s. At 0:00 「時間です。やめてください。」 and the session is handed in.
- No feedback until the end. Questions can be skipped, flagged and revisited from the answer sheet.
- Leaving ends the session and scores what was answered. A session left by closing the app shows as unfinished with Resume.
- Listening: each recording plays once. With the **offline Japanese voice** downloaded (Settings, about 110 MB once) it is spoken on the device by VOICEVOX (VOICEVOX Nemo: a man's voice for 男 lines, a woman's for 女 lines, another woman's for narration, read slower and flatter like a test announcer; the narrator's question is asked again after the dialog; longer pauses around narration); otherwise the device's `speechSynthesis`, otherwise the script is shown. The listening briefing links to the download when it's missing. In 発話表現 and 即時応答 the choices are spoken only; just their numbers are printed.

## Questions

The AI writes each 問題 type in its own request (`part_prompt`), all at once, while the learner reads the briefing. `clean_part` drops questions without the right number of distinct options, a valid answer, or the underline / ★ / script / passage their type needs, then shuffles the options. Words tested in recent sessions are avoided. Answers and explanations (in the explanation language) stay in Rust until hand-in. A session written but never started is reused next time.

## Results

Score with a teacher's stamp (たいへんよくできました ≥ 90%, よくできました ≥ 70%, がんばりましょう), time used, pace against the real pace, score per 問題 type, every question with answer and explanation, missed vocabulary → "Add to cards" (`db_upsert_review_item`), and the next section in the rotation. Confetti at ≥ 70%.

The page also shows an estimated score sheet on the real scale once every section has a result: N5/N4 Language knowledge · Reading /120 (min 38) + Listening /60 (min 19), pass 80 (N5) / 90 (N4); N3+ three parts /60 each (min 19), pass 95 / 90 / 100. A rough guide, not the official scaled score.

## History and stats

Every session is kept (`jlpt_sessions`). The hub shows the latest three under **History**, and "See all" opens the full list for the level. Each row has the date, section, score, time used, and a "time ran out" mark. Tapping one reopens its full review: every question with your answer, the right answer and the explanation, plus "Add to cards" for missed words. Back returns to where it was opened from.

The stats dashboard (`/stats`) has a **JLPT practice** card for the chosen period:
- Sessions, and the share handed in before time ran out.
- Accuracy per section, and a per-day accuracy chart with one line per section.
- The estimated score for every level that has a result in each section.

JLPT sessions also count in "Total sessions".

All animations are off under `prefers-reduced-motion`.
