// ─── SM-2 Spaced Repetition Scheduler ───────────────────────────────────────
// Implements the SM-2 algorithm with Anki-style card lifecycle:
//   New → Learning → Review → (lapse) → Relearning → Review
//
// Key features:
//   - Learning Steps (e.g. 1m 10m) for new cards
//   - Relearning Steps (e.g. 10m) for lapsed cards
//   - Graduating Interval for cards leaving Learning
//   - Easy Interval for cards graduating via "Easy"
//   - New Interval (% of previous) for lapsed Review cards (prevents Ease Hell)
//   - Easy Bonus multiplier
//   - Interval Modifier
//   - Minimum/Maximum interval caps
//   - Grace period (late review bonus)

// ─── Types ────────────────────────────────────────────────────────────────────

/** SM-2 quality rating (0-5 scale per SuperMemo) */
export type Quality = 0 | 1 | 2 | 3 | 4 | 5

/** Card lifecycle state */
export type CardState = 'new' | 'learning' | 'review' | 'relearning'

/** Anki-style grade button */
export type Grade = 'again' | 'hard' | 'good' | 'easy'

/** Mapping from Grade → Quality for each card state */
export const GRADE_QUALITY: Record<Grade, Quality> = {
  again: 1,
  hard: 3,
  good: 4,
  easy: 5,
}

/** A step definition in minutes (for learning/relearning steps) */
export type Step = number // minutes

/** Scheduling configuration — mirrors Anki deck options */
export interface SRSConfig {
  /** Steps for new cards in minutes, e.g. [1, 10] */
  learningSteps: Step[]
  /** Steps for lapsed cards in minutes, e.g. [10] */
  relearningSteps: Step[]
  /** Graduating interval in days (after completing learning steps with "Good") */
  graduatingInterval: number
  /** Easy interval in days (after completing learning steps with "Easy") */
  easyInterval: number
  /** New interval as a fraction of the previous interval on lapse (0–1).
   *  e.g. 0.5 means a 1-year interval becomes 6 months, not zero */
  newInterval: number
  /** Minimum interval in days after a lapse */
  minimumLapseInterval: number
  /** Easy bonus multiplier applied to the interval when Easy is pressed on a Review card */
  easyBonus: number
  /** Multiplier applied to all Review intervals (1.0 = no change) */
  intervalModifier: number
  /** Maximum interval in days (cap) */
  maximumInterval: number
  /** Grace period: extra days added if review is late and answer is correct.
   *  Formula: extra = daysLate * gracePeriodMultiplier */
  gracePeriodMultiplier: number
}

/** Default config matching Anki defaults */
export const DEFAULT_SRS_CONFIG: SRSConfig = {
  learningSteps: [1, 10],
  relearningSteps: [10],
  graduatingInterval: 1,
  easyInterval: 4,
  newInterval: 0.0,
  minimumLapseInterval: 1,
  easyBonus: 1.3,
  intervalModifier: 1.0,
  maximumInterval: 36500,
  gracePeriodMultiplier: 0.5,
}

/** A flashcard with full SM-2 state */
export interface Card {
  id: string
  content: string
  /** Easiness Factor — multiplier for interval growth, min 1.3 */
  EF: number
  /** Current interval in days (for Review cards) */
  I: number
  /** Repetition count (consecutive correct answers) */
  R: number
  /** Card lifecycle state */
  state: CardState
  /** Current step index within learning/relearning steps (0-based) */
  step: number
  /** Number of times this card has lapsed */
  lapseCount: number
  /** Timestamp (ms) of last review */
  lastReviewed?: number
  /** Timestamp (ms) of next scheduled review */
  nextReview?: number
  /** Grade history */
  history: Quality[]
  tags?: string[]
  language?: string
  detail?: string
}

// ─── Scheduling Result ────────────────────────────────────────────────────────

