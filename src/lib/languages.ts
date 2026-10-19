import { lang, languageName } from './i18n'

/** Languages the app teaches. */
const ALL_TARGETS = [
  { code: 'ja', flag: '🇯🇵' },
  { code: 'id', flag: '🇮🇩' },
]

/** Target languages the learner can pick: never the language they already read the app in. */
export const TARGET_LANGUAGES = ALL_TARGETS
  .filter((l) => l.code !== lang)
  .map((l) => ({ ...l, label: languageName(l.code) }))

export function languageLabel(code: string | null | undefined): string {
  if (!code) return ''
  const target = ALL_TARGETS.find((l) => l.code === code)
  return target ? `${target.flag} ${languageName(code)}` : code
}
