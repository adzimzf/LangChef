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
}

/** Anki-style grade button */
export type Grade = 'again' | 'hard' | 'good' | 'easy'

/** Card lifecycle state */
export type CardState = 'new' | 'learning' | 'review' | 'relearning'

/** Question kinds for auto-graded review (see src-tauri/src/quiz.rs) */
export type QuizKind = 'classic' | 'needs_meaning' | 'intro_tf' | 'true_false' | 'choice' | 'recall_confirm' | 'reading' | 'context'

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
}

export type ReadingText = {
  title: string
  body: string
  plain_body: string
  source: string
  language: string
  level: string
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