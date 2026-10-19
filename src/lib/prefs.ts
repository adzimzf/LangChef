/** On/off learning aids, stored per device. All apply to the whole app. */
export const CHAT_HINTS_KEY = 'chat_hints'
export const ROMAJI_KEY = 'romaji_enabled'
export const ROMAJI_INPUT_KEY = 'romaji_input'
/** Beginner reading option: answer comprehension questions in the explanation language. */
export const READING_ANSWER_NATIVE_KEY = 'reading_answer_native'

export function readFlag(key: string): boolean {
  try {
    return localStorage.getItem(key) === '1'
  } catch {
    return false
  }
}

export function writeFlag(key: string, value: boolean) {
  try {
    localStorage.setItem(key, value ? '1' : '0')
  } catch { /* storage blocked */ }
}
