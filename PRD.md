# LangChef PRD

## 1. Executive Summary
- Build a conversational language learning app that helps users master vocabulary and grammar through daily chat in the target language.
- The app uses an LLM-driven chat to encourage conversation, detects unknown/forgotten vocabulary and grammar, and schedules reviews via a spaced-repetition algorithm comparable to Anki (SM-2 variant).
- Each daily session blends 60% already-remembered items and 40% forgotten items to optimize reinforcement while addressing gaps.
- Users can configure their own LLM provider via API_HOST and API_KEY to control cost, privacy, and performance.

## 2. Problem Statement
- Who: Adult learners of foreign languages who struggle to consistently practice and retain vocabulary/grammar despite using traditional apps or memorization tools.
- What: Learners forget words and grammar quickly without daily contextual usage; review tools feel disconnected from real conversation. Friction exists switching between chat practice and separate flashcards.
- Why it matters: Without retention and confident usage, learners disengage. Integrating detection + review into an engaging chat loop increases daily practice, retention, and motivation.
- Evidence signals:
  - High drop-off in SRS tools due to rote repetition and lack of contextual usage.
  - Learners report forgetting words shortly after lessons; desire conversation-focused practice.
  - LLMs can provide fluent conversation and adaptive feedback, but most apps lack integrated retention planning.

## 3. Target Users & Personas
- Primary persona: Motivated adult self-learner (A2–B2 level) wanting daily conversation practice and structured reinforcement.
- Secondary persona: Busy professionals needing efficient practice windows (5–20 minutes/day) with measurable progress.
- JTBD:
  - Practice meaningful conversations in my target language every day.
  - Get immediate help when I don’t know a word or grammar point.
  - Retain what I’ve learned and fill gaps efficiently.

## 4. Strategic Context
- Business goals:
  - Increase daily active learning (DAU) and retention via engaging chat-based practice.
  - Differentiate by tightly integrating conversation with review scheduling.
- Competitive landscape:
  - Traditional SRS (Anki) strong for review, weak for contextual usage.
  - Chat-based apps provide practice but limited retention tracking and scheduling.
- Why now:
  - LLMs can generate realistic conversations and explanations.
  - Integrated SRS strengthens outcomes and stickiness.

## 5. Solution Overview
- A chat-first app in the target language that:
  - Encourages conversation on relevant topics.
  - Lets users flag unknown vocabulary/grammar via a button (“I don’t know”) or highlight mode.
  - Automatically detects candidate unknowns and forgotten items during chat.
  - Schedules review sessions with an SM-2-like algorithm, mixing 60% remembered and 40% forgotten items in each session.
  - Uses user-provided LLM API_HOST and API_KEY or a default provider if available.
- Core loop:
  1. Start daily conversation in target language.
  2. Unknowns highlighted; user requests explanations/translations/examples.
  3. Items added/updated in SRS queue.
  4. Review slice integrated into or appended after chat: 60% remembered + 40% forgotten.
  5. Track mastery and grammar coverage; adapt next session topics.

## 6. Success Metrics
- Primary:
  - Daily active learning sessions per user (≥ 4/week median).
  - Retention: Day 7 and Day 30 learner retention.
- Secondary:
  - Words mastered per week (stable EF and interval growth; see SRS model).
  - Grammar concepts mastered per week.
  - Average session length (5–20 minutes target band).
  - Review completion rate for scheduled items.
- Guardrails:
  - Conversation quality rating does not decline with review integration.
  - Time to first message < 10 seconds after app open.

## 7. User Stories & Requirements
- Epic hypothesis: Integrating conversational practice with adaptive SRS (SM-2) and a 60/40 remembered/forgotten mix will increase retention and motivation compared to separate chat and review experiences.
- Functional requirements:
  - Chat in target language with topic suggestions and adaptive prompts.
  - “I don’t know” button to flag words/phrases/grammar from messages.
  - Highlight mode to select tokens/phrases for explanation and review.
  - Automatic detection of candidate unknowns (e.g., out-of-vocabulary relative to learner model).
  - Explanations: definition, usage examples, grammatical notes; optional translation.
  - SRS scheduling using SM-2 variant with EF, R, I, quality q (0–5).
  - Session composition: 60% remembered items, 40% forgotten items.
  - LLM configuration screen for API_HOST and API_KEY; validation and test call.
  - Privacy settings and on-device storage configuration where applicable.
  - Progress tracking: streaks, mastery levels, grammar coverage map.

