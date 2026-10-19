import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { CloseOutlined, LoadingOutlined, PlusOutlined, CheckOutlined } from '@ant-design/icons'
import type { CardDetail, ReviewItem } from '../lib/types'
import { parseDetailJson } from '../lib/types'
import { t, lang as explainLang, errorText, languageName, partOfSpeechLabel } from '../lib/i18n'
import { readFlag, ROMAJI_KEY } from '../lib/prefs'

type Api = { apiHost: string; apiKey: string; model: string }

/**
 * Bottom sheet for a word tapped in chat: its meaning in the learner's language (from the deck,
 * or made by the AI) and a button to add it to the review cards.
 */
export default function WordSheet({ word, base, language, api, onAdd, onClose }: {
  word: string
  /** Dictionary form used for lookup and for the card (Japanese verbs/adjectives). */
  base: string
  language: string
  api: Api
  onAdd: (content: string, detail: string | null) => Promise<void>
  onClose: () => void
}) {
  const [item, setItem] = useState<ReviewItem | null>(null)
  const [detail, setDetail] = useState<Partial<CardDetail> | null>(null)
  const [rawDetail, setRawDetail] = useState<string | null>(null)
  const [romaji, setRomaji] = useState('')
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [adding, setAdding] = useState(false)
  const [added, setAdded] = useState(false)

  useEffect(() => {
    let cancelled = false
    async function load() {
      try {
        const found = await invoke<ReviewItem[]>('db_get_review_items_by_words', { words: [base, word] })
        const existing = found.find((i) => i.content === base) ?? found[0] ?? null
        if (cancelled) return
        setItem(existing)
        let text = existing?.detail ?? null
        if (!text && api.apiHost && api.apiKey) {
          text = await invoke<string>('db_generate_detail', {
            apiHost: api.apiHost, apiKey: api.apiKey, model: api.model, content: existing?.content ?? base, language,
          })
        }
        if (cancelled) return
        setRawDetail(text)
        const parsed = text ? parseDetailJson(text) : null
        setDetail(parsed)
        if (language === 'ja' && readFlag(ROMAJI_KEY)) {
          invoke<string>('to_romaji', { text: parsed?.reading || base }).then((r) => { if (!cancelled) setRomaji(r) }).catch(() => {})
        }
      } catch (e) {
        if (!cancelled) setError(errorText(e))
      }
      if (!cancelled) setLoading(false)
    }
    load()
    return () => { cancelled = true }
  }, [word, base, language])

  async function add() {
    setAdding(true)
    try {
      await onAdd(base, rawDetail)
      setAdded(true)
    } catch (e) {
      setError(errorText(e))
    }
    setAdding(false)
  }

  const inDeck = !!item || added

  return (
    <div className="fixed inset-0 z-50 flex items-end justify-center bg-black/40" onClick={onClose}>
      <div
        role="dialog"
        aria-label={t('chat.wordMeaning')}
        className="w-full max-w-lg bg-white rounded-t-2xl p-5 space-y-3 safe-bottom"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-2">
          <div>
            <div className="text-3xl font-semibold">{base}</div>
            {(detail?.reading || romaji) && (
              <div className="text-sm text-gray-500">{[detail?.reading, romaji].filter(Boolean).join(' · ')}</div>
            )}
            {base !== word && <div className="text-xs text-gray-400">{t('chat.inThisMessage', { word })}</div>}
          </div>
          <button type="button" onClick={onClose} aria-label={t('common.close')} className="w-8 h-8 flex items-center justify-center rounded-full hover:bg-gray-100">
            <CloseOutlined />
          </button>
        </div>

        {loading ? (
          <div className="flex items-center gap-2 text-sm text-gray-400 py-3"><LoadingOutlined spin /> {t('chat.lookingUp')}</div>
        ) : detail?.meaning ? (
          <div className="space-y-2">
            <div className="flex items-center gap-2 flex-wrap">
              <span className="text-xl font-semibold">{detail.meaning}</span>
              {detail.part_of_speech && (
                <span className="text-xs px-2 py-0.5 rounded-full bg-gray-100 text-gray-600">{partOfSpeechLabel(detail.part_of_speech)}</span>
              )}
            </div>
            {detail.example && (
              <div className="bg-gray-50 rounded-xl p-3 text-sm">
                <div>{detail.example}</div>
                {detail.example_meaning && <div className="text-xs text-gray-500 mt-0.5">{detail.example_meaning}</div>}
              </div>
            )}
            {!item?.detail && <div className="text-[11px] text-indigo-600">{t('common.aiMade')}</div>}
          </div>
        ) : (
          <div className="text-sm text-gray-500 py-2">
            {api.apiHost && api.apiKey ? t('quiz.noMeaningYet', { language: languageName(explainLang) }) : t('common.apiMissing')}
          </div>
        )}
        {error && <div className="text-xs text-red-500">{error}</div>}

        <button
          type="button"
          onClick={add}
          disabled={adding || inDeck}
          className="w-full flex items-center justify-center gap-2 py-3 rounded-xl bg-blue-500 text-white font-medium disabled:bg-gray-100 disabled:text-gray-500"
        >
          {adding ? <LoadingOutlined spin /> : inDeck ? <CheckOutlined /> : <PlusOutlined />}
          {inDeck ? t('chat.inYourCards') : t('chat.addToCards')}
        </button>
      </div>
    </div>
  )
}
