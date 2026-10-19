import { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import {
  ArrowLeftOutlined, DownOutlined, UpOutlined, LoadingOutlined,
  PlusOutlined, CheckCircleOutlined, ClockCircleOutlined,
  InfoCircleOutlined, CloseCircleOutlined, TranslationOutlined,
} from '@ant-design/icons'
import ReadingTextComponent from '../components/reading/ReadingText'
import ReadingSummary from '../components/reading/ReadingSummary'
import type {
  ReadingText,
  ReadingQuestion,
  AnswerEvaluation,
  SavedAnswer,
  ReadingSession,
  ReadingSessionSummary,
  ReviewItem,
} from '../lib/types'
import { t, lang as explainLang, errorText, formatDay, languageName } from '../lib/i18n'
import { TARGET_LANGUAGES } from '../lib/languages'
import { readFlag, writeFlag, READING_ANSWER_NATIVE_KEY } from '../lib/prefs'

type Phase = 'init' | 'home' | 'level-pick' | 'loading' | 'reading' | 'qa' | 'summary'

const LEVELS: Record<string, string[]> = {
  ja: ['N5', 'N4', 'N3', 'N2', 'N1'],
  id: ['A1', 'A2', 'B1', 'B2'],
}

const LANGUAGES = TARGET_LANGUAGES.map((l) => ({ ...l, levels: LEVELS[l.code] ?? [] }))

/** Beginner levels where comprehension answers may be given in the learner's own language. */
const NATIVE_ANSWER_LEVELS = ['N5', 'N4', 'A1', 'A2']

const LANG_STORAGE_KEY = 'reading_language'
const LEVEL_STORAGE_KEY = 'reading_level'

const LANG_FLAGS: Record<string, string> = { ja: '🇯🇵', id: '🇮🇩' }

function defaultFurigana(level: string): boolean {
  return ['N5', 'N4', 'N3'].includes(level)
}

/** Source badge: RSS feed name, or AI-written ("ai", or "AI Generated" in older sessions). */
function sourceLabel(source: string): string {
  return source === 'ai' || source === 'AI Generated' ? t('reading.sourceAi') : source
}

export default function Reading() {
  const [phase, setPhase] = useState<Phase>('init')
  const [language, setLanguage] = useState(() => {
    const stored = localStorage.getItem(LANG_STORAGE_KEY)
    return LANGUAGES.some((l) => l.code === stored) ? stored! : (LANGUAGES[0]?.code ?? 'ja')
  })
  const [level, setLevel] = useState(() => localStorage.getItem(LEVEL_STORAGE_KEY) || 'N4')
  const [loadingMsg, setLoadingMsg] = useState('')
  const [error, setError] = useState('')

  const [sessions, setSessions] = useState<ReadingSessionSummary[]>([])
  const [text, setText] = useState<ReadingText | null>(null)
  const [showFurigana, setShowFurigana] = useState(true)
  const [questions, setQuestions] = useState<ReadingQuestion[]>([])
  const [reviewItems, setReviewItems] = useState<ReviewItem[]>([])

  const [sessionId, setSessionId] = useState<number | null>(null)
  const [currentQ, setCurrentQ] = useState(0)
  const [answers, setAnswers] = useState<SavedAnswer[]>([])
  const [evaluation, setEvaluation] = useState<AnswerEvaluation | null>(null)
  const [submitting, setSubmitting] = useState(false)
  const [showPassage, setShowPassage] = useState(false)
  const [currentAnswer, setCurrentAnswer] = useState('')
  const [translation, setTranslation] = useState<string | null>(null)
  const [showTranslation, setShowTranslation] = useState(false)
  const [translating, setTranslating] = useState(false)
  const [answerNative, setAnswerNative] = useState(() => readFlag(READING_ANSWER_NATIVE_KEY))

  const startTimeRef = useRef<number>(Date.now())
  const [elapsedSeconds, setElapsedSeconds] = useState(0)
  const [totalRetries, setTotalRetries] = useState(0)

  const apiHost = localStorage.getItem('api_host') || ''
  const apiKey = localStorage.getItem('api_key') || ''
  const model = localStorage.getItem('api_model') || ''

  const currentLang = LANGUAGES.find(l => l.code === language)
  const levels = currentLang?.levels ?? LEVELS[language] ?? []

  // Translation of the whole passage, only shown when the learner asks; cached per session in Rust.
  async function toggleTranslation() {
    if (showTranslation) { setShowTranslation(false); return }
    setShowTranslation(true)
    if (translation || !text) return
    setTranslating(true)
    try {
      const res = await invoke<string>('llm_translate', {
        apiHost, apiKey, model,
        text: text.plain_body,
        fromLang: text.language,
        kind: sessionId && sessionId > 0 ? 'reading' : null,
        refId: sessionId && sessionId > 0 ? sessionId : null,
      })
      setTranslation(res)
    } catch (e) {
      setTranslation(errorText(e))
    }
    setTranslating(false)
  }

  useEffect(() => {
    invoke<ReviewItem[]>('db_get_review_items').then(setReviewItems).catch(() => {})
    loadHome()
  }, [])

  async function loadHome() {
    try {
      const list = await invoke<ReadingSessionSummary[]>('db_list_reading_sessions')
      setSessions(list)
    } catch {
      setSessions([])
    }
    setPhase('home')
  }

  async function openSession(summary: ReadingSessionSummary) {
    try {
      const session = await invoke<ReadingSession | null>('db_get_reading_session', { id: summary.id })
      if (!session) return
      restoreSession(session)
    } catch {
      // ignore
    }
  }

  function restoreSession(session: ReadingSession) {
    setText({ title: session.title, body: session.body, plain_body: session.plain_body,
      source: session.source, language: session.language, level: session.level })
    setQuestions(session.questions)
    setLevel(session.level)
    setLanguage(session.language)
    setSessionId(session.id)
    setCurrentQ(session.current_q)
    setAnswers(session.answers)
    setEvaluation(null)
    setTranslation(null)
    setShowTranslation(false)
    setShowFurigana(defaultFurigana(session.level))
    // Complete sessions go to reading view so user can re-read; in-progress resume at their phase
    setPhase(session.phase === 'complete' ? 'reading' : session.phase as Phase)
  }

  async function startReading() {
    localStorage.setItem(LANG_STORAGE_KEY, language)
    localStorage.setItem(LEVEL_STORAGE_KEY, level)
    setError('')
    setPhase('loading')
    setLoadingMsg(t('reading.fetching'))

    const targetWords = reviewItems
      .filter(item => !item.language || item.language === language)
      .slice(0, 20)
      .map(item => item.content)

    let readingText: ReadingText
    try {
      readingText = await invoke<ReadingText>('fetch_reading_text', {
        apiHost, apiKey, model, language, level, targetWords,
      })
    } catch (e) {
      setError(errorText(e))
      setPhase('level-pick')
      return
    }

    setLoadingMsg(t('reading.generatingQuestions'))
    let qs: ReadingQuestion[]
    try {
      qs = await invoke<ReadingQuestion[]>('generate_reading_questions', {
        apiHost, apiKey, model,
        plainBody: readingText.plain_body,
        language, level,
      })
    } catch (e) {
      setError(errorText(e))
      setPhase('level-pick')
      return
    }

    let sid: number
    try {
      sid = await invoke<number>('db_save_reading_session', {
        language, level, title: readingText.title,
        body: readingText.body, plainBody: readingText.plain_body,
        source: readingText.source, questions: JSON.stringify(qs),
      })
    } catch { sid = -1 }

    setText(readingText)
    setQuestions(qs)
    setSessionId(sid)
    setCurrentQ(0)
    setAnswers([])
    setEvaluation(null)
    setTranslation(null)
    setShowTranslation(false)
    setShowFurigana(defaultFurigana(level))
    startTimeRef.current = Date.now()
    setTotalRetries(0)
    setPhase('reading')
  }

  function beginQA() {
    setCurrentQ(0)
    setCurrentAnswer('')
    setEvaluation(null)
    setPhase('qa')
    persistSession('qa', 0, answers)
  }

  async function handleSubmitAnswer(userAnswer: string) {
    if (!text || !questions[currentQ]) return
    setSubmitting(true)
    try {
      const result = await invoke<AnswerEvaluation>('evaluate_reading_answer', {
        apiHost, apiKey, model,
        question: questions[currentQ].question,
        answerHint: questions[currentQ].answer_hint,
        userAnswer,
        plainBody: text.plain_body,
        language,
        answerInExplain: answerNative && NATIVE_ANSWER_LEVELS.includes(text.level),
      })
      setEvaluation(result)
      if (result.result !== 'correct') {
        setTotalRetries(r => r + 1)
      } else {
        const newAnswers: SavedAnswer[] = [
          ...answers,
          { question_index: currentQ, user_answer: userAnswer, result: result.result, retries: 0 },
        ]
        setAnswers(newAnswers)
        persistSession('qa', currentQ, newAnswers)
      }
    } catch (e) {
      setEvaluation({ result: 'incorrect', feedback: errorText(e) })
    } finally {
      setSubmitting(false)
    }
  }

  function advanceQuestion() {
    invoke('db_record_activity', { activityType: 'reading' }).catch(() => {})
    const next = currentQ + 1
    setEvaluation(null)
    setCurrentAnswer('')
    if (next >= questions.length) {
      finishSession()
    } else {
      setCurrentQ(next)
      persistSession('qa', next, answers)
    }
  }

  function finishSession() {
    const elapsed = Math.round((Date.now() - startTimeRef.current) / 1000)
    setElapsedSeconds(elapsed)
    persistSession('complete', currentQ, answers)
    setPhase('summary')
  }

  function persistSession(newPhase: string, qIndex: number, savedAnswers: SavedAnswer[]) {
    if (!sessionId || sessionId < 0) return
    invoke('db_update_reading_session', {
      id: sessionId, phase: newPhase, currentQ: qIndex, answers: JSON.stringify(savedAnswers),
    }).catch(() => {})
  }

  function goHome() {
    setPhase('init')
    loadHome()
  }

  // ─── Render ────────────────────────────────────────────────────────────────

  if (phase === 'init') {
    return (
      <div className="flex items-center justify-center h-full">
        <LoadingOutlined className="text-2xl text-gray-400" spin />
      </div>
    )
  }

  // ─── Home: article library ─────────────────────────────────────────────────
  if (phase === 'home') {
    return (
      <div className="h-full flex flex-col max-w-lg mx-auto">
      <div className="shrink-0 pt-6 pb-3 flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-gray-900">{t('home.reading')}</h1>
          <p className="text-sm text-gray-500 mt-0.5">{t('reading.subtitle')}</p>
        </div>
        <button
          onClick={() => { setError(''); setPhase('level-pick') }}
          className="flex items-center gap-1.5 px-4 py-2 rounded-xl bg-blue-500 text-white
            font-semibold text-sm active:bg-blue-600"
        >
          <PlusOutlined /> {t('reading.newArticle')}
        </button>
      </div>
      <div className="flex-1 overflow-y-auto pb-2">
        {sessions.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-16 gap-4 text-center">
            <span className="text-5xl">📖</span>
            <p className="text-gray-500 text-sm">{t('reading.empty')}</p>
          </div>
        ) : (
          <div className="flex flex-col gap-3">
            {sessions.map(s => {
              const isDone = s.phase === 'complete'
              const isInProgress = !isDone
              return (
                <button
                  key={s.id}
                  onClick={() => openSession(s)}
                  className="w-full text-left bg-white rounded-2xl border border-gray-100 shadow-sm px-4 py-4
                    active:bg-gray-50 flex flex-col gap-2"
                >
                  <div className="flex items-start justify-between gap-2">
                    <p className="text-[15px] font-semibold text-gray-800 leading-snug line-clamp-2 flex-1">
                      {s.title}
                    </p>
                    <span className={`shrink-0 flex items-center gap-1 text-[11px] font-medium px-2 py-0.5 rounded-full
                      ${isDone ? 'bg-green-100 text-green-600' : 'bg-blue-100 text-blue-600'}`}>
                      {isDone
                        ? <><CheckCircleOutlined /> {t('reading.done')}</>
                        : <><ClockCircleOutlined /> {t('reading.inProgress')}</>}
                    </span>
                  </div>
                  <div className="flex items-center gap-2 flex-wrap">
                    <span className="text-[11px] text-gray-400">{LANG_FLAGS[s.language] ?? ''} {s.level}</span>
                    <span className="text-[11px] text-gray-300">·</span>
                    <span className="text-[11px] text-gray-400">{sourceLabel(s.source)}</span>
                    <span className="text-[11px] text-gray-300">·</span>
                    <span className="text-[11px] text-gray-400">{formatDay(s.created_at)}</span>
                    {isInProgress && s.total_questions > 0 && (
                      <>
                        <span className="text-[11px] text-gray-300">·</span>
                        <span className="text-[11px] text-blue-500 font-medium">
                          {t('reading.questionShort', { n: s.current_q + 1, total: s.total_questions })}
                        </span>
                      </>
                    )}
                    {isDone && s.total_questions > 0 && (
                      <>
                        <span className="text-[11px] text-gray-300">·</span>
                        <span className="text-[11px] text-green-600 font-medium">
                          {t('reading.allAnswered', { n: s.total_questions })}
                        </span>
                      </>
                    )}
                  </div>
                </button>
              )
            })}
          </div>
        )}
      </div>
      </div>
    )
  }

  // ─── Level picker ─────────────────────────────────────────────────────────
  if (phase === 'level-pick') {
    return (
      <div className="h-full overflow-y-auto">
      <div className="flex flex-col pt-6 pb-4 gap-6 max-w-lg mx-auto">
        <div className="flex items-center gap-3">
          <button onClick={goHome} aria-label={t('common.back')} className="text-gray-400 active:text-gray-600 p-1">
            <ArrowLeftOutlined className="text-xl" />
          </button>
          <h1 className="text-xl font-bold text-gray-900">{t('reading.newArticle')}</h1>
        </div>

        {error && (
          <div className="bg-red-50 border border-red-200 rounded-xl px-4 py-3 text-sm text-red-700">
            {error}
          </div>
        )}

        {LANGUAGES.length > 1 && (
        <div>
          <p className="text-xs font-semibold text-gray-400 uppercase tracking-wide mb-2">{t('common.language')}</p>
          <div className="flex gap-2">
            {LANGUAGES.map(lang => (
              <button
                key={lang.code}
                onClick={() => { setLanguage(lang.code); setLevel(lang.levels[1] ?? lang.levels[0]) }}
                className={`flex-1 py-3 rounded-xl border text-[15px] font-medium flex items-center justify-center gap-2
                  ${language === lang.code
                    ? 'bg-blue-50 border-blue-300 text-blue-700'
                    : 'bg-white border-gray-200 text-gray-700 active:bg-gray-50'}`}
              >
                <span>{lang.flag}</span>{lang.label}
              </button>
            ))}
          </div>
        </div>
        )}

        <div>
          <p className="text-xs font-semibold text-gray-400 uppercase tracking-wide mb-2">{t('common.level')}</p>
          <div className="flex flex-col gap-2">
            {levels.map(lvl => (
              <button
                key={lvl}
                onClick={() => setLevel(lvl)}
                className={`flex items-center gap-3 px-4 py-3 rounded-xl border text-left
                  ${level === lvl ? 'bg-blue-50 border-blue-300' : 'bg-white border-gray-200 active:bg-gray-50'}`}
              >
                <span className={`text-[15px] font-bold w-10 shrink-0 ${level === lvl ? 'text-blue-600' : 'text-gray-700'}`}>
                  {lvl}
                </span>
                <span className="text-sm text-gray-600">{t(`readingLevel.${lvl}`)}</span>
              </button>
            ))}
          </div>
        </div>

        <button
          onClick={startReading}
          className="w-full py-3.5 rounded-xl bg-blue-500 text-white font-semibold text-[15px] active:bg-blue-600"
        >
          {t('reading.start')}
        </button>
      </div>
      </div>
    )
  }

  // ─── Loading ───────────────────────────────────────────────────────────────
  if (phase === 'loading') {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-4 text-gray-500">
        <LoadingOutlined className="text-3xl" spin />
        <p className="text-sm">{loadingMsg}</p>
      </div>
    )
  }

  // ─── Reading ───────────────────────────────────────────────────────────────
  if (phase === 'reading' && text) {
    return (
      <div className="h-full flex flex-col max-w-lg mx-auto">
        {/* Fixed header */}
        <div className="shrink-0 pt-6 pb-3 flex items-center gap-3 bg-gray-50">
          <button onClick={goHome} aria-label={t('common.back')} className="text-gray-400 active:text-gray-600 p-1">
            <ArrowLeftOutlined className="text-xl" />
          </button>
          <div className="flex-1 min-w-0">
            <h1 className="text-base font-bold text-gray-900 truncate">{text.title || t('reading.untitled')}</h1>
            <div className="flex items-center gap-2 mt-0.5">
              <span className="text-[11px] bg-gray-100 text-gray-500 px-2 py-0.5 rounded-full font-medium">
                {sourceLabel(text.source)}
              </span>
              <span className="text-[11px] bg-blue-50 text-blue-500 px-2 py-0.5 rounded-full font-medium">
                {text.level}
              </span>
            </div>
          </div>
          {text.language === 'ja' && (
            <button
              onClick={() => setShowFurigana(f => !f)}
              className={`shrink-0 px-3 py-1.5 rounded-xl text-xs font-semibold border
                ${showFurigana ? 'bg-indigo-50 border-indigo-200 text-indigo-600' : 'bg-gray-50 border-gray-200 text-gray-500'}`}
            >
              {showFurigana ? t('reading.furiganaOn') : t('reading.furiganaOff')}
            </button>
          )}
        </div>

        {/* Scrollable article */}
        <div className="flex-1 overflow-y-auto">
          <div className="bg-white rounded-2xl border border-gray-100 shadow-sm px-5 py-5 mb-4">
            <ReadingTextComponent text={text} reviewItems={reviewItems} showFurigana={showFurigana} />
          </div>
          <button
            onClick={toggleTranslation}
            className="flex items-center gap-1.5 text-sm text-blue-600 mb-2 px-1"
          >
            <TranslationOutlined />
            {showTranslation ? t('reading.hideTranslation') : t('reading.showTranslation', { language: languageName(explainLang) })}
          </button>
          {showTranslation && (
            <div className="bg-blue-50 rounded-2xl px-5 py-4 mb-4 text-sm text-gray-700 whitespace-pre-wrap">
              {translating ? <LoadingOutlined spin /> : translation}
            </div>
          )}
        </div>

        {/* Fixed footer */}
        <div className="shrink-0 pt-3 pb-2 bg-gray-50">
          <button
            onClick={beginQA}
            className="w-full py-3.5 rounded-xl bg-blue-500 text-white font-semibold text-[15px] active:bg-blue-600"
          >
            {t('reading.startQuestions')}
          </button>
        </div>
      </div>
    )
  }

  // ─── Q&A ───────────────────────────────────────────────────────────────────
  if (phase === 'qa' && text && questions.length > 0) {
    const q = questions[currentQ]
    const isAnswered = !!evaluation
    const isCorrect = evaluation?.result === 'correct'
    const isPartial = evaluation?.result === 'partial'
    const isIncorrect = isAnswered && !isCorrect && !isPartial

    const resultStyle = evaluation
      ? evaluation.result === 'correct'
        ? { bg: 'bg-green-50 border-green-200', icon: <CheckCircleOutlined className="text-green-500" />, label: t('reading.correct') }
        : evaluation.result === 'partial'
          ? { bg: 'bg-yellow-50 border-yellow-200', icon: <InfoCircleOutlined className="text-yellow-500" />, label: t('reading.partial') }
          : { bg: 'bg-red-50 border-red-200', icon: <CloseCircleOutlined className="text-red-500" />, label: t('reading.incorrect') }
      : null

    return (
      <div className="h-full flex flex-col max-w-lg mx-auto">
        {/* Fixed header */}
        <div className="shrink-0 pt-6 pb-3 flex items-center gap-3 bg-gray-50">
          <button onClick={goHome} aria-label={t('common.back')} className="text-gray-400 active:text-gray-600 p-1">
            <ArrowLeftOutlined className="text-xl" />
          </button>
          <div className="flex-1 min-w-0">
            <h1 className="text-sm font-semibold text-gray-700 truncate">{text.title || t('reading.untitled')}</h1>
            <p className="text-[11px] text-gray-400 mt-0.5">{t('reading.questionOf', { n: currentQ + 1, total: questions.length })}</p>
          </div>
        </div>

        {/* Scrollable: passage toggle + question + feedback */}
        <div className="flex-1 overflow-y-auto flex flex-col gap-4 pb-4">
          <button
            onClick={() => setShowPassage(v => !v)}
            className="flex items-center justify-between w-full bg-gray-50 rounded-xl px-4 py-3 border border-gray-100"
          >
            <span className="text-sm font-medium text-gray-600">{t('reading.viewPassage')}</span>
            {showPassage ? <UpOutlined className="text-gray-400" /> : <DownOutlined className="text-gray-400" />}
          </button>

          {showPassage && (
            <div className="bg-white rounded-2xl border border-gray-100 shadow-sm px-5 py-4 max-h-52 overflow-y-auto">
              <ReadingTextComponent text={text} reviewItems={reviewItems} showFurigana={showFurigana} />
            </div>
          )}

          <p className="text-[16px] font-medium text-gray-800 leading-relaxed">{q.question}</p>

          {resultStyle && evaluation && (
            <div className={`rounded-xl border px-4 py-3 flex gap-3 items-start ${resultStyle.bg}`}>
              <span className="mt-0.5 text-lg shrink-0">{resultStyle.icon}</span>
              <div className="flex-1">
                <p className="font-semibold text-sm">{resultStyle.label}</p>
                <p className="text-sm text-gray-700 mt-0.5">{evaluation.feedback}</p>
              </div>
            </div>
          )}
        </div>

        {/* Fixed bottom: textarea + action buttons */}
        <div className="shrink-0 border-t border-gray-100 bg-white pt-3 pb-2 flex flex-col gap-2">
          {isCorrect ? (
            <button
              onClick={advanceQuestion}
              className="w-full py-3 rounded-xl bg-green-500 text-white font-semibold text-[15px] active:bg-green-600"
            >
              {currentQ + 1 < questions.length ? `${t('reading.nextQuestion')} →` : t('quiz.finish')}
            </button>
          ) : (
            <>
              {NATIVE_ANSWER_LEVELS.includes(text.level) && explainLang !== text.language && (
                <label className="flex items-center gap-2 text-xs text-gray-600">
                  <input
                    type="checkbox"
                    checked={answerNative}
                    onChange={(e) => { setAnswerNative(e.target.checked); writeFlag(READING_ANSWER_NATIVE_KEY, e.target.checked) }}
                  />
                  {t('reading.answerNative', { language: languageName(explainLang) })}
                </label>
              )}
              <textarea
                className="w-full rounded-xl border border-gray-200 bg-gray-50 px-4 py-3 text-[15px]
                  placeholder:text-gray-400 focus:outline-none focus:ring-2 focus:ring-blue-300
                  resize-none disabled:opacity-60"
                rows={3}
                placeholder={answerNative && NATIVE_ANSWER_LEVELS.includes(text.level) ? t('reading.answerPlaceholderNative', { language: languageName(explainLang) }) : t('reading.answerPlaceholder', { language: languageName(text.language) })}
                value={currentAnswer}
                onChange={(e) => setCurrentAnswer(e.target.value)}
                disabled={submitting}
                onKeyDown={(e) => { if (e.key === 'Enter' && e.metaKey) { if (currentAnswer.trim() && !submitting) handleSubmitAnswer(currentAnswer.trim()) } }}
              />
              <div className="flex gap-2">
                {!isAnswered && (
                  <button
                    onClick={() => { if (currentAnswer.trim() && !submitting) handleSubmitAnswer(currentAnswer.trim()) }}
                    disabled={!currentAnswer.trim() || submitting}
                    className="flex-1 py-3 rounded-xl bg-blue-500 text-white font-semibold text-[15px]
                      disabled:opacity-40 active:bg-blue-600 flex items-center justify-center gap-2"
                  >
                    {submitting ? <><LoadingOutlined spin /> {t('reading.checking')}</> : t('reading.submit')}
                  </button>
                )}
                {isPartial && (
                  <>
                    <button
                      onClick={() => { setCurrentAnswer(''); setEvaluation(null) }}
                      className="flex-1 py-3 rounded-xl border border-yellow-300 bg-yellow-50 text-yellow-700
                        font-semibold text-[15px] active:bg-yellow-100"
                    >
                      {t('common.retry')}
                    </button>
                    <button
                      onClick={advanceQuestion}
                      className="flex-1 py-3 rounded-xl bg-blue-500 text-white font-semibold text-[15px] active:bg-blue-600"
                    >
                      {currentQ + 1 < questions.length ? `${t('common.continue')} →` : t('quiz.finish')}
                    </button>
                  </>
                )}
                {isIncorrect && (
                  <button
                    onClick={() => { setCurrentAnswer(''); setEvaluation(null) }}
                    className="flex-1 py-3 rounded-xl border border-red-200 bg-red-50 text-red-600
                      font-semibold text-[15px] active:bg-red-100"
                  >
                    {t('common.retry')}
                  </button>
                )}
              </div>
            </>
          )}
        </div>
      </div>
    )
  }

  // ─── Summary ───────────────────────────────────────────────────────────────
  if (phase === 'summary') {
    return (
      <div className="h-full overflow-y-auto">
      <div className="flex flex-col pt-6 pb-4 max-w-lg mx-auto">
        <div className="flex items-center gap-3 mb-4">
          <button onClick={goHome} aria-label={t('common.back')} className="text-gray-400 active:text-gray-600 p-1">
            <ArrowLeftOutlined className="text-xl" />
          </button>
          <span className="text-sm font-semibold text-gray-600">{t('reading.summary')}</span>
        </div>
        <ReadingSummary
          totalQuestions={questions.length}
          totalRetries={totalRetries}
          elapsedSeconds={elapsedSeconds}
          onReadAnother={() => { setError(''); setPhase('level-pick') }}
        />
      </div>
      </div>
    )
  }

  return null
}
