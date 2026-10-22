export type ReviewItem = {
  id: string
  content: string
  EF: number
  I: number
  R: number
  state: string
  step: number
  lapse_count: number
  next_review: number | null
  language: string | null
  detail: string | null
  mnemonic: string | null
  // FSRS fields
  fsrs_stability?: number | null
  fsrs_difficulty?: number | null
  // Pre-computed display fields from backend
  difficulty_bg_class: string
  difficulty_label: string
  state_label: string
  next_review_formatted: string
  item_type: 'word' | 'grammar'
  /** Grammar cards: the pattern's level */
  level: string | null
  /** Grammar card below the learner's level, waiting for its level check */
  placement: boolean
}

/** Anki-style grade button */
export type Grade = 'again' | 'hard' | 'good' | 'easy'

/** Card lifecycle state */
export type CardState = 'new' | 'learning' | 'review' | 'relearning'

/** Question kinds for auto-graded review (see src-tauri/src/quiz.rs) */
export type QuizKind = 'classic' | 'needs_meaning' | 'intro_tf' | 'true_false' | 'choice' | 'recall_confirm' | 'reading' | 'context'
  | 'grammar_meaning' | 'grammar_gap' | 'grammar_mistake'

/** What a grammar card teaches (see src-tauri/src/quiz.rs `GrammarInfo`) */
export type GrammarInfo = {
  level: string
  meaning: string
  form: string
  note: string
  /** Only sent where they give nothing away (intro, true/false, a draft) */
  examples: { sentence: string; target: string; translation: string }[]
}

/** Back of a card, as saved in `review_items.detail` */
export type CardDetail = {
  reading: string
  meaning: string
  part_of_speech: string
  example: string
  example_meaning: string
}

/** Value sent for "None of these" in recall → confirm */
export const NONE_ANSWER = '__none'

export type QuizOption = { text: string; item_id: string | null }

export type QuizTask = {
  kind: QuizKind
  word: string
  reading: string
  meaning: string
  options: QuizOption[]
  statement: QuizOption | null
  sentence: { before: string; target: string; after: string; translation: string } | null
  hint_available: boolean
  mature: boolean
  /** needs_meaning: AI-generated back waiting for approval */
  draft: CardDetail | null
  /** Grammar cards only */
  grammar: GrammarInfo | null
  /** Level check: a grammar pattern below the learner's level, asked once */
  placement: boolean
}

export type QuizAnswer = {
  dont_know: boolean
  choice?: string
  tf?: boolean
  elapsed_ms: number
  hint_used: boolean
}

export type GradePreview = {
  grade: string
  label: string
  next_minutes: number
  next_review_formatted: string
  lapsed: boolean
  graduated: boolean
}

/** Counters plus the next card, returned after every session step */
export type SessionProgress = {
  reviews_completed: number
  new_cards_seen: number
  remaining_count: number
  /** Cards in today's queue (words and grammar) and how many no longer wait */
  queue_total: number
  queue_done: number
  /** New grammar and level checks in today's queue: answered today / total */
  grammar_done: number
  grammar_total: number
  session_done: boolean
  next_card: ReviewItem | null
  next_task: QuizTask | null
  grade_options: GradePreview[]
}

export type AnswerVerdict = {
  answer_id: number
  grade: Grade
  label: string
  correct: boolean
  dont_know: boolean
  correct_answer: string
  /** Recall → confirm where "None of these" was right */
  none_correct: boolean
  chosen: string | null
  reasons: GradeReason[]
  next_review_formatted: string
  next_minutes: number
  can_override: boolean
  overridden: boolean
  confused_with: { word: string; reading: string; meaning: string } | null
}

/** Why a card got its grade (code from quiz.rs `compute_grade`, translated by the frontend) */
export type GradeReason = { code: string; ms?: number }

export type AnswerResult = SessionProgress & {
  verdict: AnswerVerdict
  card: ReviewItem | null
  graduated: boolean
  lapsed: boolean
}

export type SessionSummary = {
  total: number
  correct: number
  avg_ms: number | null
  hints: number
  again: number
  hard: number
  good: number
  easy: number
  mature_total: number
  mature_retention: number | null
  mixed_up: { word: string; other: string; count: number }[]
  reading_slips: { word: string; reading: string; picked: string }[]
  context_misses: { word: string; other: string; count: number }[]
  /** Answers today on word cards and on grammar cards */
  words: number
  grammar: number
}

