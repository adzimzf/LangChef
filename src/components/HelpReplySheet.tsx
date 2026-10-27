import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { CloseOutlined, LoadingOutlined } from '@ant-design/icons'
import { renderFurigana } from '../lib/furigana'
import { t, lang as explainLang, errorText, languageName } from '../lib/i18n'
import { readFlag, ROMAJI_KEY } from '../lib/prefs'
import { useBackHandler } from '../lib/backNav'

type Api = { apiHost: string; apiKey: string; model: string }
type Suggestion = { reply: string; gloss: string; annotated: string; romaji: string }

/**
 * "Help me reply": the learner writes what they want to say in their own language and gets a
 * reply in the target language at their level, to put in the input box, edit and send.
 */
export default function HelpReplySheet({ language, levelRule, context, api, onUse, onClose }: {
  language: string
  levelRule: string | null
  context: { role: string; content: string }[]
  api: Api
  onUse: (text: string) => void
  onClose: () => void
}) {
  useBackHandler(true, onClose)
  const [intent, setIntent] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [suggestion, setSuggestion] = useState<Suggestion | null>(null)

  async function ask() {
    if (!intent.trim()) return
    setLoading(true)
    setError(null)
    try {
      const res = await invoke<{ reply: string; gloss: string }>('llm_help_reply', {
        apiHost: api.apiHost, apiKey: api.apiKey, model: api.model,
        intent, contextMessages: context, targetLang: language, levelRule,
      })
      const annotated = language === 'ja'
        ? await invoke<string>('add_furigana', { text: res.reply, language }).catch(() => res.reply)
        : res.reply
      const romaji = language === 'ja' && readFlag(ROMAJI_KEY)
        ? await invoke<string>('to_romaji', { text: res.reply }).catch(() => '')
        : ''
      setSuggestion({ ...res, annotated, romaji })
    } catch (e) {
      setError(errorText(e))
    }
    setLoading(false)
  }

  return (
    <div className="anim-backdrop fixed inset-0 z-50 flex items-end justify-center bg-black/40" onClick={onClose}>
      <div
        role="dialog"
        aria-label={t('chat.helpReply')}
        className="anim-sheet w-full max-w-lg bg-white rounded-t-2xl p-5 space-y-3 safe-bottom"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <h3 className="text-base font-semibold">{t('chat.helpReply')}</h3>
          <button type="button" onClick={onClose} aria-label={t('common.close')} className="w-8 h-8 flex items-center justify-center rounded-full hover:bg-gray-100">
            <CloseOutlined />
          </button>
        </div>
        <label className="block space-y-1">
          <span className="text-xs font-medium text-gray-500">{t('chat.helpIntentLabel', { language: languageName(explainLang) })}</span>
          <textarea
            value={intent}
            onChange={(e) => setIntent(e.target.value)}
            rows={2}
            placeholder={t('chat.helpIntentPlaceholder')}
            className="w-full px-3 py-2 rounded-xl bg-gray-50 text-[15px] outline-none focus:ring-2 focus:ring-blue-200 resize-none"
          />
        </label>
        {!suggestion && (
          <button
            type="button"
            onClick={ask}
            disabled={loading || !intent.trim()}
            className="w-full py-3 rounded-xl bg-blue-500 text-white font-medium disabled:opacity-50 flex items-center justify-center gap-2"
          >
            {loading && <LoadingOutlined spin />}
            {loading ? t('chat.helpThinking') : t('chat.helpAsk', { language: languageName(language) })}
          </button>
        )}
        {error && <div className="text-xs text-red-500">{error}</div>}
        {suggestion && (
          <>
            <div className="bg-gray-50 rounded-xl p-3 space-y-1">
              <div className="text-xs font-medium text-gray-500">{t('chat.helpSuggestion')}</div>
              <div className="text-lg leading-loose">{renderFurigana(suggestion.annotated, true)}</div>
              {suggestion.romaji && <div className="text-xs text-gray-500">{suggestion.romaji}</div>}
              <div className="text-sm text-gray-700">“{suggestion.gloss}”</div>
            </div>
            <div className="grid grid-cols-2 gap-2">
              <button type="button" onClick={() => setSuggestion(null)} className="py-3 rounded-xl bg-gray-100 text-gray-700 font-medium">{t('chat.helpAgain')}</button>
              <button type="button" onClick={() => onUse(suggestion.reply)} className="py-3 rounded-xl bg-blue-500 text-white font-medium">{t('chat.helpUse')}</button>
            </div>
            <p className="text-xs text-center text-gray-400">{t('chat.helpEditNote')}</p>
          </>
        )}
      </div>
    </div>
  )
}
