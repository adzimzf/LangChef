import { useEffect, useMemo, useState, useRef } from 'react'
import { useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/core'
import type { Grade } from '../lib/types'
import {
  ImportOutlined,
  ReloadOutlined,
  RobotOutlined,
  EditOutlined,
  CloseOutlined,
  SettingOutlined,
  BulbOutlined,
} from '@ant-design/icons'
import type { AnswerResult, CardDetail, GradePreview, QuizAnswer, QuizTask, ReviewItem, SessionProgress, SessionSummary } from '../lib/types'
import { parseDetailJson } from '../lib/types'
import EditWordForm from '../components/EditWordForm'
import { GRADE_COLORS, GrammarInfoView, MeaningSetup, QuizVerdictView, ReviewQuiz, SessionSummaryView, useAutoAdvance } from '../components/ReviewQuiz'
import { t, lang as explainLang, errorText, partOfSpeechLabel as posLabel, formatInMinutes, formatDate, formatNumber, languageName, type ExplainLang } from '../lib/i18n'
import { TARGET_LANGUAGES, languageLabel } from '../lib/languages'

const MAX_REVIEWS_DEFAULT = 100
const MAX_REVIEWS_KEY = 'max_reviews_per_day'
const MAX_NEW_CARDS_DEFAULT = 20
const MAX_NEW_CARDS_KEY = 'max_new_cards_per_day'

function getMaxReviews(): number {
  const val = localStorage.getItem(MAX_REVIEWS_KEY)
  return val ? parseInt(val, 10) || MAX_REVIEWS_DEFAULT : MAX_REVIEWS_DEFAULT
}

function getMaxNewCards(): number {
  const val = localStorage.getItem(MAX_NEW_CARDS_KEY)
  return val ? parseInt(val, 10) || MAX_NEW_CARDS_DEFAULT : MAX_NEW_CARDS_DEFAULT
}

type GradeOption = { grade: Grade; label: string; nextLabel: string; color: string }

type SessionStartResult = {
  total_cards: number
  next_card: ReviewItem | null
  next_task: QuizTask | null
  grade_options: GradePreview[]
  reviews_remaining: number
  new_remaining: number
  reviews_completed: number
  new_cards_seen: number
  missing_meaning: string[]
}

type SessionGradeResult = SessionProgress & { lapsed: boolean; graduated: boolean }

function getApiConfig() {
  const apiHost = localStorage.getItem('api_host') || ''
  const apiKey = localStorage.getItem('api_key') || ''
  const model = localStorage.getItem('api_vocab_model') || localStorage.getItem('api_model') || 'gpt-3.5-turbo'
  return { apiHost, apiKey, model, ready: !!apiHost && !!apiKey }
}

function toGradeOptions(previews: GradePreview[]): GradeOption[] {
  return previews.map(p => ({
    grade: p.grade as Grade,
    label: t(`grade.${p.grade}`),
    nextLabel: formatInMinutes(p.next_minutes),
    color: GRADE_COLORS[p.grade] || 'bg-gray-500',
  }))
}

function SessionCompleteEffect({ sessionCompleted }: { sessionCompleted: number }) {
  return null
}

export default function Review() {
  const [allItems, setAllItems] = useState<ReviewItem[]>([])
  const [selectedLang, setSelectedLang] = useState<string | null>(null)
  const [revealed, setRevealed] = useState(false)
  const [generatingDetail, setGeneratingDetail] = useState(false)
  const [generatingMnemonic, setGeneratingMnemonic] = useState(false)
  const [showMnemonic, setShowMnemonic] = useState(false)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [showImport, setShowImport] = useState(false)
  const [importText, setImportText] = useState('')
  const [importLang, setImportLang] = useState(TARGET_LANGUAGES[0]?.code ?? 'ja')
  const [importMeaningLang, setImportMeaningLang] = useState<ExplainLang>(explainLang)
  const [importing, setImporting] = useState(false)
  const [importResult, setImportResult] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  // Session state — owned by backend, mirrored here for display only
  const [currentCard, setCurrentCard] = useState<ReviewItem | null>(null)
  const [sessionTotal, setSessionTotal] = useState(0)
  const [sessionRemaining, setSessionRemaining] = useState(0)
  const [sessionCompleted, setSessionCompleted] = useState(0)
  const [reviewedToday, setReviewedToday] = useState(0)
  const [newCardsToday, setNewCardsToday] = useState(0)
  const [gradeOptions, setGradeOptions] = useState<GradeOption[]>([])
  // Auto-graded question for the current card ('classic' means self-grade)
  const [currentTask, setCurrentTask] = useState<QuizTask | null>(null)
  const [answerResult, setAnswerResult] = useState<AnswerResult | null>(null)
  const [answering, setAnswering] = useState(false)
  const [overriding, setOverriding] = useState(false)
  const [summary, setSummary] = useState<SessionSummary | null>(null)
  // Cards with no meaning: generate/write the back, approve, re-queue
  const [missingMeaning, setMissingMeaning] = useState<string[]>([])
  const [bannerDismissed, setBannerDismissed] = useState(false)
  const [generatingAll, setGeneratingAll] = useState(false)
  const [meaningBusy, setMeaningBusy] = useState<'generating' | 'saving' | null>(null)
  const [meaningError, setMeaningError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const navigate = useNavigate()

  const fileInputRef = useRef<HTMLInputElement>(null)
  const isGrading = useRef(false)

  const maxReviews = getMaxReviews()
  const maxNewCards = getMaxNewCards()

  useEffect(() => {
    loadItems()
  }, [])

  async function loadItems() {
    setLoading(true)
    try {
      const items = await invoke<ReviewItem[]>('db_get_review_items')
      setAllItems(items)
    } catch {}
    setLoading(false)
  }

  async function startSession(lang: string | null) {
    if (!lang) return
    try {
      const result = await invoke<SessionStartResult>('db_session_start', {
        language: lang,
        maxReviews: getMaxReviews(),
        maxNewCards: getMaxNewCards(),
      })
      setCurrentCard(result.next_card ?? null)
      setCurrentTask(result.next_task ?? null)
      setAnswerResult(null)
      setMissingMeaning(result.missing_meaning)
      setGradeOptions(toGradeOptions(result.grade_options))
      setSessionTotal(result.total_cards)
      setSessionRemaining(result.total_cards)
      setReviewedToday(result.reviews_completed)
      setNewCardsToday(result.new_cards_seen)
      setSessionCompleted(0)
      setRevealed(false)
    } catch {}
  }

  // Group items by language for deck stats display
  const languageGroups = useMemo(() => {
    const groups: Record<string, ReviewItem[]> = {}
    for (const item of allItems) {
      const lang = item.language
      if (!lang) continue
      if (!groups[lang]) groups[lang] = []
      groups[lang].push(item)
    }
    return groups
  }, [allItems])

  // Auto-select first language
  useEffect(() => {
    const langs = Object.keys(languageGroups)
    if (langs.length > 0 && !selectedLang) {
      setSelectedLang(langs[0])
    }
  }, [languageGroups])

  // Start session whenever language changes (or on initial select)
  const prevLang = useRef<string | null>(null)
  useEffect(() => {
    if (selectedLang && selectedLang !== prevLang.current && !loading) {
      startSession(selectedLang)
    }
    prevLang.current = selectedLang
  }, [selectedLang, loading])

  async function handleGrade(grade: Grade) {
    if (!currentCard || isGrading.current) return
    isGrading.current = true

    try {
      const result = await invoke<SessionGradeResult>('db_session_grade', {
        language: currentCard.language || selectedLang || 'ja',
        cardId: currentCard.id,
        grade,
        maxReviews: getMaxReviews(),
        maxNewCards: getMaxNewCards(),
      })

      invoke('db_record_activity', { activityType: 'review' }).catch(() => {})

      applyProgress(result)
      if (result.graduated && !result.lapsed) {
        setSessionCompleted(prev => prev + 1)
      }
    } catch {}

    setTimeout(() => { isGrading.current = false }, 0)
  }

  function applyProgress(p: SessionProgress) {
    setReviewedToday(p.reviews_completed)
    setNewCardsToday(p.new_cards_seen)
    setCurrentCard(p.next_card ?? null)
    setCurrentTask(p.next_task ?? null)
    setSessionRemaining(p.remaining_count)
    setGradeOptions(toGradeOptions(p.grade_options))
    setAnswerResult(null)
    setRevealed(false)
    setShowMnemonic(false)
    setMeaningError(null)
  }

  // Short-lived message above the card (e.g. "moved to the end of today's queue")
  useEffect(() => {
    if (!notice) return
    const t = setTimeout(() => setNotice(null), 4000)
    return () => clearTimeout(t)
  }, [notice])

  async function generateMeaning() {
    if (!currentCard || !currentTask) return
    const api = getApiConfig()
    if (!api.ready) return
    setMeaningBusy('generating')
    setMeaningError(null)
    try {
      const text = await invoke<string>('db_generate_detail', {
        apiHost: api.apiHost, apiKey: api.apiKey, model: api.model,
        content: currentCard.content,
        language: currentCard.language || selectedLang || 'ja',
        draft: true,
      })
      const parsed = parseDetailJson(text)
      if (currentCard.item_type === 'grammar') {
        // The draft's practice material is shown from the question Rust rebuilds.
        await startSession(currentCard.language || selectedLang)
      } else if (!parsed?.meaning) {
        setMeaningError(t('review.noMeaningInAnswer'))
      } else {
        setCurrentTask({
          ...currentTask,
          draft: {
            reading: parsed.reading || '',
            meaning: parsed.meaning,
            part_of_speech: parsed.part_of_speech || '',
            example: parsed.example || '',
            example_meaning: parsed.example_meaning || '',
          },
        })
      }
    } catch (e) {
      setMeaningError(t('review.generationFailed', { error: errorText(e) }))
    }
    setMeaningBusy(null)
  }

  async function saveMeaning(detail: CardDetail) {
    if (!currentCard) return
    const word = currentCard.content
    setMeaningBusy('saving')
    setMeaningError(null)
    try {
      const result = await invoke<SessionProgress>('db_session_save_meaning', {
        language: currentCard.language || selectedLang || 'ja',
        cardId: currentCard.id,
        meaning: detail,
        maxReviews: getMaxReviews(),
        maxNewCards: getMaxNewCards(),
      })
      applyProgress(result)
      setMissingMeaning(prev => prev.filter(w => w !== word))
      setNotice(result.next_card?.content === word
        ? t('review.savedLast', { word })
        : t('review.savedMoved', { word }))
    } catch (e) {
      setMeaningError(errorText(e))
    }
    setMeaningBusy(null)
  }

  async function skipCard() {
    if (!currentCard) return
    const word = currentCard.content
    try {
      const result = await invoke<SessionProgress>('db_session_skip_card', {
        language: currentCard.language || selectedLang || 'ja',
        cardId: currentCard.id,
        maxReviews: getMaxReviews(),
        maxNewCards: getMaxNewCards(),
      })
      applyProgress(result)
      setMissingMeaning(prev => prev.filter(w => w !== word))
      setNotice(t('review.skipped', { word }))
    } catch (e) {
      setMeaningError(errorText(e))
    }
  }

  async function generateAllMeanings() {
    const api = getApiConfig()
    if (!api.ready || !selectedLang) return
    setGeneratingAll(true)
    try {
      const result = await invoke<{ generated: number; failed: string[] }>('db_generate_missing_meanings', {
        apiHost: api.apiHost, apiKey: api.apiKey, model: api.model,
        language: selectedLang,
        maxReviews: getMaxReviews(),
        maxNewCards: getMaxNewCards(),
      })
      setNotice(result.failed.length
        ? t('review.generatedSome', { count: result.generated, failed: result.failed.join(', ') })
        : t('review.generatedAll', { count: result.generated }))
      await startSession(selectedLang)
    } catch (e) {
      setNotice(t('review.generationFailed', { error: errorText(e) }))
    }
    setGeneratingAll(false)
  }

  async function handleAnswer(answer: QuizAnswer) {
    if (!currentCard || answering) return
    setAnswering(true)
    try {
      const result = await invoke<AnswerResult>('db_session_answer', {
        language: currentCard.language || selectedLang || 'ja',
        cardId: currentCard.id,
        answer,
        maxReviews: getMaxReviews(),
        maxNewCards: getMaxNewCards(),
      })
      invoke('db_record_activity', { activityType: 'review' }).catch(() => {})
      if (result.card) setCurrentCard(result.card)
      setReviewedToday(result.reviews_completed)
      setNewCardsToday(result.new_cards_seen)
      setAnswerResult(result)
      if (result.graduated && !result.lapsed) {
        setSessionCompleted(prev => prev + 1)
      }
    } catch (e) {
      alert(t('review.checkFailed', { error: errorText(e) }))
    }
    setAnswering(false)
  }

  async function handleOverride() {
    if (!answerResult || overriding) return
    setOverriding(true)
    try {
      const result = await invoke<AnswerResult>('db_session_override', {
        language: currentCard?.language || selectedLang || 'ja',
        answerId: answerResult.verdict.answer_id,
        maxReviews: getMaxReviews(),
        maxNewCards: getMaxNewCards(),
      })
      if (result.card) setCurrentCard(result.card)
      setReviewedToday(result.reviews_completed)
      setNewCardsToday(result.new_cards_seen)
      setAnswerResult(result)
    } catch (e) {
      alert(errorText(e))
    }
    setOverriding(false)
  }

  function handleNext() {
    if (answerResult) applyProgress(answerResult)
  }

  const cancelAutoAdvance = useAutoAdvance(answerResult, handleNext)

  // End-of-session numbers come from Rust; refresh whenever the queue runs out
  useEffect(() => {
    if (currentCard || !selectedLang || loading) return
    invoke<SessionSummary>('db_session_summary', { language: selectedLang })
      .then(setSummary)
      .catch(() => setSummary(null))
  }, [currentCard, selectedLang, loading])

  async function generateDetail() {
    if (!currentCard) return
    const apiHost = localStorage.getItem('api_host') || ''
    const apiKey = localStorage.getItem('api_key') || ''
    const model = localStorage.getItem('api_vocab_model') || localStorage.getItem('api_model') || 'gpt-3.5-turbo'
    if (!apiHost || !apiKey) {
      alert(t('common.apiMissing'))
      return
    }
    setGeneratingDetail(true)
    try {
      const lang = currentCard.language || 'ja'
      const result = await invoke<string>('db_generate_detail', {
        apiHost, apiKey, model,
        content: currentCard.content,
        language: lang,
      })
      setCurrentCard(prev => prev ? { ...prev, detail: result } : prev)
    } catch (e) {
      alert(t('review.generationFailed', { error: errorText(e) }))
    }
    setGeneratingDetail(false)
  }

  async function generateMnemonic() {
    if (!currentCard) return
    const apiHost = localStorage.getItem('api_host') || ''
    const apiKey = localStorage.getItem('api_key') || ''
    const model = localStorage.getItem('api_vocab_model') || localStorage.getItem('api_model') || 'gpt-3.5-turbo'
    if (!apiHost || !apiKey) {
      alert(t('common.apiMissing'))
      return
    }
    setGeneratingMnemonic(true)
    try {
      const lang = currentCard.language || 'ja'
      const result = await invoke<string>('db_generate_mnemonic', {
        apiHost, apiKey, model,
        id: currentCard.id,
        content: currentCard.content,
        language: lang,
        detail: currentCard.detail,
      })
      setCurrentCard(prev => prev ? { ...prev, mnemonic: result } : prev)
      setShowMnemonic(true)
    } catch (e) {
      alert(t('review.mnemonicFailed', { error: errorText(e) }))
    }
    setGeneratingMnemonic(false)
  }

  async function handleImport() {
    if (!importText.trim()) return
    setImporting(true)
    setImportResult(null)
    try {
      const lines = importText.trim().split('\n').filter(l => l.trim())
      const items = lines.map(line => {
        const parts = line.split(',').map(s => s.trim())
        return {
          content: parts[0],
          detail: parts[1] || undefined,
          language: importLang,
          meaningLang: importMeaningLang,
        }
      })
      const result = await invoke<string>('db_import_review_items', { items })
      setImportResult(t('review.imported', { count: Number(result) }))
      await loadItems()
      await startSession(selectedLang)
    } catch (e) {
      setImportResult(errorText(e))
    }
    setImporting(false)
  }

  async function handleFileImport(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0]
    if (!file) return
    const text = await file.text()
    setImportText(text)
  }

  async function refreshFromDB() {
    await loadItems()
    await startSession(selectedLang)
  }

  function renderDetailContent(detail: string | null) {
    if (!detail) {
      return (
        <div className="mt-4 p-3 bg-gray-50 rounded-xl text-center">
          <p className="text-sm text-gray-400 mb-3">{t('review.noDetail')}</p>
          <div className="flex gap-2 justify-center">
            <button
              onClick={() => generateDetail()}
              disabled={generatingDetail}
              className="flex items-center gap-1.5 px-3 py-2 rounded-xl bg-blue-500 text-white text-sm font-medium hover:bg-blue-600 active:bg-blue-700 transition-colors disabled:opacity-50"
            >
              <RobotOutlined />
              {generatingDetail ? t('common.generating') : t('review.generateAi')}
            </button>
            <button
              onClick={() => { setEditingId(currentCard?.id || null) }}
              className="flex items-center gap-1.5 px-3 py-2 rounded-xl bg-gray-200 text-gray-700 text-sm font-medium hover:bg-gray-300 active:bg-gray-400 transition-colors"
            >
              <EditOutlined />
              {t('review.addManually')}
            </button>
          </div>
        </div>
      )
    }

    const parsed = parseDetailJson(detail)
    if (parsed) {
      return (
        <div className="mt-4 space-y-2 text-left">
          {parsed.reading && (
            <div className="flex items-baseline gap-2">
              <span className="text-xs text-gray-400 w-24 shrink-0">{t('card.reading')}</span>
              <span className="text-sm font-medium">{parsed.reading}</span>
            </div>
          )}
          {parsed.meaning && (
            <div className="flex items-baseline gap-2">
              <span className="text-xs text-gray-400 w-24 shrink-0">{t('card.meaning')}</span>
              <span className="text-sm font-medium">{parsed.meaning}</span>
            </div>
          )}
          {parsed.part_of_speech && (
            <div className="flex items-baseline gap-2">
              <span className="text-xs text-gray-400 w-24 shrink-0">{t('card.partOfSpeech')}</span>
              <span className="text-xs px-2 py-0.5 rounded-full bg-gray-100 text-gray-600">{posLabel(parsed.part_of_speech)}</span>
            </div>
          )}
          {parsed.example && (
            <div className="mt-2 p-2 bg-gray-50 rounded-lg">
              <p className="text-sm italic">{parsed.example}</p>
              {parsed.example_meaning && (
                <p className="text-xs text-gray-500 mt-1">{parsed.example_meaning}</p>
              )}
            </div>
          )}
          <div className="mt-3 pt-2 border-t border-gray-100">
            <div className="flex items-center justify-between mb-1">
              <span className="text-xs text-gray-400">{t('card.mnemonic')}</span>
              {!currentCard?.mnemonic && (
                <button
                  onClick={generateMnemonic}
                  disabled={generatingMnemonic}
                  className="flex items-center gap-1 px-2 py-0.5 rounded-lg bg-amber-50 text-amber-600 text-xs font-medium hover:bg-amber-100 transition-colors disabled:opacity-50"
                >
                  <BulbOutlined />
                  {generatingMnemonic ? t('card.creating') : t('card.createMnemonic')}
                </button>
              )}
            </div>
            {currentCard?.mnemonic ? (
              <div>
                <button
                  onClick={() => setShowMnemonic(!showMnemonic)}
                  className="text-sm text-left w-full"
                >
                  {showMnemonic ? currentCard.mnemonic : t('card.tapMnemonic')}
                </button>
                <button
                  onClick={generateMnemonic}
                  disabled={generatingMnemonic}
                  className="text-xs text-gray-400 hover:text-gray-600 mt-1"
                >
                  <ReloadOutlined className="mr-1" />
                  {generatingMnemonic ? t('card.regenerating') : t('card.regenerate')}
                </button>
              </div>
            ) : (
              <p className="text-xs text-gray-300">{t('card.noMnemonic')}</p>
            )}
          </div>
          <button
            onClick={() => { setEditingId(currentCard?.id || null) }}
            className="text-xs text-gray-400 hover:text-gray-600 mt-1"
          >
            <EditOutlined className="mr-1" />{t('card.editDetail')}
          </button>
        </div>
      )
    }

    return (
      <div className="mt-4 text-left">
        <p className="text-sm text-gray-600 whitespace-pre-wrap">{detail}</p>
        <button
          onClick={() => { setEditingId(currentCard?.id || null) }}
          className="text-xs text-gray-400 hover:text-gray-600 mt-2"
        >
          <EditOutlined className="mr-1" />{t('card.editDetail')}
        </button>
      </div>
    )
  }

  // Edit detail modal
  if (editingId) {
    const card = (currentCard?.id === editingId ? currentCard : null) || allItems.find(i => i.id === editingId)
    if (!card) {
      setEditingId(null)
      return null
    }
    return (
      <EditWordForm
        item={card}
        title={t('card.editDetailTitle')}
        onCancel={() => setEditingId(null)}
        onSave={async (data) => {
          if (data.content) {
            await invoke('db_update_review_item', {
              item: { id: editingId, content: data.content },
            })
          }
          await invoke('db_update_review_detail', { id: editingId, detail: data.detail })
          await invoke('db_update_mnemonic', { id: editingId, mnemonic: data.mnemonic })
          setCurrentCard(prev =>
            prev?.id === editingId ? { ...prev, content: data.content, detail: data.detail, mnemonic: data.mnemonic } : prev
          )
          setAllItems(prev => prev.map(item =>
            item.id === editingId ? { ...item, content: data.content, detail: data.detail, mnemonic: data.mnemonic } : item
          ))
          setEditingId(null)
        }}
      />
    )
  }

  // Import modal
  if (showImport) {
    return (
      <div className="flex flex-col h-full -m-4 p-4">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold">{t('review.importTitle')}</h2>
          <button
            onClick={() => { setShowImport(false); setImportResult(null) }}
            aria-label={t('common.close')}
            className="w-8 h-8 flex items-center justify-center rounded-full hover:bg-gray-100"
          >
            <CloseOutlined />
          </button>
        </div>

        <div className="mb-3">
          <label className="text-sm text-gray-500 mb-1 block">{t('review.importWordLanguage')}</label>
          <div className="flex gap-2">
            {TARGET_LANGUAGES.map((l) => (
              <button
                key={l.code}
                onClick={() => setImportLang(l.code)}
                className={`px-3 py-1.5 rounded-xl text-sm font-medium transition-colors ${
                  importLang === l.code
                    ? 'bg-blue-500 text-white'
                    : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
                }`}
              >
                {l.flag} {l.label}
              </button>
            ))}
          </div>
        </div>

        <div className="mb-3">
          <label className="text-sm text-gray-500 mb-1 block">{t('review.importMeaningLanguage')}</label>
          <div className="flex gap-2">
            {(['id', 'en'] as ExplainLang[]).map((code) => (
              <button
                key={code}
                onClick={() => setImportMeaningLang(code)}
                className={`px-3 py-1.5 rounded-xl text-sm font-medium transition-colors ${
                  importMeaningLang === code
                    ? 'bg-blue-500 text-white'
                    : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
                }`}
              >
                {languageName(code)}
              </button>
            ))}
          </div>
          {importMeaningLang !== explainLang && (
            <p className="text-xs text-amber-600 mt-1">{t('review.importMeaningMismatch')}</p>
          )}
        </div>

        <div className="mb-2">
          <p className="text-xs text-gray-400 mb-1">
            {t('review.importHint')}
          </p>
          <input
            ref={fileInputRef}
            type="file"
            accept=".csv,.txt"
            onChange={handleFileImport}
            className="hidden"
          />
          <button
            onClick={() => fileInputRef.current?.click()}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl bg-gray-100 text-gray-600 text-sm font-medium hover:bg-gray-200 mb-2"
          >
            <ImportOutlined /> {t('review.loadFile')}
          </button>
        </div>

        <textarea
          value={importText}
          onChange={(e) => setImportText(e.target.value)}
          placeholder={t('review.importPlaceholder')}
          className="flex-1 w-full p-3 bg-gray-50 rounded-xl border-none outline-none text-sm resize-none font-mono"
        />

        {importResult && (
          <p className="text-sm text-center mt-2 text-green-600">{importResult}</p>
        )}

        <div className="flex gap-2 mt-3">
          <button
            onClick={() => { setShowImport(false); setImportResult(null) }}
            className="flex-1 py-3 rounded-xl bg-gray-100 text-gray-600 font-medium hover:bg-gray-200 active:bg-gray-300 transition-colors"
          >
            {t('common.cancel')}
          </button>
          <button
            onClick={handleImport}
            disabled={!importText.trim() || importing}
            className="flex-1 py-3 rounded-xl bg-blue-500 text-white font-medium hover:bg-blue-600 active:bg-blue-700 transition-colors disabled:opacity-50 flex items-center justify-center gap-2"
          >
            <ImportOutlined />
            {importing ? t('review.importing') : t('common.import')}
          </button>
        </div>
      </div>
    )
  }

  // Loading state
  if (loading) {
    return (
      <div className="flex flex-col items-center justify-center py-16 text-gray-400 gap-3">
        <ReloadOutlined className="text-3xl animate-spin" />
        <p className="text-sm">{t('review.loading')}</p>
      </div>
    )
  }

  // No cards at all
  if (allItems.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-16 text-gray-400 gap-4">
        <div className="text-5xl">📚</div>
        <h3 className="text-lg font-medium text-gray-600">{t('review.noCards')}</h3>
        <p className="text-sm text-center max-w-xs">
          {t('review.noCardsHint')}
        </p>
        <button
          onClick={() => setShowImport(true)}
          className="flex items-center gap-1.5 px-4 py-2 rounded-full bg-blue-500 text-white text-sm font-medium hover:bg-blue-600 active:bg-blue-700 transition-colors"
        >
          <ImportOutlined /> {t('review.importTitle')}
        </button>
      </div>
    )
  }

  const langKeys = Object.keys(languageGroups)
  const deckItems: ReviewItem[] = selectedLang ? (languageGroups[selectedLang] || []) : []
  const endOfDaySeconds = (() => {
    const d = new Date(); d.setHours(23, 59, 59, 999)
    return Math.floor(d.getTime() / 1000)
  })()
  const dueInDeck = deckItems.filter(
    (item: ReviewItem) => !item.next_review || item.next_review <= endOfDaySeconds
  ).length

  return (
    <div className="flex flex-col h-full -m-4 p-4 overflow-y-auto">
      {/* Header */}
      <div className="mb-3 shrink-0 sticky top-0 z-10 bg-gray-50 pt-4 pb-2">
        <div className="flex items-center justify-between mb-2">
          <h2 className="text-lg font-semibold">{t('review.title')}</h2>
          <div className="flex items-center gap-2">
            <span className="text-xs text-gray-400">{t('review.todayCounts', { reviewed: reviewedToday, maxReviews, newSeen: newCardsToday, maxNew: maxNewCards })}</span>
            <button
              onClick={() => setShowImport(true)}
              className="flex items-center gap-1 px-2.5 py-1 rounded-lg bg-gray-100 text-gray-600 text-xs font-medium hover:bg-gray-200 transition-colors"
            >
              <ImportOutlined /> {t('common.import')}
            </button>
          </div>
        </div>

        {/* Language tabs */}
        {langKeys.length > 1 && (
          <div className="flex gap-1.5 mb-3">
            {langKeys.map(lang => {
              const count = (languageGroups[lang] || []).length
              const dueCount = (languageGroups[lang] || []).filter(
                (item: ReviewItem) => !item.next_review || item.next_review <= endOfDaySeconds
              ).length
              return (
                <button
                  key={lang}
                  onClick={() => { setSelectedLang(lang); setRevealed(false) }}
                  className={`px-3 py-1.5 rounded-xl text-sm font-medium transition-colors ${
                    selectedLang === lang
                      ? 'bg-blue-500 text-white'
                      : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
                  }`}
                >
                  {languageLabel(lang)}
                  <span className="ml-1 text-xs opacity-70">({dueCount}/{count})</span>
                </button>
              )
            })}
          </div>
        )}

        {/* Session progress bar */}
        {currentCard !== null && (() => {
          const dailyTarget = maxReviews + maxNewCards
          const dailyDone = reviewedToday + newCardsToday
          return (
            <div className="flex items-center gap-2">
              <div className="flex-1 h-1.5 bg-gray-100 rounded-full overflow-hidden">
                <div
                  className="h-full bg-blue-500 rounded-full transition-all duration-300"
                  style={{ width: `${dailyTarget > 0 ? Math.min(100, (dailyDone / dailyTarget) * 100) : 0}%` }}
                />
              </div>
              <span className="text-xs text-gray-400 shrink-0">
                {dailyDone}/{dailyTarget}
              </span>
            </div>
          )
        })()}
      </div>

      {/* No due cards */}
      {!currentCard ? (
        <div className="flex-1 flex flex-col items-center justify-center text-gray-400 gap-3">
          <SessionCompleteEffect sessionCompleted={sessionCompleted} />
          {summary && summary.total > 0 && <SessionSummaryView summary={summary} />}
          <>
            <div className="text-4xl">🎉</div>
            <p className="text-sm text-center">{t('review.allDone', { language: languageName(selectedLang) })}</p>
            <p className="text-xs text-gray-300">{t('review.deckCounts', { count: deckItems.length, due: dueInDeck })}</p>
            {newCardsToday >= maxNewCards && (
              <p className="text-xs text-amber-500">{t('review.newLimit', { n: maxNewCards })}</p>
            )}
          </>
          <button
            onClick={refreshFromDB}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl bg-gray-100 text-gray-600 text-sm font-medium hover:bg-gray-200 transition-colors"
          >
            <ReloadOutlined /> {t('review.refresh')}
          </button>
        </div>
      ) : (
        currentTask && currentTask.kind !== 'classic' ? (
          <div className="flex-1 flex flex-col justify-center gap-3 pb-6" onPointerDown={cancelAutoAdvance}>
            {missingMeaning.length > 0 && !bannerDismissed && getApiConfig().ready && (
              <div className="w-full max-w-sm mx-auto rounded-2xl bg-orange-50 p-3 flex flex-col gap-2">
                <div className="text-sm font-semibold text-gray-700">
                  {t('review.missingTitle', { count: missingMeaning.length, language: languageName(explainLang) })}
                </div>
                <div className="text-xs text-gray-500">
                  {t('review.missingBody', { words: missingMeaning.slice(0, 5).join(', ') + (missingMeaning.length > 5 ? '…' : '') })}
                </div>
                <div className="grid grid-cols-2 gap-2">
                  <button type="button" onClick={() => setBannerDismissed(true)} className="py-2 rounded-xl bg-white text-gray-600 text-sm font-medium">{t('review.later')}</button>
                  <button type="button" disabled={generatingAll} onClick={generateAllMeanings} className="py-2 rounded-xl bg-blue-500 text-white text-sm font-medium disabled:opacity-50">
                    {generatingAll ? t('common.generating') : t('review.generateAll')}
                  </button>
                </div>
              </div>
            )}
            {/* Floating, so it appearing or fading out never moves the buttons under the thumb */}
            {notice && (
              <div className="fixed top-24 left-4 right-4 z-50 flex justify-center pointer-events-none">
                <div className="max-w-sm w-full rounded-2xl bg-green-50 border border-green-100 shadow-sm px-3 py-2 text-xs text-gray-600">{notice}</div>
              </div>
            )}
            {currentTask.kind === 'needs_meaning' ? (
              <MeaningSetup
                key={currentCard.id}
                task={currentTask}
                isGrammar={currentCard.item_type === 'grammar'}
                hasApi={getApiConfig().ready}
                generating={meaningBusy === 'generating'}
                saving={meaningBusy === 'saving'}
                error={meaningError}
                onGenerate={generateMeaning}
                onSave={saveMeaning}
                onSkip={skipCard}
                onOpenSettings={() => navigate('/settings')}
              />
            ) : answerResult ? (
              <QuizVerdictView
                task={currentTask}
                result={answerResult}
                details={currentTask.grammar ? <GrammarInfoView info={currentTask.grammar} /> : renderDetailContent(currentCard.detail)}
                onNext={handleNext}
                onOverride={handleOverride}
                overriding={overriding}
              />
            ) : (
              <ReviewQuiz
                key={currentCard.id}
                task={currentTask}
                submitting={answering}
                onAnswer={handleAnswer}
              />
            )}
          </div>
        ) : (
        <>
          {/* Flashcard (self-graded: card has no meaning to quiz on) */}
          <div className="flex-1 flex flex-col items-center justify-center pb-28 overflow-y-auto">
            <div
              onClick={() => setRevealed(true)}
              className="w-full max-w-sm bg-white rounded-2xl shadow-md p-8 text-center cursor-pointer hover:shadow-lg active:shadow transition-shadow my-auto"
            >
              {/* State badge & difficulty indicator */}
              <div className="flex items-center justify-center gap-2 mb-4">
                <span className={`text-xs px-2 py-0.5 rounded-full text-white ${
                  currentCard.state === 'new' ? 'bg-purple-500' :
                  currentCard.state === 'learning' ? 'bg-blue-500' :
                  currentCard.state === 'relearning' ? 'bg-orange-500' :
                  'bg-green-500'
                }`}>
                  {t(`cardState.${currentCard.state || 'new'}`)}
                </span>
                <div className={`w-16 h-2 rounded-full ${currentCard.difficulty_bg_class}`} />
              </div>

              <div className="text-2xl font-semibold mb-1">{currentCard.content}</div>
              {currentCard.difficulty_label && (
                <span className={`inline-block text-xs px-2 py-0.5 rounded-full text-white mt-1 ${currentCard.difficulty_bg_class}`}>
                  {t(`difficulty.${currentCard.difficulty_label}`)}
                </span>
              )}

              {!revealed ? (
                <>
                  <div className="text-sm text-gray-400 mt-4">{t('review.tapReveal')}</div>
                  <div className="text-xs text-gray-300 mt-2">{t('review.classicHint')}</div>
                </>
              ) : (
                <>
                  {renderDetailContent(currentCard.detail)}

                  <div className="mt-4 pt-3 border-t border-gray-100">
                    <div className="flex items-center justify-center gap-4 text-xs text-gray-400">
                      <span>{t('card.reps', { n: currentCard.R })}</span>
                      <span>{t('card.stability', { days: Math.round(currentCard.fsrs_stability ?? currentCard.I) })}</span>
                      <span>{t('card.difficulty', { n: currentCard.fsrs_difficulty != null ? formatNumber(currentCard.fsrs_difficulty, { maximumFractionDigits: 1 }) : '—' })}</span>
                      {currentCard.lapse_count > 0 && (
                        <span>{t('card.lapses', { n: currentCard.lapse_count })}</span>
                      )}
                    </div>
                    {currentCard.next_review && (
                      <div className="text-xs text-gray-300 mt-1">
                        {t('card.nextReview', { when: formatDate(currentCard.next_review * 1000, { dateStyle: 'medium', timeStyle: 'short' }) })}
                      </div>
                    )}
                  </div>
                </>
              )}
            </div>
          </div>

          {/* Grade buttons */}
          <div className="fixed bottom-14 left-0 right-0 px-4 pt-3 pb-4 z-40">
            {revealed ? (
              <div className="grid grid-cols-4 gap-2">
                {gradeOptions.map((g) => (
                  <button
                    key={g.grade}
                    onClick={() => handleGrade(g.grade)}
                    disabled={isGrading.current}
                    className={`flex flex-col items-center justify-center gap-1 py-3 rounded-2xl text-white font-medium ${g.color} hover:opacity-90 active:opacity-80 transition-opacity shadow-sm disabled:opacity-50`}
                  >
                    <span className="font-semibold text-sm leading-none">{g.label}</span>
                    <span className="text-[10px] opacity-75 leading-none">{g.nextLabel}</span>
                  </button>
                ))}
              </div>
            ) : (
              <div className="flex items-center justify-center">
                <button
                  onClick={() => setRevealed(true)}
                  className="w-full py-3.5 rounded-full bg-blue-500 text-white font-medium text-sm hover:bg-blue-600 active:bg-blue-700 transition-colors shadow-sm"
                >
                  {t('review.showAnswer')}
                </button>
              </div>
            )}
          </div>
        </>
        )
      )}
    </div>
  )
}
