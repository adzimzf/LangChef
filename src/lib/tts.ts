/**
 * Offline Japanese voice (src-tauri/src/tts.rs): speech made on the device with piper-plus, used by the JLPT
 * listening section. Downloaded from Settings; until then the device voice or the script is used instead.
 */
import { invoke } from '@tauri-apps/api/core'

export type TtsStatus = {
  /** This platform can run the offline voice. */
  supported: boolean
  /** Voice, dictionary and runtime are all downloaded. */
  installed: boolean
  /** Bytes used on the device. */
  bytes: number
}

/** Stage of `tts_install`, sent as the `tts-progress` event. */
export type TtsProgress = { stage: 'voice' | 'dictionary' | 'building' | 'runtime' | 'checking'; percent: number }

/** Credit the voice's terms ask for; names, so the same in every language. */
export const TTS_CREDIT = 'つくよみちゃんコーパス（CV.夢前黎） · piper-plus (ayousanz)'

export const ttsStatus = () => invoke<TtsStatus>('tts_status')

/** The recording playing now, so leaving a question can stop it. */
let current: HTMLAudioElement | null = null

export function stopOffline() {
  if (current) {
    current.pause()
    current = null
  }
}

/** Speaks a recording's lines (男：/女： marks keep their speaker) and resolves once it has finished playing. */
export async function playOffline(lines: string[]): Promise<void> {
  const wav = await invoke<ArrayBuffer>('tts_speak', { lines })
  stopOffline()
  const url = URL.createObjectURL(new Blob([wav], { type: 'audio/wav' }))
  const audio = new Audio(url)
  current = audio
  try {
    await new Promise<void>((resolve, reject) => {
      audio.onended = () => resolve()
      audio.onpause = () => resolve()
      audio.onerror = () => reject(new Error('audio playback failed'))
      audio.play().catch(reject)
    })
  } finally {
    if (current === audio) current = null
    URL.revokeObjectURL(url)
  }
}
