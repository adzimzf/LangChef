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
  tags: string | null
  language: string | null
  detail: string | null
  mnemonic: string | null
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