export interface ScheduleResult {
  card: Card
  /** Minutes until next review (for display in grade buttons) */
  nextMinutes: number
  /** Whether this was a lapse */
  lapsed: boolean
}

// ─── Core Algorithm ───────────────────────────────────────────────────────────

/**
 * Compute the new EF (Easiness Factor) from the current EF and quality.
 * Formula: EF' = EF + (0.1 - (5-q)*(0.08 + (5-q)*0.02))
 * Clamped to a minimum of 1.3.
 */
export function computeEF(EF: number, q: Quality): number {
  return Math.max(1.3, EF + (0.1 - (5 - q) * (0.08 + (5 - q) * 0.02)))
}

/**
 * Calculate the overdue bonus (grace period).
 * If a card is reviewed late and answered correctly, a bonus is added.
 */
function graceBonus(
  nextReview: number | undefined,
  now: number,
  multiplier: number
): number {
  if (!nextReview) return 0
  const dueMs = nextReview
  const daysLate = Math.max(0, (now - dueMs) / (24 * 60 * 60 * 1000))
  return daysLate * multiplier
}

/**
 * Schedule a card given the user's grade.
 *
 * This implements the full SM-2 with Anki-style lifecycle:
 *
 * NEW cards:
 *   - "Again": restart from step 0 of learning steps
 *   - "Hard": repeat current step (or first step)
 *   - "Good": advance one step; if past last step, graduate to Review
 *   - "Easy": immediately graduate to Review with easyInterval
 *
 * LEARNING cards:
 *   - Same as NEW but already in the learning steps
 *
 * REVIEW cards:
 *   - "Again": lapse → move to Relearning; interval = previous * newInterval (min minimumLapseInterval)
 *   - "Hard": interval = I * 1.2 * intervalModifier (hard multiplier)
 *   - "Good": interval = I * EF * intervalModifier (+ grace bonus)
 *   - "Easy": interval = I * EF * easyBonus * intervalModifier (+ grace bonus)
 *
 * RELEARNING cards:
 *   - "Again": restart relearning steps
 *   - "Hard"/"Good": advance through relearning steps; graduate when past last step
 *   - "Easy": immediately re-graduate with current interval
 */
