import { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { CheckCircleFilled, LoadingOutlined, SoundOutlined } from '@ant-design/icons'
import { t, errorText, formatNumber } from '../lib/i18n'
import { TTS_CREDIT, playOffline, ttsStatus, type TtsProgress, type TtsStatus } from '../lib/tts'

const SAMPLE = ['こんにちは。きょうは いい 天気ですね。']

/** Settings → Offline Japanese voice: download, try, remove. Used by the JLPT listening section. */
export default function OfflineVoiceSettings() {
  const [status, setStatus] = useState<TtsStatus | null>(null)
  const [progress, setProgress] = useState<TtsProgress | null>(null)
  const [busy, setBusy] = useState<'' | 'install' | 'remove' | 'play'>('')
  const [error, setError] = useState('')
  const [confirmRemove, setConfirmRemove] = useState(false)
  const box = useRef<HTMLDivElement>(null)

  useEffect(() => {
    ttsStatus().then(setStatus).catch(() => setStatus(null))
    const unlisten = listen<TtsProgress>('tts-progress', (e) => setProgress(e.payload))
    // Opened from the JLPT briefing: bring the section into view.
    if (window.location.hash === '#offline-voice') setTimeout(() => box.current?.scrollIntoView({ behavior: 'smooth', block: 'center' }), 300)
    return () => { unlisten.then((fn) => fn()) }
  }, [])

  async function install() {
    setBusy('install')
    setError('')
    setProgress(null)
    try {
      setStatus(await invoke<TtsStatus>('tts_install'))
    } catch (e) {
      setError(errorText(e))
    } finally {
      setBusy('')
      setProgress(null)
    }
  }

  async function remove() {
    setBusy('remove')
    setConfirmRemove(false)
    try {
      setStatus(await invoke<TtsStatus>('tts_remove'))
    } catch (e) {
      setError(errorText(e))
    } finally {
      setBusy('')
    }
  }

  async function sample() {
    setBusy('play')
    setError('')
    try {
      await playOffline(SAMPLE)
    } catch (e) {
      setError(errorText(e))
    } finally {
      setBusy('')
    }
  }

  if (!status) return null
  const mb = formatNumber(Math.round(status.bytes / 1_048_576))

  return (
    <div className="space-y-2" id="offline-voice" ref={box}>
      <h3 className="text-sm font-medium text-gray-500 uppercase tracking-wide px-1">{t('voice.title')}</h3>
      <div className="bg-white rounded-xl shadow-sm p-4 flex flex-col gap-3">
        <div className="flex items-start gap-3">
          <div className={`w-10 h-10 rounded-xl flex items-center justify-center shrink-0 ${status.installed ? 'bg-green-50 text-green-600' : 'bg-blue-50 text-blue-500'}`}>
            {status.installed ? <CheckCircleFilled /> : <SoundOutlined />}
          </div>
          <div className="flex-1 min-w-0">
            <div className="text-sm font-medium">{status.installed ? t('voice.installed', { mb }) : t('voice.notInstalled')}</div>
            <div className="text-xs text-gray-400">{status.supported ? t('voice.hint') : t('voice.unsupported')}</div>
          </div>
        </div>

        {busy === 'install' && (
          <div className="flex flex-col gap-1">
            <div className="flex justify-between text-xs text-gray-500">
              <span><LoadingOutlined className="mr-1.5" />{t(`voice.stage.${progress?.stage ?? 'voice'}`)}</span>
              {progress && progress.stage !== 'building' && progress.stage !== 'checking' && <span className="tabular-nums">{progress.percent}%</span>}
            </div>
            <div className="h-1.5 rounded-full bg-gray-100 overflow-hidden">
              <div
                className={`h-full rounded-full bg-blue-500 transition-[width] duration-300 ${progress?.stage === 'building' || progress?.stage === 'checking' ? 'animate-pulse w-full' : ''}`}
                style={progress?.stage === 'building' || progress?.stage === 'checking' ? undefined : { width: `${progress?.percent ?? 0}%` }}
              />
            </div>
          </div>
        )}

        {status.supported && (
          <div className="flex gap-2">
            {status.installed ? (
              <>
                <button className="flex-1 py-2.5 rounded-xl bg-blue-500 text-white text-sm font-medium active:scale-95 transition disabled:opacity-50" disabled={busy !== ''} onClick={sample}>
                  {busy === 'play' ? <LoadingOutlined /> : t('voice.sample')}
                </button>
                <button className="py-2.5 px-4 rounded-xl bg-gray-100 text-gray-700 text-sm font-medium active:scale-95 transition disabled:opacity-50" disabled={busy !== ''} onClick={() => setConfirmRemove(true)}>
                  {t('voice.remove')}
                </button>
              </>
            ) : (
              <button className="flex-1 py-2.5 rounded-xl bg-blue-500 text-white text-sm font-medium active:scale-95 transition disabled:opacity-50" disabled={busy !== ''} onClick={install}>
                {busy === 'install' ? t('voice.downloading') : t('voice.download')}
              </button>
            )}
          </div>
        )}

        {confirmRemove && (
          <div className="bg-gray-50 rounded-xl p-3 flex items-center gap-2 text-sm">
            <span className="flex-1">{t('voice.removeConfirm', { mb })}</span>
            <button className="px-3 py-1.5 rounded-lg text-gray-600" onClick={() => setConfirmRemove(false)}>{t('common.cancel')}</button>
            <button className="px-3 py-1.5 rounded-lg bg-red-500 text-white" onClick={remove}>{t('voice.remove')}</button>
          </div>
        )}

        {error && <p className="text-xs text-red-500">{error}</p>}
        <p className="text-[11px] text-gray-400">{t('voice.credit')} {TTS_CREDIT}</p>
      </div>
    </div>
  )
}
