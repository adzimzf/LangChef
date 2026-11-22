import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { ConfusedWord, ConfusionDiff, DiffPart } from '../lib/types'
import { errorText, parseError, t } from '../lib/i18n'
import { getApiConfig } from '../lib/providers'

type Word = { word: string; reading: string; meaning: string; parts: DiffPart[] }

/** Colors for parts, by which row they belong to: the right word (+) or the pick (−). */
const PART_TONE = {
  right: 'bg-blue-50 text-blue-900 border-blue-500',
  pick: 'bg-orange-50 text-orange-900 border-orange-500',
  same: 'bg-gray-100 text-gray-600 border-gray-200',
}

/** "Don't confuse": the asked word and the word picked instead as a diff, with the parts only one
 *  of them has highlighted (written by AI once per pair, saved by Rust) and how often the two were
 *  mixed up. Without a comparison (no AI, error) it still shows both words and the count. */
export function MixupDiff({ cardId, word, reading, meaning, other }: {
  cardId: string
  word: string
  reading: string
  meaning: string
  other: ConfusedWord
}) {
  const [diff, setDiff] = useState<ConfusionDiff | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<unknown>(null)
  const [picked, setPicked] = useState<string | null>(null)
  const api = getApiConfig()

  async function load(refresh: boolean) {
    setLoading(true)
    setError(null)
    setPicked(null)
    try {
      setDiff(await invoke<ConfusionDiff>('llm_confusion_diff', {
        apiHost: api.apiHost, apiKey: api.apiKey, model: api.model,
        cardId, otherId: other.id, refresh,
      }))
    } catch (e) {
      setError(e)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    setDiff(null)
    load(false)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cardId, other.id])

  const right: Word = diff?.words[0] ?? { word, reading, meaning, parts: [] }
  const pick: Word = diff?.words[1] ?? { word: other.word, reading: other.reading, meaning: other.meaning, parts: [] }
  const count = diff?.count ?? other.count
  const sameReading = diff?.kind === 'sound' || (!!reading && reading === other.reading)
  // Swapped order: the same kanji keeps the same color in both rows, so the swap shows.
  const orderTone = (text: string) => {
    const texts = right.parts.map(p => p.text)
    const i = texts.indexOf(text)
    return i === 0 ? PART_TONE.right : i === 1 ? PART_TONE.pick : PART_TONE.same
  }
  const toneOf = (p: DiffPart, row: 'right' | 'pick') =>
    diff?.kind === 'order' ? orderTone(p.text) : p.differs ? PART_TONE[row] : PART_TONE.same

  const info = (() => {
    if (!picked) return null
    const [row, idx] = picked.split(':')
    const w = row === 'right' ? right : pick
    const p = w.parts[Number(idx)]
    if (!p) return null
    const otherWord = row === 'right' ? pick : right
    const where = diff?.kind === 'order'
      ? t('mixup.inBoth')
      : p.differs ? t('mixup.onlyIn', { word: w.word }) : otherWord.parts.some(o => o.text === p.text) ? t('mixup.inBoth') : t('mixup.notTheClue')
    return { p, where, tone: toneOf(p, row as 'right' | 'pick') }
  })()

  const noAi = !!error && parseError(error).code === 'no_ai'

  const row = (w: Word, kind: 'right' | 'pick') => (
    <div className={`flex items-stretch ${kind === 'right' ? 'bg-green-50' : 'bg-red-50'}`}>
      <span className={`w-7 shrink-0 flex items-center justify-center text-lg font-bold ${kind === 'right' ? 'bg-green-100 text-green-700' : 'bg-red-100 text-red-700'}`}>
        {kind === 'right' ? '+' : '−'}
      </span>
      <div className="flex-1 min-w-0 flex flex-col gap-1.5 py-2.5 px-3">
        <div className="flex items-baseline gap-2 flex-wrap">
          <span className="text-xl font-bold">{w.word}</span>
          {w.reading && w.reading !== w.word && (
            <span className={`text-xs px-1.5 rounded ${sameReading ? 'bg-amber-100 text-amber-900 font-semibold' : 'text-gray-500'}`}>{w.reading}</span>
          )}
          <span className="text-[13px] text-gray-700 ml-auto">{w.meaning}</span>
        </div>
        {loading && !diff && (
          <div className="flex gap-1">
            <span className="w-11 h-11 rounded-lg bg-white/70 animate-pulse" />
            <span className="w-11 h-11 rounded-lg bg-white/70 animate-pulse" />
          </div>
        )}
        {w.parts.length > 0 && (
          <div className="flex items-center gap-1 flex-wrap">
            <span className="text-xs text-gray-500 mr-0.5">=</span>
            {w.parts.map((p, i) => {
              const key = `${kind}:${i}`
              return (
                <button
                  key={key}
                  type="button"
                  aria-label={p.meaning ? `${p.text}: ${p.meaning}` : p.text}
                  onClick={() => setPicked(picked === key ? null : key)}
                  className={`min-w-[2.75rem] min-h-[2.75rem] px-1.5 py-0.5 rounded-lg flex flex-col items-center justify-center border-[1.5px] transition ${toneOf(p, kind)} ${picked === key ? 'ring-2 ring-gray-900' : ''}`}
                >
                  <span className="text-lg font-semibold leading-tight">{p.text}</span>
                  {p.meaning && <span className="text-[10px] leading-tight max-w-[5rem] truncate">{p.meaning}</span>}
                </button>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )

  return (
    <div className="bg-white rounded-2xl border-[1.5px] border-gray-200 overflow-hidden text-left">
      <div className="flex items-center justify-between gap-2 px-3.5 py-2 bg-gray-50 border-b border-gray-200">
        <span className="text-[11px] font-bold tracking-wide uppercase text-gray-700">{t('mixup.title')}</span>
        <span className="flex items-center gap-1.5">
          {diff && <span className="text-[11px] text-gray-500">{t(`mixup.kind.${diff.kind}`)}</span>}
          <span className={`text-[11px] font-bold px-2 py-0.5 rounded-full ${count >= 3 ? 'bg-red-100 text-red-800' : 'bg-amber-100 text-amber-900'}`}>
            {t('mixup.count', { count })}
          </span>
        </span>
      </div>

      {row(pick, 'pick')}
      {row(right, 'right')}

      {info && (
        <div className={`flex items-center gap-3 px-3.5 py-2 border-t border-gray-200 anim-fade ${info.tone}`}>
          <span className="text-2xl font-bold">{info.p.text}</span>
          <div className="flex flex-col">
            {info.p.meaning && <span className="text-[13px] font-semibold">{info.p.meaning}</span>}
            <span className="text-xs">{info.where}</span>
          </div>
        </div>
      )}

      <div className="px-3.5 py-2.5 border-t border-gray-200 flex flex-col gap-1">
        {diff && <p className="text-[13px] font-medium leading-snug text-gray-900">{diff.note}</p>}
        {loading && !diff && <p className="text-xs text-gray-500">{t('mixup.loading')}</p>}
        {!loading && !diff && error != null && (
          <p className="text-xs text-gray-500">{noAi ? t('mixup.noAi') : errorText(error)}</p>
        )}
        <div className="flex items-center justify-between gap-2">
          {diff && diff.words.some(w => w.parts.length > 0) && <span className="text-[11px] text-gray-500">{t('mixup.tapPart')}</span>}
          {api.ready && !loading && (diff || !noAi) && (
            <button type="button" onClick={() => load(!!diff)} className="ml-auto py-1 text-[11px] text-gray-500 hover:text-gray-700">
              {diff ? t('mixup.regenerate') : t('mixup.retry')}
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
