import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useBackHandler } from '../lib/backNav'
import { invoke } from '@tauri-apps/api/core'
import {
  ArrowLeftOutlined, LoadingOutlined, PlusOutlined, CheckCircleFilled, CloseOutlined,
  TranslationOutlined, DeleteOutlined, RightOutlined, ReadOutlined, BulbOutlined,
  FileTextOutlined, ThunderboltOutlined, SendOutlined, TrophyOutlined,
} from '@ant-design/icons'
import ReadingTextComponent, { type TextSize } from '../components/reading/ReadingText'
import ReadingSummary, { RESULT_STYLE } from '../components/reading/ReadingSummary'
import WordSheet, { keepSheetMeaning } from '../components/WordSheet'
import type {
  ReadingText,
  ReadingQuestion,
  AnswerEvaluation,
  AnswerStats,
  SavedAnswer,
  ReadingSession,
  ReadingSessionSummary,
  ReadingOverview,
  ReviewItem,
  SavedWord,
} from '../lib/types'
import { t, lang as explainLang, errorText, errorWithDetail, formatDay, languageName } from '../lib/i18n'
import { TARGET_LANGUAGES } from '../lib/languages'
import { readFlag, writeFlag, READING_ANSWER_NATIVE_KEY } from '../lib/prefs'

type Phase = 'init' | 'home' | 'loading' | 'reading' | 'qa' | 'summary'

const LEVELS: Record<string, string[]> = {
  ja: ['N5', 'N4', 'N3', 'N2', 'N1'],
  id: ['A1', 'A2', 'B1', 'B2'],
}

const LANGUAGES = TARGET_LANGUAGES.map((l) => ({ ...l, levels: LEVELS[l.code] ?? [] }))

/** Beginner levels where comprehension answers may be given in the learner's own language. */
const NATIVE_ANSWER_LEVELS = ['N5', 'N4', 'A1', 'A2']

const LANG_STORAGE_KEY = 'reading_language'
const LEVEL_STORAGE_KEY = 'reading_level'
const TEXT_SIZE_KEY = 'reading_text_size'
const TAP_HINT_KEY = 'reading_tap_hint_seen'

const LANG_FLAGS: Record<string, string> = { ja: '🇯🇵', id: '🇮🇩' }
const TEXT_SIZES: TextSize[] = ['sm', 'md', 'lg']
const EMPTY_STATS: AnswerStats = { answered: 0, correct: 0, first_try: 0, partial: 0, retries: 0 }

function defaultFurigana(level: string): boolean {
  return ['N5', 'N4', 'N3'].includes(level)
}

/** Source badge: RSS feed name, or AI-written ("ai", or "AI Generated" in older sessions). */
function sourceLabel(source: string): string {
  return source === 'ai' || source === 'AI Generated' ? t('reading.sourceAi') : source
}

function readStored(key: string): string | null {
  try { return localStorage.getItem(key) } catch { return null }
}

function writeStored(key: string, value: string) {
  try { localStorage.setItem(key, value) } catch { /* private mode */ }
}

function Header({ title, subtitle, onBack, right }: {
  title: string; subtitle?: React.ReactNode; onBack: () => void; right?: React.ReactNode
}) {
  return (
    <div className="shrink-0 pt-5 pb-3 flex items-center gap-2 bg-gray-50">
      <button
        onClick={onBack}
        aria-label={t('common.back')}
        className="w-9 h-9 -ml-1 rounded-full flex items-center justify-center text-gray-500 active:bg-gray-200"
      >
        <ArrowLeftOutlined className="text-lg" />
      </button>
      <div className="flex-1 min-w-0">
        <h1 className="text-[15px] font-semibold text-gray-900 truncate leading-tight">{title}</h1>
        {subtitle && <div className="text-xs text-gray-500 mt-0.5">{subtitle}</div>}
      </div>
      {right}
    </div>
  )
}

/** One segment per question, colored by how it went. */
function QuestionProgress({ total, current, answers }: { total: number; current: number; answers: SavedAnswer[] }) {
  return (
    <div className="flex gap-1.5">
      {Array.from({ length: total }, (_, i) => {
        const result = answers.find((a) => a.question_index === i)?.result
        const color = result === 'correct' ? 'bg-emerald-500'
          : result === 'partial' ? 'bg-amber-400'
          : result ? 'bg-rose-400'
          : i === current ? 'bg-indigo-500' : 'bg-gray-200'
        return <div key={i} className={`h-1.5 flex-1 rounded-full transition-colors ${color}`} />
      })}
    </div>
  )
}