## 8. Out of Scope
- Real-time speech recognition and TTS beyond simple playback.
- Full course curricula; focus on conversation + review.
- Teacher marketplace, group chats, or live tutoring in MVP.

## 9. Dependencies & Risks
- Dependencies:
  - LLM provider with target language support via API_HOST/API_KEY (OpenAI-compatible APIs).
  - Tokenization for highlight/detection (language-aware).
  - Storage for items, sessions, and telemetry.
- Risks:
  - LLM inaccuracies in grammar explanations; mitigation: curated references and multi-sourced examples.
  - Over-flagging unknowns increases cognitive load; mitigation: throttle detection and user-controlled highlight mode.
  - Privacy concerns with chat content; mitigation: clear settings, local-only modes, and data minimization.
  - API provider variability; mitigation: connection tests, rate-limit handling, user notifications.

## 10. Open Questions
- Which languages are supported at launch (e.g., Spanish, French, Japanese, English as target)?
- Translation policy: native-language hints always or optional?
- Offline mode for review without LLM?
- Session pacing: embed reviews mid-chat vs. post-chat segment?
- Adaptive 60/40 mix based on learner fatigue?

## 11. Detailed Requirements

### 11.1 Conversation Experience
- Start screen suggests topics tailored to prior sessions and goals.
- Chat runs entirely in target language by default; hints appear on demand.
- Unknowns:
  - “I don’t know” button available per incoming message.
  - Highlight mode allows selecting tokens/phrases for help.
  - System suggests candidate unknowns based on learner model and recency.
- Help panel provides:
  - Definition and part of speech (for vocabulary).
  - Usage examples (2–3), one simple, one contextual to the current conversation.
  - Grammar rule explanation with minimal jargon and one contrastive example.
  - Optional translation toggle.
- Add to review queue with default quality rating based on user input:
  - Explicit “don’t know” → q=1–2
  - Hesitant/partial recall → q=3
  - Confident usage → q=4–5

### 11.2 SRS Algorithm (SM-2 Variant)
- Card fields: id, type (vocab/grammar), prompt, answer, examples, tags, EF (ease factor), I (interval days), R (repetitions), last_reviewed_at, next_due_at, deck (remembered/forgotten), language.
- Initialization: EF=2.5, R=0, I=0.
- Review update per quality q (0–5):
  - If q < 3: R=0, I=1.
  - Else:
    - R=R+1.
    - If R=1: I=1; if R=2: I=6; else: I=round(I_prev * EF).
  - EF update: EF = EF + (0.1 - (5 - q) * (0.08 + (5 - q) * 0.02)); enforce EF ≥ 1.3.
- Session composition rule:
  - Select items due today; if pool > session size, prioritize forgotten items (q<3 recently) until 40% of slice, then fill remaining 60% with remembered items (stable EF and next_due_at within window).
  - If forgotten items < 40%, fill remainder with due remembered items; if remembered items < 60%, pull near-due forgotten items.
  - Cap total items based on session length target (e.g., 8–15 items per 10–15 min).
- Grammar cards:
  - Include minimal pair examples and a correction checklist.
  - Track rule mastery separately; EF shared mechanics apply.

### 11.3 Detection & Item Creation
- Vocab detection:
  - Tokenize and compare against user’s known set; flag high-difficulty or unseen tokens.
  - Contextual frequency boosts candidate ranking.
- Grammar detection:
  - LLM classifies message patterns; maps to grammar taxonomy (tense, aspect, agreement, word order).
  - Confidence thresholds prevent noise; user confirms items before adding.

### 11.4 LLM Integration
- Provider config:
  - Settings screen for API_HOST and API_KEY.
  - Validate via test request; show latency and cost estimate.
  - Store keys securely; never log secrets.
- Conversation orchestration:
  - System prompts ensure target-language-only replies unless help requested.
  - Style controls: formality level, pace, topic switching.
- Guardrails:
  - Token limits per message; truncate with summary.
  - Safety filters for inappropriate content.

### 11.5 Data Model
- User: id, email, languages (target/native), level, goals, preferences, api_host, api_key (secure), consent flags.
- Conversation: id, user_id, messages[], topics[], created_at, duration.
- Item (Card): id, type, content fields, EF, I, R, quality history[], tags, language.
- ReviewSession: id, user_id, items[], composition_ratio (remembered/forgotten), started_at, completed_at, outcomes.
- Telemetry: daily_active, streak, session_length, mastery_progress, grammar_coverage.

