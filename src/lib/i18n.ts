/**
 * App text in the learner's explanation language ("Bahasa Anda").
 *
 * Plain module, no React context: the language is read once at startup from localStorage and a
 * change reloads the app (see `switchExplainLang`). Dictionaries are flat JSON files in
 * src/locales; English is the source of truth and `npm run i18n:check` keeps them in sync.
 */
import { invoke } from '@tauri-apps/api/core'
import en from '../locales/en.json'
import id from '../locales/id.json'

export type ExplainLang = 'en' | 'id'

const DICTS: Record<ExplainLang, Record<string, string>> = { en, id }

function readStored(): ExplainLang | null {
  try {
    const v = localStorage.getItem('explain_lang')
    return v === 'en' || v === 'id' ? v : null
  } catch {
    return null
  }
}

function detect(): ExplainLang {
  const stored = readStored()
  if (stored) return stored
  try {
    // Installs from before this setting existed keep English.
    if (localStorage.getItem('api_key') || localStorage.getItem('api_host')) return 'en'
  } catch { /* storage blocked */ }
  return typeof navigator !== 'undefined' && navigator.language?.toLowerCase().startsWith('id') ? 'id' : 'en'
}

export const lang: ExplainLang = detect()
export const locale = lang === 'id' ? 'id-ID' : 'en-US'
const dict = DICTS[lang]

type Params = Record<string, string | number>

/** Text for `key`, with `{name}` placeholders filled. A `count` param picks `key_one` / `key_other`. */
export function t(key: string, params?: Params): string {
  let k = key
  if (params && typeof params.count === 'number') {
    const plural = `${key}_${params.count === 1 ? 'one' : 'other'}`
    if (dict[plural] !== undefined || en[plural as keyof typeof en] !== undefined) k = plural
  }
  let s = dict[k] ?? (en as Record<string, string>)[k] ?? key
  if (params) {
    for (const [name, value] of Object.entries(params)) {
      s = s.split(`{${name}}`).join(String(value))
    }
  }
  return s
}

/** Mirrors the stored language into the database so Rust prompts and card meanings follow it. */
export function syncExplainLang(): Promise<void> {
  return invoke<void>('db_set_explain_lang', { lang }).catch(() => {})
}

/** Saves the new language for the whole app and reloads so every screen switches at once. */
export async function switchExplainLang(next: ExplainLang): Promise<void> {
  try {
    localStorage.setItem('explain_lang', next)
  } catch { /* storage blocked */ }
  await invoke('db_set_explain_lang', { lang: next })
  window.location.reload()
}

export function rememberExplainLang(next: ExplainLang) {
  try {
    localStorage.setItem('explain_lang', next)
  } catch { /* storage blocked */ }
}

// ---------------------------------------------------------------------------
// Errors from Rust: either {"code","detail"} JSON (see locale.rs) or a raw message.
// ---------------------------------------------------------------------------

export type AppError = { code: string; detail: string }

export function parseError(e: unknown): AppError {
  const raw = e instanceof Error ? e.message : String(e)
  try {
    const v = JSON.parse(raw)
    if (v && typeof v.code === 'string') return { code: v.code, detail: String(v.detail ?? '') }
  } catch { /* not JSON */ }
  return { code: 'generic', detail: raw }
}

/** Message for the learner, in their language. */
export function errorText(e: unknown): string {
  const { code } = parseError(e)
  const key = `error.${code}`
  const text = t(key)
  return text === key ? t('error.generic') : text
}

/** Message plus the raw technical detail, for places with room for both. */
export function errorWithDetail(e: unknown): string {
  const { detail } = parseError(e)
  const text = errorText(e)
  return detail ? `${text} (${t('error.detail')}: ${detail})` : text
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

const relative = new Intl.RelativeTimeFormat(locale, { numeric: 'auto', style: 'long' })

/** "in 3 days" / "3 hari lagi" for a number of minutes from now. */
export function formatInMinutes(minutes: number): string {
  if (minutes < 1) return t('time.lessThanMinute')
  if (minutes < 60) return relative.format(Math.round(minutes), 'minute')
  if (minutes < 24 * 60) return relative.format(Math.round(minutes / 60), 'hour')
  const days = Math.round(minutes / (24 * 60))
  if (days < 30) return relative.format(days, 'day')
  if (days < 365) return relative.format(Math.round(days / 30), 'month')
  return relative.format(Math.round(days / 365), 'year')
}

/** Due time of a card (`next_review` in unix seconds). */
export function formatNextReview(nextReview: number | null | undefined): string {
  if (nextReview == null) return t('time.now')
  const minutes = (nextReview * 1000 - Date.now()) / 60000
  if (minutes < 0) return t('time.overdue')
  return formatInMinutes(minutes)
}

/** "5 min ago" / "5 mnt lalu" for a unix-seconds timestamp in the past. */
export function formatAgo(tsSeconds: number): string {
  const minutes = (Date.now() - tsSeconds * 1000) / 60000
  if (minutes < 1) return t('time.justNow')
  if (minutes < 60) return relative.format(-Math.floor(minutes), 'minute')
  if (minutes < 24 * 60) return relative.format(-Math.floor(minutes / 60), 'hour')
  const days = Math.floor(minutes / (24 * 60))
  if (days < 7) return relative.format(-days, 'day')
  if (days < 30) return relative.format(-Math.floor(days / 7), 'week')
  if (days < 365) return relative.format(-Math.floor(days / 30), 'month')
  return relative.format(-Math.floor(days / 365), 'year')
}

export function formatDate(value: Date | number | string, options?: Intl.DateTimeFormatOptions): string {
  const d = value instanceof Date ? value : new Date(value)
  return new Intl.DateTimeFormat(locale, options ?? { day: 'numeric', month: 'short', year: 'numeric' }).format(d)
}

export function formatNumber(n: number, options?: Intl.NumberFormatOptions): string {
  return new Intl.NumberFormat(locale, options).format(n)
}

export function formatSeconds(ms: number): string {
  return t('time.seconds', { n: formatNumber(ms / 1000, { maximumFractionDigits: 1 }) })
}

/** Target language name ("Japanese" / "Jepang"). */
export function languageName(code: string | null | undefined): string {
  if (!code) return ''
  const key = `language.${code}`
  const text = t(key)
  return text === key ? code : text
}

/** Part-of-speech code from a card back, in the learner's language (free text passes through). */
export function partOfSpeechLabel(pos: string): string {
  const key = `pos.${pos.trim().toLowerCase().replace(/[\s-]+/g, '_')}`
  const text = t(key)
  return text === key ? pos : text
}
