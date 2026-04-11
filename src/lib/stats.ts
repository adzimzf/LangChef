export type Day = { date: string; sessions: number }

/**
 * Get today's date key in local timezone as YYYY-MM-DD.
 * This must match the date format stored by the Rust backend.
 */
export function todayKey(): string {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

/**
 * Format a Date object as YYYY-MM-DD in local timezone.
 */
function localDateKey(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

/**
 * Calculate the current streak (consecutive days with sessions ending today or yesterday).
 * Walks backward from today/yesterday through sorted days.
 */
export function computeStreak(days: Day[]): number {
  const sorted = [...days].sort((a, b) => a.date.localeCompare(b.date))
  if (sorted.length === 0) return 0

  const today = todayKey()
  const yesterdayDate = new Date()
  yesterdayDate.setDate(yesterdayDate.getDate() - 1)
  const yesterday = localDateKey(yesterdayDate)

  let s = 0
  let prev: string | null = null
  for (let i = sorted.length - 1; i >= 0; i--) {
    const d = sorted[i].date
    if (prev == null) {
      // Start from today or yesterday (so streak doesn't break if you haven't chatted today yet)
      if (d === today || d === yesterday) s++
      else break
    } else {
      const diff = (new Date(prev).getTime() - new Date(d).getTime()) / (24 * 60 * 60 * 1000)
      if (Math.abs(diff - 1) < 0.5) s++
      else break
    }
    prev = d
  }
  return s
}

/**
 * Get the last N days of activity data, filling in zeros for missing days.
 * Uses local timezone consistently for both key generation and display.
 */
export function getLastNDays(days: Day[], n: number): { day: string; count: number }[] {
  const result: { day: string; count: number }[] = []
  for (let i = n - 1; i >= 0; i--) {
    const d = new Date()
    d.setDate(d.getDate() - i)
    const key = localDateKey(d)
    const dayName = d.toLocaleDateString('en', { weekday: 'short' })
    const entry = days.find((dy) => dy.date === key)
    result.push({ day: dayName, count: entry?.sessions ?? 0 })
  }
  return result
}
