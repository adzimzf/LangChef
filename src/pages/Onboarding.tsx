import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { ArrowLeftOutlined, CheckCircleOutlined, ExclamationCircleOutlined, InfoCircleOutlined, LoadingOutlined } from '@ant-design/icons'
import ProviderGuide from '../components/ProviderGuide'
import { t, lang, errorText, switchExplainLang, type ExplainLang } from '../lib/i18n'
import { TARGET_LANGUAGES } from '../lib/languages'
import { DEFAULT_PROVIDER } from '../lib/providers'
import { JLPT_LEVEL_STORAGE_KEY } from '../lib/jlpt'
import { writeFlag, CHAT_HINTS_KEY, ROMAJI_KEY } from '../lib/prefs'

const DONE_KEY = 'onboarding_done'
const STEP_KEY = 'onboarding_step'

/** First run only: installs that already have an API key skip it. */
export function needsOnboarding(): boolean {
  try {
    return localStorage.getItem(DONE_KEY) !== '1' && !localStorage.getItem('api_key')
  } catch {
    return false
  }
}

type Step = 'language' | 'level' | 'ai'
type Start = 'zero' | 'kana' | 'N5' | 'N4' | 'N3'

const STARTS: { id: Start; jlpt: string }[] = [
  { id: 'zero', jlpt: 'N5' },
  { id: 'kana', jlpt: 'N5' },
  { id: 'N5', jlpt: 'N5' },
  { id: 'N4', jlpt: 'N4' },
  { id: 'N3', jlpt: 'N3' },
]

const primaryBtn = 'w-full py-3.5 rounded-xl bg-blue-500 text-white font-semibold text-[15px] hover:bg-blue-600 disabled:opacity-50 flex items-center justify-center gap-2'

function readStep(): Step {
  try {
    const s = localStorage.getItem(STEP_KEY)
    return s === 'level' || s === 'ai' ? s : 'language'
  } catch {
    return 'language'
  }
}

/**
 * Welcome → your language → what to learn and your level → connect the AI (OpenRouter by default).
 * Choosing a different language reloads the app so every screen switches at once.
 */