### 11.6 Telemetry & Metrics
- Track:
  - Sessions per week, average length.
  - Review completion rate.
  - EF distribution changes and interval growth.
  - Grammar coverage progression.
  - Retention cohorts (D7, D30).

### 11.7 Privacy, Security, Compliance
- Key storage:
  - Secure storage for API_KEY; never expose in logs or UI.
  - Allow local-only mode for review sessions without LLM where feasible.
- Data minimization:
  - Only store necessary conversation excerpts to create items.
  - Provide export/delete controls.
- Compliance:
  - Respect regional data laws; clear consent for external LLM processing.

### 11.8 Localization & Accessibility
- Localize UI labels and help content where possible.
- Accessibility:
  - Keyboard-first highlight and “I don’t know” actions.
  - Readable fonts and clear contrast.

## 12. Release Plan
- MVP (Month 1):
  - Chat with target-language-only default and topic suggestions.
  - “I don’t know” button + highlight mode.
  - Item creation and SM-2 scheduling.
  - Session composition 60/40 remembered/forgotten.
  - LLM provider configuration (API_HOST, API_KEY) with validation.
  - Basic telemetry (sessions, streaks, review completion).
- v1 (Month 2–3):
  - Automatic detection of unknowns and grammar classification.
  - Grammar taxonomy and coverage map.
  - Enhanced explanations (contrastive examples).
  - Privacy controls and export/delete.
- v1.x:
  - Adaptive 60/40 based on fatigue/accuracy.
  - Offline review mode.
  - Additional languages and personalization.

## 13. Acceptance Criteria
- Conversation:
  - On app open, user can start a chat in target language within 10 seconds.
  - Topic suggestions visible; user can select or type custom topics.
  - LLM responds in target language unless help requested.
- Unknown handling:
  - “I don’t know” flags current message tokens; user selects items to add.
  - Highlight mode allows token/phrase selection; help panel shows definition, examples, grammar notes, optional translation.
  - Items can be added to SRS with default q values informed by user input.
- SRS & session composition:
  - Items update EF/I/R per SM-2 variant.
  - Review session composes items as 60% remembered, 40% forgotten, with due-date prioritization and caps for session length.
- LLM configuration:
  - Settings screen accepts API_HOST and API_KEY; test call returns status and latency.
  - Keys stored securely; validation error shown on failure; no secrets logged.
- Telemetry:
  - Dashboard shows streaks, sessions/week, review completion; grammar coverage (v1).

## 14. Example Gherkin Scenarios

```gherkin
Feature: Start Conversation
  Scenario: User opens app and starts chat
    Given the user has selected a target language
    When the user opens the app
    Then a topic suggestion list is shown
    And the user can start a conversation within 10 seconds
    And the assistant replies in the target language

Feature: Flag unknown vocabulary
  Scenario: User flags unknown word via "I don't know"
    Given a message containing unfamiliar tokens is displayed
    When the user taps "I don't know"
    Then candidate tokens are highlighted
    And the user can add selected tokens as vocab items to the SRS queue

Feature: Spaced repetition scheduling
  Scenario: Review updates EF and intervals
    Given an item with EF=2.5, R=0, I=0
    When the user reviews with quality q=4
    Then R increases to 1
    And I is set to 1
    And EF updates per SM-2 formula and remains ≥ 1.3
```

## 15. Analytics & Experimentation
- Baselines: instrument DAU, D7/D30 retention, words mastered/week, session length.
- A/B: embed reviews mid-chat vs. post-chat segment; test translation-on-demand vs. always-on hints.
- Cohorts: track outcomes by language, level, and provider.

## 16. Documentation & Handoffs
- Maintain taxonomy of grammar rules and tags.
- Provide developer docs for SRS and provider configuration.
- Keep PRD updated with discovery outcomes and decisions.

## 17. Detailed Flows

### 17.1 First-Time Setup Flow
- App open → splash initializes.
- Language selection → target and native language set.
- Level selection or quick placement (optional).
- LLM provider setup:
  - Enter API_HOST and API_KEY or choose default provider.
  - Validate credentials with test call; show status and latency.
  - Store keys securely; never log secrets.
- Onboarding tutorial:
  - Explain “I don’t know” button and highlight mode.
  - Show quick example of adding a word and scheduling review.