export type ReadingText = {
  title: string
  body: string
  plain_body: string
  source: string
  language: string
  level: string
  /** Comes with an AI-written passage; empty for a news article */
  questions: ReadingQuestion[]
}

export type ReadingQuestion = {
  question: string
  answer_hint: string
}

export type AnswerEvaluation = {
  result: 'correct' | 'partial' | 'incorrect'
  feedback: string
}

export type SavedAnswer = {
  question_index: number
  user_answer: string
  result: string
  retries: number
  feedback?: string
}

export type AnswerStats = {
  answered: number
  correct: number
  first_try: number
  partial: number
  retries: number
}

export type ReadingOverview = {
  finished: number
  in_progress: number
  questions_answered: number
  first_try_pct: number | null
  resume: ReadingSessionSummary | null
}

export type ReadingSessionSummary = {
  id: number
  language: string
  level: string
  title: string
  source: string
  phase: string
  current_q: number
  total_questions: number
  created_at: string
  stats: AnswerStats
}

export type ReadingSession = {
  id: number
  language: string
  level: string
  title: string
  body: string
  plain_body: string
  source: string
  questions: ReadingQuestion[]
  answers: SavedAnswer[]
  phase: string
  current_q: number
  created_at: string
  elapsed_seconds: number
  stats: AnswerStats
}

export type GrammarItem = {
  id: string
  pattern: string
  detail: string
  language: string
  level: string
  is_default: number
  /** Language the explanation is written in */
  explain_lang: string
  ai_translated: boolean
  /** English row shown because the level has nothing in the learner's language yet */
  fallback: boolean
  /** The row this one was translated from */
  source_id: string | null
  /** Where the pattern stands in grammar review */
  review: GrammarReview | null
}

/** Status codes from src-tauri/src/grammar_review.rs `review_statuses` */
export type GrammarReviewStatus = 'known' | 'learning' | 'new' | 'check' | 'coming' | 'coming_check' | 'not_planned'

export type GrammarReview = { status: GrammarReviewStatus; can_add: boolean }

export type GrammarLevelProgress = {
  level: string
  role: 'below' | 'current' | 'above' | 'other'
  total: number
  known: number
  learning: number
  pct: number
}

/** The learner's grammar level and daily grammar limits (src-tauri/src/grammar_review.rs) */
export type GrammarPlan = {
  language: string
  level: string | null
  levels: GrammarLevelProgress[]
  new_per_day: number
  checks_per_day: number
  speed_up: boolean
  checks_today: number
  to_introduce: number
  to_check: number
  days_to_introduce: number | null
  days_to_check: number | null
  move_up_to: string | null
}

export function parseDetailJson(detail: string): {
  reading?: string; meaning?: string; part_of_speech?: string; example?: string; example_meaning?: string
} | null {
  try {
    const match = detail.match(/\{[\s\S]*\}/)
    if (!match) return null
    return JSON.parse(match[0])
  } catch {
    return null
  }
}
/** Chunk roles of a sentence breakdown (see src-tauri/src/breakdown.rs) */
export type BreakdownRole =
  | 'topic' | 'subject' | 'object' | 'modifier' | 'purpose' | 'reason' | 'condition'
  | 'time' | 'place' | 'connector' | 'predicate' | 'other'

export type BreakdownChunk = { text: string; role: BreakdownRole; label: string; meaning: string; implied: boolean }

export type BreakdownSection = 'chunks' | 'core' | 'layers' | 'order' | 'notes' | 'gloss' | 'check'

/** Each section comes from its own request and arrives on its own */
export type BreakdownParts = {
  chunks: { chunks: BreakdownChunk[]; chunks_match: boolean }
  core: { translation: string; core: string; core_note: string; tip: string; structures: string[] }
  /** Nesting from the main clause inwards */
  layers: { layers: { text: string; meaning: string }[] }
  order: {
    target: { text: string; role: BreakdownRole }[]
    native: { text: string; role: BreakdownRole }[]
    note: string
  }
  notes: { notes: { text: string; note: string }[] }
  gloss: { gloss: { text: string; reading: string; meaning: string }[] }
  check: { question: string; options: string[] }
}

export type SentenceBreakdown = {
  id: number
  sentence: string
  language: string
  parts: Partial<BreakdownParts>
  /** Sections not generated yet */
  missing: BreakdownSection[]
  checked: boolean | null
}

export type BreakdownSummary = {
  id: number
  sentence: string
  structures: string[]
  checked: boolean | null
  created_at: string
}

export type BreakdownCheckResult = { correct: boolean; answer: number; explanation: string }
