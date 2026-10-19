# Persona: "I know every word, but not the sentence"

Who the sentence-comprehension feature is for, how that learner uses LangChef today, and how it should work once the feature exists. Companion to a future `PRDs/Sentence Breakdown.md`.

## Primary persona — Raka, the stuck intermediate

| | |
|---|---|
| **Age / place** | 27, Jakarta |
| **Work** | Backend developer, studies on the commute (Android) and after work (desktop) |
| **Native language** | Indonesian; reads English fine but thinks in Indonesian. App set to `explain_lang = id` |
| **Japanese level** | Passed N4, studying for N3 (July). ~1,400 cards in the deck, ~900 of them mature |
| **Daily routine** | 15–20 min review on the train, one chat session (N4) in the evening, a Reading text on weekends |
| **Motivation** | Read NHK Web Easy and manga raw; eventually a job interview in Japanese |

### The problem in Raka's words

> "Semua katanya saya tahu, tapi saya tetap tidak paham kalimatnya. Siapa yang melakukan apa?"
> *("I know all the words, but I still don't get the sentence. Who did what?")*

A typical sentence that stops Raka:

> 友達が昨日教えてくれた駅前のラーメン屋に行ったけど、もう閉まっていた。

Raka knows 友達, 昨日, 教える, くれる, 駅前, ラーメン屋, 行く, けど, もう, 閉まる. Still:

- **The modifier comes first.** Indonesian puts the description *after* the noun with *yang* ("kedai ramen **yang** kemarin diberi tahu teman"). Japanese puts the whole clause 友達が昨日教えてくれた *before* 駅前のラーメン屋. Raka reads 友達が… as the start of the main sentence and expects the friend to be the one who went.
- **Missing subjects.** Nobody is named as going (it's 私) or as being closed (it's the shop). Raka can't tell the subject switched after けど.
- **Particles do the work Indonesian word order does.** が inside the clause vs. the main sentence, に for destination, は vs. が in general.
- **Stacked endings.** 教えてくれた (teach + for me + past), 閉まっていた (state + past); harder ones like 行かせられたくなかった.

### What Raka does today

1. Taps each word in Chat → `WordSheet` confirms the meanings Raka already knew. No help.
2. Taps **Translate** → gets the Indonesian sentence and understands it, but can't see which Japanese part matches which Indonesian part. The next similar sentence is just as hard.
3. Sometimes taps **Analyze grammar** → gets a list of patterns (〜てくれる, 〜ている, けど). Useful as reference, but it doesn't say *what describes what* in this sentence.
4. In Reading, gets a comprehension question wrong and only learns the right answer, not which part of the sentence was misread.
5. Review drills single words, so the deck keeps growing while reading doesn't get easier.

### Goals

- Untangle a sentence **without** jumping straight to the full translation.
- See the structure in terms Raka already understands: Indonesian order, *yang*, *oleh*, *di-*.
- Practise the patterns that caused trouble until they read naturally.
- Feel progress in reading, not just in deck size.

### Frustrations

- Translate feels like cheating; Raka learns nothing from it.
- Grammar explanations use terms Raka never learned in school (連体修飾, "relative clause").
- Long sentences in N3 practice texts; Raka gives up and skims.
- No way to save "a sentence I didn't understand" to come back to.

### What success looks like for Raka

- Taps Translate less often per message month over month.
- Answers "who did what" questions on new sentences correctly on the first try.
- Reads an NHK Web Easy article with only a couple of breakdowns, not one per sentence.

## How it will be — Raka's journey with the feature

Steps 1–4 are built (`PRDs/Sentence Breakdown.md`); 5–7 are the next steps.

1. **Stuck on a sentence.** Raka copies the ramen sentence from a chat or an article, opens the **Urai** tab and pastes it.
2. **Pick the sentence.** A pasted paragraph is split into sentences; Raka taps the confusing one.
3. **The breakdown.** The sentence is split into chunks with their roles:

   ```
   [友達が昨日教えてくれた] ─describes→ [駅前のラーメン屋] に 行った けど、 [(店は)] もう 閉まっていた
    yang kemarin diberi tahu teman      kedai ramen depan stasiun   ke  (saya) pergi  tapi  (kedainya)  sudah tutup
   ```

   - The modifier clause is underlined, with an arrow to the noun it describes, and the note: *"Dalam bahasa Indonesia keterangan ini datang **setelah** kata benda dengan *yang*. Bahasa Jepang menaruhnya **sebelum**."*
   - Omitted subjects shown in grey brackets: (saya), (kedainya).
   - Each particle gets a one-line role: が = pelaku di dalam keterangan, に = tujuan.
   - Each stacked ending is split: 教え・て・くれ・た = mengajari · (sambung) · untuk saya · (lampau).
   - A word-for-word line under the natural translation, so each Japanese part maps to an Indonesian part.
4. **Check yourself.** One or two follow-up questions on the same sentence ("Apa yang sudah tutup?"). Correct → done.
5. **Save it.** **Simpan kalimat** adds the sentence as a sentence card, tagged with the structure that tripped Raka (`noun-modifier clause`, `subject switch`).
6. **Review later.** In Review, sentence cards ask structure questions (who did what, what does this clause describe, pick the right chunk order) and are graded automatically with the same FSRS rules as words. New sentences with the same structure come up in chat, since the tutor prompt knows which structures Raka struggles with.
7. **Weekly picture.** Stats shows which structures Raka now reads correctly and which still trip them, alongside the word stats.

## Secondary persona — Maya, English-explanation learner

29, product designer in Singapore, uses `explain_lang = en`, N3, same problem but with English as the comparison language: "describing clauses go before the noun, unlike English *that/which*". Same feature, same flow; only the comparison language changes. Confirms the breakdown must compare with the **explanation language's** structure, not with English for everyone.

## Not for — Dimas, the beginner

N5, 150 cards, doesn't know most of the words in a sentence. Dimas's problem is vocabulary; word taps, furigana and the existing review already cover it. The breakdown shouldn't be pushed on N5 learners by default, and shouldn't replace word lookup.

## Design implications

- **Compare with the learner's own language.** Explanations use `explain_lang` grammar (Indonesian *yang*, *di-*/*oleh*, word order), plain words over linguistic terms.
- **Structure first, translation last.** The full translation is still there, but the breakdown is built to make it unnecessary.
- **Guess before reveal** is optional but default-on: the miss tells the app which structure to train.
- **Chunking comes from Lindera** (`furigana.rs`), so the split is consistent and cheap; the AI adds roles, omitted subjects and explanations in the explanation language. Cache per message and language like grammar analysis.
- **Sentences are cards too.** Reuse `review_items` / FSRS and `quiz.rs` grading, with structure question types.
- **Available wherever Japanese text appears:** Chat, Reading, and a card's example sentence.
- **Measure:** Translate taps per message, first-try accuracy on structure questions, breakdowns per Reading article.