- Topic preferences:
  - Pick interests and difficulty; seed conversation prompts.
- Start first conversation in target language within 10 seconds.
- End-of-session summary:
  - Items added (vocab/grammar), upcoming reviews, streak start.

### 17.2 Daily Conversation Flow
- Home screen:
  - Streak and today’s goal; “Start practice” CTA.
  - Topic suggestions personalized from past sessions.
- Chat loop:
  - Assistant opens in target language with a prompt.
  - User replies; assistant adapts pace and complexity.
  - Unknown handling:
    - “I don’t know” reveals candidate tokens from last assistant/user message.
    - Highlight mode selects tokens/phrases; help panel shows definition, examples, grammar notes, optional translation.
    - Add items to SRS queue; assign initial quality q.
  - Auto-detection (v1):
    - System suggests potential unknowns; user confirms before adding.
- Inline micro-review or post-chat review:
  - Compose due items using 60% remembered and 40% forgotten.
  - Present prompts (recall/recognition or production); collect q (0–5).
  - Update EF/I/R and next_due_at.
- Session summary:
  - Words mastered, grammar progress, next review window, suggested topics.

### 17.3 Unknown Vocabulary Flow
- Trigger:
  - User taps “I don’t know” or selects in highlight mode.
- Tokenization:
  - Split message into tokens; rank by novelty/frequency/context.
- Selection:
  - User chooses tokens/phrases to add.
- Help panel:
  - Definition, part of speech, 2–3 examples (one contextual).
  - Optional translation toggle.
- Add to SRS:
  - Create vocab card; set q (default 1–2); EF/I/R initialized.
  - Offer quick practice prompt to reinforce immediately.

### 17.4 Grammar Help Flow
- Trigger:
  - User highlights a clause or requests grammar help.
- Classification:
  - LLM maps to grammar taxonomy (tense, aspect, agreement, word order).
- Explanation:
  - Short rule description + contrastive example.
- Add to SRS:
  - Create grammar card; examples + minimal pairs; q set by user confidence.
- Immediate check:
  - Prompt user to produce one sentence; adjust q if needed.

### 17.5 Review Composition Flow
- Build review slice:
  - Gather due items for today.
  - Prioritize forgotten items (recent q<3) until 40% of slice.
  - Fill remaining 60% with remembered items; backfill if pools are imbalanced.
  - Cap items based on session length target.
- Deliver review:
  - Interleave during chat (micro-review) or run post-chat block.
  - For grammar, include minimal pairs and targeted corrections.
- Update scheduling:
  - Record q; update EF/I/R; ensure EF ≥ 1.3; compute next_due_at.

### 17.6 LLM Provider Configuration Flow
- Settings → Providers:
  - Enter API_HOST and API_KEY; masked input fields.
  - Test call:
    - Show connectivity, latency, and model availability.
  - Error handling:
    - Invalid key/host → inline error, retry guidance.
    - Rate limit/timeouts → recommend alternate provider or pacing changes.
- Persistence:
  - Secure storage; no secrets in logs or analytics.

### 17.7 Telemetry & Progress Flow
- During session:
  - Track messages, items added, review outcomes, q distribution.
- After session:
  - Update streaks, sessions/week, completion rate, mastery progress.
- Dashboard:
  - Show grammar coverage and vocab growth trends.
- Experiments:
  - Assign A/B variants (mid-chat review vs. post-chat).

### 17.8 Error & Edge Case Flow
- Provider errors:
  - Retry with backoff; offer switch provider option.
- Long responses or token limits:
  - Summarize or truncate with “continue” control.
- No due items:
  - Pull near-due items or generate contextual practice prompts.
- Low connectivity:
  - Fallback to cached review-only mode (v1.x).

### 17.9 Privacy & Data Control Flow
- Consent:
  - Explain external LLM processing; opt-in choices.
- Minimization:
  - Store only necessary excerpts for item creation.
- User controls:
  - Export/delete conversation snippets and items.
- Modes:
  - Local-only review mode where feasible; clarify limitations.

### 17.10 Onboarding Tutorial Flow
- Step-through:
  - Demonstrate “I don’t know” and highlight selection on sample message.
  - Show adding a vocab item and scheduling its review.
- Practice:
  - Short guided chat with 2–3 prompts; user flags one unknown.
- Completion:
  - Confirmation modal; next steps and first real topic suggestion.