export default function Onboarding({ onDone }: { onDone: () => void }) {
  const [step, setStepState] = useState<Step>(readStep)
  const [target, setTarget] = useState(TARGET_LANGUAGES[0]?.code ?? 'ja')
  const [start, setStart] = useState<Start>('zero')
  const [addHiragana, setAddHiragana] = useState(true)
  const [apiKey, setApiKey] = useState('')
  const [testState, setTestState] = useState<'idle' | 'testing' | 'ok' | 'error'>('idle')
  const [testError, setTestError] = useState('')
  const [switching, setSwitching] = useState(false)

  function setStep(s: Step) {
    try { localStorage.setItem(STEP_KEY, s) } catch { /* storage blocked */ }
    setStepState(s)
  }

  async function pickLanguage(code: ExplainLang) {
    if (code === lang) return
    setSwitching(true)
    await switchExplainLang(code).catch(() => setSwitching(false))
  }

  async function saveLevel() {
    localStorage.setItem('reading_language', target)
    if (target === 'ja') {
      const chosen = STARTS.find((s) => s.id === start)!
      localStorage.setItem(JLPT_LEVEL_STORAGE_KEY, chosen.jlpt)
      localStorage.setItem('reading_level', chosen.jlpt)
      const beginner = start === 'zero' || start === 'kana'
      writeFlag(ROMAJI_KEY, start === 'zero')
      writeFlag(CHAT_HINTS_KEY, beginner)
      if (start === 'zero' && addHiragana) {
        await invoke('db_seed_kana_deck', { script: 'hiragana' }).catch(() => {})
      }
    }
    setStep('ai')
  }

  async function test() {
    setTestState('testing')
    setTestError('')
    try {
      await invoke('test_provider', { apiHost: DEFAULT_PROVIDER.apiHost, apiKey: apiKey.trim() })
      setTestState('ok')
    } catch (e) {
      setTestState('error')
      setTestError(errorText(e))
    }
  }

  function finish(withKey: boolean) {
    if (withKey && apiKey.trim()) {
      localStorage.setItem('api_host', DEFAULT_PROVIDER.apiHost)
      localStorage.setItem('api_key', apiKey.trim())
      if (!localStorage.getItem('api_model')) localStorage.setItem('api_model', DEFAULT_PROVIDER.defaultModel)
    }
    localStorage.setItem(DONE_KEY, '1')
    localStorage.removeItem(STEP_KEY)
    onDone()
  }

  const stepNumber = { language: 1, level: 2, ai: 3 }[step]

  return (
    <div className="h-dvh overflow-y-auto bg-gray-50">
      <div className="max-w-lg mx-auto px-5 pt-10 pb-8 flex flex-col gap-6 min-h-full">
        <div className="flex items-center justify-between">
          {step === 'language' ? (
            <div className="flex items-center gap-2">
              <div className="w-10 h-10 rounded-xl bg-blue-500 text-white flex items-center justify-center text-xl font-bold">語</div>
              <span className="font-bold text-lg">LangChef</span>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => setStep(step === 'ai' ? 'level' : 'language')}
              aria-label={t('common.back')}
              className="w-10 h-10 rounded-xl bg-white border border-gray-200 flex items-center justify-center"
            >
              <ArrowLeftOutlined />
            </button>
          )}
          <span className="text-sm text-gray-500">{t('onboarding.step', { n: stepNumber, total: 3 })}</span>
        </div>

        {step === 'language' && (
          <>
            <div className="space-y-2">
              <h1 className="text-3xl font-bold">{t('onboarding.welcome')}</h1>
              <p className="text-gray-500">{t('onboarding.languageIntro')}</p>
            </div>
            <div className="space-y-3">
              <div className="text-sm font-semibold text-gray-500">{t('settings.yourLanguage')}</div>
              {(['id', 'en'] as ExplainLang[]).map((code) => (
                <button
                  key={code}
                  type="button"
                  aria-pressed={lang === code}
                  disabled={switching}
                  onClick={() => pickLanguage(code)}
                  className={`w-full flex items-center justify-between px-4 py-4 rounded-2xl bg-white border-2 text-left ${lang === code ? 'border-blue-500' : 'border-gray-100'}`}
                >
                  <span>
                    <span className="block text-[17px] font-semibold">{code === 'id' ? 'Bahasa Indonesia' : 'English'}</span>
                    <span className="block text-xs text-gray-500">{code === 'id' ? 'Indonesian' : 'Bahasa Inggris'}</span>
                  </span>
                  {lang === code ? <CheckCircleOutlined className="text-blue-500 text-xl" /> : switching ? <LoadingOutlined /> : null}
                </button>
              ))}
            </div>
            <div className="flex gap-2 items-start bg-blue-50 text-blue-800 rounded-xl p-3 text-sm">
              <InfoCircleOutlined className="mt-0.5" />
              <span>{t('onboarding.languageNote')}</span>
            </div>
            <div className="flex-1" />
            <button type="button" className={primaryBtn} onClick={() => setStep('level')}>{t('common.continue')}</button>
          </>
        )}

        {step === 'level' && (
          <>
            <h1 className="text-2xl font-bold">{t('onboarding.whatToLearn')}</h1>
            <div className="space-y-2">
              <div className="text-sm font-semibold text-gray-500">{t('onboarding.iWantToLearn')}</div>
              {TARGET_LANGUAGES.map((l) => (
                <button
                  key={l.code}
                  type="button"
                  aria-pressed={target === l.code}
                  onClick={() => setTarget(l.code)}
                  className={`w-full flex items-center gap-3 px-4 py-3 rounded-2xl bg-white border-2 text-left ${target === l.code ? 'border-blue-500' : 'border-gray-100'}`}
                >
                  <span className="text-2xl">{l.flag}</span>
                  <span className="text-[16px] font-semibold">{l.label}</span>
                </button>
              ))}
              {TARGET_LANGUAGES.length === 1 && lang === 'id' && (
                <p className="text-xs text-gray-500">{t('onboarding.indonesianHidden')}</p>
              )}
            </div>
            {target === 'ja' && (
              <div className="space-y-2">
                <div className="text-sm font-semibold text-gray-500">{t('onboarding.yourLevel')}</div>
                {STARTS.map((s) => (
                  <button
                    key={s.id}
                    type="button"
                    aria-pressed={start === s.id}
                    onClick={() => setStart(s.id)}
                    className={`w-full flex items-center justify-between gap-2 px-4 py-3 rounded-2xl bg-white border-2 text-left ${start === s.id ? 'border-blue-500' : 'border-gray-100'}`}
                  >
                    <span className="text-[15px] font-semibold">{t(`onboarding.start.${s.id}`)}</span>
                    <span className="text-xs text-gray-500 text-right">{t(`onboarding.startDesc.${s.id}`)}</span>
                  </button>
                ))}
                {start === 'zero' && (
                  <div className="bg-green-50 text-green-800 rounded-xl p-3 text-sm space-y-2">
                    <p>{t('onboarding.romajiOn')}</p>
                    <label className="flex items-center gap-2">
                      <input type="checkbox" checked={addHiragana} onChange={(e) => setAddHiragana(e.target.checked)} />
                      {t('onboarding.addHiragana')}
                    </label>
                  </div>
                )}
              </div>
            )}
            <div className="flex-1" />
            <button type="button" className={primaryBtn} onClick={saveLevel}>{t('common.continue')}</button>
          </>
        )}

        {step === 'ai' && (
          <>
            <div className="space-y-2">
              <h1 className="text-2xl font-bold">{t('onboarding.connectAi')}</h1>
              <p className="text-sm text-gray-500">{t('settings.apiProviderHint')}</p>
            </div>
            <ProviderGuide preset={DEFAULT_PROVIDER} onUse={() => {}} active />
            <label className="block space-y-1.5">
              <span className="text-sm font-semibold">{t('settings.apiKey')}</span>
              <input
                type="password"
                value={apiKey}
                onChange={(e) => { setApiKey(e.target.value); setTestState('idle') }}
                placeholder={`${DEFAULT_PROVIDER.keyPrefix}…`}
                className="w-full h-12 px-4 rounded-xl border border-gray-300 bg-white text-[15px] outline-none focus:ring-2 focus:ring-blue-300"
              />
            </label>
            <button
              type="button"
              onClick={test}
              disabled={!apiKey.trim() || testState === 'testing'}
              className="w-full py-3 rounded-xl bg-white border border-gray-300 font-medium disabled:opacity-50 flex items-center justify-center gap-2"
            >
              {testState === 'testing' && <LoadingOutlined spin />}
              {t('settings.test')}
            </button>
            {testState === 'ok' && (
              <div role="status" className="flex gap-2 items-start bg-green-50 text-green-800 rounded-xl p-3 text-sm">
                <CheckCircleOutlined className="mt-0.5" />{t('settings.connectionOk')}
              </div>
            )}
            {testState === 'error' && (
              <div role="alert" className="flex gap-2 items-start bg-red-50 text-red-700 rounded-xl p-3 text-sm">
                <ExclamationCircleOutlined className="mt-0.5" />{testError}
              </div>
            )}
            <p className="text-xs text-gray-500">{t('onboarding.otherProvider')}</p>
            <div className="flex-1" />
            <button type="button" className={primaryBtn} disabled={!apiKey.trim()} onClick={() => finish(true)}>
              {t('onboarding.startLearning')}
            </button>
            <button type="button" className="text-sm text-gray-500 py-2" onClick={() => finish(false)}>
              {t('onboarding.skipAi')}
            </button>
          </>
        )}
      </div>
    </div>
  )
}