export function scheduleCard(
  card: Card,
  grade: Grade,
  config: SRSConfig = DEFAULT_SRS_CONFIG,
  now: number = Date.now()
): ScheduleResult {
  const q = GRADE_QUALITY[grade]
  const newEF = computeEF(card.EF, q)
  let lapsed = false

  // Deep-clone card for mutation
  const updated: Card = {
    ...card,
    EF: newEF,
    lastReviewed: now,
    history: [...card.history, q],
  }

  let nextMinutes: number

  switch (card.state) {
    // ── NEW / LEARNING ──────────────────────────────────────────────────
    case 'new':
    case 'learning': {
      const steps = config.learningSteps

      if (grade === 'again') {
        // Restart from step 0
        updated.state = 'learning'
        updated.step = 0
        nextMinutes = steps[0]
      } else if (grade === 'hard') {
        // Repeat current step (minimum 1 step back or same step)
        updated.state = 'learning'
        updated.step = Math.max(0, card.step) // stay at current step or 0
        nextMinutes = steps[updated.step]
      } else if (grade === 'good') {
        // Advance one step
        const nextStep = card.step + 1
        if (nextStep >= steps.length) {
          // Graduate to Review
          updated.state = 'review'
          updated.step = 0
          updated.R = 1
          updated.I = config.graduatingInterval
          nextMinutes = config.graduatingInterval * 24 * 60
          updated.nextReview = now + config.graduatingInterval * 24 * 60 * 60 * 1000
        } else {
          updated.state = 'learning'
          updated.step = nextStep
          nextMinutes = steps[nextStep]
        }
      } else {
        // Easy: immediate graduation with easyInterval
        updated.state = 'review'
        updated.step = 0
        updated.R = 1
        updated.I = config.easyInterval
        nextMinutes = config.easyInterval * 24 * 60
        updated.nextReview = now + config.easyInterval * 24 * 60 * 60 * 1000
      }

      // For learning steps, nextReview is set to a short-term timestamp
      if (updated.state === 'learning') {
        updated.nextReview = now + nextMinutes * 60 * 1000
      }

      break
    }

    // ── REVIEW ──────────────────────────────────────────────────────────
    case 'review': {
      const bonus = graceBonus(card.nextReview, now, config.gracePeriodMultiplier)

      if (grade === 'again') {
        // Lapse
        lapsed = true
        updated.lapseCount = card.lapseCount + 1
        updated.R = 0
        // New interval = previous * newInterval, but at least minimumLapseInterval
        const lapseI = Math.max(
          config.minimumLapseInterval,
          Math.round(card.I * config.newInterval)
        )
        updated.I = lapseI
        updated.EF = newEF

        // Move to relearning if steps are defined
        if (config.relearningSteps.length > 0) {
          updated.state = 'relearning'
          updated.step = 0
          nextMinutes = config.relearningSteps[0]
          updated.nextReview = now + nextMinutes * 60 * 1000
        } else {
          // No relearning steps: just reschedule with the lapse interval
          updated.state = 'review'
          updated.step = 0
          updated.R = 0
          nextMinutes = lapseI * 24 * 60
          updated.nextReview = now + lapseI * 24 * 60 * 60 * 1000
        }
      } else {
        // Correct answer
        let newI: number

        if (grade === 'hard') {
          // Hard: interval * 1.2 * intervalModifier
          newI = Math.round(card.I * 1.2 * config.intervalModifier)
        } else if (grade === 'good') {
          // Good: interval * EF * intervalModifier + grace bonus
          newI = Math.round((card.I * newEF + bonus) * config.intervalModifier)
        } else {
          // Easy: interval * EF * easyBonus * intervalModifier + grace bonus
          newI = Math.round((card.I * newEF * config.easyBonus + bonus) * config.intervalModifier)
        }

        // Enforce minimum growth of 1 day
        newI = Math.max(card.I + 1, newI)
        // Cap at maximum interval
        newI = Math.min(config.maximumInterval, newI)

        updated.I = newI
        updated.R = card.R + 1
        updated.state = 'review'
        nextMinutes = newI * 24 * 60
        updated.nextReview = now + newI * 24 * 60 * 60 * 1000
      }

      break
    }

    // ── RELEARNING ──────────────────────────────────────────────────────
    case 'relearning': {
      const steps = config.relearningSteps

      if (grade === 'again') {
        // Restart relearning from step 0
        updated.state = 'relearning'
        updated.step = 0
        nextMinutes = steps[0]
        updated.nextReview = now + nextMinutes * 60 * 1000
      } else if (grade === 'hard') {
        // Repeat current step
        updated.state = 'relearning'
        updated.step = Math.max(0, card.step)
        nextMinutes = steps[updated.step]
        updated.nextReview = now + nextMinutes * 60 * 1000
      } else if (grade === 'good') {
        // Advance one step
        const nextStep = card.step + 1
        if (nextStep >= steps.length) {
          // Re-graduate to Review with the lapse interval
          updated.state = 'review'
          updated.step = 0
          updated.R = 1
          // I was already set during the lapse, keep it
          nextMinutes = updated.I * 24 * 60
          updated.nextReview = now + updated.I * 24 * 60 * 60 * 1000
        } else {
          updated.state = 'relearning'
          updated.step = nextStep
          nextMinutes = steps[nextStep]
          updated.nextReview = now + nextMinutes * 60 * 1000
        }
      } else {
        // Easy: immediately re-graduate to Review
        updated.state = 'review'
        updated.step = 0
        updated.R = 1
        // Give a slightly boosted interval for easy re-graduation
        const boostedI = Math.max(1, Math.round(updated.I * 1.3))
        updated.I = boostedI
        nextMinutes = boostedI * 24 * 60
        updated.nextReview = now + boostedI * 24 * 60 * 60 * 1000
      }

      break
    }
  }

  return { card: updated, nextMinutes, lapsed }
}

