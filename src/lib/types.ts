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