import { useState, useEffect, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import {
  ApiOutlined,
  KeyOutlined,
  CheckCircleOutlined,
  ExclamationCircleOutlined,
  LoadingOutlined,
  SaveOutlined,
  ExperimentOutlined,
  InfoCircleOutlined,
  BookOutlined,
  MessageOutlined,
  CloseOutlined,
  DownOutlined,
  UpOutlined,
} from '@ant-design/icons'
import GoogleDriveBackup from '../components/GoogleDriveBackup'
import ProviderGuide from '../components/ProviderGuide'
import { t, lang, switchExplainLang, errorText, type ExplainLang } from '../lib/i18n'
import { PROVIDER_PRESETS, TRANSLATION_REPORT_URL, type ProviderPreset } from '../lib/providers'
import { readFlag, writeFlag, CHAT_HINTS_KEY, ROMAJI_KEY, ROMAJI_INPUT_KEY } from '../lib/prefs'
import GrammarPlanSettings from '../components/GrammarPlanSettings'

const MAX_REVIEWS_DEFAULT = 100
const MAX_REVIEWS_KEY = 'max_reviews_per_day'
const MAX_NEW_CARDS_DEFAULT = 20
const MAX_NEW_CARDS_KEY = 'max_new_cards_per_day'
const MAX_DIFFICULT_WORDS_DEFAULT = 10
const MAX_DIFFICULT_WORDS_KEY = 'max_difficult_words'
const MAX_KNOWN_WORDS_DEFAULT = 20
const MAX_KNOWN_WORDS_KEY = 'max_known_words'
const CHAT_CUSTOM_PROMPT_KEY = 'chat_custom_prompt'

export default function Settings() {
  const [apiHost, setApiHost] = useState('')
  const [apiKey, setApiKey] = useState('')
  const [model, setModel] = useState('')
  const [vocabModel, setVocabModel] = useState('')
  const [models, setModels] = useState<string[]>([])
  const [status, setStatus] = useState<'idle' | 'saving' | 'saved' | 'testing' | 'ok' | 'error'>('idle')
  const [maxReviews, setMaxReviews] = useState(String(MAX_REVIEWS_DEFAULT))
  const [maxNewCards, setMaxNewCards] = useState(String(MAX_NEW_CARDS_DEFAULT))
  const [maxDifficultWords, setMaxDifficultWords] = useState(String(MAX_DIFFICULT_WORDS_DEFAULT))
  const [maxKnownWords, setMaxKnownWords] = useState(String(MAX_KNOWN_WORDS_DEFAULT))
  const [chatCustomPrompt, setChatCustomPrompt] = useState('')
  const [dirty, setDirty] = useState(false)
  const [testError, setTestError] = useState('')
  const [confirmLang, setConfirmLang] = useState<ExplainLang | null>(null)
  const [switching, setSwitching] = useState(false)
  const [chatHints, setChatHints] = useState(() => readFlag(CHAT_HINTS_KEY))
  const [romaji, setRomaji] = useState(() => readFlag(ROMAJI_KEY))
  const [romajiInput, setRomajiInput] = useState(() => readFlag(ROMAJI_INPUT_KEY))
  const [kanaMessage, setKanaMessage] = useState('')
  const [aidsOpen, setAidsOpen] = useState(false)

  // Track the last-saved values to detect changes and enable cancel
  const savedValues = useRef({ apiHost: '', apiKey: '', model: '', vocabModel: '', maxReviews: String(MAX_REVIEWS_DEFAULT), maxNewCards: String(MAX_NEW_CARDS_DEFAULT), maxDifficultWords: String(MAX_DIFFICULT_WORDS_DEFAULT), maxKnownWords: String(MAX_KNOWN_WORDS_DEFAULT), chatCustomPrompt: '' })

  useEffect(() => {
    const host = localStorage.getItem('api_host') || ''
    const key = localStorage.getItem('api_key') || ''
    const savedModel = localStorage.getItem('api_model') || ''
    const savedVocabModel = localStorage.getItem('api_vocab_model') || ''
    const savedMax = localStorage.getItem(MAX_REVIEWS_KEY)
    const savedNewCards = localStorage.getItem(MAX_NEW_CARDS_KEY)
    const savedDiffWords = localStorage.getItem(MAX_DIFFICULT_WORDS_KEY)
    const savedKnownWords = localStorage.getItem(MAX_KNOWN_WORDS_KEY)
    const savedPrompt = localStorage.getItem(CHAT_CUSTOM_PROMPT_KEY) || ''
    const mr = savedMax || String(MAX_REVIEWS_DEFAULT)
    const mnc = savedNewCards || String(MAX_NEW_CARDS_DEFAULT)
    const mdw = savedDiffWords || String(MAX_DIFFICULT_WORDS_DEFAULT)
    const mkw = savedKnownWords || String(MAX_KNOWN_WORDS_DEFAULT)
    setApiHost(host)
    setApiKey(key)
    setModel(savedModel)
    setVocabModel(savedVocabModel)
    setMaxReviews(mr)
    setMaxNewCards(mnc)
    setMaxDifficultWords(mdw)
    setMaxKnownWords(mkw)
    setChatCustomPrompt(savedPrompt)
    savedValues.current = { apiHost: host, apiKey: key, model: savedModel, vocabModel: savedVocabModel, maxReviews: mr, maxNewCards: mnc, maxDifficultWords: mdw, maxKnownWords: mkw, chatCustomPrompt: savedPrompt }
  }, [])

  function save() {
    const parsedReviews = Math.max(1, parseInt(maxReviews, 10) || MAX_REVIEWS_DEFAULT)
    const parsedNewCards = Math.max(0, parseInt(maxNewCards, 10) || MAX_NEW_CARDS_DEFAULT)
    const parsedDiffWords = Math.max(1, parseInt(maxDifficultWords, 10) || MAX_DIFFICULT_WORDS_DEFAULT)
    const parsedKnownWords = Math.max(0, parseInt(maxKnownWords, 10) || MAX_KNOWN_WORDS_DEFAULT)
    localStorage.setItem('api_host', apiHost.trim())
    localStorage.setItem('api_key', apiKey.trim())
    localStorage.setItem('api_model', model.trim())
    localStorage.setItem('api_vocab_model', vocabModel.trim())
    localStorage.setItem(MAX_REVIEWS_KEY, String(parsedReviews))
    localStorage.setItem(MAX_NEW_CARDS_KEY, String(parsedNewCards))
    localStorage.setItem(MAX_DIFFICULT_WORDS_KEY, String(parsedDiffWords))
    localStorage.setItem(MAX_KNOWN_WORDS_KEY, String(parsedKnownWords))
    localStorage.setItem(CHAT_CUSTOM_PROMPT_KEY, chatCustomPrompt.trim())
    const mr = String(parsedReviews)
    const mnc = String(parsedNewCards)
    const mdw = String(parsedDiffWords)
    const mkw = String(parsedKnownWords)
    setMaxReviews(mr)
    setMaxNewCards(mnc)
    setMaxDifficultWords(mdw)
    setMaxKnownWords(mkw)
    setChatCustomPrompt(chatCustomPrompt.trim())
    savedValues.current = { apiHost: apiHost.trim(), apiKey: apiKey.trim(), model: model.trim(), vocabModel: vocabModel.trim(), maxReviews: mr, maxNewCards: mnc, maxDifficultWords: mdw, maxKnownWords: mkw, chatCustomPrompt: chatCustomPrompt.trim() }
    setDirty(false)
    setStatus('saved')
    setTimeout(() => setStatus('idle'), 2000)
  }

  function cancel() {
    setApiHost(savedValues.current.apiHost)
    setApiKey(savedValues.current.apiKey)
    setModel(savedValues.current.model)
    setVocabModel(savedValues.current.vocabModel)
    setMaxReviews(savedValues.current.maxReviews)
    setMaxNewCards(savedValues.current.maxNewCards)
    setMaxDifficultWords(savedValues.current.maxDifficultWords)
    setMaxKnownWords(savedValues.current.maxKnownWords)
    setChatCustomPrompt(savedValues.current.chatCustomPrompt)
    setDirty(false)
  }

  // Detect dirty state
  useEffect(() => {
    const sv = savedValues.current
    const isDirty =
      apiHost !== sv.apiHost ||
      apiKey !== sv.apiKey ||
      model !== sv.model ||
      vocabModel !== sv.vocabModel ||
      maxReviews !== sv.maxReviews ||
      maxNewCards !== sv.maxNewCards ||
      maxDifficultWords !== sv.maxDifficultWords ||
      maxKnownWords !== sv.maxKnownWords ||
      chatCustomPrompt !== sv.chatCustomPrompt
    setDirty(isDirty)
  }, [apiHost, apiKey, model, vocabModel, maxReviews, maxNewCards, maxDifficultWords, maxKnownWords, chatCustomPrompt])

  function applyPreset(preset: ProviderPreset) {
    setApiHost(preset.apiHost)
    if (!model) setModel(preset.defaultModel)
  }

  function toggle(key: string, value: boolean, set: (v: boolean) => void) {
    writeFlag(key, value)
    set(value)
  }

  async function seedKana(script: 'hiragana' | 'katakana') {
    try {
      const added = await invoke<number>('db_seed_kana_deck', { script })
      setKanaMessage(t('settings.kanaAdded', { count: added, script: t(`settings.${script}`) }))
    } catch (e) {
      setKanaMessage(errorText(e))
    }
  }

  async function confirmSwitch() {
    if (!confirmLang) return
    setSwitching(true)
    try {
      await switchExplainLang(confirmLang)
    } catch (e) {
      setSwitching(false)
      setConfirmLang(null)
      setTestError(errorText(e))
    }
  }

  async function test() {
    setStatus('testing')
    setTestError('')
    try {
      const res = await invoke<string>('test_provider', { apiHost, apiKey })
      if (res === 'ok') {
        // Fetch models on success
        try {
          const modelList = await invoke<string[]>('list_models', { apiHost, apiKey })
          setModels(modelList)
          // Auto-select first model if none selected
          if (!model && modelList.length > 0) {
            setModel(modelList[0])
          }
        } catch {
          setModels([])
        }
        setStatus('ok')
      } else {
        setStatus('error')
      }
    } catch (e) {
      setStatus('error')
      setTestError(errorText(e))
    }
    setTimeout(() => setStatus('idle'), 3000)
  }

  const statusConfig = {
    idle: null,
    saving: { icon: <LoadingOutlined />, text: t('common.saving'), color: 'text-blue-500', bg: 'bg-blue-50' },
    saved: { icon: <CheckCircleOutlined />, text: t('settings.saved'), color: 'text-green-600', bg: 'bg-green-50' },
    testing: { icon: <LoadingOutlined />, text: t('settings.testing'), color: 'text-blue-500', bg: 'bg-blue-50' },
    ok: { icon: <CheckCircleOutlined />, text: t('settings.connectionOk'), color: 'text-green-600', bg: 'bg-green-50' },
    error: { icon: <ExclamationCircleOutlined />, text: testError || t('settings.connectionFailed'), color: 'text-red-500', bg: 'bg-red-50' },
  }

  const currentStatus = statusConfig[status]

  return (
    <div className={`space-y-5 h-full overflow-y-auto ${dirty ? 'pb-28' : ''}`}>
      <div className="sticky top-0 z-10 pt-4 pb-2 bg-gray-50">
        <h2 className="text-lg font-semibold">{t('settings.title')}</h2>
      </div>

      {/* Backup & Sync */}
      <GoogleDriveBackup />

      {/* Provider section */}
      <div className="space-y-3">
        <h3 className="text-sm font-medium text-gray-500 uppercase tracking-wide px-1">{t('settings.apiProvider')}</h3>
        <p className="text-xs text-gray-500 px-1 leading-relaxed">{t('settings.apiProviderHint')}</p>
        {PROVIDER_PRESETS.map((preset) => (
          <ProviderGuide key={preset.id} preset={preset} onUse={applyPreset} active={apiHost.trim() === preset.apiHost} />
        ))}

        <div className="bg-white rounded-xl shadow-sm overflow-hidden">
          {/* API Host */}
          <div className="px-4 py-3 border-b border-gray-50">
            <label className="flex items-center gap-2 text-sm text-gray-500 mb-1.5">
              <ApiOutlined />
              {t('settings.apiHost')}
            </label>
            <input
              type="text"
              placeholder="https://api.openai.com/v1"
              value={apiHost}
              onChange={(e) => setApiHost(e.target.value)}
              className="w-full text-[15px] bg-transparent outline-none placeholder:text-gray-300"
            />
          </div>

          {/* API Key */}
          <div className="px-4 py-3 border-b border-gray-50">
            <label className="flex items-center gap-2 text-sm text-gray-500 mb-1.5">
              <KeyOutlined />
              {t('settings.apiKey')}
            </label>
            <input
              type="password"
              placeholder="sk-..."
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              className="w-full text-[15px] bg-transparent outline-none placeholder:text-gray-300"
            />
          </div>

          {/* Model selection */}
          <div className="px-4 py-3">
            <label className="flex items-center gap-2 text-sm text-gray-500 mb-1.5">
              <ApiOutlined />
              {t('settings.model')}
            </label>
            {models.length > 0 ? (
              <select
                value={model}
                onChange={(e) => setModel(e.target.value)}
                className="w-full text-[15px] bg-transparent outline-none cursor-pointer"
              >
                {models.map((m) => (
                  <option key={m} value={m}>{m}</option>
                ))}
              </select>
            ) : (
              <div className="flex items-center gap-2">
                <input
                  type="text"
                  placeholder="gpt-3.5-turbo"
                  value={model}
                  onChange={(e) => setModel(e.target.value)}
                  className="w-full text-[15px] bg-transparent outline-none placeholder:text-gray-300"
                />
              </div>
            )}
            {models.length === 0 && (
              <p className="text-xs text-gray-400 mt-1">{t('settings.modelHint')}</p>
            )}
          </div>

          {/* Vocab Model selection */}
          <div className="px-4 py-3 border-t border-gray-50">
            <label className="flex items-center gap-2 text-sm text-gray-500 mb-1.5">
              <BookOutlined />
              {t('settings.vocabModel')}
            </label>
            {models.length > 0 ? (
              <select
                value={vocabModel}
                onChange={(e) => setVocabModel(e.target.value)}
                className="w-full text-[15px] bg-transparent outline-none cursor-pointer"
              >
                <option value="">{t('settings.useChatModel')}</option>
                {models.map((m) => (
                  <option key={m} value={m}>{m}</option>
                ))}
              </select>
            ) : (
              <div className="flex items-center gap-2">
                <input
                  type="text"
                  placeholder={t('settings.useChatModel')}
                  value={vocabModel}
                  onChange={(e) => setVocabModel(e.target.value)}
                  className="w-full text-[15px] bg-transparent outline-none placeholder:text-gray-300"
                />
              </div>
            )}
            <p className="text-xs text-gray-400 mt-1">{t('settings.vocabModelHint')}</p>
          </div>
        </div>

        {/* Action buttons */}
        <div className="flex gap-2">
          <button
            onClick={test}
            className="flex-1 flex items-center justify-center gap-2 py-3 rounded-xl bg-gray-100 text-gray-700 font-medium text-sm hover:bg-gray-200 active:bg-gray-300 transition-colors"
          >
            <ExperimentOutlined />
            {t('settings.test')}
          </button>
        </div>

        {/* Status indicator */}
        {currentStatus && (
          <div className={`flex items-center gap-2 px-4 py-2.5 rounded-xl ${currentStatus.bg} ${currentStatus.color} text-sm font-medium`}>
            {currentStatus.icon}
            {currentStatus.text}
          </div>
        )}
      </div>

      {/* Info section */}
      <div className="bg-gray-50 rounded-xl p-4">
        <div className="flex items-start gap-2.5">
          <InfoCircleOutlined className="text-gray-400 mt-0.5" />
          <p className="text-sm text-gray-500 leading-relaxed">
            {t('settings.keyPrivacy')}
          </p>
        </div>
      </div>

      {/* Review settings */}
      <div className="space-y-3">
        <h3 className="text-sm font-medium text-gray-500 uppercase tracking-wide px-1">{t('settings.review')}</h3>

        <div className="bg-white rounded-xl shadow-sm overflow-hidden">
          <div className="px-4 py-3 border-b border-gray-50">
            <label className="flex items-center gap-2 text-sm text-gray-500 mb-1.5">
              <BookOutlined />
              {t('settings.maxReviews')}
            </label>
            <input
              type="number"
              min={1}
              max={999}
              value={maxReviews}
              onChange={(e) => setMaxReviews(e.target.value)}
              className="w-full text-[15px] bg-transparent outline-none placeholder:text-gray-300"
            />
            <p className="text-xs text-gray-400 mt-1">
              {t('settings.maxReviewsHint', { n: Math.max(1, parseInt(maxReviews, 10) || MAX_REVIEWS_DEFAULT) })}
            </p>
          </div>
          <div className="px-4 py-3">
            <label className="flex items-center gap-2 text-sm text-gray-500 mb-1.5">
              <BookOutlined />
              {t('settings.maxNew')}
            </label>
            <input
              type="number"
              min={0}
              max={999}
              value={maxNewCards}
              onChange={(e) => setMaxNewCards(e.target.value)}
              className="w-full text-[15px] bg-transparent outline-none placeholder:text-gray-300"
            />
            <p className="text-xs text-gray-400 mt-1">
              {t('settings.maxNewHint', { n: Math.max(0, parseInt(maxNewCards, 10) || MAX_NEW_CARDS_DEFAULT) })}
            </p>
          </div>
        </div>
      </div>

      <GrammarPlanSettings />

      {/* Chat settings */}
      <div className="space-y-3">
        <h3 className="text-sm font-medium text-gray-500 uppercase tracking-wide px-1">{t('settings.chat')}</h3>

        <div className="bg-white rounded-xl shadow-sm overflow-hidden">
          <div className="px-4 py-3 border-b border-gray-50">
            <label className="flex items-center gap-2 text-sm text-gray-500 mb-1.5">
              <MessageOutlined />
              {t('settings.maxDifficult')}
            </label>
            <input
              type="number"
              min={1}
              max={999}
              value={maxDifficultWords}
              onChange={(e) => setMaxDifficultWords(e.target.value)}
              className="w-full text-[15px] bg-transparent outline-none placeholder:text-gray-300"
            />
            <p className="text-xs text-gray-400 mt-1">
              {t('settings.maxDifficultHint', { def: MAX_DIFFICULT_WORDS_DEFAULT, n: Math.max(1, parseInt(maxDifficultWords, 10) || MAX_DIFFICULT_WORDS_DEFAULT) })}
            </p>
          </div>
          <div className="px-4 py-3 border-b border-gray-50">
            <label className="flex items-center gap-2 text-sm text-gray-500 mb-1.5">
              <MessageOutlined />
              {t('settings.maxKnown')}
            </label>
            <input
              type="number"
              min={0}
              max={999}
              value={maxKnownWords}
              onChange={(e) => setMaxKnownWords(e.target.value)}
              className="w-full text-[15px] bg-transparent outline-none placeholder:text-gray-300"
            />
            <p className="text-xs text-gray-400 mt-1">
              {t('settings.maxKnownHint', { def: MAX_KNOWN_WORDS_DEFAULT, n: Math.max(0, parseInt(maxKnownWords, 10) || MAX_KNOWN_WORDS_DEFAULT) })}
            </p>
          </div>
          <div className="px-4 py-3">
            <label className="flex items-center gap-2 text-sm text-gray-500 mb-1.5">
              <MessageOutlined />
              {t('settings.customPrompt')}
            </label>
            <textarea
              rows={4}
              placeholder={t('settings.customPromptPlaceholder')}
              value={chatCustomPrompt}
              onChange={(e) => setChatCustomPrompt(e.target.value)}
              className="w-full text-[15px] bg-transparent outline-none placeholder:text-gray-300 resize-y min-h-[80px]"
            />
            <p className="text-xs text-gray-400 mt-1">
              {t('settings.customPromptHint')}
            </p>
          </div>
        </div>
      </div>

      {/* Explanation language: one switch for the whole app */}
      <div className="space-y-3">
        <h3 className="text-sm font-medium text-gray-500 uppercase tracking-wide px-1">{t('settings.yourLanguage')}</h3>
        <div className="bg-white rounded-xl shadow-sm overflow-hidden">
          {(['id', 'en'] as ExplainLang[]).map((code) => (
            <button
              key={code}
              type="button"
              aria-pressed={lang === code}
              onClick={() => code !== lang && setConfirmLang(code)}
              className="w-full flex items-center justify-between px-4 py-3 border-b border-gray-50 last:border-b-0 text-left hover:bg-gray-50"
            >
              <span className={`text-[15px] ${lang === code ? 'font-semibold' : ''}`}>{code === 'id' ? 'Bahasa Indonesia' : 'English'}</span>
              {lang === code && <CheckCircleOutlined className="text-blue-500" />}
            </button>
          ))}
        </div>
        <p className="text-xs text-gray-500 px-1 leading-relaxed">
          {t('settings.languageHint')}{' '}
          {lang !== 'en' && (
            <>
              {t('settings.aiTranslationNote')}{' '}
              <a href={TRANSLATION_REPORT_URL} target="_blank" rel="noreferrer" className="text-blue-600">{t('common.reportTranslation')}</a>
            </>
          )}
        </p>
      </div>

      {/* Learning aids: collapsed by default so they don't take much room */}
      <div className="space-y-3">
        <h3 className="text-sm font-medium text-gray-500 uppercase tracking-wide px-1">{t('settings.learningAids')}</h3>
        <div className="bg-white rounded-xl shadow-sm overflow-hidden">
          <button
            type="button"
            aria-expanded={aidsOpen}
            onClick={() => setAidsOpen(!aidsOpen)}
            className={`w-full flex items-center justify-between gap-3 px-4 py-3 text-left ${aidsOpen ? 'border-b border-gray-50' : ''}`}
          >
            <span>
              <span className="block text-[15px] font-medium">{t('settings.learningAidsToggle')}</span>
              <span className="block text-xs text-gray-400">
                {t('settings.aidsOn', { count: [chatHints, romaji, romajiInput].filter(Boolean).length })}
              </span>
            </span>
            {aidsOpen ? <UpOutlined className="text-gray-400" /> : <DownOutlined className="text-gray-400" />}
          </button>
          {aidsOpen && (
          <>
          {[
            { key: CHAT_HINTS_KEY, value: chatHints, set: setChatHints, label: t('settings.chatHints'), desc: t('settings.chatHintsDesc') },
            { key: ROMAJI_KEY, value: romaji, set: setRomaji, label: t('settings.romaji'), desc: t('settings.romajiDesc') },
            { key: ROMAJI_INPUT_KEY, value: romajiInput, set: setRomajiInput, label: t('settings.romajiInput'), desc: t('settings.romajiInputDesc') },
          ].map((row) => (
            <button
              key={row.key}
              type="button"
              role="switch"
              aria-checked={row.value}
              onClick={() => toggle(row.key, !row.value, row.set)}
              className="w-full flex items-center justify-between gap-3 px-4 py-3 border-b border-gray-50 last:border-b-0 text-left"
            >
              <span>
                <span className="block text-[15px] font-medium">{row.label}</span>
                <span className="block text-xs text-gray-400">{row.desc}</span>
              </span>
              <span className={`shrink-0 w-11 h-6 rounded-full p-0.5 flex transition-colors ${row.value ? 'bg-blue-500 justify-end' : 'bg-gray-300 justify-start'}`}>
                <span className="w-5 h-5 rounded-full bg-white shadow" />
              </span>
            </button>
          ))}
          <div className="px-4 py-3 space-y-2">
            <div className="text-[15px] font-medium">{t('settings.kanaStarter')}</div>
            <div className="text-xs text-gray-400">{t('settings.kanaStarterDesc')}</div>
            <div className="flex gap-2">
              <button type="button" onClick={() => seedKana('hiragana')} className="flex-1 py-2 rounded-lg bg-gray-100 text-sm font-medium hover:bg-gray-200">{t('settings.addHiragana')}</button>
              <button type="button" onClick={() => seedKana('katakana')} className="flex-1 py-2 rounded-lg bg-gray-100 text-sm font-medium hover:bg-gray-200">{t('settings.addKatakana')}</button>
            </div>
            {kanaMessage && <div className="text-xs text-green-700">{kanaMessage}</div>}
          </div>
          </>
          )}
        </div>
      </div>

      {/* Sticky save/cancel bar when there are unsaved changes */}
      {dirty && (
        <div className="fixed bottom-14 left-0 right-0 z-50 px-4 pt-3 pb-4 bg-white/90 backdrop-blur border-t border-gray-100">
          <div className="flex gap-2">
            <button
              onClick={cancel}
              className="flex-1 flex items-center justify-center gap-2 py-3 rounded-xl bg-gray-100 text-gray-700 font-medium text-sm hover:bg-gray-200 active:bg-gray-300 transition-colors"
            >
              <CloseOutlined />
              {t('common.cancel')}
            </button>
            <button
              onClick={save}
              className="flex-1 flex items-center justify-center gap-2 py-3 rounded-xl bg-blue-500 text-white font-medium text-sm hover:bg-blue-600 active:bg-blue-700 transition-colors"
            >
              <SaveOutlined />
              {t('settings.saveChanges')}
            </button>
          </div>
        </div>
      )}

      {/* Confirm switching the explanation language */}
      {confirmLang && (
        <div className="fixed inset-0 z-[70] flex items-center justify-center bg-black/40 px-5" onClick={() => !switching && setConfirmLang(null)}>
          <div role="dialog" aria-label={t('settings.switchTitle')} className="bg-white rounded-2xl p-5 w-full max-w-sm space-y-3" onClick={(e) => e.stopPropagation()}>
            <h3 className="text-base font-semibold">{t('settings.switchTitle', { language: confirmLang === 'id' ? 'Bahasa Indonesia' : 'English' })}</h3>
            <p className="text-sm text-gray-600">{t('settings.switchBody')}</p>
            <ul className="list-disc pl-5 text-sm text-gray-600 space-y-0.5">
              <li>{t('settings.switchItemUi')}</li>
              <li>{t('settings.switchItemAi')}</li>
              <li>{t('settings.switchItemGrammar')}</li>
            </ul>
            <div className="text-sm bg-amber-50 text-amber-800 rounded-lg p-3">{t('settings.switchCards')}</div>
            <div className="flex gap-2 justify-end">
              <button type="button" disabled={switching} onClick={() => setConfirmLang(null)} className="px-4 py-2 rounded-xl text-sm font-medium hover:bg-gray-100">{t('common.cancel')}</button>
              <button type="button" disabled={switching} onClick={confirmSwitch} className="px-4 py-2 rounded-xl text-sm font-medium bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-60">
                {switching ? <LoadingOutlined /> : t('settings.switchConfirm')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