/**
 * Preview the next interval for each grade button.
 * Returns minutes for display purposes.
 */
export function previewGrades(
  card: Card,
  config: SRSConfig = DEFAULT_SRS_CONFIG
): Record<Grade, number> {
  const now = Date.now()
  return {
    again: scheduleCard(card, 'again', config, now).nextMinutes,
    hard: scheduleCard(card, 'hard', config, now).nextMinutes,
    good: scheduleCard(card, 'good', config, now).nextMinutes,
    easy: scheduleCard(card, 'easy', config, now).nextMinutes,
  }
}

// ─── Card Factory ─────────────────────────────────────────────────────────────

export function createCard(text: string, language?: string): Card {
  return {
    id: crypto.randomUUID(),
    content: text,
    EF: 2.5,
    I: 1,
    R: 0,
    state: 'new',
    step: 0,
    lapseCount: 0,
    history: [],
    language,
  }
}

// ─── Display Helpers ──────────────────────────────────────────────────────────

/**
 * Format next review time for display in grade buttons.
 */
export function formatNextReview(minutes: number): string {
  if (minutes <= 0) return 'soon'
  if (minutes < 60) return `${Math.round(minutes)}m`
  if (minutes < 24 * 60) {
    const hours = Math.round(minutes / 60)
    return `${hours}h`
  }
  const days = Math.round(minutes / (24 * 60))
  if (days === 1) return '1d'
  if (days < 7) return `${days}d`
  if (days < 30) return `${Math.round(days / 7)}w`
  if (days < 365) return `${Math.round(days / 30)}mo`
  return `${Math.round(days / 365)}y`
}

/**
 * Maps an SRS difficulty level (based on EF and R) to a color from red (hard) to green (easy).
 * EF ranges from 1.3 (hardest) to ~3.0+ (easiest).
 * R (repetition count) also contributes: higher R = more mastered.
 * Returns a CSS color string.
 */
export function difficultyColor(EF: number, R: number): string {
  // Normalize: combine EF and R into a 0-1 scale where 0 = hardest, 1 = easiest
  // EF: 1.3 (hard) → 3.0 (easy), normalize to 0-1
  const efNorm = Math.min(1, Math.max(0, (EF - 1.3) / (3.0 - 1.3)))
  // R: 0 (new/hard) → 5+ (easy), normalize to 0-1
  const rNorm = Math.min(1, Math.max(0, R / 5))
  // Combine with weights
  const ease = 0.6 * efNorm + 0.4 * rNorm

  // Interpolate from red (hue 0) to green (hue 120)
  const hue = ease * 120
  return `hsl(${hue}, 70%, 50%)`
}

/**
 * Returns a Tailwind-compatible bg class based on difficulty.
 * Uses discrete steps for Tailwind compatibility.
 */
export function difficultyBgClass(EF: number, R: number): string {
  const ease = 0.6 * Math.min(1, Math.max(0, (EF - 1.3) / (3.0 - 1.3)))
    + 0.4 * Math.min(1, Math.max(0, R / 5))

  if (ease < 0.2) return 'bg-red-500'
  if (ease < 0.4) return 'bg-orange-500'
  if (ease < 0.5) return 'bg-amber-500'
  if (ease < 0.6) return 'bg-yellow-500'
  if (ease < 0.75) return 'bg-lime-500'
  if (ease < 0.9) return 'bg-green-500'
  return 'bg-emerald-500'
}

/**
 * Get a human-readable label for a card state.
 */
export function stateLabel(state: CardState): string {
  switch (state) {
    case 'new': return 'New'
    case 'learning': return 'Learning'
    case 'review': return 'Review'
    case 'relearning': return 'Relearning'
  }
}