export default function Reading() {
  const [phase, setPhase] = useState<Phase>('init')
  const [language, setLanguage] = useState(() => {
    const stored = readStored(LANG_STORAGE_KEY)
    return LANGUAGES.some((l) => l.code === stored) ? stored! : (LANGUAGES[0]?.code ?? 'ja')
  })
  const [level, setLevel] = useState(() => {
    const stored = readStored(LEVEL_STORAGE_KEY)
    const lv = LEVELS[language] ?? []
    return stored && lv.includes(stored) ? stored : (lv[1] ?? lv[0] ?? 'N4')
  })
  const [error, setError] = useState('')

  const [sessions, setSessions] = useState<ReadingSessionSummary[]>([])
  const [overview, setOverview] = useState<ReadingOverview | null>(null)
  const [confirmDelete, setConfirmDelete] = useState<number | null>(null)

  const [text, setText] = useState<ReadingText | null>(null)
  const [showFurigana, setShowFurigana] = useState(true)
  const [textSize, setTextSize] = useState<TextSize>(() => {
    const s = readStored(TEXT_SIZE_KEY) as TextSize | null
    return s && TEXT_SIZES.includes(s) ? s : 'md'
  })
  const [tapHintSeen, setTapHintSeen] = useState(() => readStored(TAP_HINT_KEY) === '1')
  const [readProgress, setReadProgress] = useState(0)
  const [questions, setQuestions] = useState<ReadingQuestion[]>([])
  // Questions are written while the learner reads; this tracks that request.
  const [questionsState, setQuestionsState] = useState<'ready' | 'loading' | 'error'>('ready')
  const [questionsError, setQuestionsError] = useState('')
  /** The passage now on screen, so a late answer for an earlier one is dropped. */
  const shownPassageRef = useRef('')
  const [reviewItems, setReviewItems] = useState<ReviewItem[]>([])
  const [wordSheet, setWordSheet] = useState<{ word: string; base: string } | null>(null)

  const [sessionId, setSessionId] = useState<number | null>(null)
  const [sessionPhase, setSessionPhase] = useState('reading')
  const [currentQ, setCurrentQ] = useState(0)
  const [answers, setAnswers] = useState<SavedAnswer[]>([])
  const [evaluation, setEvaluation] = useState<AnswerEvaluation | null>(null)
  const [submitting, setSubmitting] = useState(false)
  const [showPassage, setShowPassage] = useState(false)
  const [currentAnswer, setCurrentAnswer] = useState('')
  const [lastAnswer, setLastAnswer] = useState('')
  const [qRetries, setQRetries] = useState(0)
  const [showHint, setShowHint] = useState(false)
  const [translation, setTranslation] = useState<string | null>(null)
  const [showTranslation, setShowTranslation] = useState(false)
  const [translating, setTranslating] = useState(false)
  const [answerNative, setAnswerNative] = useState(() => readFlag(READING_ANSWER_NATIVE_KEY))

  const startTimeRef = useRef<number>(Date.now())
  const baseElapsedRef = useRef(0)
  const [elapsedSeconds, setElapsedSeconds] = useState(0)
  const [stats, setStats] = useState<AnswerStats>(EMPTY_STATS)

  const apiHost = readStored('api_host') || ''
  const apiKey = readStored('api_key') || ''
  const model = readStored('api_model') || ''

  const levels = LEVELS[language] ?? []

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
    loadReviewItems()
    loadHome()
  }, [])

  function loadReviewItems() {
    invoke<ReviewItem[]>('db_get_review_items').then(setReviewItems).catch(() => {})
  }

  async function loadHome() {
    const [list, ov] = await Promise.all([
      invoke<ReadingSessionSummary[]>('db_list_reading_sessions').catch(() => []),
      invoke<ReadingOverview>('db_reading_overview').catch(() => null),
    ])
    setSessions(list)
    setOverview(ov)
    setPhase('home')
  }

  async function openSession(id: number) {
    try {
      const session = await invoke<ReadingSession | null>('db_get_reading_session', { id })
      if (session) restoreSession(session)
    } catch {
      // ignore
    }
  }

  function resetReadingView(lvl: string) {
    setEvaluation(null)
    setTranslation(null)
    setShowTranslation(false)
    setShowPassage(false)
    setCurrentAnswer('')
    setQRetries(0)
    setShowHint(false)
    setReadProgress(0)
    setShowFurigana(defaultFurigana(lvl))
    startTimeRef.current = Date.now()
  }

  function restoreSession(session: ReadingSession) {
    const restored: ReadingText = { title: session.title, body: session.body, plain_body: session.plain_body,
      source: session.source, language: session.language, level: session.level }
    setText(restored)
    setQuestions(session.questions)
    shownPassageRef.current = session.plain_body
    setQuestionsState('ready')
    if (session.questions.length === 0) loadQuestions(restored, session.id)
    setSessionId(session.id)
    setSessionPhase(session.phase)
    setCurrentQ(session.current_q)
    setAnswers(session.answers)
    setStats(session.stats)
    baseElapsedRef.current = session.elapsed_seconds
    setElapsedSeconds(session.elapsed_seconds)
    resetReadingView(session.level)
    // Complete sessions go to reading view so user can re-read; in-progress resume at their phase
    setPhase(session.phase === 'qa' ? 'qa' : 'reading')
  }

  function pickLanguage(code: string) {
    setLanguage(code)
    const lv = LEVELS[code] ?? []
    setLevel(lv[1] ?? lv[0])
  }

  /** Writes the questions for the passage on screen (saved to its session) while the learner reads it. */
  async function loadQuestions(passage: ReadingText, sid: number) {
    setQuestionsState('loading')
    setQuestionsError('')
    try {
      const qs = await invoke<ReadingQuestion[]>('generate_reading_questions', {
        apiHost, apiKey, model,
        plainBody: passage.plain_body,
        language: passage.language, level: passage.level,
        sessionId: sid > 0 ? sid : null,
      })
      if (shownPassageRef.current !== passage.plain_body) return
      setQuestions(qs)
      setQuestionsState('ready')
    } catch (e) {
      if (shownPassageRef.current !== passage.plain_body) return
      setQuestionsError(errorWithDetail(e))
      setQuestionsState('error')
    }
  }

  async function startReading() {
    writeStored(LANG_STORAGE_KEY, language)
    writeStored(LEVEL_STORAGE_KEY, level)
    setError('')
    setPhase('loading')

    let readingText: ReadingText
    try {
      readingText = await invoke<ReadingText>('fetch_reading_text', {
        apiHost, apiKey, model, language, level,
      })
    } catch (e) {
      setError(errorWithDetail(e))
      loadHome()
      return
    }

    let sid: number
    try {
      sid = await invoke<number>('db_save_reading_session', {
        language, level, title: readingText.title,
        body: readingText.body, plainBody: readingText.plain_body,
        source: readingText.source, questions: '[]',
      })
    } catch { sid = -1 }

    setText(readingText)
    setQuestions([])
    shownPassageRef.current = readingText.plain_body
    setSessionId(sid)
    setSessionPhase('reading')
    setCurrentQ(0)
    setAnswers([])
    setStats(EMPTY_STATS)
    baseElapsedRef.current = 0
    resetReadingView(level)
    setPhase('reading')
    loadQuestions(readingText, sid)
  }

  function beginQA(fresh: boolean) {
    const kept = fresh ? [] : answers
    const q = fresh ? 0 : Math.min(currentQ, Math.max(questions.length - 1, 0))
    if (fresh) {
      baseElapsedRef.current = 0
      startTimeRef.current = Date.now()
    }
    setAnswers(kept)
    setCurrentQ(q)
    setCurrentAnswer('')
    setEvaluation(null)
    setQRetries(0)
    setShowHint(false)
    setSessionPhase('qa')
    setPhase('qa')
    persistSession('qa', q, kept)
  }

  async function handleSubmitAnswer(userAnswer: string) {
    if (!text || !questions[currentQ] || submitting) return
    setSubmitting(true)
    setLastAnswer(userAnswer)
    try {
      const result = await invoke<AnswerEvaluation>('evaluate_reading_answer', {
        apiHost, apiKey, model,
        question: questions[currentQ].question,
        answerHint: questions[currentQ].answer_hint,
        userAnswer,
        plainBody: text.plain_body,
        language: text.language,
        answerInExplain: answerNative && NATIVE_ANSWER_LEVELS.includes(text.level),
      })
      setEvaluation(result)
    } catch (e) {
      setEvaluation({ result: 'incorrect', feedback: errorText(e) })
    } finally {
      setSubmitting(false)
    }
  }

  function retry() {
    setQRetries((r) => r + 1)
    setCurrentAnswer('')
    setEvaluation(null)
  }

  /** Records how the current question ended (correct, partial, or skipped) and moves on. */
  function advanceQuestion(result: 'correct' | 'partial' | 'incorrect') {
    invoke('db_record_activity', { activityType: 'reading' }).catch(() => {})
    const wrongTries = qRetries + (evaluation && evaluation.result !== 'correct' ? 1 : 0)
    const newAnswers: SavedAnswer[] = [
      ...answers.filter((a) => a.question_index !== currentQ),
      {
        question_index: currentQ,
        user_answer: result === 'incorrect' && !evaluation ? '' : lastAnswer,
        result,
        retries: result === 'correct' ? qRetries : wrongTries,
        feedback: evaluation?.feedback ?? '',
      },
    ]
    setAnswers(newAnswers)
    setEvaluation(null)
    setCurrentAnswer('')
    setQRetries(0)
    setShowHint(false)
    const next = currentQ + 1
    if (next >= questions.length) {
      finishSession(newAnswers)
    } else {
      setCurrentQ(next)
      persistSession('qa', next, newAnswers)
    }
  }

  async function finishSession(finalAnswers: SavedAnswer[]) {
    const elapsed = baseElapsedRef.current + Math.round((Date.now() - startTimeRef.current) / 1000)
    setElapsedSeconds(elapsed)
    setSessionPhase('complete')
    await persistSession('complete', currentQ, finalAnswers, elapsed)
    if (sessionId && sessionId > 0) {
      const saved = await invoke<ReadingSession | null>('db_get_reading_session', { id: sessionId }).catch(() => null)
      if (saved) setStats(saved.stats)
    }
    setPhase('summary')
  }

  function persistSession(newPhase: string, qIndex: number, savedAnswers: SavedAnswer[], elapsed?: number) {
    if (!sessionId || sessionId < 0) return Promise.resolve()
    return invoke('db_update_reading_session', {
      id: sessionId, phase: newPhase, currentQ: qIndex, answers: JSON.stringify(savedAnswers),
      elapsedSeconds: elapsed ?? null,
    }).catch(() => {})
  }

  async function deleteSession(id: number) {
    setConfirmDelete(null)
    await invoke('db_delete_reading_session', { id }).catch(() => {})
    loadHome()
  }

  async function addWordFromSheet(content: string, detail: string | null): Promise<SavedWord> {
    const saved = await invoke<SavedWord>('db_upsert_review_item', { content: content.toLowerCase(), language: text?.language ?? null })
    await keepSheetMeaning(saved.content, detail)
    loadReviewItems()
    return saved
  }

  function tapWord(word: string, base: string) {
    setWordSheet({ word, base })
    if (!tapHintSeen) { setTapHintSeen(true); writeStored(TAP_HINT_KEY, '1') }
  }

  function cycleTextSize() {
    const next = TEXT_SIZES[(TEXT_SIZES.indexOf(textSize) + 1) % TEXT_SIZES.length]
    setTextSize(next)
    writeStored(TEXT_SIZE_KEY, next)
  }

  function goHome() {
    setWordSheet(null)
    shownPassageRef.current = ''

    setPhase('init')
    loadHome()
  }
  // Back: an open sheet closes first, then a text goes back to the list.
  useBackHandler(phase === 'loading' || phase === 'reading' || phase === 'qa' || phase === 'summary', goHome)
  useBackHandler(showPassage, () => setShowPassage(false))

  // Sheets go to <body> so they cover the bottom navigation.
  const wordSheetEl = wordSheet && text && createPortal(
    <WordSheet
      word={wordSheet.word}
      base={wordSheet.base}
      language={text.language}
      api={{ apiHost, apiKey, model }}
      onAdd={addWordFromSheet}
      onClose={() => setWordSheet(null)}
    />,
    document.body,
  )

  // ─── Render ────────────────────────────────────────────────────────────────

  if (phase === 'init') {
    return (
      <div className="flex items-center justify-center h-full">
        <LoadingOutlined className="text-2xl text-gray-400" spin />
      </div>
    )
  }

  // ─── Home: start an article + library ──────────────────────────────────────
  if (phase === 'home') {
    const resume = overview?.resume ?? null
    const hasHistory = sessions.length > 0
    return (
      <div className="h-full overflow-y-auto">
        <div className="max-w-lg mx-auto flex flex-col gap-5 pt-6 pb-6">
          <div>
            <h1 className="text-2xl font-bold text-gray-900">{t('home.reading')}</h1>
            <p className="text-sm text-gray-500 mt-0.5">{t('reading.subtitle')}</p>
          </div>

          {overview && hasHistory && (
            <div className="grid grid-cols-3 gap-2">
              {[
                { icon: <ReadOutlined />, value: String(overview.finished), label: t('reading.statFinished'), color: 'text-indigo-600 bg-indigo-50' },
                { icon: <TrophyOutlined />, value: overview.first_try_pct === null ? '–' : `${overview.first_try_pct}%`, label: t('reading.statFirstTry'), color: 'text-emerald-600 bg-emerald-50' },
                { icon: <FileTextOutlined />, value: String(overview.in_progress), label: t('reading.statInProgress'), color: 'text-amber-600 bg-amber-50' },
              ].map((s) => (
                <div key={s.label} className="bg-white rounded-2xl border border-gray-100 px-3 py-3">
                  <span className={`inline-flex w-7 h-7 rounded-lg items-center justify-center text-sm ${s.color}`}>{s.icon}</span>
                  <div className="text-xl font-bold text-gray-900 mt-1.5 leading-none">{s.value}</div>
                  <div className="text-[11px] text-gray-500 mt-1 leading-tight">{s.label}</div>
                </div>
              ))}
            </div>
          )}

          {resume && (
            <button
              onClick={() => openSession(resume.id)}
              className="w-full text-left rounded-3xl bg-gradient-to-br from-indigo-600 to-violet-600 text-white px-5 py-4 shadow-md shadow-indigo-200 active:opacity-90"
            >
              <div className="flex items-center justify-between">
                <span className="text-[11px] font-semibold uppercase tracking-wider text-indigo-100">{t('reading.continueTitle')}</span>
                <span className="text-[11px] font-semibold bg-white/20 rounded-full px-2 py-0.5">
                  {LANG_FLAGS[resume.language] ?? ''} {resume.level}
                </span>
              </div>
              <p className="text-[17px] font-semibold leading-snug mt-2 line-clamp-2">{resume.title || t('reading.untitled')}</p>
              <div className="flex items-center gap-3 mt-3">
                <div className="flex-1 h-1.5 bg-white/25 rounded-full overflow-hidden">
                  <div
                    className="h-full bg-white rounded-full"
                    style={{ width: `${resume.total_questions ? (resume.stats.answered / resume.total_questions) * 100 : 0}%` }}
                  />
                </div>
                <span className="text-xs text-indigo-50 shrink-0">
                  {resume.phase === 'qa'
                    ? t('reading.questionShort', { n: Math.min(resume.current_q + 1, resume.total_questions), total: resume.total_questions })
                    : t('reading.notStarted')}
                </span>
                <RightOutlined className="text-xs" />
              </div>
            </button>
          )}

          {/* Start a new article */}
          <div className="bg-white rounded-3xl border border-gray-100 shadow-sm px-5 py-5">
            <div className="flex items-start gap-3">
              <span className="w-10 h-10 rounded-2xl bg-indigo-50 text-indigo-600 flex items-center justify-center text-lg shrink-0">
                <ThunderboltOutlined />
              </span>
              <div>
                <h2 className="text-[16px] font-semibold text-gray-900 leading-tight">{t('reading.newTitle')}</h2>
                <p className="text-[13px] text-gray-500 mt-0.5 leading-snug">{t('reading.newBody')}</p>
              </div>
            </div>

            {error && (
              <div className="mt-4 bg-rose-50 border border-rose-100 rounded-xl px-3.5 py-2.5 text-sm text-rose-700 flex gap-2 items-start">
                <span className="flex-1">{error}</span>
                <button onClick={() => setError('')} aria-label={t('common.close')} className="text-rose-400"><CloseOutlined /></button>
              </div>
            )}

            {LANGUAGES.length > 1 && (
              <div className="mt-4 grid grid-cols-2 gap-1 p-1 bg-gray-100 rounded-xl">
                {LANGUAGES.map(l => (
                  <button
                    key={l.code}
                    onClick={() => pickLanguage(l.code)}
                    className={`py-2 rounded-lg text-sm font-medium flex items-center justify-center gap-1.5 transition-colors
                      ${language === l.code ? 'bg-white text-gray-900 shadow-sm' : 'text-gray-500'}`}
                  >
                    <span>{l.flag}</span>{l.label}
                  </button>
                ))}
              </div>
            )}

            <p className="text-[11px] font-semibold text-gray-400 uppercase tracking-wide mt-4 mb-2">{t('common.level')}</p>
            <div className="flex gap-2">
              {levels.map(lvl => (
                <button
                  key={lvl}
                  onClick={() => setLevel(lvl)}
                  className={`flex-1 py-2 rounded-xl text-sm font-bold border transition-colors
                    ${level === lvl ? 'bg-indigo-600 border-indigo-600 text-white' : 'bg-white border-gray-200 text-gray-600 active:bg-gray-50'}`}
                >
                  {lvl}
                </button>
              ))}
            </div>
            <p className="text-[13px] text-gray-500 mt-2 min-h-[1.25rem]">{t(`readingLevel.${level}`)}</p>

            <button
              onClick={startReading}
              className="w-full mt-4 py-3.5 rounded-2xl bg-indigo-600 text-white font-semibold text-[15px] active:bg-indigo-700 flex items-center justify-center gap-2"
            >
              <PlusOutlined /> {t('reading.getArticle')}
            </button>
          </div>

          {/* Library */}
          <div>
            <p className="text-xs font-semibold text-gray-400 uppercase tracking-wide mb-2 px-1">{t('reading.library')}</p>
            {!hasHistory ? (
              <div className="flex flex-col items-center justify-center py-10 gap-3 text-center bg-white/60 border border-dashed border-gray-200 rounded-3xl">
                <span className="text-4xl">📖</span>
                <p className="text-gray-500 text-sm max-w-[16rem]">{t('reading.empty')}</p>
              </div>
            ) : (
              <div className="flex flex-col gap-2 anim-stagger">
                {sessions.map(s => {
                  const isDone = s.phase === 'complete'
                  if (confirmDelete === s.id) {
                    return (
                      <div key={s.id} className="bg-rose-50 border border-rose-100 rounded-2xl px-4 py-3 flex items-center gap-2">
                        <p className="flex-1 text-sm text-rose-700">{t('reading.deleteConfirm')}</p>
                        <button onClick={() => setConfirmDelete(null)} className="px-3 py-1.5 rounded-lg text-sm text-gray-600 bg-white border border-gray-200">
                          {t('common.cancel')}
                        </button>
                        <button onClick={() => deleteSession(s.id)} className="px-3 py-1.5 rounded-lg text-sm text-white bg-rose-500 font-medium">
                          {t('common.delete')}
                        </button>
                      </div>
                    )
                  }
                  return (
                    <div key={s.id} className="group relative bg-white rounded-2xl border border-gray-100 flex items-stretch">
                      <button onClick={() => openSession(s.id)} className="flex-1 min-w-0 text-left px-3.5 py-3 flex items-center gap-3 active:bg-gray-50 rounded-2xl">
                        <span className={`w-11 h-11 shrink-0 rounded-xl flex flex-col items-center justify-center leading-none
                          ${isDone ? 'bg-emerald-50 text-emerald-700' : 'bg-indigo-50 text-indigo-700'}`}>
                          <span className="text-[13px]">{LANG_FLAGS[s.language] ?? ''}</span>
                          <span className="text-[11px] font-bold mt-0.5">{s.level}</span>
                        </span>
                        <span className="flex-1 min-w-0">
                          <span className="block text-[15px] font-medium text-gray-800 leading-snug line-clamp-2">
                            {s.title || t('reading.untitled')}
                          </span>
                          <span className="flex items-center gap-1.5 mt-1 text-[11px] text-gray-400">
                            <span className="truncate">{sourceLabel(s.source)}</span>
                            <span>·</span>
                            <span className="shrink-0">{formatDay(s.created_at)}</span>
                          </span>
                        </span>
                        <span className="shrink-0 flex flex-col items-end gap-1">
                          {isDone ? (
                            <span className="flex items-center gap-1 text-[11px] font-semibold text-emerald-700 bg-emerald-50 px-2 py-0.5 rounded-full">
                              <CheckCircleFilled /> {t('reading.score', { n: s.stats.correct, total: s.total_questions })}
                            </span>
                          ) : (
                            <>
                              <span className="text-[11px] font-semibold text-indigo-600">
                                {s.phase === 'qa'
                                  ? t('reading.questionShort', { n: Math.min(s.current_q + 1, s.total_questions), total: s.total_questions })
                                  : t('reading.notStarted')}
                              </span>
                              <span className="w-14 h-1 bg-gray-100 rounded-full overflow-hidden">
                                <span className="block h-full bg-indigo-500" style={{ width: `${s.total_questions ? (s.stats.answered / s.total_questions) * 100 : 0}%` }} />
                              </span>
                            </>
                          )}
                        </span>
                      </button>
                      <button
                        onClick={() => setConfirmDelete(s.id)}
                        aria-label={t('reading.delete')}
                        title={t('reading.delete')}
                        className="shrink-0 w-9 flex items-center justify-center text-gray-300 hover:text-rose-500 active:text-rose-500"
                      >
                        <DeleteOutlined />
                      </button>
                    </div>
                  )
                })}
              </div>
            )}
          </div>
        </div>
      </div>
    )
  }

  // ─── Loading ───────────────────────────────────────────────────────────────
  if (phase === 'loading') {
    return (
      <div className="h-full flex flex-col max-w-lg mx-auto pt-6">
        <div className="bg-white rounded-3xl border border-gray-100 shadow-sm px-5 py-5">
          <div className="flex items-center gap-2 mb-4">
            <span className="text-[11px] font-semibold bg-indigo-50 text-indigo-600 px-2 py-0.5 rounded-full">
              {LANG_FLAGS[language] ?? ''} {level}
            </span>
          </div>
          <div className="flex items-center gap-3">
            <span className="w-7 h-7 rounded-full flex items-center justify-center text-sm bg-indigo-50 text-indigo-600">
              <LoadingOutlined spin />
            </span>
            <span className="text-sm text-gray-800 font-medium">{t('reading.fetching')}</span>
          </div>
          <div className="mt-6 space-y-3 animate-pulse">
            <div className="h-5 bg-gray-100 rounded-lg w-2/3" />
            <div className="h-3.5 bg-gray-100 rounded w-full" />
            <div className="h-3.5 bg-gray-100 rounded w-11/12" />
            <div className="h-3.5 bg-gray-100 rounded w-full" />
            <div className="h-3.5 bg-gray-100 rounded w-4/5" />
          </div>
          <p className="text-xs text-gray-400 mt-5">{t('reading.loadingHint')}</p>
        </div>
      </div>
    )
  }

  // ─── Reading ───────────────────────────────────────────────────────────────
  if (phase === 'reading' && text) {
    const isComplete = sessionPhase === 'complete'
    return (
      <div className="h-full flex flex-col max-w-lg mx-auto">
        <Header
          title={text.title || t('reading.untitled')}
          onBack={goHome}
          right={
            <div className="flex items-center gap-1.5 shrink-0">
              <button
                onClick={cycleTextSize}
                aria-label={t('reading.textSize')}
                title={t('reading.textSize')}
                className="h-8 px-2.5 rounded-full bg-white border border-gray-200 text-gray-600 font-serif active:bg-gray-100"
              >
                <span className="text-[11px]">A</span><span className="text-[15px]">A</span>
              </button>
              {text.language === 'ja' && (
                <button
                  onClick={() => setShowFurigana(f => !f)}
                  aria-pressed={showFurigana}
                  title={t('reading.furigana')}
                  className={`h-8 px-3 rounded-full text-xs font-semibold border transition-colors
                    ${showFurigana ? 'bg-indigo-600 border-indigo-600 text-white' : 'bg-white border-gray-200 text-gray-500'}`}
                >
                  {showFurigana ? t('reading.furiganaOn') : t('reading.furiganaOff')}
                </button>
              )}
            </div>
          }
        />
        <div className="shrink-0 h-1 bg-gray-200/70 rounded-full overflow-hidden mb-3">
          <div className="h-full bg-indigo-500 transition-[width] duration-150" style={{ width: `${readProgress * 100}%` }} />
        </div>

        <div
          className="flex-1 overflow-y-auto"
          onScroll={(e) => {
            const el = e.currentTarget
            const max = el.scrollHeight - el.clientHeight
            setReadProgress(max > 0 ? Math.min(1, el.scrollTop / max) : 1)
          }}
        >
          {!tapHintSeen && (
            <div className="mb-3 flex items-start gap-2.5 bg-amber-50 border border-amber-100 rounded-2xl px-4 py-3 text-[13px] text-amber-800">
              <BulbOutlined className="mt-0.5" />
              <span className="flex-1">{t('reading.tapHint')}</span>
              <button
                onClick={() => { setTapHintSeen(true); writeStored(TAP_HINT_KEY, '1') }}
                aria-label={t('common.close')}
                className="text-amber-400"
              >
                <CloseOutlined />
              </button>
            </div>
          )}

          <article className="bg-white rounded-3xl border border-gray-100 shadow-sm px-5 pt-5 pb-6 mb-3">
            <div className="flex items-center gap-1.5 flex-wrap mb-3">
              <span className="text-[11px] font-semibold bg-indigo-50 text-indigo-600 px-2 py-0.5 rounded-full">
                {LANG_FLAGS[text.language] ?? ''} {text.level}
              </span>
              <span className="text-[11px] font-medium bg-gray-100 text-gray-500 px-2 py-0.5 rounded-full">{sourceLabel(text.source)}</span>
              {isComplete && (
                <span className="text-[11px] font-semibold bg-emerald-50 text-emerald-700 px-2 py-0.5 rounded-full flex items-center gap-1">
                  <CheckCircleFilled /> {t('reading.score', { n: stats.correct, total: questions.length })}
                </span>
              )}
            </div>
            <h2 className="text-xl font-bold text-gray-900 leading-snug mb-4">{text.title || t('reading.untitled')}</h2>
            <ReadingTextComponent text={text} reviewItems={reviewItems} showFurigana={showFurigana} size={textSize} onWordTap={tapWord} />
            <p className="text-[11px] text-gray-400 mt-5 flex items-center gap-1.5">
              <span className="inline-block w-2 h-2 rounded-full bg-orange-500" />
              <span className="inline-block w-2 h-2 rounded-full bg-lime-500 -ml-1" />
              {t('reading.legend')}
            </p>
          </article>

          <div className="bg-white rounded-2xl border border-gray-100 mb-4 overflow-hidden">
            <button onClick={toggleTranslation} className="w-full flex items-center gap-2.5 px-4 py-3 text-sm text-gray-700 active:bg-gray-50">
              <TranslationOutlined className="text-indigo-500" />
              <span className="flex-1 text-left font-medium">
                {showTranslation ? t('reading.hideTranslation') : t('reading.showTranslation', { language: languageName(explainLang) })}
              </span>
              <RightOutlined className={`text-xs text-gray-400 transition-transform ${showTranslation ? 'rotate-90' : ''}`} />
            </button>
            {showTranslation && (
              <div className="px-4 pb-4 text-[14px] text-gray-600 leading-relaxed whitespace-pre-wrap border-t border-gray-50 pt-3">
                {translating ? <span className="flex items-center gap-2 text-gray-400"><LoadingOutlined spin /> {t('common.loading')}</span> : translation}
              </div>
            )}
          </div>

          {/* At the end of the passage, so it only shows once the learner has scrolled to the bottom. */}
          <div className="pb-2 flex gap-2">
            {isComplete ? (
              <>
                <button
                  onClick={() => beginQA(true)}
                  className="flex-1 py-3.5 rounded-2xl bg-white border border-gray-200 text-gray-700 font-semibold text-[15px] active:bg-gray-50"
                >
                  {t('reading.answerAgain')}
                </button>
                <button
                  onClick={() => setPhase('summary')}
                  className="flex-1 py-3.5 rounded-2xl bg-indigo-600 text-white font-semibold text-[15px] active:bg-indigo-700"
                >
                  {t('reading.seeResults')}
                </button>
              </>
            ) : questionsState === 'loading' ? (
              <button
                disabled
                className="w-full py-3.5 rounded-2xl bg-indigo-100 text-indigo-500 font-semibold text-[15px] flex items-center justify-center gap-2"
              >
                <LoadingOutlined spin /> {t('reading.generatingQuestions')}
              </button>
            ) : questionsState === 'error' ? (
              <div className="w-full">
                <p className="text-xs text-rose-600 mb-2 break-words">{questionsError}</p>
                <button
                  onClick={() => sessionId !== null && loadQuestions(text, sessionId)}
                  className="w-full py-3.5 rounded-2xl bg-white border border-gray-200 text-gray-700 font-semibold text-[15px] active:bg-gray-50"
                >
                  {t('common.retry')}
                </button>
              </div>
            ) : (
              <button
                onClick={() => beginQA(false)}
                className="w-full py-3.5 rounded-2xl bg-indigo-600 text-white font-semibold text-[15px] active:bg-indigo-700 flex items-center justify-center gap-2"
              >
                {t('reading.answerQuestions', { count: questions.length })} <RightOutlined className="text-xs" />
              </button>
            )}
          </div>
        </div>

        {wordSheetEl}
      </div>
    )
  }

  // ─── Q&A ───────────────────────────────────────────────────────────────────
  if (phase === 'qa' && text && questions.length > 0) {
    const q = questions[currentQ]
    const result = evaluation?.result
    const isLast = currentQ + 1 >= questions.length
    const canAnswerNative = NATIVE_ANSWER_LEVELS.includes(text.level) && explainLang !== text.language
    const nativeOn = answerNative && NATIVE_ANSWER_LEVELS.includes(text.level)
    const feedbackStyle = result === 'correct'
      ? 'bg-emerald-50 border-emerald-200'
      : result === 'partial' ? 'bg-amber-50 border-amber-200' : 'bg-rose-50 border-rose-200'
    const submit = () => { if (currentAnswer.trim()) handleSubmitAnswer(currentAnswer.trim()) }

    return (
      <div className="h-full flex flex-col max-w-lg mx-auto">
        <Header
          title={text.title || t('reading.untitled')}
          onBack={goHome}
          right={
            <button
              onClick={() => setShowPassage(true)}
              className="shrink-0 h-8 px-3 rounded-full bg-white border border-gray-200 text-xs font-semibold text-gray-600 flex items-center gap-1.5 active:bg-gray-100"
            >
              <ReadOutlined /> {t('reading.viewPassage')}
            </button>
          }
        />
        <div className="shrink-0 mb-4"><QuestionProgress total={questions.length} current={currentQ} answers={answers} /></div>

        <div className="flex-1 overflow-y-auto flex flex-col gap-3 pb-4">
          <div className="bg-white rounded-3xl border border-gray-100 shadow-sm px-5 py-5">
            <span className="text-[11px] font-semibold text-indigo-500 uppercase tracking-wider">
              {t('reading.questionOf', { n: currentQ + 1, total: questions.length })}
            </span>
            <p className="text-[18px] font-semibold text-gray-900 leading-relaxed mt-1.5">{q.question}</p>
          </div>

          {(evaluation || submitting) && lastAnswer && (
            <div className="self-end max-w-[85%] bg-indigo-600 text-white rounded-2xl rounded-br-md px-4 py-2.5 text-[15px]">
              {lastAnswer}
            </div>
          )}

          {submitting && (
            <div className="flex items-center gap-2 text-sm text-gray-400 px-1"><LoadingOutlined spin /> {t('reading.checking')}</div>
          )}

          {evaluation && result && (
            <div className={`rounded-2xl border px-4 py-3.5 flex gap-3 items-start ${feedbackStyle}`}>
              <span className="text-xl leading-none mt-0.5 shrink-0">{(RESULT_STYLE[result] ?? RESULT_STYLE.incorrect).icon}</span>
              <div className="flex-1">
                <p className="font-semibold text-[15px] text-gray-900">
                  {t(result === 'correct' ? 'reading.correct' : result === 'partial' ? 'reading.partial' : 'reading.incorrect')}
                </p>
                <p className="text-sm text-gray-700 mt-0.5 leading-relaxed">{evaluation.feedback}</p>
              </div>
            </div>
          )}

          {showHint && (
            <div className="rounded-2xl bg-white border border-dashed border-amber-300 px-4 py-3 flex gap-2.5 items-start">
              <BulbOutlined className="text-amber-500 mt-0.5" />
              <div className="text-sm text-gray-700">
                <span className="font-semibold text-amber-700">{t('reading.hint')}: </span>{q.answer_hint}
              </div>
            </div>
          )}
        </div>

        <div className="shrink-0 pt-3 pb-2 bg-gray-50 flex flex-col gap-2">
          {result === 'correct' ? (
            <button
              onClick={() => advanceQuestion('correct')}
              className="w-full py-3.5 rounded-2xl bg-emerald-500 text-white font-semibold text-[15px] active:bg-emerald-600 flex items-center justify-center gap-2"
            >
              {isLast ? t('quiz.finish') : t('reading.nextQuestion')} <RightOutlined className="text-xs" />
            </button>
          ) : evaluation ? (
            <>
              <div className="flex gap-2">
                <button
                  onClick={retry}
                  className="flex-1 py-3.5 rounded-2xl bg-white border border-gray-200 text-gray-800 font-semibold text-[15px] active:bg-gray-50"
                >
                  {t('common.retry')}
                </button>
                <button
                  onClick={() => advanceQuestion(result === 'partial' ? 'partial' : 'incorrect')}
                  className="flex-1 py-3.5 rounded-2xl bg-indigo-600 text-white font-semibold text-[15px] active:bg-indigo-700"
                >
                  {result === 'partial' ? (isLast ? t('quiz.finish') : t('common.continue')) : t('reading.skip')}
                </button>
              </div>
              {!showHint && (
                <button onClick={() => setShowHint(true)} className="text-sm text-amber-700 font-medium py-1 flex items-center justify-center gap-1.5">
                  <BulbOutlined /> {t('reading.showHint')}
                </button>
              )}
            </>
          ) : (
            <>
              {canAnswerNative && (
                <button
                  onClick={() => { setAnswerNative(!answerNative); writeFlag(READING_ANSWER_NATIVE_KEY, !answerNative) }}
                  aria-pressed={answerNative}
                  className="self-start flex items-center gap-2 text-xs text-gray-600 px-1"
                >
                  <span className={`w-8 h-[18px] rounded-full p-0.5 transition-colors ${answerNative ? 'bg-indigo-600' : 'bg-gray-300'}`}>
                    <span className={`block w-[14px] h-[14px] rounded-full bg-white transition-transform ${answerNative ? 'translate-x-[14px]' : ''}`} />
                  </span>
                  {t('reading.answerNative', { language: languageName(explainLang) })}
                </button>
              )}
              <div className="flex items-end gap-2 bg-white border border-gray-200 rounded-2xl pl-4 pr-2 py-2 focus-within:ring-2 focus-within:ring-indigo-200 focus-within:border-indigo-300">
                <textarea
                  className="flex-1 bg-transparent py-1.5 text-[15px] placeholder:text-gray-400 focus:outline-none resize-none disabled:opacity-60"
                  rows={2}
                  placeholder={nativeOn
                    ? t('reading.answerPlaceholderNative', { language: languageName(explainLang) })
                    : t('reading.answerPlaceholder', { language: languageName(text.language) })}
                  value={currentAnswer}
                  onChange={(e) => setCurrentAnswer(e.target.value)}
                  disabled={submitting}
                  onKeyDown={(e) => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) submit() }}
                />
                <button
                  onClick={submit}
                  disabled={!currentAnswer.trim() || submitting}
                  aria-label={t('reading.submit')}
                  title={t('reading.submit')}
                  className="w-10 h-10 shrink-0 rounded-xl bg-indigo-600 text-white flex items-center justify-center disabled:bg-gray-200 disabled:text-gray-400 active:bg-indigo-700"
                >
                  {submitting ? <LoadingOutlined spin /> : <SendOutlined />}
                </button>
              </div>
              <div className="flex items-center justify-between px-1">
                <button onClick={() => setShowHint(true)} disabled={showHint} className="text-xs text-gray-500 flex items-center gap-1 disabled:opacity-40">
                  <BulbOutlined /> {t('reading.showHint')}
                </button>
                <button onClick={() => advanceQuestion('incorrect')} className="text-xs text-gray-400">
                  {t('reading.skip')}
                </button>
              </div>
            </>
          )}
        </div>

        {showPassage && createPortal(
          <div className="anim-backdrop fixed inset-0 z-50 flex items-end justify-center bg-black/40" onClick={() => setShowPassage(false)}>
            <div
              role="dialog"
              aria-label={t('reading.viewPassage')}
              className="anim-sheet w-full max-w-lg bg-white rounded-t-3xl max-h-[78vh] flex flex-col safe-bottom"
              onClick={(e) => e.stopPropagation()}
            >
              <div className="flex items-center justify-between px-5 pt-3 pb-2 border-b border-gray-100">
                <div className="w-10" />
                <div className="w-10 h-1 bg-gray-200 rounded-full" />
                <button onClick={() => setShowPassage(false)} aria-label={t('common.close')} className="w-10 h-8 flex items-center justify-end text-gray-400">
                  <CloseOutlined />
                </button>
              </div>
              <div className="overflow-y-auto px-5 py-4">
                <h3 className="text-lg font-bold text-gray-900 mb-3">{text.title || t('reading.untitled')}</h3>
                <ReadingTextComponent text={text} reviewItems={reviewItems} showFurigana={showFurigana} size={textSize} onWordTap={tapWord} />
              </div>
            </div>
          </div>,
          document.body,
        )}
        {wordSheetEl}
      </div>
    )
  }

  // ─── Summary ───────────────────────────────────────────────────────────────
  if (phase === 'summary' && text) {
    return (
      <div className="h-full flex flex-col max-w-lg mx-auto">
        <Header title={t('reading.summary')} onBack={goHome} />
        <div className="flex-1 overflow-y-auto">
          <ReadingSummary
            title={text.title || t('reading.untitled')}
            questions={questions}
            answers={answers}
            stats={stats}
            elapsedSeconds={elapsedSeconds}
            onReadAgain={() => { resetReadingView(text.level); setPhase('reading') }}
            onAnswerAgain={() => beginQA(true)}
            onReadAnother={() => { setError(''); goHome() }}
          />
        </div>
      </div>
    )
  }

  return null
}
