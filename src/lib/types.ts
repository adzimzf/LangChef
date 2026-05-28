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
  difficulty_color: string
  difficulty_bg_class: string
  difficulty_label: string
  state_label: string
  next_review_formatted: string
}

/** Anki-style grade button */
export type Grade = 'again' | 'hard' | 'good' | 'easy'

/** Card lifecycle state */
export type CardState = 'new' | 'learning' | 'review' | 'relearning'

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